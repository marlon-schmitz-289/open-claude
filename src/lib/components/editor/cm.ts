// CodeMirror-Zubehoer fuer CodePane: dunkles Thema aus den CSS-Variablen der App und Sprachwahl nach Dateiname.
import { EditorView } from "codemirror";
import { Compartment, type Extension } from "@codemirror/state";
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { tags as t } from "@lezer/highlight";

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
    ".cm-tooltip": { backgroundColor: "var(--popover)", border: "1px solid var(--border)", borderRadius: "6px" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--accent)",
      color: "var(--accent-foreground)",
    },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: [t.comment, t.meta], color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: [t.keyword, t.modifier, t.operatorKeyword], color: "var(--primary)" },
  { tag: [t.string, t.special(t.string), t.attributeValue], color: "oklch(0.78 0.11 150)" },
  { tag: [t.regexp, t.escape], color: "oklch(0.78 0.11 190)" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "oklch(0.78 0.12 60)" },
  { tag: [t.typeName, t.className, t.namespace, t.tagName], color: "oklch(0.8 0.1 215)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "oklch(0.83 0.1 95)" },
  { tag: [t.propertyName, t.attributeName], color: "oklch(0.8 0.08 250)" },
  { tag: [t.operator, t.punctuation, t.bracket], color: "var(--muted-foreground)" },
  { tag: t.heading, color: "var(--primary)", fontWeight: "bold" },
  { tag: t.strong, fontWeight: "bold" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: [t.link, t.url], color: "oklch(0.8 0.1 215)", textDecoration: "underline" },
  { tag: t.invalid, color: "var(--destructive)" },
]);

export const theme: Extension = [chrome, syntaxHighlighting(highlight)];

/** Platz fuer die Sprache im EditorState; sie wird nachgeladen und dann per reconfigure eingesetzt. */
export const lang = new Compartment();

// Endungen, die language-data nicht kennt, auf eine verwandte Grammatik umgebogen. Ohne Eintrag bliebe die Datei
// einfarbiger Text - das betraf vor allem .svelte.
// ponytail: .svelte/.astro/.razor laufen als HTML (Script/Style-Bloecke stimmen, {#if}-Bloecke bleiben Text);
// eine echte Svelte-Grammatik braeuchte ein zusaetzliches Paket.
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
const find = (name: string) => LanguageDescription.matchFilename(languages, name);

/** Sprache zum Dateinamen; die Grammatik kommt erst bei Bedarf als eigener Chunk. null = nur Text. */
export async function language(path: string): Promise<Extension | null> {
  const file = path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
  const name = ALIAS.find(([re]) => re.test(file))?.[1] ?? file;
  // language-data vergleicht Endungen genau; Foo.CS oder README.MD (Windows) sonst ohne Grammatik.
  const desc = find(name) ?? find(name.replace(/\.[^.]+$/, (e) => e.toLowerCase()));
  return (await desc?.load()) ?? null;
}
