<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { untrack } from "svelte";
  import { ContextMenu, DropdownMenu } from "bits-ui";
  import SettingsIcon from "@lucide/svelte/icons/settings-2";
  import CheckIcon from "@lucide/svelte/icons/check";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { git, forge, type Branch, type Tag, type Stash, type PullRequest, type Status } from "$lib/git";
  import GitBranchIcon from "@lucide/svelte/icons/git-branch";
  import CloudIcon from "@lucide/svelte/icons/cloud";
  import TagIcon from "@lucide/svelte/icons/tag";
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import GitPullRequestIcon from "@lucide/svelte/icons/git-pull-request";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import GitMergeIcon from "@lucide/svelte/icons/git-merge";
  import { age } from "$lib/utils";

  let {
    repo,
    status,
    refreshKey,
    onchange,
    onselectref,
    onshowdiff,
  }: {
    repo: string;
    status: Status;
    refreshKey: number;
    onchange: () => void;
    onselectref: (ref: string) => void;
    onshowdiff: (title: string, diff: string) => void;
  } = $props();

  let branches = $state<Branch[]>([]);
  let tags = $state<Tag[]>([]);
  let stashes = $state<Stash[]>([]);
  let originUrl = $state<string | null>(null);
  let pulls = $state<PullRequest[]>([]);
  let error = $state("");

  // Abschnitte einzeln einklappbar, Tags/Stashes zu Beginn eingeklappt.
  let collapsed = $state<Record<string, boolean>>({ tags: true, stashes: true });

  const locals = $derived(branches.filter((b) => !b.remote));
  const remoteGroups = $derived.by(() => {
    const by = new Map<string, Branch[]>();
    for (const b of branches.filter((b) => b.remote)) {
      const i = b.name.indexOf("/");
      const remote = i < 0 ? b.name : b.name.slice(0, i);
      (by.get(remote) ?? by.set(remote, []).get(remote)!).push(b);
    }
    return [...by];
  });

  const base = $derived(branches.find((b) => b.base)?.base ?? null);
  // Fest eingestellt (git config ocui.base) statt automatisch erkannt.
  let fixedBase = $state<string | null>(null);
  // Uebliche Namen fuer den Production-Branch als Schnellauswahl, Remote bevorzugt.
  const PROD = ["main", "master", "stable", "production", "prod", "release", "develop"];
  const candidates = $derived(
    PROD.map((n) => branches.find((b) => b.name === `origin/${n}`) ?? branches.find((b) => b.name === n))
      .filter((b) => b !== undefined)
      .map((b) => b.name),
  );
  const setBase = (b: string | null) => run(() => git.setBase(repo, b));
  // Die Basis selbst und ihr lokales Gegenstueck (master -> origin/master) sind nie "offen".
  const isBase = (b: Branch) => b.name === base || b.upstream === base;
  const merged = (b: Branch) => b.unmerged === 0 && !isBase(b) && !b.current;
  // Noch nicht gemergt, neueste zuerst. Remote-Branch nur, wenn ihn kein lokaler mit gleichem Stand trackt.
  const allOpen = $derived(
    branches
      .filter(
        (b) =>
          (b.unmerged ?? 0) > 0 &&
          !isBase(b) &&
          !(b.remote && locals.some((l) => l.upstream === b.name && l.sha === b.sha)),
      )
      .sort((a, b) => Date.parse(b.date) - Date.parse(a.date)),
  );
  // Standardmaessig nur Branches mit Commits aus den letzten 6 Monaten, aeltere per Klick dazu.
  const STALE_MS = 183 * 864e5;
  const stale = (b: Branch) => Date.now() - Date.parse(b.date) > STALE_MS;
  let showStale = $state(false);
  const staleCount = $derived(allOpen.filter(stale).length);
  const open = $derived(showStale ? allOpen : allOpen.filter((b) => !stale(b)));

  // Branch -> offener Branch, der ihn schon enthaelt (kommt mit dessen Merge automatisch in die Basis).
  let within = $state<Record<string, string>>({});
  $effect(() => {
    const list = open;
    untrack(() => loadWithin(list));
  });
  async function loadWithin(list: Branch[]) {
    const nest = await git.nesting(repo, list.map((b) => b.name)).catch(() => ({}) as Record<string, string[]>);
    const res: Record<string, string> = {};
    for (const b of list) {
      const has = new Set(nest[b.name] ?? []);
      // Engster Treffer = der mit den wenigsten offenen Commits. Gleicher Stand ist ein Duplikat, kein "enthalten".
      const parent = list
        .filter((o) => o.name !== b.name && o.sha !== b.sha && has.has(o.name))
        .sort((x, y) => (x.unmerged ?? 0) - (y.unmerged ?? 0))[0];
      if (parent) res[b.name] = parent.name;
    }
    if (list === open) within = res; // inzwischen neu geladen: Ergebnis verwerfen
  }
  // Baum: enthaltene Branches eingerueckt unter dem, der sie mitbringt.
  const tree = $derived.by(() => {
    const out: { b: Branch; depth: number }[] = [];
    const walk = (parent: string | null, depth: number) => {
      for (const b of open.filter((o) => (within[o.name] ?? null) === parent)) {
        out.push({ b, depth });
        walk(b.name, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  });
  const merges = $derived(tree.filter((t) => t.depth === 0).length);

  async function load() {
    error = "";
    try {
      const [b, t, s, remotes, fixed] = await Promise.all([
        git.branches(repo),
        git.tags(repo),
        git.stashes(repo),
        git.remotes(repo),
        git.getBase(repo),
      ]);
      branches = b;
      fixedBase = fixed;
      tags = t;
      stashes = s;
      originUrl = remotes.find((r) => r.name === "origin")?.url ?? null;
      pulls = originUrl ? await forge.pulls(originUrl).catch(() => []) : [];
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    repo;
    refreshKey;
    load();
  });

  async function run<T>(action: () => Promise<T>) {
    error = "";
    try {
      await action();
      onchange();
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  // Bestaetigung fuer destruktive Aktionen.
  let confirm = $state<{ title: string; message: string; run: () => Promise<void> } | null>(null);
  function askConfirm(title: string, message: string, action: () => Promise<void>) {
    confirm = { title, message, run: action };
  }
  async function doConfirm() {
    if (!confirm) return;
    const action = confirm.run;
    confirm = null;
    await run(action);
  }

  // Texteingabe fuer Umbenennen / Neuer Branch.
  let prompt = $state<{ title: string; run: (v: string) => Promise<void> } | null>(null);
  let promptValue = $state("");
  function askPrompt(title: string, value: string, action: (v: string) => Promise<void>) {
    prompt = { title, run: action };
    promptValue = value;
  }
  async function doPrompt() {
    if (!prompt) return;
    const action = prompt.run;
    const value = promptValue.trim();
    prompt = null;
    if (value) await run(() => action(value));
  }

  function checkoutLocal(b: Branch) {
    run(() => git.checkout(repo, b.name, false, null));
  }
  function checkoutRemote(b: Branch) {
    // Das Backend wechselt auf einen vorhandenen lokalen Branch oder legt den Tracking-Branch an
    // (Name ueber die echten Remotes, auch bei "up/stream/x").
    run(() => git.checkout(repo, b.name, false, null));
  }

  async function showStash(s: Stash) {
    try {
      const diff = await git.stashShow(repo, s.index);
      onshowdiff(s.message, diff);
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#snippet menuItem(label: string, onSelect: () => void, destructive = false)}
  <ContextMenu.Item
    class="hover:bg-accent flex cursor-pointer items-center rounded px-2 py-1 {destructive ? 'text-destructive' : ''}"
    {onSelect}
  >
    {label}
  </ContextMenu.Item>
{/snippet}

{#snippet choice(label: string, value: string | null)}
  <DropdownMenu.Item
    class="data-highlighted:bg-accent flex cursor-pointer items-center gap-2 rounded px-2 py-1"
    onSelect={() => setBase(value)}
  >
    <CheckIcon class="size-3 {fixedBase === value ? '' : 'invisible'}" />
    <span class={value ? "font-mono" : ""}>{label}</span>
  </DropdownMenu.Item>
{/snippet}

<div class="flex h-full flex-col overflow-y-auto text-xs">
  <Notice bind:text={error} />

  <!-- Noch nicht in die Basis gemergt: Ueberblick ueber alle offenen Branches -->
  {#if base}
    <div class="border-border border-b">
      <div class="hover:bg-accent flex items-center pr-1">
        <button
          class="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
          onclick={() => (collapsed.open = !collapsed.open)}
          aria-expanded={!collapsed.open}
          title="Branches mit Commits, die noch nicht in {base} sind{merges < open.length
            ? ` · ${merges} Merges reichen, eingerückte kommen mit`
            : ''}"
        >
          <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.open ? '' : 'rotate-90'}" />
          <GitMergeIcon class="size-3.5 shrink-0" />
          <span class="min-w-0 flex-1 truncate">Nicht in <span class="font-mono">{base}</span></span>
          <span class="font-mono text-[10px] {open.length ? 'text-amber-400' : 'text-muted-foreground'}"
            >{open.length}</span
          >
        </button>
        <!-- Production-Branch waehlen: automatisch oder fest pro Repo -->
        <DropdownMenu.Root>
          <DropdownMenu.Trigger
            class="text-muted-foreground hover:text-foreground grid size-5 place-items-center rounded {fixedBase
              ? 'text-primary'
              : ''}"
            title={fixedBase ? `Production-Branch fest: ${fixedBase}` : "Production-Branch wählen"}
            aria-label="Production-Branch wählen"><SettingsIcon class="size-3" /></DropdownMenu.Trigger
          >
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              align="end"
              class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-52 rounded-md p-1 text-xs ring-1"
            >
              <p class="text-muted-foreground px-2 py-1 text-[10px]">Production-Branch</p>
              {@render choice("Automatisch erkennen", null)}
              {#each candidates as c (c)}
                {@render choice(c, c)}
              {/each}
              {#if fixedBase && !candidates.includes(fixedBase)}
                {@render choice(fixedBase, fixedBase)}
              {/if}
              <p class="text-muted-foreground px-2 pt-1.5 pb-1 text-[10px]">Andere: Rechtsklick auf einen Branch.</p>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
      {#if !collapsed.open}
        <div class="pb-1">
          {#if open.length === 0}
            <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">
              {allOpen.length ? "Nichts aus den letzten 6 Monaten." : "Alles gemergt."}
            </p>
          {/if}
          {#each tree as { b, depth } (b.name)}
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 text-left text-[11px] {stale(b)
                ? 'opacity-50'
                : ''}"
              style="padding-left:{24 + depth * 12}px"
              onclick={() => onselectref(b.sha)}
              ondblclick={() => (b.remote ? checkoutRemote(b) : checkoutLocal(b))}
              title="{b.name}: {b.unmerged} {b.unmerged === 1 ? 'Commit' : 'Commits'} noch nicht in {base}{within[b.name]
                ? ` · schon in ${within[b.name]}, kommt mit dessen Merge`
                : ''} · letzter: {b.subject}"
            >
              {#if depth}<span class="text-muted-foreground -ml-1 shrink-0">↳</span>{/if}
              <!-- Remote-Praefix als Icon, sonst schneidet "origin/" den eigentlichen Namen ab -->
              {#if b.remote}<CloudIcon class="text-muted-foreground size-3 shrink-0" />{/if}
              <span class="min-w-0 flex-1 truncate font-mono {b.remote ? 'text-muted-foreground' : ''} {b.current
                  ? 'text-primary font-semibold'
                  : ''}">{b.remote ? b.name.slice(b.name.indexOf("/") + 1) : b.name}</span
              >
              <span class="shrink-0 font-mono text-[10px] text-amber-400">+{b.unmerged}</span>
              <span class="text-muted-foreground w-9 shrink-0 text-right font-mono text-[10px]">{age(b.date)}</span>
            </button>
          {/each}
          {#if staleCount}
            <button
              class="text-muted-foreground hover:text-foreground px-2 py-1 pl-6 text-left text-[10px]"
              onclick={() => (showStale = !showStale)}
            >
              {showStale ? "Ältere ausblenden" : `${staleCount} ältere (über 6 Monate) anzeigen`}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Branches -->
  <div class="border-border border-b">
    <button
      class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
      onclick={() => (collapsed.branches = !collapsed.branches)}
      aria-expanded={!collapsed.branches}
    >
      <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.branches ? '' : 'rotate-90'}" />
      <GitBranchIcon class="size-3.5" />
      <span class="flex-1">Branches</span>
      <span class="text-muted-foreground font-mono text-[10px]">{locals.length}</span>
    </button>
    {#if !collapsed.branches}
      <div class="pb-1">
        {#if locals.length === 0}
          <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">Keine.</p>
        {/if}
        {#each locals as b (b.name)}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              <button
                class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 pl-6 text-left text-[11px] {b.current
                  ? 'text-primary font-semibold'
                  : ''} {merged(b) ? 'opacity-50' : ''}"
                ondblclick={() => checkoutLocal(b)}
                onclick={() => onselectref(b.sha)}
                title={merged(b) ? `In ${base} gemergt` : undefined}
              >
                <span class="min-w-0 flex-1 truncate font-mono">{b.name}</span>
                {#if b.ahead}
                  <span class="text-primary flex items-center font-mono text-[10px]"
                    ><ArrowUpIcon class="size-2.5" />{b.ahead}</span
                  >
                {/if}
                {#if b.behind}
                  <span class="text-muted-foreground flex items-center font-mono text-[10px]"
                    ><ArrowDownIcon class="size-2.5" />{b.behind}</span
                  >
                {/if}
              </button>
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content
                class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
              >
                {@render menuItem("Auschecken", () => checkoutLocal(b))}
                {#if !b.current}
                  {@render menuItem("In aktuellen mergen", () => run(() => git.merge(repo, b.name, false)))}
                  {@render menuItem("Aktuellen darauf rebasen", () => run(() => git.rebase(repo, b.name)))}
                {/if}
                {#if b.name !== base}
                  {@render menuItem("Als Production-Branch festlegen", () => setBase(b.name))}
                {/if}
                {@render menuItem("Umbenennen", () =>
                  askPrompt("Branch umbenennen", b.name, (v) => git.branchRename(repo, b.name, v)),
                )}
                {@render menuItem("Neuer Branch ab hier", () =>
                  askPrompt("Neuer Branch ab " + b.name, "", async (v) => {
                    await git.checkout(repo, v, true, b.name);
                  }),
                )}
                {#if !b.current}
                  {@render menuItem(
                    "Löschen",
                    () =>
                      askConfirm("Branch löschen", `„${b.name}“ lokal löschen?`, async () => {
                        try {
                          await git.branchDelete(repo, b.name, false, false);
                        } catch (e) {
                          if (!String(e).includes("not fully merged")) throw e;
                          askConfirm(
                            "Branch nicht gemergt",
                            `„${b.name}“ enthält Commits, die in keinem anderen Branch liegen. Trotzdem löschen?`,
                            async () => void (await git.branchDelete(repo, b.name, true, false)),
                          );
                        }
                      }),
                    true,
                  )}
                {/if}
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Remotes -->
  <div class="border-border border-b">
    <button
      class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
      onclick={() => (collapsed.remotes = !collapsed.remotes)}
      aria-expanded={!collapsed.remotes}
    >
      <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.remotes ? '' : 'rotate-90'}" />
      <CloudIcon class="size-3.5" />
      <span class="flex-1">Remotes</span>
      <span class="text-muted-foreground font-mono text-[10px]">{branches.length - locals.length}</span>
    </button>
    {#if !collapsed.remotes}
      <div class="pb-1">
        {#if remoteGroups.length === 0}
          <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">Keine.</p>
        {/if}
        {#each remoteGroups as [remote, list] (remote)}
          <p class="text-muted-foreground px-2 pt-1 pl-6 font-mono text-[10px]">{remote}</p>
          {#each list as b (b.name)}
            <ContextMenu.Root>
              <ContextMenu.Trigger>
                <button
                  class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 pl-6 text-left text-[11px] {merged(b)
                    ? 'opacity-50'
                    : ''}"
                  ondblclick={() => checkoutRemote(b)}
                  onclick={() => onselectref(b.sha)}
                  title={merged(b) ? `In ${base} gemergt` : undefined}
                >
                  <span class="min-w-0 flex-1 truncate font-mono">{b.name.slice(remote.length + 1)}</span>
                </button>
              </ContextMenu.Trigger>
              <ContextMenu.Portal>
                <ContextMenu.Content
                  class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
                >
                  {@render menuItem("Auschecken (Tracking-Branch)", () => checkoutRemote(b))}
                  {#if b.name !== base}
                    {@render menuItem("Als Production-Branch festlegen", () => setBase(b.name))}
                  {/if}
                  {@render menuItem(
                    "Löschen (remote)",
                    () =>
                      askConfirm("Remote-Branch löschen", `„${b.name}“ auf dem Server löschen?`, async () => {
                        await git.branchDelete(repo, b.name, false, true);
                      }),
                    true,
                  )}
                </ContextMenu.Content>
              </ContextMenu.Portal>
            </ContextMenu.Root>
          {/each}
        {/each}
      </div>
    {/if}
  </div>

  <!-- Tags -->
  <div class="border-border border-b">
    <button
      class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
      onclick={() => (collapsed.tags = !collapsed.tags)}
      aria-expanded={!collapsed.tags}
    >
      <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.tags ? '' : 'rotate-90'}" />
      <TagIcon class="size-3.5" />
      <span class="flex-1">Tags</span>
      <span class="text-muted-foreground font-mono text-[10px]">{tags.length}</span>
    </button>
    {#if !collapsed.tags}
      <div class="pb-1">
        {#if tags.length === 0}
          <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">Keine.</p>
        {/if}
        {#each tags as t (t.name)}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              <button
                class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 pl-6 text-left text-[11px]"
                onclick={() => onselectref(t.sha)}
              >
                <span class="min-w-0 flex-1 truncate font-mono">{t.name}</span>
              </button>
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content
                class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
              >
                {@render menuItem(
                  "Löschen",
                  () => askConfirm("Tag löschen", `Tag „${t.name}“ löschen?`, () => git.tagDelete(repo, t.name)),
                  true,
                )}
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Stashes -->
  <div class="border-border border-b">
    <button
      class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
      onclick={() => (collapsed.stashes = !collapsed.stashes)}
      aria-expanded={!collapsed.stashes}
    >
      <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.stashes ? '' : 'rotate-90'}" />
      <ArchiveIcon class="size-3.5" />
      <span class="flex-1">Stashes</span>
      <span class="text-muted-foreground font-mono text-[10px]">{stashes.length}</span>
    </button>
    {#if !collapsed.stashes}
      <div class="pb-1">
        {#if stashes.length === 0}
          <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">Keine.</p>
        {/if}
        {#each stashes as s (s.index)}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              <button
                class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 pl-6 text-left text-[11px]"
                onclick={() => showStash(s)}
              >
                <span class="min-w-0 flex-1 truncate">{s.message}</span>
                <span class="text-muted-foreground font-mono text-[10px]">{s.branch}</span>
              </button>
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content
                class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
              >
                {@render menuItem("Anwenden", () => run(() => git.stashApply(repo, s.index, false)))}
                {@render menuItem("Pop", () => run(() => git.stashApply(repo, s.index, true)))}
                {@render menuItem("Anzeigen", () => showStash(s))}
                {@render menuItem(
                  "Löschen",
                  () => askConfirm("Stash löschen", `„${s.message}“ verwerfen?`, () => git.stashDrop(repo, s.index)),
                  true,
                )}
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Pull Requests (nur wenn origin bekannt) -->
  {#if originUrl}
    <div class="border-border border-b">
      <button
        class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-semibold"
        onclick={() => (collapsed.pulls = !collapsed.pulls)}
        aria-expanded={!collapsed.pulls}
      >
        <ChevronRightIcon class="size-3 shrink-0 transition-transform {collapsed.pulls ? '' : 'rotate-90'}" />
        <GitPullRequestIcon class="size-3.5" />
        <span class="flex-1">Pull Requests</span>
        <span class="text-muted-foreground font-mono text-[10px]">{pulls.length}</span>
      </button>
      {#if !collapsed.pulls}
        <div class="pb-1">
          {#if pulls.length === 0}
            <p class="text-muted-foreground px-2 py-1 pl-6 text-[11px]">Keine offenen.</p>
          {/if}
          {#each pulls as p (p.number)}
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-1 pl-6 text-left text-[11px]"
              onclick={() => forge.openUrl(p.web_url)}
              title={p.title}
            >
              <span class="text-muted-foreground font-mono">#{p.number}</span>
              <span class="min-w-0 flex-1 truncate">{p.title}</span>
              {#if p.draft}<Badge variant="outline" class="text-muted-foreground text-[10px]">Entwurf</Badge>{/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<Dialog.Root open={confirm !== null} onOpenChange={(o) => !o && (confirm = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{confirm?.title}</Dialog.Title>
      <Dialog.Description>{confirm?.message}</Dialog.Description>
    </Dialog.Header>
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (confirm = null)}>Abbrechen</Button>
      <Button variant="destructive" size="sm" onclick={doConfirm}>Löschen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={prompt !== null} onOpenChange={(o) => !o && (prompt = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{prompt?.title}</Dialog.Title>
    </Dialog.Header>
    <Input bind:value={promptValue} onkeydown={(e) => e.key === "Enter" && doPrompt()} autofocus />
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (prompt = null)}>Abbrechen</Button>
      <Button size="sm" onclick={doPrompt}>OK</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
