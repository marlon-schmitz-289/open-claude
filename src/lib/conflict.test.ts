import { test } from "node:test";
import assert from "node:assert/strict";
import { parseConflicts, render } from "./conflict.ts";

const merge = "a\n<<<<<<< HEAD\nM\n=======\nX\n>>>>>>> x\nc\n";
const diff3 = "a\n<<<<<<< HEAD\nM\n||||||| 65ada38\nb\n=======\nX\n>>>>>>> x\nc\n";

test("zerlegt Standard-Marker", () => {
  const s = parseConflicts(merge);
  assert.equal(s.length, 3);
  assert.deepEqual(s[0], { kind: "plain", text: "a\n" });
  assert.equal(s[1].kind, "conflict");
  if (s[1].kind !== "conflict") return;
  assert.equal(s[1].ours, "M\n");
  assert.equal(s[1].theirs, "X\n");
  assert.equal(s[1].base, null);
  assert.equal(s[1].oursLabel, "HEAD");
  assert.equal(s[1].theirsLabel, "x");
});

test("versteht diff3-Basis", () => {
  const c = parseConflicts(diff3)[1];
  assert.ok(c.kind === "conflict" && c.base === "b\n" && c.ours === "M\n" && c.theirs === "X\n");
  assert.equal(render(parseConflicts(diff3), ["base"]), "a\nb\nc\n");
});

test("setzt Entscheidungen zusammen, offene Bloecke behalten Marker", () => {
  const s = parseConflicts(merge + merge);
  assert.equal(render(s, []), merge + merge);
  assert.equal(render(s, ["ours", "theirs"]), "a\nM\nc\na\nX\nc\n");
  assert.equal(render(s, ["ours-theirs", "theirs-ours"]), "a\nM\nX\nc\na\nX\nM\nc\n");
  assert.equal(render(s, [null, "ours"]), merge + "a\nM\nc\n");
});

test("CRLF und unvollstaendige Marker bleiben unveraendert", () => {
  const crlf = merge.replaceAll("\n", "\r\n");
  const s = parseConflicts(crlf);
  assert.equal(render(s, []), crlf);
  assert.equal(render(s, ["theirs"]), "a\r\nX\r\nc\r\n");
  const broken = "a\n<<<<<<< HEAD\nb\n=======\nc\n";
  assert.deepEqual(parseConflicts(broken), [{ kind: "plain", text: broken }]);
  assert.deepEqual(parseConflicts("x\n========\ny"), [{ kind: "plain", text: "x\n========\ny" }]);
});
