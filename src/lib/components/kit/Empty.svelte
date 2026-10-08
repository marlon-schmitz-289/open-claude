<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<div class="text-muted-foreground grid h-full place-items-center p-4 text-center text-xs">{@render children()}</div>
