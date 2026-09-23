// Property-Tests fuer parseConflicts/render: zufaellige Konfliktdateien (merge/diff3, CRLF, ohne Newline am Ende,
// kaputte und verschachtelte Marker) und Abgleich mit echtem `git merge-file`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseConflicts, render, type Choice } from "./conflict.ts";

let seed = 1;
const ri = (n: number) => Math.floor(((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x80000000) * n);
const pick = <T>(a: T[]) => a[ri(a.length)];

const TEXT = ["a", "b", "ä ö ü", "", "  x", "======== acht", "<<<<<<<<", "<<<<<<x", "=======x", ">>>>>>>>> neun", "|||||||x", "日本"];
const CHOICES: Choice[] = ["ours", "theirs", "ours-theirs", "theirs-ours", "base"];

test("wohlgeformte Konflikte: Zerlegung und alle Entscheidungen stimmen exakt", () => {
  for (let s = 1; s <= 3000; s++) {
    seed = s;
    const eol = ri(3) === 0 ? "\r\n" : "\n";
    const block = (n: number) => Array.from({ length: n }, () => pick(TEXT) + eol).join("");
    let text = "";
    let want = "";
    const choices: (Choice | null)[] = [];
    const expected: { ours: string; base: string | null; theirs: string }[] = [];
    const n = ri(4);
    for (let k = 0; k <= n; k++) {
      const plain = block(ri(3));
      text += plain;
      want += plain;
      if (k === n) break;
      const [ours, theirs] = [block(ri(3)), block(ri(3))];
      const base = ri(2) ? block(ri(3)) : null;
      const c = ri(6) === 0 ? null : pick(CHOICES);
      const raw =
        `<<<<<<< HEAD${eol}${ours}` + (base !== null ? `||||||| base${eol}${base}` : "") +
        `=======${eol}${theirs}>>>>>>> feature/x${eol}`;
      text += raw;
      choices.push(c);
      expected.push({ ours, base, theirs });
      want +=
        c === "ours" ? ours : c === "theirs" ? theirs : c === "ours-theirs" ? ours + theirs
          : c === "theirs-ours" ? theirs + ours : c === "base" ? (base ?? "") : raw;
    }
    if (ri(2) && text.endsWith(eol)) (text = text.slice(0, -eol.length)), (want = want.slice(0, -eol.length));
    const segs = parseConflicts(text);
    const conflicts = segs.filter((x) => x.kind === "conflict");
    const msg = `Seed ${s}: ${JSON.stringify(text)}`;
    // ohne Newline am Ende faellt das letzte Zeilenende weg, sonst muss alles exakt passen
    if (text.endsWith(eol) || !text.endsWith(`feature/x`)) {
      assert.equal(conflicts.length, expected.length, msg);
      conflicts.forEach((c, i) => {
        assert.equal(c.ours, expected[i].ours, msg);
        assert.equal(c.base, expected[i].base, msg);
        assert.equal(c.theirs, expected[i].theirs, msg);
        assert.equal(c.oursLabel, "HEAD", msg);
        assert.equal(c.theirsLabel, "feature/x", msg);
      });
      assert.equal(render(segs, choices), want, msg);
    }
    assert.equal(render(segs, []), text, msg);
  }
});

test("beliebige Zeilen mit kaputten/verschachtelten Markern: render(parse(x), []) === x", () => {
  const LINES = [...TEXT, "<<<<<<< HEAD", "<<<<<<<", "|||||||", "||||||| base", "=======", ">>>>>>> x", ">>>>>>>"];
  for (let s = 1; s <= 5000; s++) {
    seed = s;
    const eol = pick(["\n", "\r\n"]);
    const text = Array.from({ length: ri(14) }, () => pick(LINES)).join(eol) + (ri(2) ? eol : "");
    const segs = parseConflicts(text);
    assert.equal(render(segs, []), text, `Seed ${s}`);
    for (const x of segs)
      if (x.kind === "conflict") {
        assert.ok(x.raw.startsWith("<<<<<<<") && /(^|\n)>>>>>>>[^\n]*\n?$/.test(x.raw), JSON.stringify(x.raw));
        assert.equal(render([x], ["ours"]) + render([x], ["theirs"]), x.ours + x.theirs);
      }
    // Jede Entscheidung liefert Text ohne Absturz
    render(segs, segs.map(() => pick(CHOICES)));
  }
});

test("gegen echtes git merge-file (merge, diff3, zdiff3; LF und CRLF)", () => {
  const dir = mkdtempSync(join(tmpdir(), "ocui-conflict-"));
  let total = 0;
  try {
    for (let s = 1; s <= 12; s++) {
      seed = s * 31;
      const eol = s % 2 ? "\n" : "\r\n";
      const lines = Array.from({ length: 12 }, (_, i) => `z${i} ä`);
      const edit = () => lines.map((l) => (ri(3) === 0 ? `${l} ${ri(100)}` : l)).join(eol) + eol;
      const [base, ours, theirs] = [lines.join(eol) + eol, edit(), edit()];
      const mergeFile = (...flags: string[]) => {
        writeFileSync(join(dir, "o"), ours);
        writeFileSync(join(dir, "b"), base);
        writeFileSync(join(dir, "t"), theirs);
        try {
          execFileSync("git", ["merge-file", ...flags, "-L", "HEAD", "-L", "base", "-L", "feature", "o", "b", "t"], {
            cwd: dir,
            stdio: "pipe",
          });
        } catch (e: any) {
          if (!(e.status > 0 && e.status < 128)) throw e; // Exit-Code = Anzahl Konflikte
        }
        return readFileSync(join(dir, "o"), "utf8");
      };
      for (const style of ["merge", "diff3", "zdiff3"]) {
        const merged = mergeFile(...(style === "merge" ? [] : [`--${style}`]));
        const segs = parseConflicts(merged);
        assert.equal(render(segs, []), merged);
        const conflicts = segs.filter((x) => x.kind === "conflict");
        total += conflicts.length;
        assert.equal(conflicts.length, (merged.match(/^<<<<<<< /gm) ?? []).length);
        for (const c of conflicts) {
          assert.equal(c.base !== null, style !== "merge");
          assert.deepEqual([c.oursLabel, c.theirsLabel], ["HEAD", "feature"]);
        }
        // Alles "ours"/"theirs"/beide muss git merge-file --ours/--theirs/--union entsprechen
        const all = (c: Choice) => render(segs, segs.map(() => c));
        if (style !== "zdiff3") {
          assert.equal(all("ours"), mergeFile("--ours"));
          assert.equal(all("theirs"), mergeFile("--theirs"));
          assert.equal(all("ours-theirs"), mergeFile("--union"));
        }
      }
    }
    assert.ok(total > 10, `nur ${total} Konflikte`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
