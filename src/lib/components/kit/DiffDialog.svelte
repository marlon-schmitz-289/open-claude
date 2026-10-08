<script lang="ts">
  // Diff im grossen Dialog; mit step blaettern ←/→ bzw. Knoepfe durch mehrere Dateien. Esc schliesst (bits-ui).
  import type { Snippet } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import DiffView from "$lib/components/git/DiffView.svelte";
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  let {
    diff,
    title,
    description,
    empty,
    onclose,
    step,
    children,
  }: {
    diff: string | null;
    title: string;
    description?: string;
    empty?: string;
    onclose: () => void;
    step?: (d: 1 | -1) => void;
    children?: Snippet;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    const d = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
    if (!step || !d || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    e.preventDefault();
    step(d);
  }
</script>

<Dialog.Root open={diff !== null} onOpenChange={(o) => !o && onclose()}>
  <Dialog.Content class="flex h-[80vh] flex-col gap-3 sm:max-w-4xl" {onkeydown}>
    <Dialog.Header>
      <div class="flex min-w-0 items-center gap-2 pr-8">
        <Dialog.Title class="min-w-0 truncate">{title}</Dialog.Title>
        {@render children?.()}
        {#if step}
          <span class="ml-auto flex shrink-0 gap-0.5">
            <Button variant="ghost" size="icon-xs" title="Vorherige Datei (←)" aria-label="Vorherige Datei" onclick={() => step(-1)}
              ><ChevronLeftIcon /></Button
            >
            <Button variant="ghost" size="icon-xs" title="Nächste Datei (→)" aria-label="Nächste Datei" onclick={() => step(1)}
              ><ChevronRightIcon /></Button
            >
          </span>
        {/if}
      </div>
      {#if description}<Dialog.Description class="truncate font-mono text-[11px]">{description}</Dialog.Description>{/if}
    </Dialog.Header>
    <div class="border-border/60 min-h-0 flex-1 overflow-hidden rounded-lg border">
      <DiffView diff={diff ?? ""} {empty} />
    </div>
  </Dialog.Content>
</Dialog.Root>
