// Zustand des Test-Explorers je Projekt. Lebt auf Modulebene wie editor(): das Panel wird beim Andocken neu
// gemountet, Baum, Ergebnisse und laufender Lauf bleiben. Gespeichert (localStorage "tests:<repo>"): nur expanded.
// Grosse Maps sind $state.raw und werden pro Batch einmal neu zugewiesen, nie tief reaktiv.
import { fs, testCancel as cancelCmd, testRun as runCmd, type TestMsg, type TestScope } from "./files.ts";
import { plain } from "./editor.logic.ts";
import {
  JS_FW,
  aggregate,
  discover,
  dirOf,
  flush,
  joinPath,
  jsEvent,
  jsFw,
  keyOf,
  markers,
  parseLines,
  parser,
  placer,
  projects,
  relTo,
  replaceFiles,
  stackLinks,
  subtree,
  inScope,
  type Ev,
  type Fw,
  type Loc,
  type Result,
  type St,
  type TestNode,
  type TestProject,
  type Tree,
} from "./testing.logic.ts";

/** ponytail: Obergrenze fuer die Discovery; darueber Hinweis im Panel. */
const MAX_FILES = 3000;
const MAX_LOG = 20_000;
/** Dateien je Block: zwischen den Bloecken kommt die UI dran. */
const BLOCK = 50;

export type RunInfo = { id: string; scope: string; started: number };

export class RepoTests {
  readonly repo: string;
  projects = $state.raw<TestProject[]>([]);
  nodes = $state.raw<Tree>({});
  results = $state.raw<Record<string, Result>>({});
  /** Laufender Lauf; null = keiner. */
  run = $state.raw<RunInfo | null>(null);
  /** Letzter Lauf: Dauer und Exit-Code des letzten Schritts. */
  last = $state.raw<{ ms: number; exit: number | null } | null>(null);
  log = $state.raw<string[]>([]);
  expanded = $state.raw<string[]>([]);
  selected = $state<string | null>(null);
  filter = $state("");
  failedOnly = $state(false);
  tab = $state<"details" | "output">("details");
  error = $state("");
  /** Hinweis (Obergrenze, MTP). */
  note = $state("");
  scanning = $state(false);
  scanned = $state(false);
  states = $derived(aggregate(this.nodes, this.results));

  constructor(repo: string) {
    this.repo = repo;
  }
}

const store = new Map<string, RepoTests>();
/** Zaehlt neue Projekte: peek() wird damit reaktiv, ohne selbst etwas anzulegen. */
let created = $state(0);
/** mtime je gelesener Testdatei, fuer das Nachscannen nach einem Lauf. Nicht reaktiv. */
const mtimes = new Map<string, Map<string, number>>();
/** Abbruch angefordert (je Repo). */
const cancelled = new Set<string>();

/** Zustand des Projekts; beim ersten Aufruf mit expanded aus localStorage. Pro Repo immer dasselbe Objekt. */
export function tests(repo: string): RepoTests {
  const had = store.get(repo);
  if (had) return had;
  const t = new RepoTests(repo);
  try {
    const p = JSON.parse(localStorage.getItem(`tests:${repo}`) ?? "{}");
    if (Array.isArray(p?.expanded)) t.expanded = p.expanded.filter((x: unknown) => typeof x === "string");
  } catch {
    // Ohne gespeicherten Stand: alles zu.
  }
  store.set(repo, t);
  mtimes.set(repo, new Map());
  // Nicht synchron: tests() laeuft auch in $derived.
  queueMicrotask(() => created++);
  return t;
}

/** Wie tests(), legt aber nichts an (CodePane, Speichern). Reaktiv auf neu angelegte Projekte. */
export function peek(repo: string): RepoTests | undefined {
  void created;
  return store.get(repo);
}

function persist(t: RepoTests) {
  try {
    localStorage.setItem(`tests:${t.repo}`, JSON.stringify({ expanded: t.expanded }));
  } catch {
    // Gilt dann nur fuer diese Sitzung.
  }
}

