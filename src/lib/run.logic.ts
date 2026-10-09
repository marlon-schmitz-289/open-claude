// Reine Entscheidungen der Run-Leiste (was laesst sich im Projekt starten), ohne Runen, damit `node --test` sie laedt.
import { LOCKFILES, ancestors, packageManager, plain } from "./editor.logic.ts";

/** Spiegel von pty::Target. */
export type Target = { kind: "npm"; pm: string; script: string } | { kind: "dotnet"; watch: boolean } | { kind: "cargo"; bin: string | null };
/** label ist eindeutig und dient als gespeicherte Auswahl. dir relativ zum Repo, "" = Projektordner. */
export type RunConfig = { label: string; dir: string; target: Target };

const dirOf = (p: string) => p.slice(0, Math.max(0, p.lastIndexOf("/")));
const at = (dir: string, f: string) => (dir ? `${dir}/${f}` : f);
const where = (dir: string) => (dir ? ` (${dir})` : "");

/** Wie name_ok in pty.rs: nur solche Namen laesst Rust in die Shell. */
export const nameOk = (s: string) => /^[A-Za-z0-9:_.-]+$/.test(s) && !s.startsWith("-");

const FIRST = ["dev", "start", "serve", "watch"];
const LIFECYCLE = new Set(["install", "uninstall", "prepare", "prepublishOnly", "publish", "pack", "version"]);

/** Alle Scripts ausser Lifecycle und pre*-/post*-Hooks; dev, start, serve, watch zuerst. */
export function npmConfigs(pkg: string, dir: string, pm: string): RunConfig[] {
  let scripts: unknown;
  try {
    scripts = JSON.parse(pkg)?.scripts;
  } catch {
    return [];
  }
  if (!scripts || typeof scripts !== "object") return [];
  const all = scripts as Record<string, unknown>;
  const hook = (n: string) => {
    const rest = n.replace(/^(pre|post)/, "");
    return LIFECYCLE.has(n) || (rest !== n && (rest in all || LIFECYCLE.has(rest)));
  };
  const names = Object.keys(all).filter((n) => typeof all[n] === "string" && nameOk(n) && !hook(n));
  const rank = (n: string) => (FIRST.includes(n) ? FIRST.indexOf(n) : FIRST.length);
  return names
    .sort((a, b) => rank(a) - rank(b))
    .map((script) => ({ label: `${pm} run ${script}${where(dir)}`, dir, target: { kind: "npm", pm, script } }));
}

/** Startbares .NET-Projekt: Exe/WinExe oder Web-/Worker-/Blazor-SDK, nie ein Testprojekt. */
export function dotnetRunnable(csproj: string): boolean {
  if (/<IsTestProject>\s*true|Microsoft\.NET\.Test\.Sdk/i.test(csproj)) return false;
  return /<OutputType>\s*(Win)?Exe\s*</i.test(csproj) || /Sdk="Microsoft\.NET\.Sdk\.(Web|Worker|BlazorWebAssembly)"/i.test(csproj);
}

/** bins = Quelldateien relativ zum Crate: src/main.rs -> bin null, src/bin/x.rs bzw. src/bin/x/main.rs -> "x". */
export function cargoConfigs(toml: string, dir: string, bins: string[]): RunConfig[] {
  if (!/^\s*\[package\]/m.test(toml)) return [];
  return bins.flatMap((f): RunConfig[] => {
    const bin = f === "src/main.rs" ? null : (/^src\/bin\/([^/]+?)(?:\.rs|\/main\.rs)$/.exec(f)?.[1] ?? undefined);
    if (bin === undefined || (bin !== null && !nameOk(bin))) return [];
    return [{ label: `cargo run${bin ? ` --bin ${bin}` : ""}${where(dir)}`, dir, target: { kind: "cargo", bin } }];
  });
}

const PKG = /(^|\/)package\.json$/;
const PROJ = /\.[cf]sproj$/;
const CARGO = /(^|\/)Cargo\.toml$/;

