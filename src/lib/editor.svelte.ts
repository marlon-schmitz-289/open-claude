// Zustand des eingebauten Editors je Projekt. Lebt auf Modulebene, nicht in Komponenten: Panels werden beim
// Andocken neu gemountet und die Editor-Ansicht beim Wechsel zu Terminal/Git abgebaut, Puffer bleiben trotzdem.
// Gespeichert (localStorage "editor:<repo>", JSON, try/catch): layout, active, expanded, pin, url, wpf, run.
// Ungespeicherte Inhalte ueberleben keinen Neustart der App.
import { untrack } from "svelte";
import type { EditorState, StateEffect } from "@codemirror/state";
import {
  FILES,
  PREVIEW,
  RUN,
  TESTS,
  activate,
  closePanel,
  defaultLayout,
  filePanel,
  groupOf,
  groups,
  openPanel,
  panelPath,
  panels,
  parse,
  renamePanel,
  splitAt,
  type Layout,
  type PanelId,
} from "./dock.ts";
import { KONFLIKT, fs, isBinaryImage, previewAllow } from "./files.ts";
import { moved, under, verdict } from "./editor.logic.ts";
import { runStop } from "./run.svelte.ts";
import { rescan } from "./testing.svelte.ts";

/** Platte weicht vom Puffer ab, waehrend lokale Aenderungen offen sind. */
export type Conflict = "changed" | "deleted";

export type OpenFile = {
  path: string;
  /** Aktueller Pufferinhalt (LF, ohne BOM). Nur ueber setText/reload aendern. */
  text: string;
  /** Stand der Platte beim letzten Lesen/Speichern. */
  saved: string;
  crlf: boolean;
  bom: boolean;
  /** mtime beim letzten Lesen/Speichern = `expected` fuer fs.write; null = Datei gibt es (noch) nicht. */
  mtime: number | null;
  /** text !== saved. */
  dirty: boolean;
  /** Gesetzt vom Polling oder von save (KONFLIKT); CodePane zeigt dann die Leiste. */
  conflict: Conflict | null;
  /** Zaehlt hoch, wenn text von aussen ersetzt wurde (stilles Nachladen, reload): CodePane zieht den Editor nach. */
  rev: number;
};

export type RepoEditor = {
  repo: string;
  /** Wird immer als Ganzes ersetzt (setLayout), nie veraendert. */
  layout: Layout;
  /** Geladene Dateien nach Pfad. Ein Datei-Panel ohne Eintrag hier laedt noch. */
  files: Record<string, OpenFile>;
  /** Zuletzt aktivierte Datei: Ziel von Strg+S, Markierung im Baum, Quelle der Vorschau. Neue Dateien oeffnen in ihrer Gruppe. */
  active: string | null;
  /** Vorschau an diese Datei geheftet; null = folgt active. */
  pin: string | null;
  /** Dev-Server-URL der Vorschau; "" = Datei anzeigen. */
  url: string;
  /** WPF-Vorschau fuer dieses Projekt erlaubt: XAML laden kann Code ausfuehren, darum erst nach Rueckfrage. */
  wpf: boolean;
  /** Zuletzt gewaehltes Startziel der Run-Leiste (RunConfig.label); null = Vorauswahl. */
  run: string | null;
  /** Aufgeklappte Ordner im Baum. */
  expanded: string[];
  /** mtime der beobachteten Ordner ("" = Projektordner + expanded). Aendert sich ein Wert, liest der Baum den Ordner neu. */
  dirs: Record<string, number | null>;
  /** Letzter Fehler (Oeffnen, Speichern) fuer die Anzeige in EditorView; "" = keiner. */
  error: string;
  /** Zeile, die CodePane anspringen soll (Stack-Link, Testdetails); setzt es danach zurueck. */
  reveal: { path: string; line: number } | null;
};

