// Run-Leiste je Projekt: erkannte Startziele und hoechstens ein Prozess (PTY). Lebt auf Modulebene wie editor():
// RunPane wird beim Andocken neu gemountet, Prozess und Ausgabe bleiben. Die Auswahl steht im Editor-Store (run).
// Invariante: laeuft etwas, ist das Panel RUN im Layout (runStart dockt es an, Schliessen/Zuruecksetzen stoppt).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { RUN, activate, groupOf, mainGroup, splitAt } from "./dock.ts";
import { fs, ptyClose, ptyWrite, runStart as startCmd } from "./files.ts";
import { serverUrl } from "./editor.logic.ts";
import { configs, markers, pickDefault, scan, type RunConfig } from "./run.logic.ts";
import { editor, setLayout } from "./editor.svelte.ts";

export type Run = { cfg: RunConfig; id: string; state: "running" | "exited" | "idle"; exit: number | null; started: number; url: string };
type Sink = (b: Uint8Array) => void;

export class RepoRun {
  readonly repo: string;
  configs = $state.raw<RunConfig[]>([]);
  /** Letzter oder laufender Prozess; idle = vom User gestoppt. */
  current = $state.raw<Run | null>(null);
  constructor(repo: string) {
    this.repo = repo;
  }
  get selected(): string | null {
    return editor(this.repo).run;
  }
}

const store = new Map<string, RepoRun>();
/** Erkennung je Projekt (einmal je Sitzung); laeuft sie noch, wartet runStart darauf. */
const detected = new Map<string, Promise<void>>();

/**
 * Rohe Ausgabe fuer das Replay beim Mount von RunPane, nicht reaktiv. ponytail: 1 MB, aelteste Chunks fallen raus.
 * tail = letzte 4 KB ohne Escape-Sequenzen fuer die URL-Erkennung, fixed = "Local"-Adresse gefunden.
 */
type Buf = { id: string; chunks: Uint8Array[]; bytes: number; sink: Sink | null; tail: string; fixed: boolean; off: Promise<UnlistenFn>[] };
const bufs = new Map<string, Buf>();
const MAX_BYTES = 1 << 20;
// RIS: setzt das angehaengte xterm vor einem neuen Lauf zurueck.
const RESET = new TextEncoder().encode("\x1bc");

function buf(repo: string): Buf {
  let b = bufs.get(repo);
  if (!b) bufs.set(repo, (b = { id: "", chunks: [], bytes: 0, sink: null, tail: "", fixed: false, off: [] }));
  return b;
}

function push(b: Buf, bytes: Uint8Array) {
  b.chunks.push(bytes);
  b.bytes += bytes.length;
  while (b.bytes > MAX_BYTES && b.chunks.length > 1) b.bytes -= b.chunks.shift()!.length;
  b.sink?.(bytes);
}

/** Zustand des Projekts, pro Repo immer dasselbe Objekt. */
export function run(repo: string): RepoRun {
  let r = store.get(repo);
  if (!r) store.set(repo, (r = new RepoRun(repo)));
  return r;
}

/** Startziele erkennen (einmal je Sitzung). Liest nur, startet nichts. */
export function detect(repo: string): Promise<void> {
  let p = detected.get(repo);
  if (!p) {
    p = scanConfigs(repo).catch((e) => {
      detected.delete(repo);
      editor(repo).error = String(e);
    });
    detected.set(repo, p);
  }
  return p;
}

async function scanConfigs(repo: string) {
  const files = await fs.files(repo);
  const ms = markers(files);
  const texts = await Promise.all(ms.map((f) => fs.read(repo, f).then((t) => t.content, () => "")));
  run(repo).configs = configs(files, Object.fromEntries(ms.map((f, i) => [f, texts[i]])));
}

/** Gewaehltes Ziel bzw. die Vorauswahl. */
export const chosen = (repo: string) => pickDefault(run(repo).configs, editor(repo).run, editor(repo).active);

/** Sink fuer RunPane setzen (null = abgehaengt); liefert die bisherige Ausgabe zum Nachspielen. */
export function attach(repo: string, sink: Sink | null): Uint8Array[] {
  const b = buf(repo);
  b.sink = sink;
  return [...b.chunks];
}