export function setExpanded(repo: string, id: string, open: boolean): void {
  const t = tests(repo);
  t.expanded = open ? [...t.expanded.filter((x) => x !== id), id] : t.expanded.filter((x) => x !== id);
  persist(t);
}

/** Hoechstens n gleichzeitig. */
async function pool<T, R>(items: T[], n: number, f: (x: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let i = 0;
  const worker = async () => {
    while (i < items.length) {
      const k = i++;
      out[k] = await f(items[k]);
    }
  };
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, worker));
  return out;
}

/** Projekt einer Testdatei (JS: auch Pakete mit anderer Vorgabe), naechstes zuerst. */
function projectOf(t: RepoTests, file: string, fw?: Fw): TestProject | undefined {
  const fits = (p: TestProject) =>
    (fw === undefined ? p.files.includes(file) : fw === p.fw || (JS_FW.includes(fw) && JS_FW.includes(p.fw))) &&
    (!p.dir || file.startsWith(`${p.dir}/`));
  return t.projects.filter(fits).sort((a, b) => b.dir.length - a.dir.length)[0];
}

/** cargo: Crate-Ordner (naechste Cargo.toml) fuer den Modulpfad; bekannt aus der Dateiliste. */
const crates = new Map<string, Set<string>>();

/** Dateien lesen und neu einordnen, in Bloecken; zwischen den Bloecken bekommt die UI den Thread. */
async function scanFiles(t: RepoTests, files: string[]) {
  const times = mtimes.get(t.repo)!;
  for (let i = 0; i < files.length; i += BLOCK) {
    const block = files.slice(i, i + BLOCK);
    const texts = await pool(block, 8, (f) => fs.read(t.repo, f).catch(() => null));
    const found: TestNode[] = [];
    block.forEach((f, k) => {
      const text = texts[k];
      const p = projectOf(t, f);
      if (!text || !p) return;
      times.set(f, text.mtime);
      const fw = JS_FW.includes(p.fw) ? jsFw(text.content, p.fw) : p.fw;
      if (!fw) return;
      const base = fw === "cargo" ? crateOf(t.repo, f, p.dir) : p.dir;
      const ns = discover(fw, f, text.content, base);
      // Dateien ohne erkannte Tests zeigen wir nicht (Helfer in cargo/dotnet-Projekten).
      if (ns.length > 1) found.push(...ns);
    });
    t.nodes = replaceFiles(t.nodes, block, found);
    await new Promise((r) => setTimeout(r));
  }
}

function crateOf(repo: string, file: string, root: string): string {
  const dirs = crates.get(repo);
  for (let d = dirOf(file); ; d = dirOf(d)) {
    if (dirs?.has(d)) return d;
    if (!d || d === root) return root;
  }
}

/**
 * Tests neu suchen. Ohne files: Projekte erkennen und alle Testdateien lesen ("Neu suchen", erstes Oeffnen).
 * Mit files: nur diese Dateien, und nur wenn schon gesucht wurde (nach dem Speichern, nach einem Lauf).
 */
export async function rescan(repo: string, files?: string[]): Promise<void> {
  if (files) {
    const t = store.get(repo);
    if (!t?.scanned) return;
    const known = files.filter((f) => projectOf(t, f));
    if (known.length) await scanFiles(t, known);
    return;
  }
  const t = tests(repo);
  if (t.scanning) return;
  t.scanning = true;
  t.error = "";
  try {
    const all = await fs.files(repo);
    crates.set(repo, new Set(all.filter((f) => /(^|\/)Cargo\.toml$/.test(f)).map(dirOf)));
    const ms = markers(all);
    const texts = await pool(ms, 8, (f) => fs.read(repo, f).then((x) => x.content, () => ""));
    t.projects = projects(all, Object.fromEntries(ms.map((f, i) => [f, texts[i]])));
    const files = t.projects.flatMap((p) => p.files);
    t.note =
      files.length > MAX_FILES
        ? `Nur die ersten ${MAX_FILES} von ${files.length} Testdateien`
        : (t.projects.find((p) => p.note)?.note ?? "");
    t.nodes = {};
    await scanFiles(t, files.slice(0, MAX_FILES));
    t.scanned = true;
  } catch (e) {
    t.error = String(e);
  } finally {
    t.scanning = false;
  }
}

