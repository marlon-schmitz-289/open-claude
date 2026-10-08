<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { keymap } from "@codemirror/view";
  import { indentWithTab, isolateHistory } from "@codemirror/commands";
  import { setDiagnostics } from "@codemirror/lint";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import DiffView from "$lib/components/git/DiffView.svelte";
  import { diff, edited, editor, flush, keep, peek, prefs, reload, save, snapKey, snapshots, toggleWrap } from "$lib/editor.svelte";
  import { huge, span } from "$lib/editor.logic";
  import { lang, language, setTestMarks, testGutter, theme, wrap } from "./cm";
  import { peek as peekTests, testRun } from "$lib/testing.svelte";
  import { fails, marks } from "$lib/testing.logic";

  // path bleibt fuer die Lebensdauer der Instanz gleich (Dock mountet je Panel-ID neu).
  let { repo, path }: { repo: string; path: string } = $props();

  const s = $derived(editor(repo));
  const f = $derived(s.files[path]);

  let host = $state<HTMLDivElement>();
  /** Diff fuer "Vergleichen"; null = Dialog zu. */
  let compare = $state<string | null>(null);

  // Die Callbacks greifen nur auf den Store zu, nie auf die Komponente: der EditorState ueberlebt sie.
  const fresh = (doc: string) =>
    EditorState.create({
      doc,
      extensions: [
        keymap.of([
          { key: "Mod-s", run: () => (void save(repo, path), true) },
          { key: "Alt-z", run: () => (toggleWrap(), true) },
          indentWithTab,
        ]),
        basicSetup,
        theme,
        lang.of([]),
        wrap.of(prefs.wrap ? EditorView.lineWrapping : []),
        testGutter((id) => void testRun(repo, id)),
        EditorView.updateListener.of((u) => u.docChanged && edited(repo, path, () => u.view.state.doc.toString())),
      ],
    });

  /** Zustand, dessen Dokument `text` ist. Nur die geaenderte Mitte wird ersetzt, als eigener Undo-Schritt. */
  const sync = (state: EditorState, text: string) => {
    const old = state.doc.toString();
    return old === text ? null : state.update({ changes: span(old, text), annotations: isolateHistory.of("full") });
  };

  // f wird beim Nachladen als Ganzes ersetzt: der Aufbau haengt nur daran, ob die Datei geladen ist.
  const ready = $derived(!!f);
  let view = $state.raw<EditorView>();
  /** Ohne Grammatik geoeffnet, weil die Datei zu gross ist. */
  let plain = $state(false);

  // Editor aufbauen, sobald die Datei geladen ist. Beim Abbau wandern Zustand (samt Undo) und Scrollposition in den Store.
  $effect(() => {
    if (!ready || !host) return;
    const parent = host;
    const key = snapKey(repo, path);
    const v = untrack(() => {
      const snap = snapshots.get(key);
      const state = snap?.state ?? fresh(f!.text);
      // Waehrend das Panel abgebaut war, kann die Datei nachgeladen worden sein.
      return new EditorView({ parent, state: sync(state, f!.text)?.state ?? state, scrollTo: snap?.scroll });
    });
    view = v;
    // Fokus nur fuer die aktive Datei, und nie aus Eingabefeldern (Umbenennen im Baum, Strg+P) heraus.
    if (untrack(() => s.active) === path && !document.activeElement?.closest("input, textarea, [role=dialog]")) v.focus();
    const loaded = lang.get(v.state);
    plain = Array.isArray(loaded) && !loaded.length && untrack(() => huge(f!.text));
    if (Array.isArray(loaded) && !loaded.length && !plain)
      language(path).then(
        (ext) => ext && v.dom.isConnected && v.dispatch({ effects: lang.reconfigure(ext) }),
        (e) => console.error("Grammatik", path, e), // Nicht ladbar: bleibt reiner Text.
      );
    return () => {
      flush(repo, path);
      // Geschlossene/umbenannte Dateien hinterlassen nichts; peek statt editor: nach drop() darf nichts neu entstehen.
      if (peek(repo)?.files[path]) snapshots.set(key, { state: v.state, scroll: v.scrollSnapshot() });
      v.destroy();
      view = undefined;
    };
  });

  // Inhalt von aussen ersetzt (rev steigt): in den bestehenden Editor einspielen, damit Undo-Verlauf, Cursor,
  // Scrollposition und Fokus bleiben.
  $effect(() => {
    void f?.rev;
    const text = untrack(() => f?.text);
    const tr = view && text !== undefined ? sync(view.state, text) : null;
    if (tr) view!.dispatch(tr);
  });

  // Test-Explorer: Marken und Fehler dieser Datei, ein Dispatch je Batch. Nur wenn er fuer das Projekt gesucht hat.
  let marked = false;
  $effect(() => {
    const t = peekTests(repo);
    const v = view;
    if (!t || !v) return;
    const m = marks(t.nodes, t.states, path);
    const d = fails(t.nodes, t.results, path);
    if (!m.length && !d.length && !marked) return;
    marked = m.length > 0 || d.length > 0;
    untrack(() => {
      const doc = v.state.doc;
      const diags = d
        .filter((x) => x.line <= doc.lines)
        .map((x) => ({ from: doc.line(x.line).from, to: doc.line(x.line).to, severity: "error" as const, message: x.msg }));
      v.dispatch(setDiagnostics(v.state, diags), { effects: setTestMarks.of(m) });
    });
  });

  // Zeilenumbruch ist global: offene Editoren ziehen mit, wiederhergestellte Zustaende auch.
  $effect(() => {
    const w = prefs.wrap;
    view?.dispatch({ effects: wrap.reconfigure(w ? EditorView.lineWrapping : []) });
  });

  // Sprungziel (Stack-Link, Testdetails): Cursor auf die Zeile, mittig zeigen, Ziel verbrauchen.
  $effect(() => {
    const r = s.reveal;
    const v = view;
    if (!r || r.path !== path || !v) return;
    untrack(() => {
      const l = v.state.doc.line(Math.min(Math.max(1, r.line), v.state.doc.lines));
      v.dispatch({ selection: { anchor: l.from }, effects: EditorView.scrollIntoView(l.from, { y: "center" }) });
      v.focus();
      s.reveal = null;
    });
  });

  const showDiff = () =>
    diff(repo, path).then(
      (d) => (compare = d),
      (e) => (s.error = String(e)),
    );