/** ponytail: hoechstens 200 Marker lesen; reicht fuer Monorepos, sonst Obergrenze hochsetzen. */
export const markers = (files: string[]) =>
  files.filter((f) => !/(^|\/)node_modules\//.test(f) && (PKG.test(f) || PROJ.test(f) || CARGO.test(f))).slice(0, 200);

/** Alle Startziele aus Dateiliste (fs.files) und gelesenen Markern (Pfad -> Inhalt). Reihenfolge: npm, dotnet, cargo. */
export function configs(files: string[], texts: Record<string, string>): RunConfig[] {
  const has = new Set(files);
  const npm = Object.keys(texts)
    .filter((f) => PKG.test(f))
    .flatMap((f) => {
      const dir = dirOf(f);
      // Paketmanager nach dem naechsten Lockfile ab dem Paket aufwaerts (Monorepo: Lockfile an der Wurzel).
      const lockDir = ancestors(f).find((d) => LOCKFILES.some((l) => has.has(at(d, l))));
      const present = lockDir === undefined ? [] : LOCKFILES.filter((l) => has.has(at(lockDir, l)));
      return npmConfigs(texts[f], dir, packageManager(texts[f], present));
    });
  // Rust nimmt die eine Projektdatei im Ordner; Ordner mit mehreren fallen weg.
  const projs = Object.keys(texts).filter((f) => PROJ.test(f));
  const dotnet = projs
    .filter((f) => projs.filter((g) => dirOf(g) === dirOf(f)).length === 1 && dotnetRunnable(texts[f]))
    .flatMap((f): RunConfig[] => {
      const dir = dirOf(f);
      const mk = (watch: boolean): RunConfig => ({ label: `dotnet ${watch ? "watch" : "run"}${where(dir)}`, dir, target: { kind: "dotnet", watch } });
      return [mk(false), mk(true)];
    });
  const cargo = Object.keys(texts)
    .filter((f) => CARGO.test(f))
    .flatMap((f) => {
      const dir = dirOf(f);
      const pre = dir ? `${dir}/` : "";
      const bins = files.filter((x) => x.startsWith(pre)).map((x) => x.slice(pre.length));
      return cargoConfigs(texts[f], dir, bins);
    });
  return [...npm, ...dotnet, ...cargo];
}

/** Dev-Server fuer eine Datei: die naechste package.json darueber mit dev, start oder serve (Monorepo: das Paket der Datei). */
export function devConfig(cfgs: RunConfig[], path: string): RunConfig | null {
  for (const dir of ancestors(path)) {
    const c = cfgs.find((c) => c.dir === dir && c.target.kind === "npm" && ["dev", "start", "serve"].includes(c.target.script));
    if (c) return c;
  }
  return null;
}

/** Vorauswahl: zuletzt gewaehlt, sonst Dev-Server zur aktiven Datei, sonst erstes dotnet-, cargo-, irgendein Ziel. */
export function pickDefault(cfgs: RunConfig[], last: string | null, activeFile: string | null): RunConfig | null {
  return (
    cfgs.find((c) => c.label === last) ??
    devConfig(cfgs, activeFile ?? "") ??
    cfgs.find((c) => c.target.kind === "dotnet") ??
    cfgs.find((c) => c.target.kind === "cargo") ??
    cfgs[0] ??
    null
  );
}

/**
 * Letzte 4 KB des Outputs ohne Escape-Sequenzen fuer die URL-Erkennung. Eine am Chunk-Ende abgeschnittene Sequenz
 * bleibt in tail stehen und wird mit dem naechsten Chunk entfernt; Aufwand O(Chunk + 4 KB).
 */
export const scan = (tail: string, chunk: string) => plain(tail + chunk).slice(-4096);

/** Spiegel von pty::Profile; name ist eindeutig und dient als gespeicherte Auswahl. */
export type Profile = { name: string; release: boolean; args: string[] };
export const BUILTIN: Profile[] = [
  { name: "Debug", release: false, args: [] },
  { name: "Release", release: true, args: [] },
];

/** Wie arg_ok in pty.rs: Flags und Werte ohne Leerzeichen, Quotes oder Shell-Zeichen. */
export const argOk = (s: string) => /^[A-Za-z0-9_.:=/@+,-]+$/.test(s);

/** Argumente aus dem Eingabefeld; null, wenn eines nicht erlaubt ist. */
export function parseArgs(text: string): string[] | null {
  const args = text.split(/\s+/).filter(Boolean);
  return args.every(argOk) ? args : null;
}

/** Gewaehltes Profil, sonst Debug. */
export const profileOf = (custom: Profile[], name: string): Profile => [...BUILTIN, ...custom].find((p) => p.name === name) ?? BUILTIN[0];
