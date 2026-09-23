// Fuzz-Tests fuer hunkPatch/linesPatch gegen echtes git: zufaellige Aenderungen (Umlaute, CRLF, ohne Newline
// am Ende, neue/geloeschte Dateien), beliebige Zeilenauswahl; das Ergebnis muss exakt der Auswahl entsprechen.
// Runden: OCUI_FUZZ_ROUNDS (Standard 1, je autocrlf=false/true).
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseDiff, hunkPatch, linesPatch, type DiffFile, type Hunk } from "./diff.ts";

let seed = 1;
const ri = (n: number) => Math.floor(((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x80000000) * n);
const pick = <T>(a: T[]) => a[ri(a.length)];

// Inhalte, die wie Diff-Syntax aussehen, sind Absicht.
const POOL = ["a", "b", "c", "same", "same", "ä ö ü ß", "€ ✓ 日本", "diff --git a/x b/x", "--- a/x", "+++ b/x",
  "@@ -1 +1 @@", "\\ No newline at end of file", "", " lead", "trail ", "\tx", "-", "+", "x\ry"];

/** Erwarteter Inhalt der Basisseite (vorwaerts: alt, rueckwaerts: neu) nach Anwenden der Auswahl. */
function expected(base: string, h: Hunk, sel: Set<number> | null, reverse: boolean): string {
  const L = reverse
    ? h.lines.map((l) => ({ ...l, kind: l.kind === "add" ? "del" : l.kind === "del" ? "add" : l.kind }))
    : h.lines;
  const seg: { t: string; nl: boolean }[] = [];
  L.forEach((l, i) => {
    const on = sel === null || sel.has(i);
    if (l.kind === "ctx" || (l.kind === "del" && !on) || (l.kind === "add" && on))
      seg.push({ t: l.text, nl: L[i + 1]?.kind !== "meta" });
  });
  const items = base ? base.split(/(?<=\n)/).map((x) => ({ t: x.replace(/\n$/, ""), nl: x.endsWith("\n") })) : [];
  const [start, count] = reverse ? [h.newStart, h.newLines] : [h.oldStart, h.oldLines];
  items.splice(count ? start - 1 : start, count, ...seg);
  return items.map((x, i) => x.t + (x.nl || i < items.length - 1 ? "\n" : "")).join("");
}

function fuzz(autocrlf: "false" | "true", rounds: number) {
  const root = mkdtempSync(join(tmpdir(), "ocui-diff-fuzz-"));
  const tmpIdx = join(root, ".git", "fuzz-index");
  // Wie git.rs: kein Quoting, keine Farben, a/ b/ Praefixe
  const git = (args: string[], input?: string, env: Record<string, string> = {}) => {
    try {
      return execFileSync("git", ["-c", `core.autocrlf=${autocrlf}`, "-c", "core.safecrlf=false", "-c", "core.quotepath=off",
        "-c", "color.ui=false", "-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false", ...args],
        { cwd: root, input, encoding: "utf8", stdio: "pipe", env: { ...process.env, ...env, LC_ALL: "C" } });
    } catch (e: any) {
      if (args.includes("--no-index") && e.status === 1) return e.stdout as string;
      throw new Error(`git ${args.join(" ")}: ${e.stderr}`);
    }
  };
  const idx = (p: string, env: Record<string, string>) => {
    const ls = git(["ls-files", "-s", "--", p], undefined, env);
    return ls.trim() ? git(["cat-file", "blob", ls.split(/\s+/)[1]]) : null;
  };
  // autocrlf=true schreibt CRLF in die Arbeitskopie; verglichen wird der Inhalt, den git sieht
  const wt = (p: string) => {
    if (!existsSync(join(root, p))) return null;
    const t = readFileSync(join(root, p), "utf8");
    return autocrlf === "true" ? t.replaceAll("\r\n", "\n") : t;
  };
  const line = () => pick(POOL) + (autocrlf === "false" && ri(4) === 0 ? "\r" : "");
  const content = (n: number) => Array.from({ length: n }, line).join("\n") + (n && ri(3) ? "\n" : "");
  const mutate = (t: string) => {
    const ls = t.split("\n");
    const nl = t.endsWith("\n");
    if (nl) ls.pop();
    for (let k = 0; k <= ri(4); k++) {
      const at = ri(ls.length + 1);
      const op = ri(3);
      if (op === 0) ls.splice(at, 0, ...Array.from({ length: 1 + ri(3) }, line));
      else if (op === 1) ls.splice(at, 1 + ri(2));
      else ls.splice(at, 1, line());
    }
    return ls.join("\n") + (ls.length && (ri(4) ? nl : !nl) ? "\n" : "");
  };

  let patches = 0;
  /** stage: --cached vorwaerts, discard: Arbeitskopie rueckwaerts, unstage: --cached rueckwaerts (wie ChangesPanel). */
  const run = (files: DiffFile[], mode: "stage" | "discard" | "unstage") => {
    const reverse = mode !== "stage";
    const cached = mode !== "discard";
    for (const f of files)
      for (const h of f.hunks) {
        const ch = h.lines.flatMap((l, i) => (l.kind === "add" || l.kind === "del" ? [i] : []));
        const sels: (Set<number> | null)[] = [null];
        if (ch.length <= 4) for (let m = 1; m < 1 << ch.length; m++) sels.push(new Set(ch.filter((_, b) => m & (1 << b))));
        else for (let k = 0; k < 6; k++) sels.push(new Set([pick(ch), ...ch.filter(() => ri(2) === 0)]));
        for (const sel of sels) {
          const p = sel ? linesPatch(f, h, sel, reverse) : hunkPatch(f, h);
          const env: Record<string, string> = cached ? { GIT_INDEX_FILE: tmpIdx } : {};
          if (cached) copyFileSync(join(root, ".git", "index"), tmpIdx);
          const path = reverse ? f.newPath : f.oldPath;
          const before = cached ? idx(path, env) : wt(path);
          const raw = cached ? null : existsSync(join(root, path)) ? readFileSync(join(root, path)) : null;
          const flags = [...(cached ? ["--cached"] : []), ...(reverse ? ["--reverse"] : [])];
          const all = sel === null || ch.every((i) => sel.has(i));
          const msg = `${autocrlf} ${mode} ${f.status} ${f.newPath} sel=${sel ? [...sel] : "hunk"}\n${p}`;
          git(["apply", "--check", ...flags, "-"], p, env);
          git(["apply", "--recount", "--whitespace=nowarn", ...flags, "-"], p, env); // wie git.rs apply()
          const gone = all && (reverse ? f.status === "added" : f.status === "deleted");
          const want = gone ? null : expected(before ?? "", h, sel, reverse);
          assert.equal(cached ? idx(f.newPath, env) : wt(f.newPath), want, msg);
          if (!cached) raw === null ? rmSync(join(root, path), { force: true }) : writeFileSync(join(root, path), raw);
          patches++;
        }
      }
  };

  try {
    git(["init", "-q"]);
    for (let r = 0; r < rounds; r++) {
      git(["rm", "-rqf", "--ignore-unmatch", "."]);
      rmSync(join(root, "d"), { recursive: true, force: true });
      mkdirSync(join(root, "d"));
      const names = ["m1.txt", "ä ö.txt", "d/x y.txt", "weg.txt"];
      const base: Record<string, string> = {};
      for (const n of names) writeFileSync(join(root, n), (base[n] = content(ri(12))));
      git(["add", "-A"]);
      git(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", `r${r}`, "--allow-empty"]);
      for (const n of names.slice(0, 3)) writeFileSync(join(root, n), mutate(base[n]));
      rmSync(join(root, "weg.txt"));
      writeFileSync(join(root, "neu ü.txt"), content(1 + ri(8)));
      // Unstaged wie git.rs diff(): getrackt per `git diff`, untracked per --no-index gegen /dev/null
      const files = parseDiff(git(["diff", "--no-color", "--no-ext-diff"]));
      files.push(...parseDiff(git(["diff", "--no-color", "--no-ext-diff", "--no-index", "--", "/dev/null", "neu ü.txt"])));
      run(files, "stage");
      run(files, "discard");
      git(["add", "-A"]);
      run(parseDiff(git(["diff", "--no-color", "--no-ext-diff", "--cached"])), "unstage");
    }
    assert.ok(patches > 20, `nur ${patches} Patches`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const ROUNDS = +(process.env.OCUI_FUZZ_ROUNDS ?? 1);
test("Fuzz: Patches ohne autocrlf passen exakt zur Auswahl", () => ((seed = 42), fuzz("false", ROUNDS)));
test("Fuzz: Patches mit core.autocrlf=true passen exakt zur Auswahl", () => ((seed = 4711), fuzz("true", ROUNDS)));

// Regression: linesPatch rueckwaerts (Zeilen verwerfen/unstagen). Alte letzte Zeile ohne Newline ("-b" + "\ No newline"),
// gewaehlt "-b" und "+b", nicht gewaehlt "+c": der Patch setzte "b" ohne Newline vor die Kontextzeile "c",
// git apply akzeptierte das und verschmolz die Zeilen zu "bc".
test("Zeilen verwerfen bei alter Zeile ohne Newline verschmilzt keine Zeilen", () => {
  const dir = mkdtempSync(join(tmpdir(), "ocui-diff-noeol-"));
  const git = (a: string[], input?: string) =>
    execFileSync("git", ["-c", "core.autocrlf=false", ...a], { cwd: dir, input, encoding: "utf8", stdio: "pipe" });
  try {
    git(["init", "-q"]);
    writeFileSync(join(dir, "f"), "a\nb");
    git(["add", "f"]);
    git(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "i"]);
    writeFileSync(join(dir, "f"), "a\nb\nc\n");
    const [f] = parseDiff(git(["diff"])); // " a", "-b", "\ No newline", "+b", "+c"
    git(["apply", "--recount", "-R", "-"], linesPatch(f, f.hunks[0], new Set([1, 3]), true));
    assert.equal(readFileSync(join(dir, "f"), "utf8"), "a\nb\nc\n"); // tatsaechlich "a\nbc\n"
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
