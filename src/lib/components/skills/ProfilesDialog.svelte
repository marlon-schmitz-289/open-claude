<script lang="ts">
  import { untrack } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import Notice from "$lib/components/Notice.svelte";
  import { Segmented, Select } from "$lib/components/kit";
  import { fuzzy } from "$lib/fuzzy";
  import { skills, renameProfile, fromProject, adjust, emptyProject, type SkillStore, type SkillsInfo } from "$lib/skills";
  import TrashIcon from "@lucide/svelte/icons/trash-2";

  type Repo = { path: string; rel: string; kind?: string | null };

  let {
    open = $bindable(false),
    store,
    repos,
    repo = "",
    onsave,
  }: { open: boolean; store: SkillStore; repos: Repo[]; repo?: string; onsave: (next: SkillStore) => void } = $props();

  // Arbeitskopie: erst "Speichern" gibt sie zurueck (und loest den Sync aus).
  let draft = $state<SkillStore>({ profiles: {}, kindProfiles: {}, projects: {} });
  let sel = $state("");
  let name = $state("");
  let ctx = $state("");
  let info = $state<SkillsInfo | null>(null);
  let error = $state("");
  let query = $state("");

  // Nur beim Oeffnen uebernehmen, sonst verwirft ein Neu-Einlesen der Repos die Bearbeitung.
  $effect(() => {
    if (open)
      untrack(() => {
        draft = $state.snapshot(store);
        sel = Object.keys(draft.profiles).sort()[0] ?? "";
        name = query = error = "";
        ctx = repo || repos[0]?.path || "";
      });
  });

  // Skill-Liste kommt aus einem Projekt: User- und Plugin-Skills sind ueberall gleich, Projekt-Skills nur dort.
  $effect(() => {
    const p = ctx;
    if (!open || !p) return;
    skills.list(p).then(
      (i) => p === ctx && (info = i),
      (e) => (error = String(e)),
    );
  });

  const names = $derived(Object.keys(draft.profiles).sort());
  const kinds = $derived([...new Set(repos.map((r) => r.kind).filter((k): k is string => !!k))].sort());
  const profile = $derived(draft.profiles[sel]);
  const list = $derived(info?.skills.filter((sk) => fuzzy(sk.key, query.trim())) ?? []);
  const nameOk = $derived(!!name.trim() && !(name.trim() in draft.profiles));

  function create(copy: boolean) {
    const n = name.trim();
    if (!nameOk) return;
    const kind = repos.find((r) => r.path === ctx)?.kind;
    draft.profiles[n] = copy ? fromProject(draft, ctx, kind) : emptyProject();
    sel = n;
    name = "";
  }

  function rename(to: string | null) {
    if (to !== null && (!to.trim() || to.trim() in draft.profiles)) return;
    draft = renameProfile(draft, sel, to?.trim() ?? null);
    sel = to?.trim() ?? names[0] ?? "";
  }

  function setKind(kind: string, n: string) {
    if (n) draft.kindProfiles[kind] = n;
    else delete draft.kindProfiles[kind];
  }

  function save() {
    onsave($state.snapshot(draft));
    open = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="flex h-[85vh] flex-col sm:max-w-4xl">
    <Dialog.Header>
      <Dialog.Title>Skill-Profile</Dialog.Title>
      <Dialog.Description>
        Profile sind Vorlagen: Projekte nutzen sie per Projekttyp oder explizit, Anpassungen im Projekt ändern das Profil nie.
      </Dialog.Description>
    </Dialog.Header>
    <Notice bind:text={error} />

    <div class="flex min-h-0 flex-1 gap-4 text-xs">
      <div class="flex w-56 shrink-0 flex-col gap-3 overflow-y-auto">
        <div class="flex flex-col gap-0.5">
          {#each names as n (n)}
            <button
              class="truncate rounded px-2 py-1 text-left {sel === n ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-secondary'}"
              onclick={() => (sel = n)}>{n}</button
            >
          {:else}
            <p class="text-muted-foreground px-2">Noch keine Profile.</p>
          {/each}
        </div>

        <div class="flex flex-col gap-1.5">
          <Input bind:value={name} placeholder="Neues Profil" class="h-7 text-xs" onkeydown={(e) => e.key === "Enter" && create(false)} />
          <div class="flex gap-1.5">
            <Button size="sm" variant="outline" class="h-6 flex-1 text-[11px]" disabled={!nameOk} onclick={() => create(false)}>Leer</Button>
            <Button size="sm" variant="outline" class="h-6 flex-1 text-[11px]" disabled={!nameOk || !ctx} onclick={() => create(true)}
              >Aus Projekt</Button
            >
          </div>
        </div>

        <label class="flex flex-col gap-1">
          <span class="text-muted-foreground">Projekt (Skill-Liste, „Aus Projekt“)</span>
          <Select bind:value={ctx} options={repos.map((r) => [r.rel, r.path] as const)} />
        </label>

        {#if kinds.length}
          <div class="flex flex-col gap-1">
            <span class="text-muted-foreground">Projekttyp → Profil</span>
            {#each kinds as k (k)}
              <label class="flex items-center gap-2">
                <span class="flex-1 truncate">{k}</span>
                <Select
                  class="w-28"
                  bind:value={() => draft.kindProfiles[k] ?? "", (n) => setKind(k, n)}
                  options={[["kein Profil", ""], ...names.map((n) => [n, n] as const)]}
                  aria-label="Profil für {k}"
                />
              </label>
            {/each}
          </div>
        {/if}
      </div>

      <div class="border-border flex min-w-0 flex-1 flex-col gap-2 border-l pl-4">
        {#if profile}
          {#key sel}
            <div class="flex items-center gap-2">
              <Input
                value={sel}
                class="h-7 w-48 text-xs"
                title="Umbenennen mit Enter"
                onkeydown={(e) => e.key === "Enter" && rename(e.currentTarget.value)}
                onblur={(e) => e.currentTarget.value !== sel && rename(e.currentTarget.value)}
              />
              <span class="flex-1"></span>
              <Input bind:value={query} placeholder="Skill suchen" class="h-7 w-40 text-xs" />
              <Button variant="ghost" size="sm" class="text-destructive h-7" title="Profil löschen" onclick={() => rename(null)}>
                <TrashIcon class="size-3.5" />
              </Button>
            </div>
          {/key}
          <div class="min-h-0 flex-1 overflow-y-auto">
            {#if info}
              {#each info.plugins as id (id)}
                <div class="flex items-center gap-2 py-1">
                  <span class="flex-1 truncate font-mono text-[11px]">Plugin {id}</span>
                  <Segmented
                    value={profile.plugins[id]}
                    options={[
                      ["–", undefined, "Nicht festgelegt"],
                      ["an", true],
                      ["aus", false],
                    ] as const}
                    onchange={(v) => (draft.profiles[sel] = adjust(profile, "plugin", id, v))}
                  />
                </div>
              {/each}
              {#each list as sk (sk.key)}
                <div class="flex items-center gap-2 py-1">
                  <div class="min-w-0 flex-1">
                    <div class="truncate font-mono text-[11px]">{sk.key}</div>
                    {#if sk.description}<div class="text-muted-foreground truncate text-[11px]" title={sk.description}>{sk.description}</div>{/if}
                  </div>
                  <Segmented
                    value={profile.overrides[sk.key]}
                    options={[
                      ["–", undefined, "Nicht festgelegt"],
                      ["an", "on"],
                      ["aus", "off"],
                    ] as const}
                    onchange={(v) => (draft.profiles[sel] = adjust(profile, "skill", sk.key, v))}
                  />
                </div>
              {/each}
            {:else}
              <p class="text-muted-foreground">Lese Skills …</p>
            {/if}
          </div>
        {:else}
          <p class="text-muted-foreground">Profil anlegen oder links auswählen.</p>
        {/if}
      </div>
    </div>

    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (open = false)}>Abbrechen</Button>
      <Button size="sm" onclick={save}>Speichern</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