// --- Laeufe ---

type Step = { fw: Fw; dir: string; scope: TestScope; scopeId: string | null };

/** Test-Keys unter einem Knoten, ohne Parameter-Kinder (die laufen mit ihrem Basistest). */
const testKeys = (nodes: Tree, id: string) =>
  subtree(nodes, id)
    .map((i) => nodes[i])
    .filter((n) => n.kind === "test" && nodes[n.parent ?? ""]?.kind !== "test")
    .map((n) => n.key);

const isPseudo = (n: TestNode) => n.kind === "file" && n.name.startsWith("(");

function stepFor(t: RepoTests, id: string): Step | null {
  let n = t.nodes[id];
  if (!n) return null;
  // Parameter-Variante: ueber den Basistest.
  if (n.parent && t.nodes[n.parent]?.kind === "test") n = t.nodes[n.parent];
  const p = projectOf(t, n.file, n.fw) ?? { dir: dirOf(n.file) };
  const file = relTo(p.dir, n.file);
  const base = { fw: n.fw, dir: p.dir, scopeId: n.id };
  if (isPseudo(n)) return { ...base, scope: { kind: "all", files: [] } };
  if (n.kind === "file") return { ...base, scope: { kind: "file", file, keys: testKeys(t.nodes, n.id) } };
  return { ...base, scope: { kind: "test", file, key: n.key, suite: n.kind === "suite" || n.children.length > 0 } };
}

/** Schritte fuer "all", "failed" oder einen Knoten. Je (fw, dir) ein Prozess, nacheinander. */
function plan(t: RepoTests, target: string): Step[] {
  const files = Object.values(t.nodes).filter((n) => n.kind === "file");
  if (target === "failed") return files.filter((n) => t.states[n.id] === "failed").flatMap((n) => stepFor(t, n.id) ?? []);
  if (target !== "all") return [stepFor(t, target)].flatMap((s) => s ?? []);
  const groups = new Map<string, Step>();
  for (const n of files) {
    const p = projectOf(t, n.file, n.fw);
    if (!p || p.note) continue;
    const k = `${n.fw}|${p.dir}`;
    const g = groups.get(k) ?? { fw: n.fw, dir: p.dir, scope: { kind: "all", files: [] }, scopeId: null };
    if (g.scope.kind === "all" && !isPseudo(n)) g.scope.files.push(relTo(p.dir, n.file));
    groups.set(k, g);
  }
  return [...groups.values()];
}

/** Knoten eines Schritts (fuer queued/running und das Zuruecksetzen). */
function stepIds(t: RepoTests, s: Step): string[] {
  if (s.scopeId) return subtree(t.nodes, s.scopeId);
  return Object.values(t.nodes)
    .filter((n) => n.fw === s.fw && (!s.dir || n.file.startsWith(`${s.dir}/`)))
    .map((n) => n.id);
}

function mark(t: RepoTests, ids: string[], state: St) {
  const r = { ...t.results };
  // Datei-/Suite-Ergebnisse (Datei-Fehler) gelten nur bis zum naechsten Lauf, sonst bleibt die Datei rot.
  for (const id of ids)
    if (t.nodes[id]?.kind === "test") r[id] = { state };
    else delete r[id];
  t.results = r;
}

