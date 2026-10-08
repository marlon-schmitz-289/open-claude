<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, mono = false, children }: { title: string; mono?: boolean; children?: Snippet } = $props();
</script>

<div class="bg-chrome border-border flex items-center gap-3 border-y px-3 py-1.5">
  <span class="flex-1 truncate text-[11px] font-semibold {mono ? 'font-mono' : ''}">{title}</span>
  {@render children?.()}
</div>
