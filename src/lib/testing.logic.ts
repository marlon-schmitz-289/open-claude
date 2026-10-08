// Reine Logik des Test-Explorers: Projekte erkennen, Tests statisch finden, Ergebnisse zuordnen, Text-Output parsen,
// Baumzeilen bauen. Ohne Runen und Tauri, damit `node --test` sie direkt laedt. Pfade relativ zum Repo mit "/".
import { fuzzy } from "./fuzzy.ts";

export type Fw = "vitest" | "jest" | "node" | "cargo" | "dotnet" | "pytest";
/** Projekt: Testdateien unter dir (cwd des Laufs). JS: fw ist die Vorgabe des Pakets, "node" = nur per Import. */
export type TestProject = { fw: Fw; dir: string; files: string[]; note?: string };
export type TestNode = {
  /** fw|file|key, bleibt ueber Laeufe gleich. */
  id: string;
  kind: "file" | "suite" | "test";
  name: string;
  file: string;
  line: number | null;
  fw: Fw;
  /** Match-Schluessel: JS "a > b", cargo "mod::fn", dotnet "Ns.Class.Method", pytest "datei::Class::test". */
  key: string;
  parent: string | null;
  children: string[];
};
export type Tree = Record<string, TestNode>;
export type St = "idle" | "queued" | "running" | "passed" | "failed" | "skipped";
export type Loc = { file: string; line: number };
/** Reporter-Protokoll ("@@ocui <json>") und Ergebnis der Text-Parser. file: JS absolut, sonst relativ zur cwd, "" = unbekannt. */
export type Ev = {
  e: "start" | "done";
  file: string;
  path: string[];
  line?: number;
  state?: "passed" | "failed" | "skipped";
  ms?: number;
  msg?: string;
  exp?: string;
  act?: string;
  diff?: string;
  stack?: string;
  at?: Loc;
  labels?: [string, string];
};
export type Result = {
  state: St;
  ms?: number;
  msg?: string;
  exp?: string;
  act?: string;
  diff?: string;
  stack?: string;
  at?: Loc;
  /** Beschriftung von exp/act, sonst "Erwartet"/"Tatsaechlich" (cargo: right/left). */
  labels?: [string, string];
};

export const JS_FW: readonly Fw[] = ["vitest", "jest", "node"];
const SEP: Record<Fw, string> = { vitest: " > ", jest: " > ", node: " > ", cargo: "::", dotnet: ".", pytest: "::" };
export const keyOf = (fw: Fw, path: string[]) => path.join(SEP[fw]);
export const nodeId = (fw: Fw, file: string, key: string) => `${fw}|${file}|${key}`;

export const dirOf = (p: string) => p.slice(0, Math.max(0, p.lastIndexOf("/")));
const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
const under = (p: string, dir: string) => !dir || p.startsWith(`${dir}/`);
/** p relativ zu dir (p liegt darunter). */
export const relTo = (dir: string, p: string) => (dir ? p.slice(dir.length + 1) : p);
export const joinPath = (dir: string, p: string) => (dir ? `${dir}/${p}` : p);

// --- Erkennung ---

const JS_TEST = /(^|\/)__tests__\/.+\.[cm]?[jt]sx?$|\.(test|spec)\.[cm]?[jt]sx?$/;
const DOTNET_TEST = /Microsoft\.NET\.Test\.Sdk|xunit|NUnit|MSTest/i;
const isMarker = (f: string) =>
  !f.includes("node_modules/") && (/(^|\/)(package\.json|global\.json|pyproject\.toml)$/.test(f) || /\.[cf]sproj$/.test(f));

/** Dateien, deren Inhalt projects() braucht (package.json, csproj, global.json, pyproject.toml). */
export const markers = (files: string[]) => files.filter(isMarker);

/** Naechster Ordner aus dirs ueber der Datei; undefined = keiner. */
function nearest(dirs: Set<string>, file: string): string | undefined {
  for (let d = dirOf(file); ; d = dirOf(d)) {
    if (dirs.has(d)) return d;
    if (!d) return undefined;
  }
}

