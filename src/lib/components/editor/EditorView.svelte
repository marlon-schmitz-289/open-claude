<script lang="ts">
  // Editor-Ansicht eines Projekts: verdrahtet Dock, Dateibaum und Code-Panels mit dem Store.
  // Haelt selbst keinen Zustand: beim Wechsel zu Terminal/Git wird sie abgebaut, Puffer und Layout liegen in $lib/editor.svelte.
  import Notice from "$lib/components/Notice.svelte";
  import { Button } from "$lib/components/ui/button/index.js";
  import SaveIcon from "@lucide/svelte/icons/save";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";
  import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
  import { FILES, PREVIEW, groupOf, panelPath, type PanelId } from "$lib/dock";
  import { closeTab, editor, focus, resetLayout, save, setLayout, togglePanel, watch } from "$lib/editor.svelte";
  import Dock, { type TabInfo } from "./Dock.svelte";
  import FileTree from "./FileTree.svelte";
  import CodePane from "./CodePane.svelte";
  import QuickOpen from "./QuickOpen.svelte";
  import PreviewPane from "./PreviewPane.svelte";

  let { repo }: { repo: string } = $props();

  const s = $derived(editor(repo));
  const dirty = $derived(Object.values(s.files).filter((f) => f.dirty).length);
  let quick = $state(false);

  // Aenderungen von aussen (claude im Terminal) nur verfolgen, solange die Ansicht sichtbar ist.
  $effect(() => watch(repo));

  function info(id: PanelId): TabInfo {
    if (id === FILES) return { title: "Dateien" };
    if (id === PREVIEW) return { title: "Vorschau" };
    const path = panelPath(id) ?? "";
    const f = s.files[path];
    // Auf der Platte geloescht, Puffer hat noch Aenderungen: der Tab bleibt und sagt es.
    const gone = f?.conflict === "deleted" ? " (gelöscht)" : "";
    return { title: path.slice(path.lastIndexOf("/") + 1) + gone, hint: path + gone, dirty: f?.dirty };
  }

  function onKey(e: KeyboardEvent) {
    // CodeMirror speichert die eigene Datei selbst (Mod-s) und setzt dann defaultPrevented.
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
    const key = e.key.toLowerCase();
    if (key === "s") {
      if (!e.defaultPrevented) save(repo);
    } else if (key === "p") quick = true;
    else return;
    // Auch Strg+P: sonst oeffnet die WebView den Druckdialog.
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} />

<QuickOpen {repo} bind:open={quick} />

{#snippet panel(id: PanelId)}
  {#if id === FILES}
    <FileTree {repo} />
  {:else if id === PREVIEW}
    <PreviewPane {repo} />
  {:else}
    <CodePane {repo} path={panelPath(id) ?? ""} />
  {/if}
{/snippet}

{#snippet empty()}
  <div class="text-muted-foreground grid h-full place-items-center p-4 text-center text-xs">
    Datei im Baum wählen oder mit Strg+P suchen.
  </div>
{/snippet}

{#snippet toggle(id: "files" | "preview", label: string, Icon: typeof SaveIcon)}
  {@const on = !!groupOf(s.layout, id)}
  <Button
    variant="ghost"
    size="xs"
    class={on ? "text-foreground" : "text-muted-foreground"}
    aria-pressed={on}
    title="{label} {on ? 'ausblenden' : 'einblenden'}"
    onclick={() => togglePanel(repo, id)}><Icon /> {label}</Button
  >
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class="bg-chrome border-border flex items-center gap-0.5 border-b px-2 py-1">
    <Button
      variant="ghost"
      size="xs"
      class="text-muted-foreground"
      disabled={!s.active || !s.files[s.active]?.dirty}
      title="Speichern (Strg+S)"
      onclick={() => save(repo)}><SaveIcon /> Speichern</Button
    >
    {#if dirty}
      <span class="text-muted-foreground ml-1 flex items-center gap-1.5 text-[11px]" role="status">
        <span class="bg-foreground size-1.5 rounded-full"></span>
        {dirty} ungespeichert
      </span>
    {/if}
    <span class="flex-1"></span>
    {@render toggle("files", "Dateien", PanelLeftIcon)}
    {@render toggle("preview", "Vorschau", PanelRightIcon)}
    <Button
      variant="ghost"
      size="xs"
      class="text-muted-foreground"
      title="Dateien links, Editor in der Mitte; offene Dateien bleiben"
      onclick={() => resetLayout(repo)}><RotateCcwIcon /> Layout zurücksetzen</Button
    >
  </div>
  <Notice bind:text={() => s.error, (v) => (s.error = v)} />
  <div class="min-h-0 flex-1">
    <Dock
      layout={s.layout}
      onlayout={(l) => setLayout(repo, l)}
      onclose={(p) => closeTab(repo, p)}
      onfocus={(p) => focus(repo, p)}
      tab={info}
      {panel}
      {empty}
    />
  </div>
</div>