/** Pfad aus dem Output relativ zum Repo: absolut -> ohne Repo-Praefix, sonst relativ zur cwd. "" = unbekannt. */
function toRepo(repo: string, dir: string, p: string): string | null {
  if (!p) return null;
  const f = p.replaceAll("\\", "/");
  const root = repo.replaceAll("\\", "/").replace(/\/$/, "");
  if (f.startsWith(`${root}/`)) return f.slice(root.length + 1);
  // Ausserhalb des Repos (oder anderes Laufwerk): so lassen, Oeffnen meldet dann einen Fehler.
  if (f.startsWith("/") || /^[A-Za-z]:/.test(f)) return f;
  return joinPath(dir, f);
}

const appendLog = (t: RepoTests, lines: string[]) => {
  if (!lines.length) return;
  const next = t.log.concat(lines);
  t.log = next.length > MAX_LOG ? next.slice(-MAX_LOG) : next;
};

/** Ereignisse eines Batches einarbeiten: ein neues nodes/results-Objekt je Batch. Liefert die Zahl der Ergebnisse. */
function apply(t: RepoTests, s: Step, evs: Ev[], seen: Set<string>): number {
  if (!evs.length) return 0;
  const nodes = { ...t.nodes };
  const results = { ...t.results };
  const place = placer(nodes, results, seen);
  let n = 0;
  for (const e of evs) {
    const file = toRepo(t.repo, s.dir, e.file);
    const id = place(s.fw, s.dir, file, keyOf(s.fw, e.path), e.line);
    // Befund: vitest -t meldet Tests ausserhalb des Filters als skipped; die alten Ergebnisse bleiben.
    if (!inScope(nodes, id, s.scopeId)) continue;
    if (e.e === "start") {
      results[id] = { state: "running" };
      continue;
    }
    n++;
    const stack = e.stack === undefined ? undefined : plain(e.stack);
    // JS liefert keinen Ort: der erste Stack-Eintrag in der Testdatei, sonst die Testdefinition.
    const fromStack = stackLinks(stack ?? "")
      .flatMap((x) => (x.loc ? [{ file: toRepo(t.repo, s.dir, x.loc.file) ?? "", line: x.loc.line }] : []))
      .find((l) => l.file === file);
    const at: Loc | undefined = e.at ? { file: toRepo(t.repo, s.dir, e.at.file) ?? e.at.file, line: e.at.line } : fromStack;
    results[id] = {
      state: e.state ?? "failed",
      ms: e.ms,
      msg: e.msg === undefined ? undefined : plain(e.msg),
      exp: e.exp,
      act: e.act,
      diff: e.diff === undefined ? undefined : plain(e.diff),
      stack,
      at,
      labels: e.labels,
    };
  }
  t.nodes = nodes;
  t.results = results;
  return n;
}

/** Einen Schritt ausfuehren; endet mit dem Exit. count = Zahl der Ergebnisse, -1 = Start fehlgeschlagen. */
function runStep(t: RepoTests, s: Step): Promise<{ code: number | null; count: number }> {
  return new Promise((resolve) => {
    const id = `test-${crypto.randomUUID()}`;
    t.run = t.run && { ...t.run, id };
    const st = JS_FW.includes(s.fw) ? null : parser(s.fw);
    let count = 0;
    let done = false;
    // Knoten mit Ereignis in diesem Schritt: alle anderen auf derselben Zeile sind Vorlagen (it.each, Schleifen).
    const seen = new Set<string>();
    const on = (m: TestMsg) => {
      if (done) return;
      switch (m.event) {
        case "lines": {
          const evs: Ev[] = [];
          const out: string[] = [];
          for (const l of m.data.lines) {
            const e = st ? null : jsEvent(l);
            if (e) evs.push(e);
            else out.push(l);
          }
          if (st) evs.push(...parseLines(st, m.data.lines));
          count += apply(t, s, evs, seen);
          appendLog(t, out);
          break;
        }
        case "exit":
          if (st) count += apply(t, s, flush(st), seen);
          done = true;
          resolve({ code: m.data.code, count });
          break;
        default: {
          const never: never = m;
          void never;
        }
      }
    };
    runCmd(id, t.repo, s.dir, s.fw, s.scope, on).catch((e) => {
      if (done) return;
      done = true;
      t.error = String(e);
      resolve({ code: null, count: -1 });
    });
  });
}

