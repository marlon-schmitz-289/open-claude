// CodeMirror-Zubehoer fuer CodePane: Thema aus den CSS-Variablen der App und Sprachwahl nach Dateiname.
import { EditorView } from "codemirror";
import { Compartment, RangeSet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { GutterMarker, gutter } from "@codemirror/view";
import { HighlightStyle, LanguageDescription, LanguageSupport, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { styleTags, tags as t } from "@lezer/highlight";
import type { St } from "../../testing.logic.ts";

const chrome = EditorView.theme(
  {
    "&": { height: "100%", color: "var(--foreground)", backgroundColor: "var(--background)", fontSize: "13px" },
    // body hat user-select: none; ohne das laesst WebKit im Editor weder markieren noch tippen.
    ".cm-content": { caretColor: "var(--primary)", userSelect: "text", WebkitUserSelect: "text" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.5" },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--primary)" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: "color-mix(in oklch, var(--primary) 30%, transparent)" },
    ".cm-activeLine": { backgroundColor: "color-mix(in oklch, var(--foreground) 4%, transparent)" },
    ".cm-gutters": { backgroundColor: "var(--background)", color: "var(--muted-foreground)", border: "none" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--foreground)" },
    ".cm-foldPlaceholder": { backgroundColor: "var(--muted)", border: "none", color: "var(--muted-foreground)" },
    ".cm-selectionMatch": { backgroundColor: "color-mix(in oklch, var(--primary) 18%, transparent)" },
    ".cm-searchMatch": { backgroundColor: "oklch(0.8 0.13 85 / 0.3)" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "oklch(0.8 0.13 85 / 0.55)" },
    "&.cm-focused .cm-matchingBracket": { backgroundColor: "color-mix(in oklch, var(--primary) 35%, transparent)" },
    ".cm-panels": { backgroundColor: "var(--card)", color: "var(--foreground)" },
    ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
    ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--border)" },
    ".cm-textfield": { backgroundColor: "var(--background)", border: "1px solid var(--input)", borderRadius: "4px" },
    ".cm-button": {
      backgroundImage: "none",
      backgroundColor: "var(--secondary)",
      border: "1px solid var(--border)",
      borderRadius: "4px",
    },
    ".cm-tooltip": { backgroundColor: "var(--popover)", color: "var(--popover-foreground)", border: "1px solid var(--border)", borderRadius: "6px" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--accent)",
      color: "var(--accent-foreground)",
    },
  },
);

// Spezifischere Tags gewinnen (definition(...) vor variableName), die Reihenfolge ist egal.
const highlight = HighlightStyle.define([
  { tag: t.comment, color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: t.docComment, color: "var(--muted-foreground)" },
  { tag: [t.macroName, t.annotation, t.processingInstruction, t.meta], color: "var(--syn-meta)" },
  { tag: [t.keyword, t.modifier, t.operatorKeyword], color: "var(--primary)" },
  { tag: [t.controlKeyword, t.moduleKeyword], color: "var(--primary)", fontStyle: "italic" },
  { tag: [t.string, t.special(t.string), t.attributeValue, t.monospace], color: "var(--syn-string)" },
  { tag: [t.regexp, t.escape], color: "var(--syn-regexp)" },
  { tag: [t.number, t.bool, t.null, t.atom, t.color, t.unit], color: "var(--syn-number)" },
  { tag: [t.constant(t.variableName), t.self, t.special(t.variableName), t.labelName], color: "var(--syn-const)" },
  { tag: [t.typeName, t.className, t.namespace, t.standard(t.variableName)], color: "var(--syn-type)" },
  { tag: [t.definition(t.typeName), t.definition(t.className)], color: "var(--syn-type)", fontWeight: "500" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--syn-fn)" },
  { tag: t.function(t.definition(t.variableName)), color: "var(--syn-fn)", fontWeight: "500" },
  { tag: [t.definition(t.variableName), t.definition(t.propertyName)], color: "var(--foreground)", fontWeight: "500" },
  { tag: [t.propertyName, t.attributeName, t.tagName], color: "var(--syn-prop)" },
  { tag: [t.operator, t.punctuation, t.bracket], color: "var(--muted-foreground)" },
  { tag: t.heading, color: "var(--primary)", fontWeight: "bold" },
  { tag: t.quote, color: "var(--muted-foreground)" },
  { tag: t.strong, fontWeight: "bold" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: [t.link, t.url], color: "var(--syn-type)", textDecoration: "underline" },
  { tag: t.inserted, color: "var(--success)" },
  { tag: t.deleted, color: "var(--destructive)" },
  { tag: t.changed, color: "var(--warning)" },
  { tag: t.invalid, color: "var(--destructive)" },
]);

export const theme: Extension = [chrome, syntaxHighlighting(highlight)];

/** Platz fuer den Zeilenumbruch (Alt+Z), umgeschaltet per reconfigure. */
export const wrap = new Compartment();

/** Platz fuer die Sprache im EditorState; sie wird nachgeladen und dann per reconfigure eingesetzt. */
export const lang = new Compartment();

