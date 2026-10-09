<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Textarea } from "$lib/components/ui/textarea/index.js";
  import { Segmented } from "$lib/components/kit";
  import { MODES } from "$lib/chat.logic";

  let { open = $bindable(false) }: { open: boolean } = $props();

  // Die Datei ist die Wahrheit; Modus und Modell sind nur Abkuerzungen in den JSON-Text.
  let text = $state("");
  let loaded = $state("");
  let error = $state("");
  let busy = $state(false);

  const parsed = $derived.by(() => {
    try {
      const v = JSON.parse(text.trim() || "{}");
      return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, any>) : null;
    } catch {
      return null;
    }
  });
  const mode = $derived(parsed?.permissions?.defaultMode ?? "default");
  const model = $derived(parsed?.model ?? "");

  async function load() {
    error = "";
    try {
      loaded = await invoke<string>("claude_settings_read");
      text = loaded;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    if (open) load();
  });

  function patch(fn: (s: Record<string, any>) => void) {
    if (!parsed) return;
    const s = $state.snapshot(parsed);
    fn(s);
    text = JSON.stringify(s, null, 2) + "\n";
  }

  // "default" = Key weg, dann gilt der Standard von Claude Code.
  function setMode(m: string) {
    patch((s) => {
      if (m !== "default") s.permissions = { ...s.permissions, defaultMode: m };
      else if (s.permissions) {
        delete s.permissions.defaultMode;
        if (!Object.keys(s.permissions).length) delete s.permissions;
      }
    });
  }

  function setModel(m: string) {
    patch((s) => {
      if (m.trim()) s.model = m.trim();
      else delete s.model;
    });
  }

  async function save() {
    busy = true;
    error = "";
    try {
      await invoke("claude_settings_write", { text, expected: loaded });
      loaded = text;
      open = false;
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>Claude-Einstellungen</Dialog.Title>
      <Dialog.Description
        >~/.claude/settings.json, gilt für neue Sitzungen. Im Chat wechselt Umschalt+Tab den Modus sofort.</Dialog.Description
      >
    </Dialog.Header>

    <div class="flex items-center gap-2 text-xs">
      <span class="flex-1">Standard-Modus</span>
      <Segmented value={mode} options={MODES} onchange={setMode} disabled={!parsed} />
    </div>

    <label class="flex items-center gap-2 text-xs">
      <span class="flex-1">Modell</span>
      <Input
        value={model}
        disabled={!parsed}
        placeholder="Standard, z. B. opus"
        class="h-7 w-56 font-mono text-xs"
        onchange={(e) => setModel(e.currentTarget.value)}
      />
    </label>

    <label class="flex flex-col gap-1.5">
      <span class="text-muted-foreground text-[11px]">Alle Einstellungen (JSON)</span>
      <Textarea bind:value={text} spellcheck={false} class="h-64 font-mono text-[11px] md:text-[11px]" placeholder={"{}"} />
    </label>
    {#if !parsed}<p class="text-destructive text-xs">Kein gültiges JSON-Objekt</p>{/if}
    {#if error}<p class="text-destructive text-xs">{error}</p>{/if}

    <div class="flex gap-1.5">
      <Button variant="outline" size="sm" class="text-xs" disabled={busy} onclick={load}>Neu laden</Button>
      <Button size="sm" class="flex-1 text-xs" disabled={busy || !parsed || text === loaded} onclick={save}>Speichern</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