// Bewusst eine Map mit je einem eigenen $state-Objekt statt eines $state-Records: editor() wird auch aus
// $derived gerufen, und dort darf kein bestehender Zustand geschrieben werden. `count` macht isDirty() trotzdem
// reaktiv auf neue/verworfene Projekte.
const store = new Map<string, RepoEditor>();
let count = $state(0);

/**
 * CodeMirror-Zustand (samt Undo-Verlauf) und Scrollposition je Datei, nur fuer CodePane. Nicht reaktiv: ein
 * $state-Proxy wuerde den EditorState zerstoeren.
 */
export type Snapshot = { state: EditorState; scroll: StateEffect<unknown> };
export const snapshots = new Map<string, Snapshot>();
export const snapKey = (repo: string, path: string) => `${repo}\n${path}`;

/** Dateien, deren fs.write gerade laeuft: das Polling saehe sonst die neue mtime vor dem Store und meldete einen Konflikt. */
const saving = new Set<string>();

/** `saved` nach "Meine behalten": gleicht nie einem Puffer (fs.read lehnt NUL ab), die Datei bleibt bis zum Speichern geaendert. */
const NEVER = "\0";

function persist(s: RepoEditor) {
  try {
    const { layout, active, expanded, pin, url, wpf, run } = s;
    localStorage.setItem(`editor:${s.repo}`, JSON.stringify({ layout, active, expanded, pin, url, wpf, run }));
  } catch {
    // Ohne localStorage gilt das Layout nur fuer diese Sitzung.
  }
}

/**
 * Datei von der Platte in den Store lesen. Gibt es den Puffer schon, wird er ersetzt (rev++), ausser er hat
 * inzwischen lokale Aenderungen und force fehlt: dann nur Konflikt melden.
 */
async function load(s: RepoEditor, path: string, force = false) {
  const t = await fs.read(s.repo, path);
  if (store.get(s.repo) !== s) return;
  const f = s.files[path];
  if (f && f.dirty && !force) {
    f.conflict = "changed";
    return;
  }
  s.files[path] = {
    path,
    text: t.content,
    saved: t.content,
    crlf: t.crlf,
    bom: t.bom,
    mtime: t.mtime,
    dirty: false,
    conflict: null,
    rev: f ? f.rev + 1 : 0,
  };
}

/** Tab, Puffer und CodeMirror-Zustand einer Datei entfernen; active wandert zum Nachbarn. */
function forget(s: RepoEditor, path: string) {
  const panel = filePanel(path);
  const group = groupOf(s.layout, panel)?.id;
  s.layout = closePanel(s.layout, panel);
  delete s.files[path];
  snapshots.delete(snapKey(s.repo, path));
  if (s.pin === path) s.pin = null;
  if (s.active === path) {
    const next = [groups(s.layout).find((g) => g.id === group)?.active, ...panels(s.layout)].find(
      (p) => p && panelPath(p) !== null,
    );
    s.active = next ? panelPath(next) : null;
  }
}

/** Pfade aller Datei-Panels, auch der noch ladenden. */
const openPaths = (s: RepoEditor) => panels(s.layout).flatMap((p) => panelPath(p) ?? []);

const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/**
 * Zustand des Projekts; beim ersten Aufruf aus localStorage wiederhergestellt (Layout ueber dock.parse) und
 * previewAllow(repo) gerufen. Die Datei-Panels des Layouts laden im Hintergrund; nicht lesbare werden geschlossen.
 * Das Ergebnis ist reaktiv ($state) und pro Repo immer dasselbe Objekt.
 */
/** Wie editor(), legt aber nichts an: fuer Aufraeumcode, der nach drop() laeuft. */
export const peek = (repo: string): RepoEditor | undefined => store.get(repo);

