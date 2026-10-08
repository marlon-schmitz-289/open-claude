<script lang="ts">
  import type { Snippet } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";

  let {
    open,
    title,
    description,
    value = $bindable(),
    placeholder,
    action = "OK",
    mono = false,
    onsubmit,
    oncancel,
    children,
  }: {
    open: boolean;
    title: string;
    description?: string;
    value: string;
    placeholder?: string;
    action?: string;
    mono?: boolean;
    onsubmit: () => void;
    oncancel: () => void;
    children?: Snippet;
  } = $props();
</script>

<Dialog.Root {open} onOpenChange={(o) => !o && oncancel()}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{title}</Dialog.Title>
      {#if description}<Dialog.Description>{description}</Dialog.Description>{/if}
    </Dialog.Header>
    <Input bind:value {placeholder} class={mono ? "font-mono" : ""} onkeydown={(e) => e.key === "Enter" && onsubmit()} autofocus />
    {@render children?.()}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={oncancel}>Abbrechen</Button>
      <Button size="sm" onclick={onsubmit}>{action}</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