</script>

{#if !f}
  <div class="text-muted-foreground grid h-full place-items-center text-xs">Lädt …</div>
{:else}
  <div class="flex h-full min-h-0 flex-col">
    {#if f.conflict}
      <div class="bg-destructive/15 flex shrink-0 flex-wrap items-center gap-1.5 border-b px-3 py-1.5 text-xs">
        <span class="mr-auto">Datei wurde außerhalb {f.conflict === "deleted" ? "gelöscht" : "geändert"}.</span>
        <Button size="xs" variant="outline" onclick={() => reload(repo, path)}>Neu laden</Button>
        <Button size="xs" variant="outline" onclick={() => keep(repo, path)}>Meine behalten</Button>
        <Button size="xs" variant="outline" onclick={showDiff}>Vergleichen</Button>
      </div>
    {/if}
    {#if plain}
      <div class="text-muted-foreground shrink-0 border-b px-3 py-1 text-xs">Große Datei: ohne Highlighting</div>
    {/if}
    <div bind:this={host} class="min-h-0 flex-1 overflow-hidden"></div>
  </div>

  <Dialog.Root open={compare !== null} onOpenChange={(o) => !o && (compare = null)}>
    <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-4xl">
      <Dialog.Header>
        <Dialog.Title>Platte → meine Änderungen</Dialog.Title>
        <Dialog.Description>{path}</Dialog.Description>
      </Dialog.Header>
      <div class="min-h-0 flex-1 overflow-auto">
        <DiffView diff={compare ?? ""} empty="Kein Unterschied." />
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