export function editor(repo: string): RepoEditor {
  const had = store.get(repo);
  if (had) return had;
  let p: Record<string, unknown> = {};
  try {
    p = JSON.parse(localStorage.getItem(`editor:${repo}`) ?? "{}") ?? {};
  } catch {
    // Kaputter oder fehlender Eintrag: Standardlayout.
  }
  const str = (v: unknown) => (typeof v === "string" ? v : null);
  const layout = parse(p.layout ? JSON.stringify(p.layout) : null);
  const paths = panels(layout).flatMap((x) => panelPath(x) ?? []);
  const active = str(p.active);
  const s: RepoEditor = $state({
    repo,
    layout,
    files: {},
    active: active !== null && paths.includes(active) ? active : (paths[0] ?? null),
    pin: str(p.pin),
    url: str(p.url) ?? "",
    wpf: p.wpf === true,
    run: str(p.run),
    expanded: Array.isArray(p.expanded) ? p.expanded.filter((d) => typeof d === "string") : [],
    dirs: {},
    error: "",
    reveal: null,
  });
  store.set(repo, s);
  // Nicht synchron: editor() laeuft auch in $derived.
  queueMicrotask(() => count++);
  previewAllow(repo).catch(() => {});
  for (const path of paths)
    load(s, path).catch(() => {
      forget(s, path);
      persist(s);
    });
  return s;
}

/**
 * Datei als Tab zeigen (laedt sie beim ersten Mal) und zur aktiven machen; line = dorthin springen. Binaere Bilder (files.isBinaryImage)
 * bekommen keinen Tab: sie werden an die Vorschau geheftet und die Vorschau geoeffnet. Fehler landen in error.
 * Neue Tabs kommen in die Gruppe der aktiven Datei, sonst in die main-Gruppe (nie zu Baum oder Vorschau).
 */
export async function open(repo: string, path: string, line?: number): Promise<void> {
  const s = editor(repo);
  s.error = "";
  if (line !== undefined) s.reveal = { path, line };
  if (isBinaryImage(path)) {
    s.pin = path;
    // Eine Dev-Server-URL haette Vorrang und verdeckte das Bild.
    s.url = "";
    s.layout = groupOf(s.layout, PREVIEW) ? activate(s.layout, PREVIEW) : splitAt(s.layout, PREVIEW, null, "right");
    persist(s);
    return;
  }
  s.layout = openPanel(s.layout, filePanel(path), s.active === null ? null : groupOf(s.layout, filePanel(s.active))?.id);
  s.active = path;
  persist(s);
  if (s.files[path]) return;
  try {
    await load(s, path);
  } catch (e) {
    forget(s, path);
    persist(s);
    s.error = String(e);
  }
}

/** Tab einer Datei schliessen. Bei ungespeicherten Aenderungen erst window.confirm, ausser force. false = abgebrochen. */
export function close(repo: string, path: string, force = false): boolean {
  const s = editor(repo);
  if (!force && s.files[path]?.dirty && !window.confirm(`Ungespeicherte Änderungen in „${name(path)}“ verwerfen?`))
    return false;
  forget(s, path);
  persist(s);
  return true;
}

/** Beliebiges Panel schliessen (Docks onclose): Dateien ueber close(), die anderen direkt. false = abgebrochen. */
export function closeTab(repo: string, panel: PanelId): boolean {
  const path = panelPath(panel);
  if (path !== null) return close(repo, path);
  const s = editor(repo);
  s.layout = closePanel(s.layout, panel);
  persist(s);
  // Laeuft etwas, ist die Ausgabe im Layout: ohne sie gaebe es keinen Stopp-Knopf mehr.
  if (panel === RUN) void runStop(repo);
  return true;
}

/**
 * Speichern (path weggelassen = active). Meldet fs.write KONFLIKT, wird conflict gesetzt und nichts geschrieben.
 * force ueberschreibt die Platte. Andere Fehler landen in error. true = geschrieben.
 * Laeuft fuer die Datei schon ein Speichern, passiert nichts: ein zweites schickte dieselbe mtime und bekaeme KONFLIKT.
 */
