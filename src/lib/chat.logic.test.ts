import { test } from "node:test";
import assert from "node:assert/strict";
import { answers, fmt, mcpConfig, nextMode, resultText, route, suggestionLabel } from "./chat.logic.ts";

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

test("nextMode: Kreis wie Umschalt+Tab, Unbekanntes startet vorn", () => {
  assert.deepEqual(["default", "acceptEdits", "plan", "auto"].map(nextMode), ["acceptEdits", "plan", "auto", "default"]);
  assert.equal(nextMode("dontAsk"), "default");
});

test("answers: Auswahl und Freitext pro Frage", () => {
  const qs = [
    { question: "Tee?", options: [{ label: "Ja" }, { label: "Nein" }] },
    { question: "Was noch?", options: [{ label: "Kekse" }, { label: "Kuchen" }], multiSelect: true },
  ];
  assert.deepEqual(answers(qs, [["Ja"], ["Kekse", "Kuchen"]], ["", " Obst "]), { "Tee?": "Ja", "Was noch?": "Kekse, Kuchen, Obst" });
  assert.deepEqual(answers(qs.slice(0, 1), [], ["Kaffee"]), { "Tee?": "Kaffee" });
});

test("resultText: String, Bloecke, Unsinn", () => {
  assert.equal(resultText("ok"), "ok");
  assert.equal(resultText([{ type: "text", text: "a" }, { type: "image" }, { type: "x" }]), "a\n[Bild]");
  assert.equal(resultText(undefined), "");
});

test("suggestionLabel: Modus, Regeln, Fallback", () => {
  assert.equal(suggestionLabel([{ type: "setMode", mode: "acceptEdits" }]), "Edits in dieser Sitzung ohne Rückfrage");
  assert.equal(suggestionLabel([{ type: "addRules", rules: [{ toolName: "Bash", ruleContent: "npm test:*" }, { toolName: "Read" }] }]), "Bash(npm test:*), Read immer erlauben");
  assert.equal(suggestionLabel([{ type: "unbekannt" }]), "Nicht mehr fragen");
});

test("mcpConfig: Befehl mit Argumenten, nur http(s)-URLs", () => {
  assert.deepEqual(mcpConfig("stdio", " npx -y @mcp/server-git  "), { command: "npx", args: ["-y", "@mcp/server-git"] });
  assert.equal(mcpConfig("stdio", "   "), null);
  assert.deepEqual(mcpConfig("http", "https://example.com/mcp"), { type: "http", url: "https://example.com/mcp" });
  for (const bad of ["example.com/mcp", "file:///x", "https://a b"]) assert.equal(mcpConfig("http", bad), null, bad);
});
