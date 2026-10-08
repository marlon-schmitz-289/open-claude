<script lang="ts" generics="T">
  let {
    value,
    options,
    onchange,
    disabled = false,
  }: {
    value: T;
    options: readonly (readonly [label: string, value: T, title?: string])[];
    onchange: (v: T) => void;
    disabled?: boolean;
  } = $props();
</script>

<div class="bg-secondary flex shrink-0 rounded p-0.5">
  {#each options as [label, v, title] (label)}
    <button
      class="rounded px-1.5 py-0.5 text-[10px] {value === v
        ? 'bg-primary text-primary-foreground'
        : 'text-muted-foreground hover:text-foreground'}"
      aria-pressed={value === v}
      {title}
      {disabled}
      onclick={() => onchange(v)}>{label}</button
    >
  {/each}
</div>
