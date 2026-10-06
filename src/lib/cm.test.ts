import { test } from "node:test";
import assert from "node:assert/strict";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, highlightingFor } from "@codemirror/language";
import { highlightTree } from "@lezer/highlight";
import { lang, language, theme } from "./components/editor/cm.ts";

// Pro Datei: Sprache laden, in den bestehenden Zustand einsetzen (wie CodePane), Baum und Farbklassen pruefen.
const samples: Record<string, string> = {
  "a.ts": "// c\nexport const x: number = 1;\nfunction f(a: string) { return `${a}`; }\n",
  "a.js": "const a = 'x'; // c\n",
  "a.svelte": "<script lang=\"ts\">\n  let a: number = 1;\n</script>\n<div class=\"x\">{a}</div>\n",
  "a.vue": "<template><div class=\"x\">a</div></template>\n",
  "a.json": '{ "a": [1, true, null] }\n',
  "a.html": '<div class="x">a</div>\n',
  "a.css": "a { color: red; }\n",
  "a.rs": 'fn main() { let x = 1; println!("a"); }\n',
  "a.py": "def f(a):\n    return 'x'  # c\n",
  "a.cs": 'class A { int F() { return 1; } } // c\n',
  "a.md": "# Titel\n\n**fett** und `code`\n",
  "a.yaml": "a: 1\nb: 'x'\n",
  "a.toml": 'a = 1\nb = "x"\n',
  "a.xml": '<a b="c">d</a>\n',
  "src\\App.xaml": '<Window x:Class="A"><Grid/></Window>\n',
  "tsconfig.jsonc": '{ "a": 1 }\n',
  "Cargo.lock": 'version = 4\nname = "x"\n',
  "A.csproj": "<Project><PropertyGroup/></Project>\n",
  "Foo.CS": 'class A { int F() { return 1; } } // c\n',
  "README.MD": "# Titel\n\n**fett**\n",
  "flake.lock": '{ "a": [1, true, null] }\n',
  "a.uxml": '<ui:UXML><ui:Label text="a"/></ui:UXML>\n',
  "a.uss": ".a { color: red; }\n",
  "a.asmdef": '{ "name": "A", "x": 1 }\n',
  "A.cs.meta": "fileFormatVersion: 2\nguid: 'x'\n",
};

for (const [path, doc] of Object.entries(samples))
  test(`Hervorhebung: ${path}`, async () => {
    let state = EditorState.create({ doc, extensions: [theme, lang.of([])] });
    const ext = await language(path);
    assert.ok(ext, "keine Sprache gefunden");
    state = state.update({ effects: lang.reconfigure(ext) }).state;
    const tree = ensureSyntaxTree(state, doc.length, 5000);
    assert.ok(tree && tree.length === doc.length, "kein Syntaxbaum");
    const classes = new Set<string>();
    highlightTree(tree, { style: (tags) => highlightingFor(state, tags) }, (_from, _to, cls) => classes.add(cls));
    assert.ok(classes.size >= 2, `zu wenige Farbklassen: ${[...classes]}`);
  });

test("Svelte: TypeScript im Script-Block wird als solches geparst", async () => {
  const doc = samples["a.svelte"];
  const state = EditorState.create({ doc, extensions: [lang.of((await language("a.svelte"))!)] });
  assert.equal(ensureSyntaxTree(state, doc.length, 5000)!.resolveInner(doc.indexOf("number"), 1).name, "TypeName");
});

test("unbekannte Endung bleibt Text", async () => {
  assert.equal(await language("a.unbekannt"), null);
  assert.equal(await language("yarn.lock"), null); // kein TOML
});
