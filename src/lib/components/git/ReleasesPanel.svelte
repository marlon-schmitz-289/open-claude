<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { untrack } from "svelte";
  import { age } from "$lib/utils";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Textarea } from "$lib/components/ui/textarea/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { git, forge, type ForgeKind, type Release, type ReleaseInput } from "$lib/git";
  import Splitter, { stored } from "$lib/components/Splitter.svelte";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import PackageIcon from "@lucide/svelte/icons/package";

  let { repo, refreshKey }: { repo: string; refreshKey: number } = $props();

  const PAGE = 30;

  let originUrl = $state<string | null>(null);
  let kind = $state<ForgeKind | null>(null);
  let items = $state<Release[]>([]);
  let page = $state(0);
  let done = $state(false);
  let loading = $state(false);
  /** Remote passt zu keinem Konto (ForgeList null). */
  let noAccount = $state(false);
  let error = $state("");
  let busy = $state(false);
  let selected = $state<Release | null>(null);
  let listW = $state(stored("git.releases", 280));

  // gen verwirft Antworten, die ein neueres reload() ueberholt hat.
  let gen = 0;

  const host = $derived(originUrl?.match(/^(?:[\w+]+:\/\/)?(?:[^@/]+@)?([^/:]+)/)?.[1] ?? "");
  const latestId = $derived(items.find((r) => !r.draft && !r.prerelease)?.id ?? null);

  async function reload(select: Release | null = selected) {
    const g = ++gen;
    loading = true;
    error = "";
    try {
      const remotes = await git.remotes(repo);
      if (g !== gen) return;
      originUrl = remotes.find((r) => r.name === "origin")?.url ?? null;
      const res = originUrl ? await forge.releases(repo, originUrl, 1) : null;
      if (g !== gen) return;
      noAccount = !!originUrl && !res;
      kind = res?.kind ?? null;
      items = res?.items ?? [];
      page = 1;
      done = !res || res.items.length < PAGE;
      // Gespeichertes Release kann ausserhalb der ersten Seite liegen: dann den gelieferten Stand zeigen.
      selected = select ? (items.find((r) => r.id === select.id) ?? (select === selected ? null : select)) : null;
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) loading = false;
    }
  }

  async function loadMore() {
    if (!originUrl || done || loading) return;
    const g = gen;
    loading = true;
    try {
      const res = await forge.releases(repo, originUrl, page + 1);
      if (g !== gen) return;
      const next = res?.items ?? [];
      items = [...items, ...next.filter((r) => !items.some((o) => o.id === r.id))];
      page++;
      done = next.length < PAGE;
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) loading = false;
    }
  }

  $effect(() => {
    repo;
    refreshKey;
    untrack(() => reload());
  });

  async function run(action: () => Promise<Release | null>) {
    busy = true;
    error = "";
    try {
      const r = await action();
      await reload(r);
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  // ---------- Formular ----------

  let form = $state<(ReleaseInput & { id: string | null }) | null>(null);
  let branch = $state("");

  function openForm(r: Release | null) {
    form = r
      ? { id: r.id, tag: r.tag, name: r.name, body: r.body, draft: r.draft, prerelease: r.prerelease, target: null }
      : { id: null, tag: "", name: "", body: "", draft: false, prerelease: false, target: null };
    git.status(repo).then((s) => (branch = s.branch ?? "")).catch(() => (branch = ""));
  }

  async function save() {
    if (!form || !originUrl || !form.tag.trim()) return;
    const url = originUrl;
    const { id, ...input } = form;
    const ok = await run(() =>
      forge.releaseSave(repo, url, id, {
        ...input,
        tag: input.tag.trim(),
        name: input.name.trim() || input.tag.trim(),
        target: input.target?.trim() || null,
      }),
    );
    if (ok) form = null;
  }

  function publish(r: Release) {
    const url = originUrl!;
    run(() =>
      forge.releaseSave(repo, url, r.id, { tag: r.tag, name: r.name, body: r.body, draft: false, prerelease: r.prerelease, target: null }),
    );
  }

  let confirmDelete = $state<Release | null>(null);
  async function doDelete() {
    const r = confirmDelete;
    confirmDelete = null;
    if (!r || !originUrl) return;
    const url = originUrl;
    await run(async () => {
      await forge.releaseDelete(repo, url, r.id);
      return null;
    });
  }

  function size(n: number) {
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < units.length - 1) (n /= 1024), i++;
    return `${i ? n.toFixed(1) : n} ${units[i]}`;
  }
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
  <Notice bind:text={error} />

  {#if !originUrl && !loading}
    <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Kein Remote origin.</p>
  {:else if noAccount}
    <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">
      Kein GitHub-/GitLab-Konto für {host} — unter Konten anmelden.
    </p>
  {:else}
    <div class="flex min-h-0 flex-1">
      <div class="border-border flex shrink-0 flex-col border-r" style="width:{listW}px; max-width:60%">
        <div class="border-border flex items-center gap-2 border-b px-2 py-1.5">
          <span class="text-muted-foreground flex-1 truncate">{host}</span>
          <Button size="sm" variant="outline" class="h-6 text-[11px]" disabled={busy || !kind} onclick={() => openForm(null)}>
            <PlusIcon class="size-3" />Neues Release
          </Button>
        </div>
        <div class="min-h-0 flex-1 overflow-y-auto" role="listbox" aria-label="Releases">
          {#each items as r (r.id)}
            <button
              role="option"
              aria-selected={selected?.id === r.id}
              class="hover:bg-accent flex w-full items-center gap-2 px-2 py-1 text-left {selected?.id === r.id ? 'bg-accent' : ''}"
              onclick={() => (selected = r)}
            >
              <span class="min-w-0 flex-1">
                <span class="block truncate text-[11px]">{r.name || r.tag}</span>
                <span class="text-muted-foreground block truncate font-mono text-[10px]">{r.tag}</span>
              </span>
              {#if r.draft}<Badge variant="outline" class="shrink-0 text-[10px]">Draft</Badge>{/if}
              {#if r.prerelease}<Badge variant="outline" class="shrink-0 text-[10px] text-amber-400 border-amber-400/40">Pre-Release</Badge>{/if}
              {#if r.id === latestId}<Badge variant="outline" class="text-primary border-primary/50 shrink-0 text-[10px]">Latest</Badge>{/if}
              <span class="text-muted-foreground w-10 shrink-0 text-right font-mono text-[10px] tabular-nums">
                {age(r.published_at ?? r.created_at)}
              </span>
            </button>
          {/each}
          {#if loading}
            <p class="text-muted-foreground px-2 py-2 text-center text-[11px]">Lädt …</p>
          {:else if !items.length}
            <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Noch keine Releases.</p>
          {:else if !done}
            <button class="text-muted-foreground hover:text-foreground w-full px-2 py-2 text-center text-[11px]" onclick={loadMore}>
              Mehr laden
            </button>
          {/if}
        </div>
      </div>
      <Splitter bind:size={listW} min={180} key="git.releases" />
      <div class="min-w-0 flex-1 overflow-y-auto">
        {#if selected}
          {@const r = selected}
          <div class="space-y-2 px-3 py-2">
            <div class="flex flex-wrap items-center gap-2">
              <h2 class="min-w-0 flex-1 truncate text-sm font-semibold">{r.name || r.tag}</h2>
              <Button size="sm" variant="ghost" class="h-6 text-[11px]" onclick={() => forge.openUrl(r.web_url).catch((e) => (error = String(e)))}>
                <ExternalLinkIcon class="size-3" />Im Browser
              </Button>
              {#if kind === "github" && r.draft}
                <Button size="sm" class="h-6 text-[11px]" disabled={busy} onclick={() => publish(r)}>Veröffentlichen</Button>
              {/if}
              <Button size="sm" variant="outline" class="h-6 text-[11px]" disabled={busy} onclick={() => openForm(r)}>Bearbeiten</Button>
              <Button size="sm" variant="destructive" class="h-6 text-[11px]" disabled={busy} onclick={() => (confirmDelete = r)}>Löschen</Button>
            </div>
            <p class="text-muted-foreground text-[10px]">
              <span class="font-mono">{r.tag}</span> · {r.author} · {new Date(r.published_at ?? r.created_at).toLocaleString("de-DE")}
            </p>
            <!-- Body ist fremdes Markdown: nur als Text, nie {@html} -->
            {#if r.body}
              <p class="whitespace-pre-wrap break-words text-[11px] select-text">{r.body}</p>
            {:else}
              <p class="text-muted-foreground text-[11px] italic">Keine Beschreibung.</p>
            {/if}
            {#if r.assets.length}
              <ul class="border-border space-y-0.5 border-t pt-2">
                {#each r.assets as a (a.url)}
                  <li>
                    <button
                      class="hover:bg-accent flex w-full items-center gap-2 rounded px-1 py-0.5 text-left text-[11px]"
                      onclick={() => forge.openUrl(a.url).catch((e) => (error = String(e)))}
                    >
                      <PackageIcon class="text-muted-foreground size-3 shrink-0" />
                      <span class="min-w-0 flex-1 truncate">{a.name}</span>
                      <span class="text-muted-foreground shrink-0 font-mono text-[10px]">{size(a.size)}</span>
                      {#if kind === "github"}
                        <span class="text-muted-foreground w-16 shrink-0 text-right font-mono text-[10px]">{a.downloads} ↓</span>
                      {/if}
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {:else if items.length}
          <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Release wählen.</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<Dialog.Root open={form !== null} onOpenChange={(o) => !o && !busy && (form = null)}>
  <Dialog.Content class="sm:max-w-lg">
    {#if form}
      {@const lockTag = form.id !== null && kind === "gitlab"}
      <Dialog.Header>
        <Dialog.Title>{form.id ? "Release bearbeiten" : "Neues Release"}</Dialog.Title>
      </Dialog.Header>
      <div class="space-y-2 text-xs">
        <label class="block space-y-1">
          <span class="text-muted-foreground">Tag</span>
          <Input bind:value={form.tag} placeholder="v1.0.0" disabled={lockTag} autofocus />
        </label>
        {#if !lockTag}
          <label class="block space-y-1">
            <span class="text-muted-foreground">Ziel, falls der Tag noch nicht existiert (Branch oder SHA)</span>
            <Input bind:value={form.target} placeholder={branch || "HEAD"} />
          </label>
        {/if}
        <label class="block space-y-1">
          <span class="text-muted-foreground">Name</span>
          <Input bind:value={form.name} placeholder={form.tag || "Name"} />
        </label>
        <label class="block space-y-1">
          <span class="text-muted-foreground">Beschreibung (Markdown)</span>
          <Textarea bind:value={form.body} rows={10} class="font-mono text-[11px]" />
        </label>
        {#if kind === "github"}
          <div class="flex gap-4">
            <label class="flex items-center gap-1.5">
              <input type="checkbox" bind:checked={form.draft} class="accent-primary" />Draft
            </label>
            <label class="flex items-center gap-1.5">
              <input type="checkbox" bind:checked={form.prerelease} class="accent-primary" />Pre-Release
            </label>
          </div>
        {/if}
      </div>
      <div class="flex justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={busy} onclick={() => (form = null)}>Abbrechen</Button>
        <Button size="sm" disabled={busy || !form.tag.trim()} onclick={save}>{busy ? "Speichert …" : "Speichern"}</Button>
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={confirmDelete !== null} onOpenChange={(o) => !o && (confirmDelete = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Release löschen</Dialog.Title>
      <Dialog.Description>
        Release „{confirmDelete?.name || confirmDelete?.tag}“ löschen? Der Git-Tag {confirmDelete?.tag} bleibt erhalten.
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (confirmDelete = null)}>Abbrechen</Button>
      <Button variant="destructive" size="sm" onclick={doDelete}>Löschen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
