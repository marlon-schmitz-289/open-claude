<script lang="ts">
  import { slide } from "svelte/transition";
  import XIcon from "@lucide/svelte/icons/x";

  // Kompakte Meldungsleiste: eine Zeile, Klick zeigt den ganzen Text, verschwindet nach timeout von selbst
  // (0 = bleibt). Solange die Maus drauf ist oder der Text aufgeklappt ist, bleibt sie stehen.
  let {
    text = $bindable(""),
    kind = "error",
    timeout = kind === "error" ? 8000 : 5000,
  }: { text: string; kind?: "error" | "note"; timeout?: number } = $props();

  let expanded = $state(false);
  let hover = $state(false);

  // Neue Meldung startet wieder eingeklappt.
  $effect(() => {
    text;
    expanded = false;
  });

  $effect(() => {
    if (!text || !timeout || expanded || hover) return;
    const t = setTimeout(() => (text = ""), timeout);
    return () => clearTimeout(t);
  });
</script>

{#if text}
  <div
    transition:slide={{ duration: 120 }}
    role="status"
    class="flex items-start gap-2 border-b px-3 py-1 text-[11px] {kind === 'error'
      ? 'bg-destructive/10 text-destructive border-destructive/20'
      : 'text-muted-foreground border-border font-mono'}"
    onmouseenter={() => (hover = true)}
    onmouseleave={() => (hover = false)}
  >
    <button
      class="min-w-0 flex-1 text-left {expanded ? 'max-h-40 overflow-y-auto break-words whitespace-pre-wrap' : 'truncate'}"
      onclick={() => (expanded = !expanded)}
      title={expanded ? "Einklappen" : text}>{expanded ? text : text.replace(/\s+/g, " ")}</button
    >
    <button class="hover:text-foreground mt-0.5 shrink-0 opacity-70" onclick={() => (text = "")} aria-label="Meldung schließen">
      <XIcon class="size-3" />
    </button>
  </div>
{/if}