/** Ausgabe leeren ("Leeren"-Knopf). */
export function clearOutput(repo: string): void {
  const b = buf(repo);
  b.chunks = [];
  b.bytes = 0;
  b.sink?.(RESET);
}

function unlisten(b: Buf) {
  for (const p of b.off) p.then((f) => f(), () => {});
  b.off = [];
}

/**
 * Startet cfg (sonst das gewaehlte Ziel) und dockt die Ausgabe an. Fuehrt Code aus dem Projekt aus: NUR aus Klick
 * oder Taste rufen. Laeuft schon etwas, passiert nichts. Fehler beim Start landen in der Ausgabe.
 */
export async function runStart(repo: string, cfg?: RunConfig): Promise<void> {
  const r = run(repo);
  await detect(repo);
  const c = cfg ?? chosen(repo);
  const s = editor(repo);
  if (!c) {
    s.error = "Nichts zum Starten gefunden (package.json-Script, startbares .csproj/.fsproj, Cargo.toml).";
    return;
  }
  if (r.current?.state === "running") return;
  setLayout(repo, groupOf(s.layout, RUN) ? activate(s.layout, RUN) : splitAt(s.layout, RUN, mainGroup(s.layout).id, "bottom"));

  const b = buf(repo);
  // Listener eines beendeten Laufs abbauen; seine letzten Zeilen kamen bis hierher noch an.
  unlisten(b);
  const id = `run-${crypto.randomUUID()}`;
  Object.assign(b, { id, chunks: [], bytes: 0, tail: "", fixed: false });
  b.sink?.(RESET);
  const mine = () => b.id === id && r.current?.id === id;
  const dec = new TextDecoder();
  r.current = { cfg: c, id, state: "running", exit: null, started: Date.now(), url: "" };
  // Listener vor dem Start, damit kein frueher Output verloren geht.
  b.off = [
    listen<number[]>(`pty:${id}`, (e) => {
      if (!mine()) return;
      const bytes = new Uint8Array(e.payload);
      push(b, bytes);
      const chunk = dec.decode(bytes, { stream: true });
      // Cursor-Abfrage (ConPTY beim Start): mit angehaengtem xterm antwortet es selbst.
      if (!b.sink && chunk.includes("\x1b[6n")) ptyWrite(id, "\x1b[1;1R").catch(() => {});
      if (c.target.kind !== "npm" || b.fixed) return;
      b.tail = scan(b.tail, chunk);
      const local = serverUrl(b.tail, true);
      b.fixed = !!local;
      const cur = r.current!;
      // Ohne "Local"-Zeile bleibt die erste Adresse stehen.
      const url = local ?? (cur.url || serverUrl(b.tail) || "");
      if (url !== cur.url) r.current = { ...cur, url };
    }),
    listen<number | null>(`pty-exit:${id}`, (e) => {
      if (mine()) r.current = { ...r.current!, state: "exited", exit: e.payload ?? null, url: "" };
    }),
  ];
  try {
    await Promise.all(b.off);
    // Inzwischen gestoppt: gar nicht erst starten.
    if (!mine()) return;
    await startCmd(id, repo, c.dir, c.target);
  } catch (e) {
    if (!mine()) return;
    push(b, new TextEncoder().encode(`\x1b[31m${e}\x1b[0m\r\n`));
    r.current = { ...r.current!, state: "exited" };
  }
}

/** Prozess samt Kindern stoppen; die Ausgabe bleibt. Das Promise endet, wenn die Prozesse beendet sind. */
export function runStop(repo: string): Promise<void> {
  const r = store.get(repo);
  const b = bufs.get(repo);
  if (!r?.current || !b?.id) return Promise.resolve();
  const id = b.id;
  b.id = "";
  unlisten(b);
  if (r.current.state === "running") r.current = { ...r.current, state: "idle", url: "" };
  return ptyClose(id).catch(() => {});
}

/** Stoppen, warten, dasselbe Ziel neu starten (ohne Lauf: das gewaehlte). */
export async function runRestart(repo: string): Promise<void> {
  const cfg = store.get(repo)?.current?.cfg;
  await runStop(repo);
  await runStart(repo, cfg);
}

/** Eingabe aus RunPane an den laufenden Prozess. */
export function runWrite(repo: string, data: string): void {
  const cur = store.get(repo)?.current;
  if (cur?.state === "running") ptyWrite(cur.id, data).catch(() => {});
}
