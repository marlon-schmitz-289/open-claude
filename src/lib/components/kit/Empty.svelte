<script lang="ts">
  import type { Snippet } from "svelte";
  import type XIcon from "@lucide/svelte/icons/x";

  let { children, icon: Icon }: { children: Snippet; icon?: typeof XIcon } = $props();
</script>

<div class="text-muted-foreground grid h-full place-items-center p-4 text-center text-xs">
  <div>
    {#if Icon}<Icon class="text-muted-foreground/40 mx-auto mb-2 size-8" />{/if}
    {@render children()}
  </div>
</div>
