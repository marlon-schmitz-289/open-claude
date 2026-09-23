import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseDiff, hunkPatch, linesPatch } from "./diff.ts";

const sample = `diff --git a/src/x.ts b/src/x.ts
index 1111111..2222222 100644
--- a/src/x.ts
+++ b/src/x.ts
@@ -1,3 +1,3 @@ function f() {
 a
-b
+B
 c
diff --git "a/\\303\\244 neu.txt" "b/\\303\\244 neu.txt"
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ "b/\\303\\244 neu.txt"
@@ -0,0 +1 @@
+x
\\ No newline at end of file
diff --git a/alt.md b/neu.md
similarity index 100%
rename from alt.md
rename to neu.md
diff --git a/bild.png b/bild.png
index 4444444..5555555 100644
Binary files a/bild.png and b/bild.png differ
`;

test("parst Dateien, Status, Pfade und Zeilennummern", () => {
  const [mod, add, ren, bin] = parseDiff(sample);
  assert.equal(mod.newPath, "src/x.ts");
  assert.equal(mod.status, "modified");
  assert.deepEqual(mod.hunks[0].lines, [
    { kind: "ctx", text: "a", old: 1, new: 1 },
    { kind: "del", text: "b", old: 2 },
    { kind: "add", text: "B", new: 2 },
    { kind: "ctx", text: "c", old: 3, new: 3 },
  ]);
  assert.equal(add.status, "added");
  assert.equal(add.newPath, "ä neu.txt");
  assert.equal(add.oldPath, "ä neu.txt");
  assert.equal(add.hunks[0].newLines, 1);
  assert.equal(add.hunks[0].lines[1].kind, "meta");
  assert.deepEqual([ren.status, ren.oldPath, ren.newPath, ren.hunks.length], ["renamed", "alt.md", "neu.md", 0]);
  assert.deepEqual([bin.binary, bin.newPath], [true, "bild.png"]);
});

test("Zeilenauswahl: + weglassen, - zu Kontext, Header neu gerechnet", () => {
  const [f] = parseDiff(sample);
  assert.equal(
    linesPatch(f, f.hunks[0], new Set([2])),
    "diff --git a/src/x.ts b/src/x.ts\nindex 1111111..2222222 100644\n--- a/src/x.ts\n+++ b/src/x.ts\n" +
      "@@ -1,3 +1,4 @@\n a\n b\n+B\n c\n",
  );
  // rueckwaerts (nur -b unstagen): nicht gewaehltes + bleibt als Kontext
  assert.ok(linesPatch(f, f.hunks[0], new Set([1]), true).endsWith("@@ -1,4 +1,3 @@\n a\n-b\n B\n c\n"));
  assert.equal(linesPatch(f, f.hunks[0], new Set([0, 3])), "");
});

// Praxistest gegen echtes git: alle Patches muessen `git apply --cached --check` bestehen
test("git apply akzeptiert Hunk- und Zeilen-Patches", () => {
  const dir = mkdtempSync(join(tmpdir(), "ocui-diff-"));
  const git = (args: string[], input?: string) =>
    execFileSync("git", ["-c", "core.autocrlf=false", ...args], { cwd: dir, input, encoding: "utf8", stdio: "pipe" });
  try {
    git(["init", "-q"]);
    writeFileSync(join(dir, "m.txt"), "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
    writeFileSync(join(dir, "e.txt"), "a\nb");
    writeFileSync(join(dir, "d.txt"), "x\ny\n");
    git(["add", "-A"]);
    git(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "i"]);
    writeFileSync(join(dir, "m.txt"), "a\nB\nc\nd\ne\nf\ng\nh\ni\nJ\nk\n");
    writeFileSync(join(dir, "e.txt"), "a\nc\nd");
    writeFileSync(join(dir, "n.txt"), "1\n2");
    rmSync(join(dir, "d.txt"));
    git(["add", "-N", "n.txt"]);
    for (const reverse of [false, true]) {
      if (reverse) git(["add", "-A"]);
      const files = parseDiff(git(reverse ? ["diff", "--cached"] : ["diff"]));
      assert.equal(files.length, 4);
      for (const f of files)
        for (const h of f.hunks) {
          const patches = [hunkPatch(f, h)];
          const idx = h.lines.flatMap((l, i) => (l.kind === "add" || l.kind === "del" ? [i] : []));
          for (let mask = 1; mask < 1 << idx.length; mask++)
            patches.push(linesPatch(f, h, new Set(idx.filter((_, b) => mask & (1 << b))), reverse));
          for (const p of patches) git(["apply", "--cached", "--check", ...(reverse ? ["-R"] : []), "-"], p);
        }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
