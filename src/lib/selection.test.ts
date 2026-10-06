import { test } from "node:test";
import assert from "node:assert/strict";
import { pruneSel, rangeSel, toggleSel } from "./selection.ts";

const order = ["a", "b", "c", "d", "e"];

test("toggleSel fuegt hinzu und entfernt", () => {
  assert.deepEqual(toggleSel([], "a"), ["a"]);
  assert.deepEqual(toggleSel(["a"], "c"), ["a", "c"]);
  assert.deepEqual(toggleSel(["a", "c"], "a"), ["c"]);
  assert.deepEqual(toggleSel(["a"], "a"), []);
});

test("toggleSel veraendert die Eingabe nicht", () => {
  const sel = ["a"];
  toggleSel(sel, "b");
  toggleSel(sel, "a");
  assert.deepEqual(sel, ["a"]);
});

test("rangeSel vorwaerts, rueckwaerts und auf sich selbst", () => {
  assert.deepEqual(rangeSel(order, "b", "d"), ["b", "c", "d"]);
  assert.deepEqual(rangeSel(order, "d", "b"), ["b", "c", "d"]);
  assert.deepEqual(rangeSel(order, "c", "c"), ["c"]);
  assert.deepEqual(rangeSel(order, "a", "e"), order);
});

test("rangeSel ohne gueltigen Anker waehlt nur das Ziel", () => {
  assert.deepEqual(rangeSel(order, "", "c"), ["c"]);
  assert.deepEqual(rangeSel(order, "weg", "c"), ["c"]);
});

test("rangeSel mit unbekanntem Ziel ist leer", () => {
  assert.deepEqual(rangeSel(order, "a", "weg"), []);
  assert.deepEqual(rangeSel([], "a", "b"), []);
});

test("pruneSel wirft Verschwundenes raus und sortiert nach Liste", () => {
  assert.deepEqual(pruneSel(["d", "weg", "a"], order), ["a", "d"]);
  assert.deepEqual(pruneSel(["a", "b"], []), []);
  assert.deepEqual(pruneSel([], order), []);
  assert.deepEqual(pruneSel(["a", "a"], order), ["a"]);
});