export async function save(repo: string, path?: string, force = false): Promise<boolean> {
  const s = editor(repo);
  const p = path ?? s.active;
  const f = p === null ? undefined : s.files[p];
  if (p === null || !f) return false;
  flush(repo, p);
  const text = f.text;
  const key = snapKey(repo, p);
  if (saving.has(key)) return false;
  saving.add(key);
  try {
    f.mtime = await fs.write(repo, p, text, f.crlf, f.bom, f.mtime, force);
    f.saved = text;
    // Waehrend des Schreibens kann weitergetippt worden sein.
    f.dirty = f.text !== text;
    f.conflict = null;
    s.error = "";
    // Test-Explorer: Tests der Datei neu einlesen (nur wenn er schon gesucht hat).
    void rescan(repo, [p]);
    return true;
  } catch (e) {
    if (String(e) === KONFLIKT) {
      const [now] = await fs.stat(repo, [p]).catch(() => [0]);
      f.conflict = now === null ? "deleted" : "changed";
    } else s.error = String(e);
    return false;
  } finally {
    saving.delete(key);
  }
}

/** "Neu laden": Puffer verwerfen, Platte lesen, conflict loeschen, rev++. Gibt es die Datei nicht mehr, schliesst der Tab. */
export async function reload(repo: string, path: string): Promise<void> {
  const s = editor(repo);
  try {
    await load(s, path, true);
  } catch (e) {
    const [now] = await fs.stat(repo, [path]).catch(() => [0]);
    if (now === null) {
      forget(s, path);
      persist(s);
    } else s.error = String(e);
  }
}

/**
 * "Meine behalten": conflict loeschen und mtime auf den Stand der Platte setzen; das naechste Speichern ueberschreibt.
 * Die Platte weicht jetzt vom Puffer ab, auch wenn er wieder dem alten Stand gleicht: bis dahin bleibt er dirty.
 */
export async function keep(repo: string, path: string): Promise<void> {
  const s = editor(repo);
  try {
    const [now] = await fs.stat(repo, [path]);
    const f = s.files[path];
    if (!f) return;
    f.mtime = now;
    f.conflict = null;
    f.saved = NEVER;
    f.dirty = true;
  } catch (e) {
    s.error = String(e);
  }
}

/** "Vergleichen": Unified Diff Platte -> Puffer fuer DiffView. */
export function diff(repo: string, path: string): Promise<string> {
  flush(repo, path);
  return fs.diff(repo, path, editor(repo).files[path]?.text ?? "");
}

/** Setzt text und dirty. */
export function setText(repo: string, path: string, text: string): void {
  const f = editor(repo).files[path];
  if (!f) return;
  f.text = text;
  // Laenge zuerst: beim Tippen unterscheidet sie sich fast immer, der volle Vergleich entfaellt.
  f.dirty = text.length !== f.saved.length || text !== f.saved;
}

/** Zeilenumbruch (Alt+Z), global fuer alle Projekte; localStorage "editor:wrap". */
export const prefs = $state({ wrap: false });
try {
  prefs.wrap = localStorage.getItem("editor:wrap") === "1";
} catch {
  // Ohne localStorage: kein Umbruch.
}

export function toggleWrap(): void {
  prefs.wrap = !prefs.wrap;
  try {
    localStorage.setItem("editor:wrap", prefs.wrap ? "1" : "0");
  } catch {
    // Gilt dann nur fuer diese Sitzung.
  }
}

/** Ausstehende Texte aus CodePane: toString ist O(Datei), darum hoechstens einmal pro Frame. */
const pending = new Map<string, () => string>();

/** Von CodePane bei jeder Aenderung: dirty sofort, text im naechsten Frame (oder frueher ueber flush). */
export function edited(repo: string, path: string, read: () => string): void {
  const f = peek(repo)?.files[path];
  if (!f) return;
  f.dirty = true;
  const key = snapKey(repo, path);
  if (!pending.has(key)) requestAnimationFrame(() => flush(repo, path));
  pending.set(key, read);
}

/** Ausstehenden Text sofort uebernehmen; vor allem, was f.text liest und nicht warten darf (save, diff). */
export function flush(repo: string, path: string): void {
  const key = snapKey(repo, path);
  const read = pending.get(key);
  pending.delete(key);
  if (read && peek(repo)?.files[path]) setText(repo, path, read());
}

