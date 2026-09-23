import { test } from "node:test";
import assert from "node:assert/strict";
import { age, cn } from "./utils.ts";

const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

test("age: Einheiten und Grenzen", () => {
  assert.equal(age(""), "");
  assert.equal(age(ago(10e3)), "jetzt");
  assert.equal(age(ago(-60e3)), "jetzt"); // Uhr des Servers voraus
  assert.equal(age(ago(5 * 6e4)), "5 min");
  assert.equal(age(ago(3 * 36e5)), "3 h");
  assert.equal(age(ago(2 * 864e5)), "2 T");
  assert.equal(age(ago(62 * 864e5)), "2 Mo");
  assert.equal(age(ago(800 * 864e5)), "2 J");
  // abgerundet, nicht gerundet: 350 Tage sind "11 Mo", 1,6 Jahre sind "1 J"
  assert.equal(age(ago(350 * 864e5)), "11 Mo");
  assert.equal(age(ago(1.6 * 31536e6)), "1 J");
  assert.equal(age("2024-01-01T10:00:00+02:00").endsWith(" J") || age("2024-01-01T10:00:00+02:00").endsWith(" Mo"), true);
});

test("cn: Tailwind-Konflikte loest der letzte Wert", () => {
  assert.equal(cn("p-2", false && "x", "p-4"), "p-4");
});
