import { test } from "node:test";
import assert from "node:assert/strict";
import { join, dirname, rows } from "./tree.ts";

const e = (name: string, dir = false, ignored = false) => ({ name, dir, ignored });

test("join und dirname kennen den Projektordner", () => {
  assert.equal(join("", "a.ts"), "a.ts");
  assert.equal(join("src/lib", "a.ts"), "src/lib/a.ts");
  assert.equal(dirname("a.ts"), "");
  assert.equal(dirname("src/lib/a.ts"), "src/lib");
});

test("rows zeigt nur aufgeklappte Ordner und vererbt ignored", () => {
  const lists = {
    "": [e("src", true), e("out", true, true), e("a.md")],
    src: [e("lib", true), e("x.ts")],
    "src/lib": [e("y.ts")],
    out: [e("z.js")],
  };
  assert.deepEqual(
    rows(lists, ["src", "out"]).map((r) => [r.path, r.depth, r.open, r.ignored]),
    [
      ["src", 0, true, false],
      ["src/lib", 1, false, false],
      ["src/x.ts", 1, false, false],
      ["out", 0, true, true],
      ["out/z.js", 1, false, true],
      ["a.md", 0, false, false],
    ],
  );
  // Aufgeklappt, aber noch nicht gelistet: keine Kinder, kein Fehler.
  assert.deepEqual(rows({ "": [e("src", true)] }, ["src"]).map((r) => r.path), ["src"]);
  assert.deepEqual(rows({}, []), []);
});
