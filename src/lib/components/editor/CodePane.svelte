<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { keymap } from "@codemirror/view";
  import { indentWithTab, isolateHistory } from "@codemirror/commands";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import DiffView from "$lib/components/git/DiffView.svelte";
  import { diff, editor, keep, peek, reload, save, setText, snapKey, snapshots } from "$lib/editor.svelte";
  import { span } from "$lib/editor.logic";
  import { lang, language, theme } from "./cm";

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
        keymap.of([{ key: "Mod-s", run: () => (void save(repo, path), true) }, indentWithTab]),
        basicSetup,
        theme,
        lang.of([]),
        EditorView.updateListener.of((u) => u.docChanged && setText(repo, path, u.state.doc.toString())),
      ],
    });

  /** Zustand, dessen Dokument `text` ist. Nur die geaenderte Mitte wird ersetzt, als eigener Undo-Schritt. */
  const sync = (state: EditorState, text: string) => {
    const old = state.doc.toString();
    return old === text ? null : state.update({ changes: span(old, text), annotations: isolateHistory.of("full") });
  };

  // f wird beim Nachladen als Ganzes ersetzt: der Aufbau haengt nur daran, ob die Datei geladen ist.
  const ready = $derived(!!f);
  let view: EditorView | undefined;

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
    if (Array.isArray(loaded) && !loaded.length)
      language(path).then(
        (ext) => ext && v.dom.isConnected && v.dispatch({ effects: lang.reconfigure(ext) }),
        () => {}, // Grammatik nicht ladbar: bleibt reiner Text.
      );
    return () => {
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
