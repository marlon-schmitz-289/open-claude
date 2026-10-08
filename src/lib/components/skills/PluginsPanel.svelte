<script lang="ts">
  import { confirm } from "$lib/confirm.svelte";
  import Notice from "$lib/components/Notice.svelte";
  import { RefreshButton, SectionHead, Segmented, Switch } from "$lib/components/kit";
  import { untrack, type Snippet } from "svelte";
  import { Input } from "$lib/components/ui/input/index.js";
  import { fuzzy } from "$lib/fuzzy";
  import { skills, groupInstalled, pickAvailable, type PluginsInfo, type PluginAction, type InstalledPlugin } from "$lib/skills";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";

  let { repo, lead, busy = $bindable(null) }: { repo: string; lead?: Snippet; busy?: string | null } = $props();

  let info = $state<PluginsInfo | null>(null);
  let loading = $state(true);
  let error = $state("");
  let note = $state("");
  let query = $state("");
  let scopeChoice = $state<"user" | "project">("user");
  let source = $state("");

  async function reload() {
    loading = true;
    try {
      info = await skills.plugins(repo);
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  $effect(() => {
    repo;
    untrack(reload);
  });

  const RESTART: PluginAction[] = ["install", "update", "enable", "disable"];

  /** Ein CLI-Aufruf samt Bestaetigung eines Marketplace-Kommandos; Notiz bei Erfolg, null wenn abgelehnt, wirft bei Fehler. */
  async function exec(action: PluginAction, target: string, scope?: string, cwd = repo) {
    let res = await skills.pluginRun(cwd, action, target, scope);
    const shown = res.shownCommand;
    if (shown?.sha256) {
      const cmd = JSON.stringify(shown.command ?? shown);
      if (!(await confirm(`Der Marketplace will dieses Kommando ausführen:\n${cmd}`, "Zulassen"))) return null;
      res = await skills.pluginRun(cwd, action, target, scope, shown.sha256);
    }
    if (res.outcome !== "ok") throw res.message ?? JSON.stringify(res);
    if (res.updateOutcome === "up_to_date") return "Bereits aktuell.";
    return (res.message ?? "Erledigt.") + (RESTART.includes(action) ? " Greift beim nächsten Claude-Start." : "");
  }

  /** Mehrere Aufrufe nacheinander (eine Sperre); Fehler je Aufruf gesammelt. true wenn alle ok. */
  async function batch(key: string, calls: [PluginAction, string, string?, string?][]) {
    busy = key;
    error = note = "";
    const notes: string[] = [];
    const errs: string[] = [];
    for (const [action, target, scope, cwd] of calls) {
      const label = calls.length > 1 ? `${cwd && cwd !== repo ? folder(cwd) : target}: ` : "";
      try {
        const n = await exec(action, target, scope, cwd);
        if (n === null) errs.push(`${label}abgelehnt`);
        else notes.push(label + n);
      } catch (e) {
        errs.push(label + String(e));
      }
    }
    note = notes.join("\n");
    error = errs.join("\n");
    busy = null;
    await reload();
    return !errs.length;
  }

  const run = (action: PluginAction, target: string, scope?: string, cwd = repo) => batch(target, [[action, target, scope, cwd]]);

  const checkUpdates = () => batch("updates", Object.keys(info?.marketplaces ?? {}).map((n) => ["mp-update", n] as [PluginAction, string]));

  /** Alle Installationen eines Plugins updaten, auch lokale (dort hat local Vorrang). */
  const updateAll = (g: { id: string; main?: InstalledPlugin; locals: InstalledPlugin[] }) =>
    batch(
      g.id,
      [...(g.main ? [g.main] : []), ...g.locals].map((p) => ["update", g.id, p.scope, p.projectPath ?? repo]),
    );

  async function addMarketplace() {
    if (await run("mp-add", source.trim())) source = "";
  }

  const folder = (p?: string) => p?.split(/[\\/]/).filter(Boolean).pop() ?? p ?? "";
  const SCOPE = { user: "User", project: "Projekt", local: "lokal" };

  async function remove(p: InstalledPlugin) {
    if (!(await confirm(`${p.id} aus ${SCOPE[p.scope]} entfernen? Gespeicherte Plugin-Daten werden gelöscht.`, "Entfernen"))) return;
    run("uninstall", p.id, p.scope, p.projectPath ?? repo);
  }

  const q = $derived(query.trim());
  const desc = (id: string) => info?.meta[id]?.description ?? "";
  const groups = $derived(info ? groupInstalled(info.installed).filter((g) => fuzzy(`${g.id} ${desc(g.id)}`, q)) : []);
  const avail = $derived(info ? pickAvailable(info.available, new Set(info.installed.map((p) => p.id)), q) : { items: [], total: 0 });
  const mps = $derived(Object.entries(info?.marketplaces ?? {}).filter(([n]) => fuzzy(n, q)));
  const mpSource = (s: PluginsInfo["marketplaces"][string]["source"]) => s.repo ?? s.url ?? s.path ?? s.source;
</script>

{#snippet action(label: string, id: string, onclick: () => void, title = "")}
  <button
    class="border-border hover:bg-secondary shrink-0 rounded border px-1.5 py-0.5 text-[10px] disabled:opacity-50"
    {title}
    disabled={!!busy}
    {onclick}>{busy === id ? "…" : label}</button
  >
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <Notice bind:text={error} />
  <Notice bind:text={note} kind="note" />

  <div class="border-border flex flex-wrap items-center gap-2 border-b px-3 py-2">
    {@render lead?.()}
    <span class="flex-1"></span>
    <Input bind:value={query} placeholder="Plugin suchen" class="h-7 w-48 text-xs" />
    <button
      class="border-border hover:bg-secondary rounded border px-2 py-0.5 text-[11px] disabled:opacity-50"
      title="Marketplaces aktualisieren und nach neuen Versionen suchen"
      disabled={!!busy || !info}
      onclick={checkUpdates}>{busy === "updates" ? "Prüfe …" : "Updates prüfen"}</button
    >
    <RefreshButton loading={loading || busy === "updates"} disabled={loading || !!busy} onclick={reload} />
  </div>

  {#if loading && !info}
    <p class="text-muted-foreground px-3 py-4">Lese Plugins …</p>
  {:else if info}
    <div class="min-h-0 flex-1 overflow-y-auto">
      <SectionHead title={`Installiert (${groups.length})`} />
      {#if !groups.length}
        <p class="text-muted-foreground px-3 py-2">{q ? `Kein Plugin passt zu „${q}“.` : "Keine Plugins installiert."}</p>
      {/if}
      <ul>
        {#each groups as g (g.id)}
          {@const m = info.meta[g.id]}
          {@const p = g.main ?? g.locals[0]}
          <li class="flex items-center gap-3 px-3 py-1.5">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate font-mono text-[11px] font-semibold">{g.id}</span>
                <span class="text-muted-foreground shrink-0 font-mono text-[10px]"
                  >{p.version}{#if m?.update && m.latest}&nbsp;→ <span class="text-primary">{m.latest}</span>{/if}</span
                >
                {#if g.main}<span class="text-muted-foreground border-border rounded border px-1 text-[10px]">{SCOPE[g.main.scope]}</span>{/if}
              </div>
              {#if m?.description}<div class="text-muted-foreground truncate text-[11px]" title={m.description}>{m.description}</div>{/if}
              {#if g.locals.length}
                <div class="text-muted-foreground truncate text-[10px]" title={g.locals.map((l) => l.projectPath).join("\n")}>
                  {g.main ? "auch " : ""}lokal in: {g.locals.map((l) => folder(l.projectPath)).join(", ")}
                </div>
              {/if}
            </div>
            {#if m?.update}
              {@render action("Update", g.id, () => updateAll(g))}
            {:else if !m?.latest}
              <!-- Neueste Version unbekannt: CLI entscheidet selbst (up_to_date) -->
              {@render action("Update suchen", g.id, () => updateAll(g), "Neueste Version unbekannt")}
            {/if}
            {#if g.main}
              {@const main = g.main}
              <Switch
                on={main.enabled}
                label={g.id}
                disabled={!!busy}
                onclick={() => run(main.enabled ? "disable" : "enable", g.id, main.scope, main.projectPath ?? repo)}
              />
              <button
                class="text-muted-foreground hover:text-destructive shrink-0 disabled:opacity-50"
                title="Aus {SCOPE[main.scope]} entfernen"
                aria-label="Entfernen"
                disabled={!!busy}
                onclick={() => remove(main)}><Trash2Icon class="size-3.5" /></button
              >
            {:else}
              <span class="text-muted-foreground w-24 shrink-0 text-right text-[10px]">pro Projekt im Skills-Modus</span>
            {/if}
          </li>
        {/each}
      </ul>

      <SectionHead title={`Verfügbar (${avail.items.length} von ${avail.total})`}>
        <span class="text-muted-foreground text-[10px]">Scope</span>
        <Segmented
          value={scopeChoice}
          options={[
            ["User", "user"],
            ["Projekt", "project"],
          ] as const}
          onchange={(v) => (scopeChoice = v)}
          disabled={!!busy}
        />
      </SectionHead>
      {#if !avail.items.length}
        <p class="text-muted-foreground px-3 py-2">{q ? `Kein Plugin passt zu „${q}“.` : "Keine weiteren Plugins verfügbar."}</p>
      {/if}
      <ul>
        {#each avail.items as a (a.pluginId)}
          <li class="flex items-center gap-3 px-3 py-1.5">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate font-mono text-[11px] font-semibold">{a.name}</span>
                <span class="text-muted-foreground truncate text-[10px]">{a.marketplaceName}</span>
                {#if a.version}<span class="text-muted-foreground shrink-0 font-mono text-[10px]">v{a.version}</span>{/if}
                {#if a.installCount}<span class="text-muted-foreground shrink-0 text-[10px]">↓{a.installCount}</span>{/if}
              </div>
              {#if a.description}<div class="text-muted-foreground truncate text-[11px]" title={a.description}>{a.description}</div>{/if}
            </div>
            {@render action("Installieren", a.pluginId, () => run("install", a.pluginId, scopeChoice))}
          </li>
        {/each}
      </ul>

      <SectionHead title={`Marketplaces (${mps.length})`} />
      {#if !mps.length}
        <p class="text-muted-foreground px-3 py-2">
          {q ? `Kein Marketplace passt zu „${q}“.` : "Keine Marketplaces. Füge einen hinzu (z. B. anthropics/claude-plugins-official)."}
        </p>
      {/if}
      <ul>
        {#each mps as [name, mp] (name)}
          <li class="flex items-center gap-3 px-3 py-1.5">
            <span class="w-48 shrink-0 truncate font-mono text-[11px] font-semibold">{name}</span>
            <span class="text-muted-foreground min-w-0 flex-1 truncate text-[11px]" title={mp.installLocation}>{mpSource(mp.source)}</span>
            {@render action("Aktualisieren", name, () => run("mp-update", name))}
            {@render action("Entfernen", name, async () => {
              if (await confirm(`Marketplace ${name} entfernen? Seine Plugins sind danach nicht mehr installierbar.`, "Entfernen")) run("mp-remove", name);
            })}
          </li>
        {/each}
      </ul>
      <form
        class="flex items-center gap-2 px-3 py-2"
        onsubmit={(e) => {
          e.preventDefault();
          if (source.trim()) addMarketplace();
        }}
      >
        <Input bind:value={source} placeholder="owner/repo, https-/git-URL oder Pfad" class="h-7 flex-1 text-xs" />
        <button
          type="submit"
          class="border-border hover:bg-secondary rounded border px-2 py-0.5 text-[11px] disabled:opacity-50"
          disabled={!!busy || !source.trim()}>{busy === source.trim() && busy ? "…" : "Hinzufügen"}</button
        >
      </form>
    </div>
  {:else}
    <p class="text-muted-foreground px-3 py-4">Plugins konnten nicht gelesen werden.</p>
  {/if}
</div>
