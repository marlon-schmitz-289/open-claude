import { test } from "node:test";
import assert from "node:assert/strict";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, foldable, highlightingFor } from "@codemirror/language";
import { highlightTree, tags as t, type Tag } from "@lezer/highlight";
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

/** Farbklasse am Anfang von `needle` und die Klasse, die `tag` bekommt. */
async function at(path: string, doc: string, needle: string, tag: Tag) {
  const state = EditorState.create({ doc, extensions: [theme, lang.of((await language(path))!)] });
  const tree = ensureSyntaxTree(state, doc.length, 5000)!;
  const pos = doc.indexOf(needle);
  let got = "";
  highlightTree(tree, { style: (tags) => highlightingFor(state, tags) }, (from, to, cls) => {
    if (from <= pos && pos < to) got = cls;
  });
  return { got, want: highlightingFor(state, [tag]) };
}

const tagged: [string, string, string, Tag][] = [
  ["a.rs", 'fn main() { println!("a"); }\n', "println", t.meta],
  ["a.rs", "impl A { fn f(&self) { self.x; } }\n", "self.x", t.self],
  ["a.cs", "class A { [Fact] void F() {} }\n", "Fact", t.meta],
  ["a.ts", "function f() {}\n", "f()", t.function(t.definition(t.variableName))],
  ["a.diff", "--- a\n+++ b\n+a\n", "+a\n", t.inserted],
];
for (const [path, doc, needle, tag] of tagged)
  test(`Farbe: ${needle.trim()} in ${path}`, async () => {
    const { got, want } = await at(path, doc, needle, tag);
    assert.ok(want, "Regel fehlt");
    assert.equal(got, want);
  });

test("C#: eigene Grammatik ohne Fehlerknoten, Klasse faltbar", async () => {
  const doc = "namespace N;\n\npublic class A\n{\n    [Fact]\n    public void F()\n    {\n        var s = $\"x{1}\";\n    }\n}\n";
  for (const path of ["a.cs", "Foo.CS"]) {
    const state = EditorState.create({ doc, extensions: [lang.of((await language(path))!)] });
    const tree = ensureSyntaxTree(state, doc.length, 5000)!;
    let errors = 0;
    tree.iterate({ enter: (n) => void (n.type.isError && errors++) });
    assert.equal(errors, 0, path);
    const brace = state.doc.line(4); // Allman: die Klammerzeile faltet den Klassenrumpf
    assert.deepEqual(foldable(state, brace.from, brace.to), { from: brace.to, to: doc.lastIndexOf("}") }, path);
  }
});
