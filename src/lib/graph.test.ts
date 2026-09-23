import { test } from "node:test";
import assert from "node:assert/strict";
import { layout } from "./graph.ts";

const c = (sha: string, ...parents: string[]) => ({ sha, parents });

test("lineare Historie bleibt in Spalte 0", () => {
  const rows = layout([c("c", "b"), c("b", "a"), c("a")]);
  assert.deepEqual(rows.map((r) => r.lane), [0, 0, 0]);
  assert.deepEqual(rows[1].edges, [
    { from: 0, to: 0, kind: "in", color: 0 },
    { from: 0, to: 0, kind: "out", color: 0 },
  ]);
  assert.equal(rows[2].lanesOut, 0);
});

test("Merge oeffnet eine Spalte, die danach wieder frei wird", () => {
  // m = merge(b, f); f zweigt von a ab
  const rows = layout([c("m", "b", "f"), c("f", "a"), c("b", "a"), c("a"), c("x")]);
  assert.deepEqual(rows.map((r) => r.lane), [0, 1, 0, 0, 0]);
  assert.deepEqual(
    rows[0].edges.filter((e) => e.kind === "out").map((e) => e.to),
    [0, 1],
  );
  // b trifft auf a, das schon in Spalte 1 erwartet wird: Spalte 1 schwenkt nach links
  assert.ok(rows[2].edges.some((e) => e.kind === "pass" && e.from === 1 && e.to === 0));
  assert.equal(rows[2].lanesOut, 1);
  // neue Wurzel nutzt die freie Spalte 0
  assert.equal(rows[4].lane, 0);
});

test("zwei Spitzen mit gemeinsamem Elternteil laufen zusammen", () => {
  const rows = layout([c("t1", "p"), c("t2", "p"), c("p")]);
  assert.deepEqual(rows.map((r) => r.lane), [0, 1, 0]);
  assert.equal(rows[2].lanesIn, 1);
  assert.notEqual(rows[0].color, rows[1].color);
  // Eltern ausserhalb der geladenen Seite: Spalte bleibt offen
  assert.equal(layout([c("a", "fehlt")])[0].lanesOut, 1);
});
