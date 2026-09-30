import { test } from "node:test";
import assert from "node:assert/strict";
import { clampZoom, zoomKey, zoomStep } from "./zoom.ts";

const key = (key: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {}) => ({
  key,
  ctrlKey: true,
  metaKey: false,
  altKey: false,
  ...mods,
});

test("zoomStep: 10er-Schritte, Grenzen, Reset", () => {
  assert.equal(zoomStep(100, 1), 110);
  assert.equal(zoomStep(100, -1), 90);
  assert.equal(zoomStep(95, 1), 110);
  assert.equal(zoomStep(200, 1), 200);
  assert.equal(zoomStep(50, -1), 50);
  assert.equal(zoomStep(170, 0), 100);
});

test("clampZoom: kaputte Werte aus settings.json", () => {
  assert.equal(clampZoom(150), 150);
  assert.equal(clampZoom(5), 50);
  assert.equal(clampZoom(9999), 200);
  assert.equal(clampZoom(undefined), 100);
  assert.equal(clampZoom("150"), 100);
  assert.equal(clampZoom(NaN), 100);
});

test("zoomKey: Tasten, Nummernblock", () => {
  assert.equal(zoomKey(key("+"), false), 1);
  assert.equal(zoomKey(key("="), false), 1);
  assert.equal(zoomKey(key("-"), false), -1);
  assert.equal(zoomKey(key("0"), false), 0);
  // Nummernblock ohne NumLock: Strg+Einfg ist Kopieren, kein Zoom.
  assert.equal(zoomKey(key("Insert"), false), null);
});

test("zoomKey: Shell-Tasten bleiben unberuehrt", () => {
  assert.equal(zoomKey(key("-", { ctrlKey: false }), false), null);
  // Strg+Umschalt+- ist Undo in Readline/claude
  assert.equal(zoomKey(key("_"), false), null);
  // AltGr = Strg+Alt
  assert.equal(zoomKey(key("0", { altKey: true }), false), null);
  assert.equal(zoomKey(key("1"), false), null);
  // macOS: Cmd zoomt, Strg gehoert der Shell
  assert.equal(zoomKey(key("-"), true), null);
  assert.equal(zoomKey(key("-", { ctrlKey: false, metaKey: true }), true), -1);
});
