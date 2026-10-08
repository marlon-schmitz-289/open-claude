<script lang="ts">
  // Editor-Ansicht eines Projekts: verdrahtet Dock, Dateibaum und Code-Panels mit dem Store.
  // Haelt selbst keinen Zustand: beim Wechsel zu Terminal/Git wird sie abgebaut, Puffer und Layout liegen in $lib/editor.svelte.
  import Notice from "$lib/components/Notice.svelte";
  import { Empty, fileIcon } from "$lib/components/kit";
  import { Button } from "$lib/components/ui/button/index.js";
  import SaveIcon from "@lucide/svelte/icons/save";
  import PanelLeftIcon from "@lucide/svelte/icons/panel-left";
  import PanelRightIcon from "@lucide/svelte/icons/panel-right";
  import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
  import FlaskConicalIcon from "@lucide/svelte/icons/flask-conical";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import SquareIcon from "@lucide/svelte/icons/square";
  import FileCodeIcon from "@lucide/svelte/icons/file-code";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import SquareTerminalIcon from "@lucide/svelte/icons/square-terminal";
  import { FILES, PREVIEW, RUN, TESTS, groupOf, panelPath, type PanelId } from "$lib/dock";
  import { closeTab, editor, focus, resetLayout, save, setLayout, setRun, togglePanel, watch } from "$lib/editor.svelte";
  import { chosen, detect, run, runRestart, runStart, runStop } from "$lib/run.svelte";
  import type { RunConfig } from "$lib/run.logic";
  import Dock, { type TabInfo } from "./Dock.svelte";
  import FileTree from "./FileTree.svelte";
  import CodePane from "./CodePane.svelte";
  import QuickOpen from "./QuickOpen.svelte";
  import PreviewPane from "./PreviewPane.svelte";
  import TestPane from "./TestPane.svelte";
  import RunPane from "./RunPane.svelte";

  let { repo }: { repo: string } = $props();

  const s = $derived(editor(repo));
  const dirty = $derived(Object.values(s.files).filter((f) => f.dirty).length);
  let quick = $state(false);

  // Aenderungen von aussen (claude im Terminal) nur verfolgen, solange die Ansicht sichtbar ist.
  $effect(() => watch(repo));

  // Run-Leiste: Ziele einmal je Sitzung erkennen (liest nur). Gestartet wird allein per Klick oder F5.
  $effect(() => void detect(repo));
  const r = $derived(run(repo));
  const cfg = $derived(chosen(repo));
  const running = $derived(r.current?.state === "running");
  const kinds: [RunConfig["target"]["kind"], string][] = [
    ["npm", "npm-Scripts"],
    ["dotnet", ".NET"],
    ["cargo", "Cargo"],
  ];

  function info(id: PanelId): TabInfo {
    if (id === FILES) return { title: "Dateien", icon: FolderTreeIcon };
    if (id === PREVIEW) return { title: "Vorschau", icon: PanelRightIcon };
    if (id === TESTS) return { title: "Tests", icon: FlaskConicalIcon };
    if (id === RUN) return { title: "Ausgabe", icon: SquareTerminalIcon };
    const path = panelPath(id) ?? "";
    const f = s.files[path];
    // Auf der Platte geloescht, Puffer hat noch Aenderungen: der Tab bleibt und sagt es.
    const gone = f?.conflict === "deleted" ? " (gelöscht)" : "";
    const { icon, tint } = fileIcon(path);
    return { title: path.slice(path.lastIndexOf("/") + 1) + gone, hint: path + gone, dirty: f?.dirty, icon, tint };
  }

  function onKey(e: KeyboardEvent) {
    // Wie VS Code: F5 startet, Umschalt+F5 stoppt, Strg+Umschalt+F5 startet neu. Sonst laedt die WebView die Seite neu.
    if (e.key === "F5" && !e.altKey) {
      e.preventDefault();
      if (e.shiftKey && (e.ctrlKey || e.metaKey)) void runRestart(repo);
      else if (e.shiftKey) void runStop(repo);
      else if (!e.ctrlKey && !e.metaKey) void runStart(repo);
      return;
    }
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
  {:else if id === TESTS}
    <TestPane {repo} />
  {:else if id === RUN}
    <RunPane {repo} />
  {:else}
    <CodePane {repo} path={panelPath(id) ?? ""} />
  {/if}
{/snippet}

{#snippet empty()}
  <Empty icon={FileCodeIcon}
    >Datei im Baum wählen oder <kbd class="border-border/60 bg-secondary rounded border px-1 font-mono text-[10px]">Strg+P</kbd></Empty
  >
{/snippet}

{#snippet toggle(id: "files" | "preview" | "tests", label: string, Icon: typeof SaveIcon)}
  {@const on = !!groupOf(s.layout, id)}
  <Button
    variant="ghost"
    size="xs"
    class={on ? "bg-background text-foreground ring-foreground/10 ring-1 [&_svg]:text-primary" : "text-muted-foreground"}
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
        <span class="bg-primary size-1.5 rounded-full"></span>
        {dirty} ungespeichert
      </span>
    {/if}
    <span class="bg-background/60 ring-foreground/10 ml-3 flex items-center gap-0.5 rounded-md p-0.5 ring-1">
      {#if running}<span class="bg-success mx-1 size-1.5 animate-pulse rounded-full" title="Läuft"></span>{/if}
      <select
        class="hover:bg-accent/50 h-5 max-w-60 min-w-0 rounded border-0 bg-transparent px-1.5 font-mono text-[11px] outline-none focus-visible:ring-ring/50 focus-visible:ring-2 disabled:opacity-50"
        value={cfg?.label ?? ""}
        disabled={!r.configs.length}
        aria-label="Startziel"
        title="Startziel (F5 startet)"
        onchange={(e) => setRun(repo, e.currentTarget.value)}
      >
        {#if !r.configs.length}<option value="">Nichts zum Starten</option>{/if}
        {#each kinds as [kind, label] (kind)}
          {@const list = r.configs.filter((c) => c.target.kind === kind)}
          {#if list.length}
            <optgroup {label} class="bg-popover">
              {#each list as c (c.label)}<option value={c.label} class="bg-popover text-popover-foreground">{c.label}</option>{/each}
            </optgroup>
          {/if}
        {/each}
      </select>
      <Button
        variant="ghost"
        size="icon-xs"
        class={cfg && !running ? "text-success hover:text-success" : "text-muted-foreground"}
        disabled={!cfg || running}
        title={cfg ? `${cfg.label} starten (F5) – führt Code aus dem Projekt aus` : "Nichts zum Starten"}
        onclick={() => runStart(repo, cfg ?? undefined)}><PlayIcon /></Button
      >
      <Button
        variant="ghost"
        size="icon-xs"
        class={running ? "text-destructive hover:text-destructive" : "text-muted-foreground"}
        disabled={!running}
        title="Stoppen (Umschalt+F5)"
        onclick={() => runStop(repo)}><SquareIcon /></Button
      >
      <Button
        variant="ghost"
        size="icon-xs"
        class="text-muted-foreground"
        disabled={!r.current}
        title="Neu starten (Strg+Umschalt+F5)"
        onclick={() => runRestart(repo)}><RotateCwIcon /></Button
      >
    </span>
    <span class="flex-1"></span>
    {@render toggle("files", "Dateien", PanelLeftIcon)}
    {@render toggle("tests", "Tests", FlaskConicalIcon)}
    {@render toggle("preview", "Vorschau", PanelRightIcon)}
    <Button
      variant="ghost"
      size="icon-xs"
      class="text-muted-foreground"
      aria-label="Layout zurücksetzen"
      title="Layout zurücksetzen: Dateien links, Editor in der Mitte; offene Dateien bleiben, ein laufender Prozess stoppt"
      onclick={() => resetLayout(repo)}><RotateCcwIcon /></Button
    >
  </div>
  <Notice bind:text={() => s.error, (v) => (s.error = v)} />
  <div class="min-h-0 flex-1 p-1">
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