/** Nur die obersten Ordner (Workspace-Wurzel), verschachtelte fallen weg. */
const topmost = (dirs: string[]) => dirs.filter((d) => !dirs.some((o) => o !== d && (o === "" || d.startsWith(`${o}/`))));

/** Testprojekte aus der Dateiliste (git ls-files) und den Inhalten der markers(). */
export function projects(files: string[], texts: Record<string, string>): TestProject[] {
  const out: TestProject[] = [];
  // all: alle Ordner dieser Art (JS: auch Pakete mit anderer Vorgabe), damit eine Datei nur zum naechsten gehoert.
  const group = (fw: Fw, dirs: string[], match: (f: string) => boolean, extra?: (dir: string) => Partial<TestProject>, all = dirs) => {
    const set = new Set(all);
    const by = new Map<string, string[]>();
    for (const f of files) {
      if (f.includes("node_modules/") || !match(f)) continue;
      const d = nearest(set, f);
      if (d === undefined) continue;
      if (!by.has(d)) by.set(d, []);
      by.get(d)!.push(f);
    }
    for (const dir of dirs) if (by.has(dir)) out.push({ fw, dir, files: by.get(dir)!, ...extra?.(dir) });
  };
  const at = (re: RegExp) => files.filter((f) => !f.includes("node_modules/") && re.test(f));

  // JS: je package.json; Vorgabe vitest vor jest, sonst nur per Import (node:test).
  const fwOf = (pkg: string): Fw => {
    try {
      const j = JSON.parse(texts[pkg] ?? "{}");
      const deps = { ...j?.dependencies, ...j?.devDependencies };
      return "vitest" in deps ? "vitest" : "jest" in deps ? "jest" : "node";
    } catch {
      return "node"; // Kaputtes JSON: nur per Import.
    }
  };
  for (const fw of JS_FW) {
    const pkgs = at(/(^|\/)package\.json$/).filter((p) => fwOf(p) === fw);
    group(fw, pkgs.map(dirOf), (f) => JS_TEST.test(f), undefined, at(/(^|\/)package\.json$/).map(dirOf));
  }

  group("cargo", topmost(at(/(^|\/)Cargo\.toml$/).map(dirOf)), (f) => f.endsWith(".rs"));

  const mtp = Object.entries(texts).some(([f, t]) => base(f) === "global.json" && t.includes("Microsoft.Testing.Platform"));
  const csproj = at(/\.[cf]sproj$/).filter((f) => DOTNET_TEST.test(texts[f] ?? ""));
  // ponytail: MTP (dotnet test ueber Microsoft.Testing.Platform) wird nur markiert; Upgrade: --project + --report-trx.
  group("dotnet", csproj.map(dirOf), (f) => f.endsWith(".cs"), () => (mtp ? { note: "MTP noch nicht unterstützt" } : {}));

  const py = [
    ...at(/(^|\/)(pytest\.ini|conftest\.py)$/),
    ...at(/(^|\/)pyproject\.toml$/).filter((f) => (texts[f] ?? "").includes("[tool.pytest")),
  ].map(dirOf);
  group("pytest", topmost([...new Set(py)]), (f) => /(^|\/)(test_[^/]*|[^/]*_test)\.py$/.test(f));
  return out;
}

