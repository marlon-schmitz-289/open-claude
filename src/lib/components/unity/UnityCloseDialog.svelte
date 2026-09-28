<script lang="ts">
  import { Button } from "$lib/components/ui/button/index.js";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { unity } from "$lib/unity";

  /** Projektpfad; null = Dialog zu. */
  let { path = $bindable(), onclosed }: { path: string | null; onclosed?: (path: string) => void } = $props();

  let force = $state(false);
  let closing = $state(false);
  let error = $state("");

  function dismiss() {
    path = null;
    force = false;
    error = "";
  }

  async function run() {
    const p = path;
    if (!p) return;
    closing = true;
    error = "";
    try {
      await unity.close(p, force);
      dismiss();
      onclosed?.(p);
    } catch (e) {
      // Im Dialog statt dahinter: so kann man direkt mit "Erzwingen" nochmal.
      error = String(e);
    }
    closing = false;
  }
</script>

<Dialog.Root open={path !== null} onOpenChange={(o) => !o && !closing && dismiss()}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Unity-Editor schließen</Dialog.Title>
      <Dialog.Description>Schließt ohne zu speichern. Ungesicherte Änderungen im Editor gehen verloren.</Dialog.Description>
    </Dialog.Header>
    <label class="flex items-center gap-2 text-xs">
      <input type="checkbox" bind:checked={force} class="accent-primary" />
      Erzwingen (Prozess hart beenden, falls nötig)
    </label>
    {#if error}<p class="text-destructive text-xs">{error}</p>{/if}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" disabled={closing} onclick={dismiss}>Abbrechen</Button>
      <Button variant="destructive" size="sm" disabled={closing} onclick={run}>
        {closing ? "Schließt …" : "Schließen"}
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
