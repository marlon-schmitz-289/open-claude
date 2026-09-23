// Property-Tests fuer fuzzy(): Treffer sind eine echte Subsequenz und markieren die richtigen Zeichen.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fold, fuzzy } from "./fuzzy.ts";

let seed = 1;
const ri = (n: number) => Math.floor(((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x80000000) * n);
const CH = [..."abcABC-_./\ äÖß1İ"];
const word = (n: number) => Array.from({ length: n }, () => CH[ri(CH.length)]).join("");

/** Referenz: ist q (ohne Leerzeichen, ohne Gross/Klein und Akzente) eine Subsequenz von t? */
function isSub(t: string, q: string) {
  let at = 0;
  const hay = t.split("").map(fold).join("");
  for (const c of q.split("").map(fold).join("").replaceAll(" ", "")) {
    at = hay.indexOf(c, at);
    if (at < 0) return false;
    at++;
  }
  return true;
}

test("Treffer genau dann, wenn Subsequenz; hits steigend und auf den passenden Zeichen", () => {
  for (let s = 1; s <= 20000; s++) {
    seed = s;
    const text = word(ri(20));
    const q = ri(2) ? word(ri(5)) : [...text].filter(() => ri(3) === 0).join("");
    const m = fuzzy(text, q);
    const msg = `${JSON.stringify(text)} / ${JSON.stringify(q)}`;
    assert.equal(m !== null, isSub(text, q), msg);
    if (!m) continue;
    const chars = q.split("").map(fold).filter((c) => c && c !== " ");
    assert.equal(m.hits.length, chars.length, msg);
    m.hits.forEach((h, i) => {
      assert.ok(i === 0 || h > m.hits[i - 1], msg);
      assert.equal(fold(text[h]), chars[i], msg); // so markiert +page.svelte (name.split(""))
    });
    assert.ok(Number.isFinite(m.score), msg);
  }
});

test("Leere Suche und reine Leerzeichen passen immer", () => {
  assert.deepEqual(fuzzy("x", ""), { score: 0, hits: [] });
  assert.deepEqual(fuzzy("x", "  ")?.hits, []);
});

// Regression: toLowerCase() aenderte bei "İ" (U+0130) die Laenge ("i̇"), die hits verrutschten gegenueber dem Originaltext.
test("Treffer-Positionen verrutschen nicht nach İ", () => {
  assert.deepEqual(fuzzy("İzmir-app", "app")?.hits, [6, 7, 8]);
});

test("Umlaute ohne Punkte tippen: uber findet Überprüfung", () => {
  assert.deepEqual(fuzzy("Überprüfung", "uber")?.hits, [0, 1, 2, 3]);
  assert.ok(fuzzy("Überprüfung", "über"));
});
