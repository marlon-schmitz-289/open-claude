<script lang="ts" module>
  import type { DiffFile, Hunk } from "$lib/diff";

  /** Aktion pro Hunk; lineLabel gilt, sobald Zeilen im Hunk gewaehlt sind. */
  export type HunkAction = {
    label: string;
    lineLabel?: string;
    destructive?: boolean;
    run: (file: DiffFile, hunk: Hunk, selected: Set<number>) => void;
  };
</script>

<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { parseDiff } from "$lib/diff";
  import { Button } from "$lib/components/ui/button/index.js";

  let {
    diff,
    actions = [],
    empty = "Keine Änderungen.",
  }: { diff: string; actions?: HunkAction[]; empty?: string } = $props();

  const files = $derived(parseDiff(diff));

  // Gewaehlte Zeilen je Hunk ("datei:hunk" -> Indizes in hunk.lines), Anker fuer Shift-Klick.
  let selected = $state<Record<string, number[]>>({});
  let anchor = $state<{ key: string; idx: number } | null>(null);

  // Grosse Dateien (Lockfiles, generierter Code) erst auf Wunsch rendern.
  const BIG = 1500;
  const opened = new SvelteSet<number>();
  const size = (f: DiffFile) => f.hunks.reduce((n, h) => n + h.lines.length, 0);

  $effect(() => {
    diff;
    selected = {};
    anchor = null;
    opened.clear();
  });

  function pick(key: string, hunk: Hunk, idx: number, shift: boolean) {
    const cur = new Set(selected[key] ?? []);
    if (shift && anchor?.key === key) {
      const [a, b] = [Math.min(anchor.idx, idx), Math.max(anchor.idx, idx)];
      for (let i = a; i <= b; i++) if (hunk.lines[i].kind === "add" || hunk.lines[i].kind === "del") cur.add(i);
    } else {
      cur.has(idx) ? cur.delete(idx) : cur.add(idx);
      anchor = { key, idx };
    }
    selected[key] = [...cur];
  }

  const bg = { add: "bg-emerald-500/10", del: "bg-red-500/10", ctx: "", meta: "text-muted-foreground italic" };
  const sign = { add: "+", del: "-", ctx: " ", meta: "" };
</script>

<div class="h-full overflow-auto font-mono text-[11px] leading-[1.45]">
  {#if files.length === 0}
    <p class="text-muted-foreground p-3 font-sans text-xs">{empty}</p>
  {/if}
  {#each files as file, fi (fi)}
    <div class="border-border border-b">
      <div class="bg-chrome border-border sticky top-0 z-10 truncate border-b px-2 py-1 font-semibold">
        {file.oldPath !== file.newPath ? `${file.oldPath} → ${file.newPath}` : file.newPath}
      </div>
      {#if file.binary}
        <p class="text-muted-foreground px-2 py-1 font-sans">Binärdatei</p>
      {:else if file.hunks.length === 0}
        <p class="text-muted-foreground px-2 py-1 font-sans">Keine Inhaltsänderung.</p>
      {:else if size(file) > BIG && !opened.has(fi)}
        <button class="text-muted-foreground hover:text-foreground px-2 py-1 font-sans" onclick={() => opened.add(fi)}>
          {size(file)} Zeilen – anzeigen
        </button>
      {/if}
      {#each size(file) > BIG && !opened.has(fi) ? [] : file.hunks as hunk, hi (hi)}
        {@const key = `${fi}:${hi}`}
        {@const sel = new Set(selected[key] ?? [])}
        <div class="bg-primary/5 text-muted-foreground flex items-center gap-1 px-2 py-0.5">
          <span class="min-w-0 flex-1 truncate">{hunk.header}</span>
          {#each actions as a (a.label)}
            <Button
              variant={a.destructive ? "destructive" : "ghost"}
              size="xs"
              class="h-5 font-sans text-[11px]"
              onclick={() => a.run(file, hunk, sel)}
            >
              {sel.size && a.lineLabel ? a.lineLabel : a.label}
            </Button>
          {/each}
        </div>
        {#each hunk.lines as line, li (li)}
          {@const change = line.kind === "add" || line.kind === "del"}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="flex {bg[line.kind]} {sel.has(li) ? 'ring-primary/60 ring-1 ring-inset' : ''} {change &&
            actions.length
              ? 'cursor-pointer'
              : ''}"
            onclick={(e) => change && actions.length && pick(key, hunk, li, e.shiftKey)}
          >
            <span class="text-muted-foreground/60 w-10 shrink-0 pr-1 text-right select-none">{line.old ?? ""}</span>
            <span class="text-muted-foreground/60 w-10 shrink-0 pr-1 text-right select-none">{line.new ?? ""}</span>
            <span
              class="w-4 shrink-0 text-center select-none {line.kind === 'add'
                ? 'text-emerald-400'
                : line.kind === 'del'
                  ? 'text-red-400'
                  : ''}">{sign[line.kind]}</span
            >
            <span class="pr-2 whitespace-pre">{line.text}</span>
          </div>
        {/each}
      {/each}
    </div>
  {/each}
</div>
