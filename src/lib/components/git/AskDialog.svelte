<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { pending, setNoAsk } from "$lib/ask.svelte";
  import { onDestroy } from "svelte";

  // Sonst taucht eine offene Rueckfrage beim naechsten Repo wieder auf und wirkt auf das alte.
  onDestroy(() => (pending.ask = null));

  let never = $state(false);
  $effect(() => {
    if (pending.ask) never = false;
  });

  function ok() {
    const a = pending.ask;
    if (!a) return;
    pending.ask = null;
    if (never) setNoAsk(a.key, true);
    a.run();
  }
</script>

<Dialog.Root open={pending.ask !== null} onOpenChange={(o) => !o && (pending.ask = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{pending.ask?.title}</Dialog.Title>
      <Dialog.Description>{pending.ask?.message}</Dialog.Description>
    </Dialog.Header>
    <label class="text-muted-foreground flex items-center gap-2 text-xs">
      <input type="checkbox" class="accent-primary" bind:checked={never} />
      Nicht erneut anzeigen
    </label>
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (pending.ask = null)}>Abbrechen</Button>
      <Button size="sm" onclick={ok}>Bestätigen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
