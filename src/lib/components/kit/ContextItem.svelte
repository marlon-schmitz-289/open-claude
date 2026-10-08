<script lang="ts">
  import { ContextMenu } from "bits-ui";
  import { cn } from "$lib/utils";
  import { menuItem } from "./menu";

  let { label, onSelect, destructive = false }: { label: string; onSelect: () => void; destructive?: boolean } = $props();
</script>

<ContextMenu.Item class={cn(menuItem, destructive && "text-destructive")} {onSelect}>{label}</ContextMenu.Item>
