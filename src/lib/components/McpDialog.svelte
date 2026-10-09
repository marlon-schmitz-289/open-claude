<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Segmented, Switch } from "$lib/components/kit";
  import { forge } from "$lib/git";
  import RefreshIcon from "@lucide/svelte/icons/refresh-cw";
  import TrashIcon from "@lucide/svelte/icons/trash-2";
  import { mcpConfig, type McpServer } from "$lib/chat.logic";

  let {
    open = $bindable(false),
    cwd,
    control,
    onrestart,
  }: { open: boolean; cwd: string; control: (subtype: string, extra?: object) => Promise<any>; onrestart: () => void } = $props();

  let servers = $state<McpServer[]>([]);
  let error = $state("");
  let busy = $state("");
  // Hinzugefuegt/entfernt: claude laedt Server nur beim Start, darum Neustart anbieten.
  let changed = $state(false);

  let name = $state("");
  let kind = $state<"stdio" | "http">("stdio");
  let target = $state("");
  let scope = $state<"local" | "project" | "user">("local");

  const STATUS: Record<string, [string, string]> = {
    connected: ["verbunden", "bg-green-500"],
    failed: ["Fehler", "bg-destructive"],
    "needs-auth": ["Anmeldung nötig", "bg-yellow-500"],
    pending: ["verbindet …", "bg-primary animate-pulse"],
    disabled: ["aus", "bg-muted-foreground"],
  };
  const own = (s: McpServer) => ["local", "user", "project"].includes(s.scope ?? "");

  async function load() {
    error = "";
    try {
      servers = (await control("mcp_status"))?.mcpServers ?? [];
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    if (open) load();
  });

  async function act(s: McpServer, fn: () => Promise<unknown>) {
    busy = s.name;
    error = "";
    try {
      await fn();
    } catch (e) {
      error = `${s.name}: ${e}`;
    }
    busy = "";
    await load();
  }

  const toggle = (s: McpServer) => act(s, () => control("mcp_toggle", { serverName: s.name, enabled: s.status === "disabled" }));
  const reconnect = (s: McpServer) => act(s, () => control("mcp_reconnect", { serverName: s.name }));
  const remove = (s: McpServer) =>
    act(s, async () => {
      await invoke("chat_mcp_remove", { cwd, name: s.name, scope: s.scope });
      changed = true;
    });

  /** claude.ai-Connectoren meldet man auf claude.ai an, eigene OAuth-Server ueber claude selbst. */
  const login = (s: McpServer) =>
    act(s, async () => {
      if (s.config?.type === "claudeai-proxy") return forge.openUrl("https://claude.ai/settings/connectors");
      const r = await control("mcp_authenticate", { serverName: s.name });
      if (r?.authUrl) await forge.openUrl(r.authUrl);
    });

  async function add() {
    const config = mcpConfig(kind, target);
    if (!config) return (error = kind === "http" ? "URL muss mit http:// oder https:// beginnen" : "Befehl fehlt");
    busy = "+";
    error = "";
    try {
      await invoke("chat_mcp_add", { cwd, name: name.trim(), config, scope });
      changed = true;
      name = target = "";
    } catch (e) {
      error = String(e);
    }
    busy = "";
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>MCP-Server</Dialog.Title>
      <Dialog.Description>Status, Tools und Verbindungen dieser Sitzung. Neue Server lädt claude erst nach einem Neustart.</Dialog.Description>
    </Dialog.Header>

    {#if servers.length}
      <ul class="border-border divide-border max-h-72 divide-y overflow-y-auto rounded border text-xs">
        {#each servers as s (s.name)}
          {@const [label, dot] = STATUS[s.status] ?? [s.status, "bg-muted-foreground"]}
          <li class="flex flex-col gap-1 px-2.5 py-1.5">
            <div class="flex items-center gap-2">
              <span class="size-2 shrink-0 rounded-full {dot}" title={label}></span>
              <span class="min-w-0 flex-1 truncate font-medium" title={s.name}>{s.name}</span>
              <span class="text-muted-foreground shrink-0 text-[10px]">{label}{s.scope ? ` · ${s.scope}` : ""}</span>
              {#if s.status === "needs-auth"}
                <Button size="sm" variant="outline" class="h-6 text-[11px]" disabled={!!busy} onclick={() => login(s)}>Anmelden</Button>
              {/if}
              <Button
                variant="ghost"
                size="icon"
                class="text-muted-foreground size-6"
                disabled={!!busy || s.status === "disabled"}
                title="Neu verbinden"
                aria-label="{s.name} neu verbinden"
                onclick={() => reconnect(s)}><RefreshIcon class="size-3.5 {busy === s.name ? 'animate-spin' : ''}" /></Button
              >
              <Switch on={s.status !== "disabled"} label="{s.name} an/aus" disabled={!!busy} onclick={() => toggle(s)} />
              {#if own(s)}
                <Button
                  variant="ghost"
                  size="icon"
                  class="text-muted-foreground hover:text-destructive size-6"
                  disabled={!!busy}
                  aria-label="{s.name} entfernen"
                  onclick={() => window.confirm(`MCP-Server ${s.name} (${s.scope}) entfernen?`) && remove(s)}><TrashIcon class="size-3.5" /></Button
                >
              {/if}
            </div>
            {#if s.error}<p class="text-destructive truncate text-[11px]" title={s.error}>{s.error}</p>{/if}
            {#if s.tools?.length}
              <details class="text-muted-foreground text-[11px]">
                <summary class="cursor-pointer select-none">{s.tools.length} Tools</summary>
                <p class="mt-1 font-mono">{s.tools.map((t) => t.name).join(", ")}</p>
              </details>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-muted-foreground text-xs">Keine MCP-Server in dieser Sitzung.</p>
    {/if}

    <div class="flex flex-col gap-2">
      <div class="flex items-center gap-2 text-xs">
        <span class="flex-1 font-medium">Server hinzufügen</span>
        <Segmented value={kind} options={[["Befehl", "stdio"], ["URL", "http"]] as const} onchange={(v) => (kind = v)} />
        <Segmented
          value={scope}
          options={[
            ["Nur ich, hier", "local", "~/.claude.json, nur dieses Projekt"],
            ["Projekt", "project", ".mcp.json im Repo, für alle"],
            ["Überall", "user", "~/.claude.json, alle Projekte"],
          ] as const}
          onchange={(v) => (scope = v)}
        />
      </div>
      <div class="flex gap-1.5">
        <Input bind:value={name} placeholder="Name" class="h-8 w-32 text-xs" />
        <Input
          bind:value={target}
          placeholder={kind === "http" ? "https://…/mcp" : "npx -y @modelcontextprotocol/server-…"}
          class="h-8 flex-1 font-mono text-xs"
          onkeydown={(e) => e.key === "Enter" && add()}
        />
        <Button size="sm" class="h-8 text-xs" disabled={!!busy || !name.trim() || !target.trim()} onclick={add}>Hinzufügen</Button>
      </div>
      {#if error}<p class="text-destructive text-xs break-all">{error}</p>{/if}
      {#if changed}
        <Button size="sm" variant="secondary" class="text-xs" onclick={() => ((changed = false), (open = false), onrestart())}
          >Sitzung neu starten, um Änderungen zu laden</Button
        >
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
