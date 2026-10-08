<script lang="ts">
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";

  let { loading, onclick, disabled = loading }: { loading: boolean; onclick: () => void; disabled?: boolean } = $props();
</script>

<button class="text-muted-foreground hover:text-foreground disabled:opacity-50" title="Neu laden" {disabled} {onclick}>
  <RefreshCwIcon class="size-3.5 {loading ? 'animate-spin' : ''}" />
</button>
