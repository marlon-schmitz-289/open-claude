<script lang="ts">
  import { Select } from "bits-ui";
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import { cn } from "$lib/utils";
  import { menuContent, menuItem } from "./menu";

  type Option = readonly [label: string, value: string];

  let {
    value = $bindable(),
    options = [],
    groups = [],
    onchange,
    placeholder = "",
    disabled = false,
    class: className = "",
    title,
    "aria-label": ariaLabel,
  }: {
    value: string;
    // Flache Liste oder Gruppen mit Ueberschrift
    options?: readonly Option[];
    groups?: readonly { label?: string; options: readonly Option[] }[];
    onchange?: (v: string) => void;
    placeholder?: string;
    disabled?: boolean;
    class?: string;
    title?: string;
    "aria-label"?: string;
  } = $props();

  const all = $derived(options.length ? [{ label: undefined, options }, ...groups] : groups);
  const current = $derived(all.flatMap((g) => g.options).find(([, v]) => v === value)?.[0]);
  // bits-ui wertet "" als "nichts gewaehlt", darum intern mit Praefix
  const key = (v: string) => "#" + v;
</script>

<Select.Root
  type="single"
  value={current === undefined ? "" : key(value)}
  onValueChange={(k) => {
    value = k.slice(1);
    onchange?.(value);
  }}
  {disabled}
>
  <Select.Trigger
    class={cn(
      "bg-secondary border-border hover:bg-accent/50 focus-visible:ring-ring/50 flex h-6 min-w-0 items-center gap-1 rounded border px-1.5 text-xs outline-none focus-visible:ring-2 disabled:opacity-50",
      className,
    )}
    {title}
    aria-label={ariaLabel}
  >
    <span class="flex-1 truncate text-left">{current ?? placeholder}</span>
    <ChevronDownIcon class="text-muted-foreground size-3 shrink-0" />
  </Select.Trigger>
  <Select.Portal>
    <Select.Content class="{menuContent} max-h-80 overflow-y-auto" sideOffset={4}>
      {#each all as g, i (i)}
        {#if g.options.length}
          <Select.Group>
            {#if g.label}<Select.GroupHeading class="text-muted-foreground px-2 pt-1.5 pb-0.5 text-[10px] uppercase">{g.label}</Select.GroupHeading>{/if}
            {#each g.options as [label, v] (v)}
              <Select.Item value={key(v)} {label} class={menuItem}>
                {#snippet children({ selected })}
                  <span class="flex-1 truncate">{label}</span>
                  {#if selected}<CheckIcon class="size-3" />{/if}
                {/snippet}
              </Select.Item>
            {/each}
          </Select.Group>
        {/if}
      {/each}
    </Select.Content>
  </Select.Portal>
</Select.Root>