/** Nicht gemeldete Tests (gefiltert, abgebrochen) bekommen ihr altes Ergebnis zurueck. */
function restore(t: RepoTests, prev: Record<string, Result>, ids: string[]) {
  const r = { ...t.results };
  for (const id of ids) {
    if (r[id]?.state !== "queued" && r[id]?.state !== "running") continue;
    if (prev[id]) r[id] = prev[id];
    else delete r[id];
  }
  t.results = r;
}

/**
 * Tests ausfuehren: "all", "failed" (eine Datei je Lauf) oder ein Knoten. Fuehrt Code aus dem Projekt aus: NUR auf
 * Klick. Es laeuft hoechstens ein Prozess; mehrere Projekte nacheinander.
 */
// ponytail: seriell; paralleles Starten je Projekt erst bei Bedarf.
export async function testRun(repo: string, target: string): Promise<void> {
  const t = tests(repo);
  if (t.run) return;
  const steps = plan(t, target);
  if (!steps.length) return;
  cancelled.delete(repo);
  const started = performance.now();
  t.run = { id: "", scope: target, started };
  t.error = "";
  t.log = [];
  const prev = t.results;
  for (const s of steps) mark(t, stepIds(t, s), "queued");
  let exit: number | null = null;
  for (const s of steps) {
    if (cancelled.has(repo)) break;
    const ids = stepIds(t, s);
    mark(t, ids, "running");
    const { code, count } = await runStep(t, s);
    exit = code;
    restore(t, prev, ids);
    if (!cancelled.has(repo) && count === 0) {
      if (code !== 0) {
        t.error = `Lauf fehlgeschlagen (Exit ${code ?? "?"})`;
        t.tab = "output";
      } else if (s.scopeId) t.error = "Kein Test gefunden (Filter passt nicht?) – einmal alle ausführen";
    }
  }
  // Was nach einem Abbruch noch wartet.
  restore(t, prev, Object.keys(t.results));
  t.last = { ms: performance.now() - started, exit };
  t.run = null;
  await refresh(t);
}

/** Laufenden Lauf abbrechen (samt Prozessbaum); weitere Schritte starten nicht mehr. */
export async function testCancel(repo: string): Promise<void> {
  const t = store.get(repo);
  if (!t?.run) return;
  cancelled.add(repo);
  if (t.run.id) await cancelCmd(t.run.id).catch((e) => (t.error = String(e)));
}

/** Nach einem Lauf: Testdateien mit neuer mtime neu einlesen (ein fs.stat fuer alle). Kein Polling. */
async function refresh(t: RepoTests) {
  const times = mtimes.get(t.repo)!;
  const files = [...times.keys()];
  if (!files.length) return;
  const now = await fs.stat(t.repo, files).catch(() => null);
  if (!now) return;
  const changed = files.filter((f, i) => now[i] !== null && now[i] !== times.get(f));
  if (changed.length) await scanFiles(t, changed);
}

/** Ort aus Output oder Stack (absolut oder relativ zur cwd des Knotens) als Repo-Pfad; null = unbekannt. */
export function where(repo: string, id: string, file: string): string | null {
  const t = tests(repo);
  const n = t.nodes[id];
  return toRepo(repo, n ? (projectOf(t, n.file, n.fw)?.dir ?? "") : "", file);
}

/** Lesbare Bezeichnung fuer die Detailansicht: Pfad ohne Datei. */
export function title(t: RepoTests, id: string): string {
  const parts: string[] = [];
  for (let n: TestNode | undefined = t.nodes[id]; n && n.kind !== "file"; n = n.parent ? t.nodes[n.parent] : undefined) parts.unshift(n.name);
  return parts.join(" › ");
}