// Endungen, die language-data nicht kennt, auf eine verwandte Grammatik umgebogen. Ohne Eintrag bliebe die Datei
// einfarbiger Text - das betraf vor allem .svelte.
// ponytail: .svelte/.astro/.razor laufen als HTML (Script/Style-Bloecke stimmen, {#if}-Bloecke bleiben Text);
// @replit/codemirror-lang-svelte 6.0.0 (2022) erzeugt auf src/routes/+page.svelte Fehlerknoten, darum nicht eingebaut.
const ALIAS: [RegExp, string][] = [
  [/\.(svelte|astro|razor|cshtml)$/i, "a.html"],
  [/\.(xaml|axaml|csproj|fsproj|vbproj|props|targets|slnx|resx|config|manifest|nuspec|plist|storyboard|uxml)$/i, "a.xml"],
  [/\.(jsonc|json5|webmanifest|asmdef|asmref)$|^(composer|flake|bun)\.lock$/i, "a.json"],
  // Nur die Lockfiles, die wirklich TOML sind; yarn.lock und Gemfile.lock bleiben Text.
  [/^(cargo|poetry|uv)\.lock$|(^|\.)editorconfig$/i, "a.toml"],
  // Unity
  [/\.uss$/i, "a.css"],
  [/\.(meta|unity|prefab|asset)$/i, "a.yaml"],
];
// Grammatiken ausserhalb von language-data; sie gehen vor. C#: echter Baum statt clike (Folding, Einrueckung, Definitionen).
const extra = [
  LanguageDescription.of({
    name: "C#",
    extensions: ["cs"],
    // Attribute wie [Fact] als Meta faerben, wie #[test] in Rust (das Paket faerbt sie als Property).
    load: () =>
      import("@codincod/codemirror-lang-csharp").then(
        (m) =>
          new LanguageSupport(
            m.csharpLanguage.configure({ props: [styleTags({ "Attribute/TypeName Attribute/ScopedTypeName/TypeName": t.meta })] }),
            m.csharp().support,
          ),
      ),
  }),
];
const find = (name: string) => LanguageDescription.matchFilename(extra, name) ?? LanguageDescription.matchFilename(languages, name);

/** Sprache zum Dateinamen; die Grammatik kommt erst bei Bedarf als eigener Chunk. null = nur Text. */
export async function language(path: string): Promise<Extension | null> {
  const file = path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
  const name = ALIAS.find(([re]) => re.test(file))?.[1] ?? file;
  // language-data vergleicht Endungen genau; Foo.CS oder README.MD (Windows) sonst ohne Grammatik.
  const desc = find(name) ?? find(name.replace(/\.[^.]+$/, (e) => e.toLowerCase()));
  return (await desc?.load()) ?? null;
}

// --- Test-Explorer: Marken an Testdefinitionen ---


class TestMarker extends GutterMarker {
  readonly id: string;
  readonly state: St;
  constructor(id: string, state: St) {
    super();
    this.id = id;
    this.state = state;
  }
  eq(o: TestMarker) {
    return o.id === this.id && o.state === this.state;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = `cm-test-${this.state}`;
    s.textContent = this.state === "passed" ? "✓" : this.state === "failed" ? "✗" : this.state === "running" || this.state === "queued" ? "◌" : "▶";
    s.title = this.state === "running" || this.state === "queued" ? "Läuft …" : this.state === "idle" ? "Test ausführen" : "Erneut ausführen";
    return s;
  }
}

/** Marken ersetzen (Zeilen 1-basiert); danach wandern sie mit den Aenderungen mit. */
export const setTestMarks = StateEffect.define<{ line: number; id: string; state: St }[]>();

const testMarks = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(set, tr) {
    set = set.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setTestMarks)) continue;
      const doc = tr.state.doc;
      const ranges = e.value.filter((m) => m.line <= doc.lines).map((m) => new TestMarker(m.id, m.state).range(doc.line(m.line).from));
      set = RangeSet.of(ranges, true);
    }
    return set;
  },
});

/** Gutter mit ▶/✓/✗ an Testdefinitionen; Klick startet den Test ueber run(id). */
export function testGutter(run: (id: string) => void): Extension {
  return [
    testMarks,
    gutter({
      class: "cm-test-gutter",
      markers: (v) => v.state.field(testMarks),
      domEventHandlers: {
        mousedown(view, line) {
          let id: string | null = null;
          view.state.field(testMarks).between(line.from, line.from, (_a, _b, m) => void (id = (m as TestMarker).id));
          if (id === null) return false;
          run(id);
          return true;
        },
      },
    }),
    EditorView.theme({
      ".cm-test-gutter .cm-gutterElement": { cursor: "pointer", padding: "0 2px", fontSize: "11px" },
      ".cm-test-idle, .cm-test-queued, .cm-test-running": { color: "var(--muted-foreground)" },
      ".cm-test-skipped": { color: "var(--warning)" },
      ".cm-test-passed": { color: "var(--success)" },
      ".cm-test-failed": { color: "var(--destructive)" },
    }),
  ];
}