/** Layout setzen und speichern (Docks onlayout). */
export function setLayout(repo: string, layout: Layout): void {
  const s = editor(repo);
  s.layout = layout;
  persist(s);
}

/** Docks onfocus: ist das Panel eine Datei, wird sie active. */
export function focus(repo: string, panel: PanelId): void {
  const s = editor(repo);
  const path = panelPath(panel);
  // Kommt bei jedem Klick in ein Panel: nur schreiben, wenn sich etwas aendert.
  if (path === null || s.active === path) return;
  s.active = path;
  persist(s);
}

/**
 * "files", "preview" bzw. "tests" ein-/ausblenden. Neu: files an die linke, preview an die rechte Kante des Layouts,
 * tests als Tab neben files (wie die Seitenleiste in VS Code), ohne files an die linke Kante.
 */
export function togglePanel(repo: string, panel: "files" | "preview" | "tests"): void {
  const s = editor(repo);
  const files = groupOf(s.layout, FILES)?.id;
  s.layout = groupOf(s.layout, panel)
    ? closePanel(s.layout, panel)
    : panel === TESTS && files
      ? openPanel(s.layout, panel, files)
      : splitAt(s.layout, panel, null, panel === PREVIEW ? "right" : "left");
  persist(s);
}

/** "Layout zuruecksetzen": Standardlayout, offene Dateien bleiben als Tabs in der main-Gruppe. */
export function resetLayout(repo: string): void {
  const s = editor(repo);
  let layout = defaultLayout();
  for (const path of openPaths(s)) layout = openPanel(layout, filePanel(path));
  if (s.active !== null) layout = activate(layout, filePanel(s.active));
  s.layout = layout;
  persist(s);
  // Das Standardlayout hat keine Ausgabe.
  void runStop(repo);
}

/** Ordner im Baum auf-/zuklappen und speichern. */
export function setExpanded(repo: string, dir: string, open: boolean): void {
  const s = editor(repo);
  s.expanded = s.expanded.filter((d) => d !== dir);
  if (open) s.expanded.push(dir);
  persist(s);
}

/** Vorschau anheften (null = folgt der aktiven Datei) und speichern. */
export function setPin(repo: string, path: string | null): void {
  const s = editor(repo);
  s.pin = path;
  persist(s);
}

/** Dev-Server-URL der Vorschau setzen und speichern. */
export function setUrl(repo: string, url: string): void {
  const s = editor(repo);
  s.url = url;
  persist(s);
}

/** Startziel der Run-Leiste merken und speichern. */
export function setRun(repo: string, label: string): void {
  const s = editor(repo);
  s.run = label;
  persist(s);
}

/** WPF-Vorschau fuer das Projekt erlauben und speichern. */
export function allowWpf(repo: string): void {
  const s = editor(repo);
  s.wpf = true;
  persist(s);
}

/** Nach fs.rename (Datei oder Ordner): offene Dateien, Panels, active, pin und expanded ziehen mit, Puffer bleiben. */
export function renamed(repo: string, from: string, to: string): void {
  const s = editor(repo);
  for (const old of openPaths(s)) {
    const path = moved(old, from, to);
    if (path === null) continue;
    s.layout = renamePanel(s.layout, filePanel(old), filePanel(path));
    const f = s.files[old];
    if (f) {
      delete s.files[old];
      s.files[path] = { ...f, path };
    }
    // Der CodeMirror-Zustand haengt mit seinen Callbacks am alten Pfad: CodePane baut ihn aus dem Puffer neu auf.
    snapshots.delete(snapKey(repo, old));
  }
  if (s.active !== null) s.active = moved(s.active, from, to) ?? s.active;
  if (s.pin !== null) s.pin = moved(s.pin, from, to) ?? s.pin;
  s.expanded = s.expanded.map((d) => moved(d, from, to) ?? d);
  persist(s);
}

