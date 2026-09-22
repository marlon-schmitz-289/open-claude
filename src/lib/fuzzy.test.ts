import { test } from "node:test";
import assert from "node:assert/strict";
import { fuzzy } from "./fuzzy.ts";

test("findet Subsequenzen und markiert die Treffer", () => {
  assert.equal(fuzzy("adesk-backend", "xyz"), null);
  assert.deepEqual(fuzzy("adesk-backend", "abe")?.hits, [0, 6, 10]);
  assert.deepEqual(fuzzy("acarma", "")?.hits, []);
});

test("bewertet zusammenhaengende Treffer hoeher als verstreute", () => {
  const solid = fuzzy("vestigo", "vest")!.score;
  const spread = fuzzy("vorher-etwas-sonst-tja", "vest")!.score;
  assert.ok(solid > spread, `${solid} > ${spread}`);
});

test("bevorzugt den kuerzeren Namen bei gleichem Treffer", () => {
  const short = fuzzy("ae-core-hcv", "core")!.score;
  const long = fuzzy("ae-core-graphics-wpf", "core")!.score;
  assert.ok(short > long, `${short} > ${long}`);
});
