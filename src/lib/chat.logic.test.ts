import { test } from "node:test";
import assert from "node:assert/strict";
import { fmt, route } from "./chat.logic.ts";

const claude = [
  { name: "usage", description: "", aliases: ["cost"] },
  { name: "plug:skill", description: "" },
];

test("route: claude zuerst samt Aliasen, dann lokal, sonst unbekannt", () => {
  assert.equal(route("/Users/x kaputt", claude), null);
  assert.equal(route("hallo /status", claude), null);
  assert.deepEqual(route("/cost", claude), { kind: "claude", name: "usage", args: "" });
  assert.deepEqual(route("/status", claude), { kind: "local", name: "status", args: "" });
  assert.deepEqual(route("/quit", claude), { kind: "local", name: "exit", args: "" });
  assert.deepEqual(route("/pr-comments", claude), { kind: "unknown", name: "pr-comments", args: "" });
  assert.deepEqual(route("/pr-comments", []), { kind: "claude", name: "pr-comments", args: "" });
  assert.deepEqual(route("/resume abc", []), { kind: "local", name: "resume", args: "abc" });
  assert.deepEqual(route("/plug:skill a b\nc ", claude), { kind: "claude", name: "plug:skill", args: "a b\nc" });
});

test("fmt: Sections als Liste, sonst JSON", () => {
  assert.equal(fmt({ sections: [{ title: "S", rows: [{ label: "a", value: "1" }] }] }), "S\n  a: 1");
  assert.equal(fmt({ x: 1 }), '{\n  "x": 1\n}');
});
