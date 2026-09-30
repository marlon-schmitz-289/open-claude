import { test } from "node:test";
import assert from "node:assert/strict";
import { devUrl, moved, span, under, verdict } from "./editor.logic.ts";

test("unveraenderte Platte: nichts tun, egal ob lokal geaendert", () => {
  assert.equal(verdict(false, 5, 5), "none");
  assert.equal(verdict(true, 5, 5), "none");
  // "Meine behalten" nach dem Loeschen setzt mtime auf null: kein neuer Konflikt, solange die Datei fehlt.
  assert.equal(verdict(true, null, null), "none");
});

test("ohne lokale Aenderungen: still nachladen bzw. Tab schliessen", () => {
  assert.equal(verdict(false, 5, 6), "reload");
  assert.equal(verdict(false, 5, 4), "reload"); // aeltere mtime (git checkout) zaehlt auch
  assert.equal(verdict(false, 5, null), "close");
  assert.equal(verdict(false, null, 7), "reload");
});

test("mit lokalen Aenderungen: nur Konflikt melden", () => {
  assert.equal(verdict(true, 5, 6), "changed");
  assert.equal(verdict(true, 5, null), "deleted");
  assert.equal(verdict(true, null, 7), "changed");
});

test("under trifft Datei und Ordnerinhalt, aber keine Namensvettern", () => {
  assert.ok(under("src/a.ts", "src/a.ts"));
  assert.ok(under("src/a.ts", "src"));
  assert.ok(!under("src2/a.ts", "src"));
  assert.ok(!under("src", "src/a.ts"));
});

test("moved zieht Datei und Ordnerinhalt mit", () => {
  assert.equal(moved("a.ts", "a.ts", "b.ts"), "b.ts");
  assert.equal(moved("src/x/a.ts", "src", "lib"), "lib/x/a.ts");
  assert.equal(moved("src2/a.ts", "src", "lib"), null);
});

test("span ersetzt nur die geaenderte Mitte", () => {
  const apply = (a: string, b: string) => {
    const { from, to, insert } = span(a, b);
    return a.slice(0, from) + insert + a.slice(to);
  };
  assert.deepEqual(span("eins\nzwei\ndrei", "eins\nZWEI\ndrei"), { from: 5, to: 9, insert: "ZWEI" });
  assert.deepEqual(span("abc", "abc"), { from: 3, to: 3, insert: "" });
  assert.deepEqual(span("aa", "aaa"), { from: 2, to: 2, insert: "a" });
  assert.deepEqual(span("aaa", "aa"), { from: 2, to: 3, insert: "" });
  for (const [a, b] of [["", "x"], ["x", ""], ["abab", "ab"], ["ab", "abab"], ["xay", "xby"], ["a\n", "b\n"], ["ab", "ba"]])
    assert.equal(apply(a, b), b, `${a} -> ${b}`);
});

test("devUrl: nur http(s), Schema wird ergaenzt", () => {
  assert.equal(devUrl("  "), "");
  assert.equal(devUrl("localhost:5173"), "http://localhost:5173/");
  assert.equal(devUrl("https://example.com/a?b=1"), "https://example.com/a?b=1");
  assert.equal(devUrl("javascript:alert(1)"), null);
  assert.equal(devUrl("file:///etc/passwd"), null);
  assert.equal(devUrl("http://"), null);
});
