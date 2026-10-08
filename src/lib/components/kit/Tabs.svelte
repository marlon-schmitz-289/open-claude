<script lang="ts" generics="T extends string">
  import type { Snippet } from "svelte";

  // Unterstrich-Tabs; children = Bereich rechts
  let {
    tabs,
    value = $bindable(),
    children,
  }: { tabs: { id: T; label: string; count?: number; title?: string }[]; value: T; children?: Snippet } = $props();
</script>

<div class="border-border flex items-center gap-1 border-b px-2" role="tablist">
  {#each tabs as t (t.id)}
    <button
      role="tab"
      aria-selected={value === t.id}
      title={t.title}
      class="-mb-px border-b-2 px-2 py-1.5 {value === t.id
        ? 'border-primary text-foreground'
        : 'text-muted-foreground hover:text-foreground border-transparent'}"
      onclick={() => (value = t.id)}
      >{t.label}{#if t.count}<span class="text-muted-foreground ml-1.5 tabular-nums">{t.count}</span>{/if}</button
    >
  {/each}
  {@render children?.()}
</div>