/** Framework einer JS-Testdatei: Import vor Paket-Vorgabe; null = keine Testdatei fuer uns. */
export function jsFw(text: string, fallback: Fw): Fw | null {
  if (/from\s*['"]vitest['"]/.test(text)) return "vitest";
  if (/['"]node:test['"]/.test(text)) return "node";
  return fallback === "vitest" || fallback === "jest" ? fallback : null;
}

// --- Statische Discovery ---

/** Rust-Modulpfad einer Datei relativ zum Crate: src/a/b.rs -> "a::b"; Crate-Wurzeln, bin/ und tests/ -> "". */
export function modPath(rel: string): string {
  const m = /^src\/(.+)\.rs$/.exec(rel);
  if (!m || /^(lib|main)$/.test(m[1]) || m[1].startsWith("bin/")) return "";
  return m[1].replace(/\/mod$/, "").replaceAll("/", "::");
}

const JS_RE = /^(\s*)(describe|suite|it|test)(?:\.(?:only|skip|todo|concurrent|each\([^)]*\)))*\s*\(\s*(['"`])(.*?)\3/;
const RS_ATTR = /^\s*#\[(?:tokio::test|test|rstest)\b/;
const RS_FN = /\bfn\s+(\w+)/;
const RS_MOD = /^(\s*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{/;
const CS_NS = /^\s*namespace\s+([\w.]+)/;
const CS_CLASS = /\bclass\s+(\w+)/;
const CS_ATTR = /^\s*\[(?:Fact|Theory|Test|TestCase|TestMethod|DataTestMethod)\b/;
const CS_METHOD = /(\w+)\s*\(/;
const PY_CLASS = /^class\s+(Test\w+)/;
const PY_DEF = /^(\s*)(?:async\s+)?def\s+(test_\w+)/;

/**
 * Tests einer Datei aus dem Quelltext, ohne Build: Datei-Knoten zuerst, dann Suites und Tests in Quellreihenfolge.
 * dir = Ordner, auf den sich Modulpfad (cargo: Crate) bzw. pytest-nodeid beziehen.
 */
export function discover(fw: Fw, file: string, text: string, dir = ""): TestNode[] {
  const fileId = nodeId(fw, file, "");
  const nodes: TestNode[] = [{ id: fileId, kind: "file", name: file, file, line: null, fw, key: "", parent: null, children: [] }];
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const add = (kind: "suite" | "test", key: string, name: string, line: number, parent: string) => {
    const id = nodeId(fw, file, key);
    if (byId.has(id)) return id;
    const n: TestNode = { id, kind, name, file, line, fw, key, parent, children: [] };
    byId.set(id, n);
    nodes.push(n);
    byId.get(parent)!.children.push(id);
    return id;
  };
  const lines = text.split("\n");

  if (JS_FW.includes(fw)) {
    // ponytail: Verschachtelung ueber die Einrueckung; bei unformatiertem Code wird der Baum flach. Upgrade: Lezer-Baum.
    const stack: { indent: number; id: string; path: string[] }[] = [];
    lines.forEach((l, i) => {
      const m = JS_RE.exec(l);
      if (!m) return;
      const indent = m[1].length;
      while (stack.length && stack.at(-1)!.indent >= indent) stack.pop();
      const top = stack.at(-1);
      const path = [...(top?.path ?? []), m[4]];
      const kind = m[2] === "describe" || m[2] === "suite" ? "suite" : "test";
      const id = add(kind, keyOf(fw, path), m[4], i + 1, top?.id ?? fileId);
      // Auch Tests: node:test erlaubt Tests in Tests.
      stack.push({ indent, id, path });
    });
  } else if (fw === "cargo") {
    const prefix = modPath(relTo(dir, file));
    const mods: { indent: number; name: string; line: number }[] = [];
    let pending = 0;
    lines.forEach((l, i) => {
      const indent = l.length - l.trimStart().length;
      const t = l.trim();
      // Nur "}" und Geschwister-mods schliessen: Rohstrings ab Spalte 0 sollen das Modul nicht beenden.
      while (mods.length && indent <= mods.at(-1)!.indent && (t.startsWith("}") || RS_MOD.test(l))) mods.pop();
      const mod = RS_MOD.exec(l);
      if (mod) mods.push({ indent, name: mod[2], line: i + 1 });
      if (RS_ATTR.test(l)) pending = 4;
      const fn = pending > 0 && !t.startsWith("#") ? RS_FN.exec(l) : null;
      if (pending > 0) pending--;
      if (!fn) return;
      pending = 0;
      let parent = fileId;
      const path = prefix ? [prefix] : [];
      for (const m of mods) {
        path.push(m.name);
        parent = add("suite", path.join("::"), m.name, m.line, parent);
      }
      add("test", [...path, fn[1]].join("::"), fn[1], i + 1, parent);
    });
  } else if (fw === "dotnet") {
    // ponytail: verschachtelte Klassen werden ignoriert (die innere gilt).
    let ns = "";
    let cls: { name: string; line: number } | null = null;
    let pending = false;
    lines.forEach((l, i) => {
      const n = CS_NS.exec(l);
      if (n) ns = n[1];
      const c = CS_CLASS.exec(l);
      if (c && !pending) cls = { name: c[1], line: i + 1 };
      if (CS_ATTR.test(l)) return void (pending = true);
      if (!pending || l.trim().startsWith("[") || !l.trim()) return;
      pending = false;
      const m = CS_METHOD.exec(l);
      if (!m) return;
      const owner = [ns, cls?.name].filter(Boolean).join(".");
      const parent = cls ? add("suite", owner, cls.name, cls.line, fileId) : fileId;
      add("test", [owner, m[1]].filter(Boolean).join("."), m[1], i + 1, parent);
    });
  } else if (fw === "pytest") {
    const rel = relTo(dir, file);
    let cls: { name: string; line: number } | null = null;
    lines.forEach((l, i) => {
      const c = PY_CLASS.exec(l);
      if (c) return void (cls = { name: c[1], line: i + 1 });
      // Alles andere in Spalte 0 beendet die Klasse.
      if (/^\S/.test(l)) cls = null;
      const d = PY_DEF.exec(l);
      if (!d) return;
      const inClass = cls && d[1].length > 0;
      const parent = inClass ? add("suite", `${rel}::${cls!.name}`, cls!.name, cls!.line, fileId) : fileId;
      add("test", [rel, inClass ? cls!.name : "", d[2]].filter(Boolean).join("::"), d[2], i + 1, parent);
    });
  }
  return nodes;
}

/** Knoten der Dateien durch neu gefundene ersetzen (Kopie). Ergebnisse haengen an der id und bleiben. */
export function replaceFiles(nodes: Tree, files: string[], found: TestNode[]): Tree {
  const drop = new Set(files);
  const next: Tree = {};
  for (const [id, n] of Object.entries(nodes)) if (!drop.has(n.file)) next[id] = n;
  // Gelernter Key (cargo #[path]-Module, s. placer) ueberlebt das Neueinlesen: die id bleibt gleich.
  for (const n of found) next[n.id] = nodes[n.id] && nodes[n.id].key !== n.key ? { ...n, key: nodes[n.id].key } : n;
  return next;
}

// --- Ergebnisse zuordnen ---

/** id liegt unter scope (oder ist es); scope null = alles. */
export function inScope(nodes: Tree, id: string, scope: string | null): boolean {
  for (let n: TestNode | undefined = nodes[id]; n; n = n.parent ? nodes[n.parent] : undefined) if (n.id === scope) return true;
  return scope === null;
}

const lastSeg = (fw: Fw, key: string) => key.split(SEP[fw]).at(-1)!;
/** Parameter abschneiden: dotnet "M(x: 1)", pytest "t[1]". */
const stripParams = (fw: Fw, key: string) =>
  fw === "dotnet" ? key.replace(/\(.*\)$/, "") : fw === "pytest" ? key.replace(/\[.*\]$/, "") : key;

/**
 * Zuordner fuer einen Batch. `nodes` ist die Kopie des Aufrufers und wird veraendert (neue Knoten, nachgefuehrte Keys).
 * Liefert je Ergebnis die Knoten-id:
 *  1. exakt fw|file|key, sonst eindeutig gleicher key im Projekt (cargo meldet die Crate-Wurzel als Datei)
 *  2. Parameter-Variante (Theory, pytest-param): Kind unter dem Basistest
 *  3. eindeutig ueber das letzte Key-Segment (cargo #[path]-Module); der Key wird nachgefuehrt
 *  4. sonst neuer Knoten unter Suite/Datei bzw. "(<fw>)"; ein nie gelaufener statischer Geschwister mit
 *     derselben Zeile (Vorlage von it.each) faellt dann weg.
 */
export function placer(nodes: Tree, results: Record<string, Result>, seen = new Set<string>()) {
  let index: Map<string, string[]> | null = null;
  const idx = () => {
    if (index) return index;
    index = new Map();
    for (const n of Object.values(nodes)) if (n.kind !== "file") remember(n);
    return index;
  };
  const push = (k: string, id: string) => {
    if (!index!.has(k)) index!.set(k, []);
    index!.get(k)!.push(id);
  };
  const remember = (n: TestNode) => {
    push(`k|${n.fw}|${n.key}`, n.id);
    push(`s|${n.fw}|${lastSeg(n.fw, n.key)}`, n.id);
  };
  const put = (n: TestNode) => {
    nodes[n.id] = n;
    if (index && n.kind !== "file") remember(n);
    if (n.parent) nodes[n.parent] = { ...nodes[n.parent], children: [...nodes[n.parent].children, n.id] };
  };
  const one = (ids: string[] | undefined, dir: string, file: string | null) => {
    const live = (ids ?? []).filter((id) => nodes[id] && under(nodes[id].file, dir));
    const same = live.filter((id) => nodes[id].file === file);
    return same.length === 1 ? same[0] : live.length === 1 ? live[0] : undefined;
  };
  const find = (fw: Fw, dir: string, file: string | null, key: string): string | undefined => {
    if (file && nodes[nodeId(fw, file, key)]) return nodeId(fw, file, key);
    // JS kennt die Datei immer genau: kein Raten ueber andere Dateien.
    if (JS_FW.includes(fw)) return undefined;
    const exact = one(idx().get(`k|${fw}|${key}`), dir, file);
    if (exact) return exact;
    const seg = one(idx().get(`s|${fw}|${lastSeg(fw, key)}`), dir, file);
    if (!seg || nodes[seg].kind !== "test") return undefined;
    nodes[seg] = { ...nodes[seg], key };
    remember(nodes[seg]);
    return seg;
  };
  const pseudo = (fw: Fw, dir: string) => {
    const file = joinPath(dir, `(${fw})`);
    const id = nodeId(fw, file, "");
    if (!nodes[id]) put({ id, kind: "file", name: `(${fw})`, file, line: null, fw, key: "", parent: null, children: [] });
    return id;
  };

  const place = (fw: Fw, dir: string, file: string | null, key: string, line?: number): string => {
    if (!key) {
      if (!file) return pseudo(fw, dir);
      const id = nodeId(fw, file, "");
      if (!nodes[id]) put({ id, kind: "file", name: file, file, line: null, fw, key: "", parent: null, children: [] });
      return id;
    }
    const hit = find(fw, dir, file, key);
    if (hit) return hit;
    const stripped = stripParams(fw, key);
    const owner = stripped !== key ? find(fw, dir, file, stripped) : undefined;
    if (owner) {
      const o = nodes[owner];
      const id = nodeId(fw, o.file, key);
      if (!nodes[id]) put({ id, kind: "test", name: o.name + key.slice(stripped.length), file: o.file, line: o.line, fw, key, parent: owner, children: [] });
      return id;
    }
    const seg = JS_FW.includes(fw) ? key.split(SEP[fw]) : [key];
    const suite = file && seg.length > 1 ? nodes[nodeId(fw, file, seg.slice(0, -1).join(SEP[fw]))] : undefined;
    const fileId = file ? nodeId(fw, file, "") : "";
    if (file && !nodes[fileId]) put({ id: fileId, kind: "file", name: file, file, line: null, fw, key: "", parent: null, children: [] });
    const fileNode = nodes[fileId];
    const parent = suite?.id ?? fileNode?.id ?? pseudo(fw, dir);
    const id = nodeId(fw, nodes[parent].file, key);
    put({ id, kind: "test", name: parent === fileNode?.id || suite ? seg.at(-1)! : key, file: nodes[parent].file, line: line ?? null, fw, key, parent, children: [] });
    if (line !== undefined) {
      const template = nodes[parent].children.find((c) => {
        const n = nodes[c];
        const st = results[c]?.state;
        return c !== id && n.kind === "test" && n.line === line && !n.children.length && !seen.has(c) && (!st || st === "queued" || st === "running");
      });
      if (template) {
        delete nodes[template];
        nodes[parent] = { ...nodes[parent], children: nodes[parent].children.filter((c) => c !== template) };
      }
    }
    return id;
  };
  return (fw: Fw, dir: string, file: string | null, key: string, line?: number): string => {
    const id = place(fw, dir, file, key, line);
    seen.add(id);
    return id;
  };
}

// --- Status ---

const RANK: Record<St, number> = { idle: 0, skipped: 1, passed: 2, queued: 3, running: 4, failed: 5 };

/** Status je Knoten: eigenes Ergebnis und Kinder, failed > running > queued > passed > skipped > idle. O(n). */
export function aggregate(nodes: Tree, results: Record<string, Result>): Record<string, St> {
  const out: Record<string, St> = {};
  const visit = (id: string): St => {
    let s = results[id]?.state ?? "idle";
    for (const c of nodes[id]?.children ?? []) {
      const cs = visit(c);
      if (RANK[cs] > RANK[s]) s = cs;
    }
    out[id] = s;
    return s;
  };
  for (const n of Object.values(nodes)) if (n.parent === null) visit(n.id);
  return out;
}

/** Alle Knoten unter id (inklusive). */
export function subtree(nodes: Tree, id: string): string[] {
  const out: string[] = [];
  const walk = (i: string) => {
    if (!nodes[i]) return;
    out.push(i);
    nodes[i].children.forEach(walk);
  };
  walk(id);
  return out;
}

export type Row = { id: string; depth: number; open: boolean; kids: boolean };

/** Sichtbare Zeilen wie tree.rows(): Dateien sortiert, darunter Quellreihenfolge. Filter (fuzzy) und Fehler-Filter klappen auf. */
export function rows(nodes: Tree, states: Record<string, St>, expanded: string[], filter: string, failedOnly: boolean): Row[] {
  const open = new Set(expanded);
  let keep: Set<string> | null = null;
  if (filter.trim()) {
    keep = new Set();
    for (const n of Object.values(nodes)) {
      if (!fuzzy(n.name, filter)) continue;
      for (let p: TestNode | undefined = n; p && !keep.has(p.id); p = p.parent ? nodes[p.parent] : undefined) keep.add(p.id);
    }
  }
  const show = (id: string) => (!keep || keep.has(id)) && (!failedOnly || states[id] === "failed");
  const forced = !!keep || failedOnly;
  const out: Row[] = [];
  const walk = (id: string, depth: number) => {
    if (!show(id)) return;
    const n = nodes[id];
    const o = n.children.length > 0 && (forced || open.has(id));
    out.push({ id, depth, open: o, kids: n.children.length > 0 });
    if (o) for (const c of n.children) walk(c, depth + 1);
  };
  const roots = Object.values(nodes).filter((n) => n.parent === null);
  roots.sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : 0));
  for (const r of roots) walk(r.id, 0);
  return out;
}

// --- Reporter-Protokoll und Text-Parser ---

const PREFIX = "@@ocui ";
const isStr = (v: unknown): v is string => typeof v === "string";
const optStr = (v: unknown) => v === undefined || isStr(v);
const optNum = (v: unknown) => v === undefined || typeof v === "number";

/** Zeile eines JS-Reporters; null = normaler Output (oder kaputt). */
export function jsEvent(line: string): Ev | null {
  if (!line.startsWith(PREFIX)) return null;
  let v: unknown;
  try {
    v = JSON.parse(line.slice(PREFIX.length));
  } catch {
    return null;
  }
  if (typeof v !== "object" || v === null) return null;
  const o = v as Record<string, unknown>;
  const ok =
    (o.e === "start" || o.e === "done") &&
    isStr(o.file) &&
    Array.isArray(o.path) &&
    o.path.every(isStr) &&
    (o.state === undefined || o.state === "passed" || o.state === "failed" || o.state === "skipped") &&
    optNum(o.line) &&
    optNum(o.ms) &&
    [o.msg, o.exp, o.act, o.diff, o.stack].every(optStr);
  return ok ? (o as Ev) : null;
}

type Block = { key: string; ms?: number; mode: "msg" | "stack" | "out" | null; lines: string[]; msg: string[]; stack: string[] };
/** Zustand ueber Batches: Fehlerbloecke koennen ueber mehrere Batches gehen. */
export type ParseState = { fw: Fw; file: string; block: Block | null };
export const parser = (fw: Fw): ParseState => ({ fw, file: "", block: null });

const CARGO_RUN = /^\s+Running (?:unittests )?(\S+) \(/;
const CARGO_DOC = /^\s+Doc-tests /;
const CARGO_TEST = /^test (\S+) \.\.\. (ok|FAILED|ignored)/;
const CARGO_BLOCK = /^---- (\S+) (?:stdout|stderr) ----$/;
const CARGO_PANIC = /panicked at ([^\n]+?):(\d+):(\d+):/;
const DN_RESULT = /^\s{2}(Passed|Failed|Skipped) (.+?)(?: \[(<\s*)?([\d.]+)\s*(ms|s|m)\])?$/;
const PY_RESULT = /^(\S+::\S.*?) (PASSED|FAILED|SKIPPED|XFAIL|XPASS|ERROR)\b/;
const PY_SUMMARY = /^(FAILED|ERROR) (\S+?)(?: - (.*))?$/;

const grab = (re: RegExp, s: string) => re.exec(s)?.[1];

function cargoBlock(st: ParseState, b: Block): Ev {
  const at = b.lines.findIndex((l) => CARGO_PANIC.test(l));
  const p = at >= 0 ? CARGO_PANIC.exec(b.lines[at]) : null;
  const tail = at >= 0 ? b.lines.slice(at + 1) : b.lines;
  const end = tail.findIndex((l) => l.startsWith("note:"));
  const body = end >= 0 ? tail.slice(0, end) : tail;
  const text = b.lines.join("\n").trim();
  const msg = body.filter((l) => !/^\s*(left|right):/.test(l)).join("\n").trim();
  return {
    e: "done",
    file: st.file,
    path: [b.key],
    state: "failed",
    msg: msg || text,
    act: grab(/^\s*left: (.*)$/m, text),
    exp: grab(/^\s*right: (.*)$/m, text),
    labels: ["right", "left"],
    stack: text,
    at: p ? { file: p[1], line: Number(p[2]) } : undefined,
  };
}

function dotnetBlock(b: Block): Ev {
  const msg = b.msg.map((l) => l.replace(/^ {3}/, "")).join("\n").trim();
  const ms = /Expected:<(.*)>\. Actual:<(.*)>\./.exec(msg);
  const loc = b.stack.map((l) => / in (.+):line (\d+)/.exec(l)).find(Boolean);
  return {
    e: "done",
    file: "",
    path: [b.key],
    state: "failed",
    ms: b.ms,
    msg,
    exp: ms?.[1] ?? grab(/^\s*Expected:\s*(.*)$/m, msg),
    act: ms?.[2] ?? grab(/^\s*(?:Actual|But was):\s*(.*)$/m, msg),
    stack: b.stack.join("\n").trim(),
    at: loc ? { file: loc[1], line: Number(loc[2]) } : undefined,
  };
}

/** Offenen Block abschliessen (Ende des Laufs oder Blockende). */
export function flush(st: ParseState): Ev[] {
  const b = st.block;
  st.block = null;
  if (!b) return [];
  return [st.fw === "cargo" ? cargoBlock(st, b) : dotnetBlock(b)];
}

/** Text-Output von cargo, dotnet und pytest auswerten. Zeilen ohne Bedeutung liefern nichts. */
export function parseLines(st: ParseState, lines: string[]): Ev[] {
  const out: Ev[] = [];
  for (const raw of lines) {
    const l = raw.replace(/\r$/, "");
    if (st.fw === "cargo") {
      if (st.block) {
        if (CARGO_BLOCK.test(l) || l === "failures:" || l.startsWith("test result:")) out.push(...flush(st));
        else {
          st.block.lines.push(l);
          continue;
        }
      }
      const run = CARGO_RUN.exec(l);
      if (run) st.file = run[1];
      else if (CARGO_DOC.test(l)) st.file = "";
      const t = CARGO_TEST.exec(l);
      if (t) out.push({ e: "done", file: st.file, path: [t[1]], state: t[2] === "ok" ? "passed" : t[2] === "FAILED" ? "failed" : "skipped" });
      const b = CARGO_BLOCK.exec(l);
      if (b) st.block = { key: b[1], mode: null, lines: [], msg: [], stack: [] };
    } else if (st.fw === "dotnet") {
      if (l.startsWith("[xUnit.net")) continue;
      const r = DN_RESULT.exec(l);
      if (r || /^Test Run|^Total tests/.test(l)) out.push(...flush(st));
      if (r) {
        const n = Number(r[4]);
        const ms = r[4] === undefined ? undefined : r[5] === "s" ? n * 1000 : r[5] === "m" ? n * 60000 : n;
        if (r[1] === "Failed") st.block = { key: r[2], ms, mode: null, lines: [], msg: [], stack: [] };
        else out.push({ e: "done", file: "", path: [r[2]], state: r[1] === "Passed" ? "passed" : "skipped", ms });
        continue;
      }
      const b = st.block;
      if (!b) continue;
      if (/^\s*Error Message:\s*$/.test(l)) b.mode = "msg";
      else if (/^\s*Stack Trace:\s*$/.test(l)) b.mode = "stack";
      else if (/^\s*Standard (Output|Error) Messages:\s*$/.test(l)) b.mode = "out";
      else if (b.mode === "msg") b.msg.push(l);
      else if (b.mode === "stack") b.stack.push(l);
    } else if (st.fw === "pytest") {
      const r = PY_RESULT.exec(l);
      if (r) {
        const state = r[2] === "PASSED" || r[2] === "XFAIL" || r[2] === "XPASS" ? "passed" : r[2] === "SKIPPED" ? "skipped" : "failed";
        out.push({ e: "done", file: r[1].split("::")[0], path: [r[1]], state });
        continue;
      }
      const s = PY_SUMMARY.exec(l);
      if (!s) continue;
      // Ohne "::": Datei laesst sich nicht sammeln (Importfehler).
      const whole = !s[2].includes("::");
      out.push({ e: "done", file: s[2].split("::")[0], path: whole ? [] : [s[2]], state: "failed", msg: s[3] ?? s[1] });
    }
  }
  return out;
}

// --- Editor ---

export type Mark = { line: number; id: string; state: St };

/** Gutter-Marken fuer eine Datei: jede Suite/jeder Test mit Zeile, Parameter-Kinder ausgenommen. */
export function marks(nodes: Tree, states: Record<string, St>, file: string): Mark[] {
  const out: Mark[] = [];
  for (const n of Object.values(nodes)) {
    if (n.file !== file || n.kind === "file" || n.line === null) continue;
    if (n.parent && nodes[n.parent]?.kind === "test") continue;
    out.push({ line: n.line, id: n.id, state: states[n.id] ?? "idle" });
  }
  return out.sort((a, b) => a.line - b.line);
}

/** Fehler als Diagnostics fuer eine Datei: an der Fehlerzeile, sonst an der Testdefinition. */
export function fails(nodes: Tree, results: Record<string, Result>, file: string): { line: number; msg: string }[] {
  const out: { line: number; msg: string }[] = [];
  for (const [id, r] of Object.entries(results)) {
    if (r.state !== "failed") continue;
    const n = nodes[id];
    const line = r.at?.file === file ? r.at.line : n?.file === file ? n.line : null;
    if (line) out.push({ line, msg: r.msg || "Test fehlgeschlagen" });
  }
  return out;
}

/** Klickbare Orte in einem Stack: "pfad.ext:zeile" bzw. dotnet "pfad.ext:line n" (file://-Praefix entfernt). */
export function stackLinks(text: string): { text: string; loc?: Loc }[] {
  const out: { text: string; loc?: Loc }[] = [];
  const re = /((?:file:\/\/)?[^\s()'"]+\.\w+):(?:line )?(\d+)/g;
  let last = 0;
  for (const m of text.matchAll(re)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    out.push({ text: m[0], loc: { file: decodeURI(m[1].replace(/^file:\/\//, "")), line: Number(m[2]) } });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}
