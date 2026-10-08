<script lang="ts">
  let { on, label, onclick, disabled = false }: { on: boolean; label: string; onclick: () => void; disabled?: boolean } =
    $props();
</script>

<button
  class="relative h-3.5 w-6 shrink-0 rounded-full transition-colors disabled:opacity-50 {on
    ? 'bg-primary'
    : 'bg-secondary ring-border ring-1'}"
  role="switch"
  aria-checked={on}
  aria-label={label}
  {disabled}
  {onclick}
  ><span class="bg-foreground absolute top-0.5 size-2.5 rounded-full transition-all {on ? 'left-3' : 'left-0.5'}"></span></button
>
