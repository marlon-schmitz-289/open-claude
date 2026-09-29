<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { untrack } from "svelte";
  import { Input } from "$lib/components/ui/input/index.js";
  import { fuzzy } from "$lib/fuzzy";
  import { skills, layers, resolve, adjust, emptyProject, pkey, type SkillsInfo, type SkillStore, type ProjectSkills, type Skill, type Source } from "$lib/skills";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";

  let {
    repo,
    kind = null,
    store: s,
    onproject,
  }: { repo: string; kind?: string | null; store: SkillStore; onproject: (ps: ProjectSkills) => void } = $props();

  let info = $state<SkillsInfo | null>(null);
  let loading = $state(true);
  let error = $state("");
  let query = $state("");

  async function reload() {
    loading = true;
    error = "";
    try {
      info = await skills.list(repo);
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  $effect(() => {
    repo;
    untrack(reload);
  });

  const ps = $derived(s.projects[pkey(repo)]);
  const l = $derived(info && layers(info, s, repo, kind));
  const byKind = $derived((kind && s.kindProfiles[kind]) || "kein Profil");
  const names = $derived(Object.keys(s.profiles).sort());

  const match = (sk: Skill) => fuzzy(sk.key, query.trim());
  const own = $derived(info?.skills.filter((sk) => !sk.plugin && match(sk)) ?? []);
  const plugins = $derived(
    (info?.plugins ?? [])
      .map((id) => ({ id, items: info!.skills.filter((sk) => sk.plugin === id && match(sk)) }))
      .filter((g) => g.items.length || fuzzy(g.id, query.trim())),
  );

  const ORIGIN: Record<Source, string> = { default: "Standard", user: "User", team: "Team", profile: "Profil", project: "angepasst" };
  const SOURCE = { user: "User", project: "Projekt", plugin: "Plugin" };

  // Select-Werte: "" = nach Typ, NONE = kein Profil, sonst der Name (Namen sind nie leer).
  const NONE = "\0";
  function setProfile(v: string) {
    const { profile: _, ...rest } = ps ?? emptyProject();
    onproject(v === "" ? rest : { ...rest, profile: v === NONE ? null : v });
  }
</script>

{#snippet toggle(on: boolean, onclick: () => void, label: string)}
  <button
    class="relative h-3.5 w-6 shrink-0 rounded-full transition-colors {on ? 'bg-primary' : 'bg-secondary ring-border ring-1'}"
    role="switch"
    aria-checked={on}
    aria-label={label}
    {onclick}
    ><span class="bg-foreground absolute top-0.5 size-2.5 rounded-full transition-all {on ? 'left-3' : 'left-0.5'}"></span></button
  >
{/snippet}

{#snippet origin(source: Source, reset: () => void)}
  <span class="w-16 shrink-0 text-right text-[10px] {source === 'project' ? 'text-primary' : 'text-muted-foreground'}"
    >{ORIGIN[source]}</span
  >
  <button
    class="text-muted-foreground hover:text-foreground shrink-0 {source === 'project' ? '' : 'invisible'}"
    title="Projekt-Anpassung zurücksetzen"
    aria-label="Zurücksetzen"
    onclick={reset}><RotateCcwIcon class="size-3" /></button
  >
{/snippet}

{#snippet row(sk: Skill, dim: boolean)}
  {@const v = resolve("skill", sk.key, l!)}
  <li class="flex items-center gap-3 px-3 py-1.5 {dim ? 'opacity-50' : ''}">
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2">
        <span class="truncate font-mono text-[11px] font-semibold">{sk.plugin ? sk.name : sk.key}</span>
        <span class="text-muted-foreground border-border rounded border px-1 text-[10px]">{SOURCE[sk.source]}</span>
        {#if v.value !== "on" && v.value !== "off"}<span class="text-muted-foreground text-[10px]">{v.value}</span>{/if}
      </div>
      {#if sk.description}<div class="text-muted-foreground truncate text-[11px]" title={sk.description}>{sk.description}</div>{/if}
    </div>
    {@render origin(v.source, () => onproject(adjust(ps, "skill", sk.key)))}
    {@render toggle(v.value !== "off", () => onproject(adjust(ps, "skill", sk.key, v.value === "off" ? "on" : "off")), sk.key)}
  </li>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <Notice bind:text={error} />

  <div class="border-border flex flex-wrap items-center gap-2 border-b px-3 py-2">
    <span class="text-muted-foreground">Profil</span>
    <select
      class="bg-secondary border-border rounded border px-1.5 py-0.5 text-xs"
      bind:value={() => (ps?.profile === undefined ? "" : (ps.profile ?? NONE)), setProfile}
    >
      <option value="">Nach Typ ({byKind})</option>
      <option value={NONE}>Kein Profil</option>
      {#each names as n (n)}<option value={n}>{n}</option>{/each}
    </select>
    <span class="flex-1"></span>
    <Input bind:value={query} placeholder="Skill suchen" class="h-7 w-48 text-xs" />
    <button
      class="text-muted-foreground hover:text-foreground disabled:opacity-50"
      title="Neu laden"
      disabled={loading}
      onclick={reload}><RefreshCwIcon class="size-3.5 {loading ? 'animate-spin' : ''}" /></button
    >
  </div>

  {#if loading && !info}
    <p class="text-muted-foreground px-3 py-4">Lese Skills …</p>
  {:else if l}
    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if !own.length && !plugins.length}
        <p class="text-muted-foreground px-3 py-4">{query.trim() ? `Kein Skill passt zu „${query.trim()}“.` : "Keine Skills gefunden."}</p>
      {/if}
      <ul>
        {#each own as sk (sk.key)}{@render row(sk, false)}{/each}
      </ul>
      {#each plugins as g (g.id)}
        {@const pv = resolve("plugin", g.id, l)}
        <div class="bg-chrome border-border flex items-center gap-3 border-y px-3 py-1.5">
          <span class="flex-1 truncate font-mono text-[11px] font-semibold">{g.id}</span>
          {@render origin(pv.source, () => onproject(adjust(ps, "plugin", g.id)))}
          {@render toggle(pv.value, () => onproject(adjust(ps, "plugin", g.id, !pv.value)), g.id)}
        </div>
        <ul>
          {#each g.items as sk (sk.key)}{@render row(sk, !pv.value)}{/each}
        </ul>
      {/each}
    </div>
  {/if}
</div>
