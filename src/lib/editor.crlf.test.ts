// Der Store vergleicht den Text aus CodeMirror mit dem von fs_read (dirty): beide muessen dieselben Zeilenenden haben.
import { test } from "node:test";
import assert from "node:assert/strict";
import { EditorState } from "@codemirror/state";

const roundtrip = (s: string) => EditorState.create({ doc: s }).doc.toString();

test("LF-Inhalt ueberlebt CodeMirror unveraendert (auch ohne Zeilenende am Schluss)", () => {
  for (const s of ["", "a", "a\n", "a\n\nb", "\n", " x "]) assert.equal(roundtrip(s), s);
});

test("CodeMirror macht aus jedem CR ein LF, darum liefert fs_read nie ein CR", () => {
  assert.equal(roundtrip("a\rb"), "a\nb");
  assert.equal(roundtrip("a\r\nb"), "a\nb");
});
