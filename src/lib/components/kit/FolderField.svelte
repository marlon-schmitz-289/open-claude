<script lang="ts">
  import FolderIcon from "@lucide/svelte/icons/folder";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";

  let { value = $bindable(), oninput, onbrowse }: { value: string; oninput: () => void; onbrowse: () => void } = $props();
</script>

<div class="flex items-center gap-1.5">
  <Input bind:value {oninput} placeholder="Zielordner" class="font-mono text-xs" />
  <Button variant="outline" size="icon" class="shrink-0" onclick={onbrowse} aria-label="Ordner wählen">
    <FolderIcon class="size-3.5" />
  </Button>
</div>
