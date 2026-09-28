import { test } from "node:test";
import assert from "node:assert/strict";
import { verdict, unityBadgeText, formatDuration } from "./unity.ts";

test("verdict: gruen nur ohne failed/skipped/inconclusive", () => {
  assert.equal(verdict({ failed: 0, skipped: 0, inconclusive: 0 }), "green");
  assert.equal(verdict({ failed: 0, skipped: 1, inconclusive: 0 }), "yellow");
  assert.equal(verdict({ failed: 0, skipped: 0, inconclusive: 1 }), "yellow");
  // failed schlaegt skipped/inconclusive: bleibt rot, nicht gelb
  assert.equal(verdict({ failed: 1, skipped: 2, inconclusive: 3 }), "red");
});

test("unityBadgeText: Version wenn bekannt, sonst nur 'Unity'", () => {
  assert.equal(unityBadgeText(undefined), "Unity");
  assert.equal(unityBadgeText(null), "Unity");
  assert.equal(
    unityBadgeText({ version: null, cli: true, editorInstalled: true, editorOpen: false, pipeline: false, connected: false, skill: false }),
    "Unity",
  );
  assert.equal(
    unityBadgeText({ version: "6000.7.0a3", cli: true, editorInstalled: true, editorOpen: true, pipeline: true, connected: true, skill: true }),
    "Unity 6000.7.0a3",
  );
});

test("formatDuration: Sekunden mit bis zu 2 Nachkommastellen, ab einer Minute mm:ss", () => {
  assert.equal(formatDuration(0.0813309), "0.08 s");
  assert.equal(formatDuration(49), "49 s");
  assert.equal(formatDuration(3.5), "3.5 s");
  assert.equal(formatDuration(65), "1:05");
  assert.equal(formatDuration(125.9), "2:06");
});