/** Nach fs.delete (Datei oder Ordner): betroffene Tabs ohne Rueckfrage schliessen, pin/expanded bereinigen. */
export function removed(repo: string, path: string): void {
  const s = editor(repo);
  for (const p of openPaths(s)) if (under(p, path)) forget(s, p);
  if (s.pin !== null && under(s.pin, path)) s.pin = null;
  s.expanded = s.expanded.filter((d) => !under(d, path));
  persist(s);
}

/**
 * Eine Runde fs.stat ueber offene Dateien und beobachtete Ordner:
 * - Datei geaendert, ohne lokale Aenderungen: still nachladen (rev++); geloescht: Tab schliessen.
 * - Datei geaendert/geloescht mit lokalen Aenderungen: conflict setzen, Puffer bleibt unberuehrt.
 * - Ordner: dirs aktualisieren.
 * Der Baum ruft das nach eigenen Dateioperationen, damit die Anzeige sofort stimmt.
 */
export async function poll(repo: string): Promise<void> {
  const s = store.get(repo);
  if (!s) return;
  const files = Object.values(s.files).map((f) => [f.path, f.mtime] as const);
  const dirs = ["", ...s.expanded];
  let now: (number | null)[];
  try {
    now = await fs.stat(repo, [...files.map(([p]) => p), ...dirs]);
  } catch {
    return; // Naechste Runde versucht es wieder.
  }
  if (store.get(repo) !== s) return;
  files.forEach(([path, was], i) => {
    const f = s.files[path];
    // Inzwischen gespeichert, neu geladen oder geschlossen: der Vergleich waere veraltet.
    if (!f || f.mtime !== was || saving.has(snapKey(repo, path))) return;
    const v = verdict(f.dirty, was, now[i]);
    if (v === "reload") load(s, path).catch((e) => (s.error = String(e)));
    else if (v === "close") {
      forget(s, path);
      persist(s);
    } else if (v !== "none") f.conflict = v;
  });
  const next = Object.fromEntries(dirs.map((d, i) => [d, now[files.length + i]]));
  // Nur bei Aenderung zuweisen, sonst liest der Baum alle 1,5 s umsonst neu.
  if (JSON.stringify(next) !== JSON.stringify(s.dirs)) s.dirs = next;
}

/**
 * Polling starten: sofort, dann alle 1,5 s und bei Fensterfokus. Liefert die Stopp-Funktion.
 * EditorView: $effect(() => watch(repo)).
 */
export function watch(repo: string): () => void {
  // untrack: poll liest den Store; der $effect in EditorView soll davon nicht neu starten.
  const run = () => void untrack(() => poll(repo));
  run();
  const timer = setInterval(run, 1500);
  window.addEventListener("focus", run);
  return () => {
    clearInterval(timer);
    window.removeEventListener("focus", run);
  };
}

/** Ungespeicherte Aenderungen in diesem Projekt; ohne repo: in irgendeinem. Reaktiv. */
export function isDirty(repo?: string): boolean {
  void count;
  const any = (s: RepoEditor | undefined) => !!s && Object.values(s.files).some((f) => f.dirty);
  return repo === undefined ? [...store.values()].some(any) : any(store.get(repo));
}

/**
 * Editor des Projekts schliessen (zurueck zur Liste): fragt bei ungespeicherten Aenderungen (window.confirm),
 * (ausser force), verwirft dann die Puffer; Layout und offene Tabs bleiben gespeichert. false = abgebrochen.
 * Der Wechsel zu Terminal oder Git ruft das NICHT.
 */
export function drop(repo: string, force = false): boolean {
  if (!store.has(repo)) return true;
  if (!force && isDirty(repo) && !window.confirm("Ungespeicherte Änderungen im Editor verwerfen?")) return false;
  void runStop(repo);
  for (const key of [...snapshots.keys()]) if (key.startsWith(snapKey(repo, ""))) snapshots.delete(key);
  store.delete(repo);
  previewAllow(repo, false).catch(() => {});
  count++;
  return true;
}
