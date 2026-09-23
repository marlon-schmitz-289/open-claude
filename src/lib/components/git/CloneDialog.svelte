<script lang="ts">
  import { age } from "$lib/utils";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { forge, type Account, type RemoteRepo, type CloneProgress } from "$lib/git";
  import { fuzzy } from "$lib/fuzzy";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import SearchIcon from "@lucide/svelte/icons/search";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import StarIcon from "@lucide/svelte/icons/star";
  import LockIcon from "@lucide/svelte/icons/lock";
  import GitForkIcon from "@lucide/svelte/icons/git-fork";
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import CheckIcon from "@lucide/svelte/icons/check";
  import Loader2Icon from "@lucide/svelte/icons/loader-2";

  let {
    open = $bindable(false),
    accounts,
    root,
    existing,
    onclone,
    onaccounts,
  }: {
    open: boolean;
    accounts: Account[];
    root: string;
    existing: string[];
    onclone: (path: string) => void;
    onaccounts: () => void;
  } = $props();

  let mode = $state<"browse" | "url">("browse");
  let account = $state<Account | null>(null);
  let query = $state("");
  let own = $state<RemoteRepo[]>([]);
  let page = $state(1);
  let searched = $state<RemoteRepo[] | null>(null); // null = noch keine Fernsuche
  let loading = $state(false);
  let error = $state("");

  let selected = $state<RemoteRepo | null>(null);
  let url = $state("");
  let protocol = $state<"https" | "ssh">("https");
  let dest = $state("");
  let destTouched = false; // sobald der Nutzer den Zielordner selbst aendert, nicht mehr automatisch mitfuehren

  let cloneId = $state("");
  let progress = $state<CloneProgress | null>(null);
  let cloning = $state(false);

  const existingNames = $derived(
    new Set(existing.map((p) => p.replace(/\\/g, "/").split("/").pop()?.toLowerCase())),
  );

  function nameFromUrl(u: string): string {
    return (
      u
        .trim()
        .replace(/\.git$/, "")
        .replace(/\/$/, "")
        .split(/[\\/:]/)
        .pop() ?? ""
    );
  }

  // Zielordner nachfuehren, solange der Nutzer ihn nicht selbst angefasst hat.
  $effect(() => {
    if (destTouched) return;
    const name = mode === "url" ? nameFromUrl(url) : (selected?.name ?? "");
    dest = name ? `${root}/${name}` : "";
  });

  async function selectAccount(a: Account) {
    account = a;
    mode = "browse";
    selected = null;
    own = [];
    searched = null;
    query = "";
    page = 1;
    error = "";
    await loadOwn();
  }

  async function loadOwn() {
    if (!account) return;
    loading = true;
    error = "";
    try {
      const more = await forge.repos(account.kind, account.host, "", page);
      own = page === 1 ? more : [...own, ...more];
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  async function searchRemote() {
    if (!account || !query.trim()) {
      searched = null;
      return;
    }
    loading = true;
    error = "";
    try {
      searched = await forge.repos(account.kind, account.host, query.trim(), 1);
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  const localHits = $derived.by(() => {
    const q = query.trim();
    if (!q) return own;
    return own
      .map((r) => ({ r, m: fuzzy(r.full_name, q) }))
      .filter((x): x is { r: RemoteRepo; m: NonNullable<ReturnType<typeof fuzzy>> } => x.m !== null)
      .sort((a, b) => b.m.score - a.m.score)
      .map((x) => x.r);
  });

  const results = $derived(searched ?? localHits);

  function pick(r: RemoteRepo) {
    selected = r;
    destTouched = false;
  }

  async function browseDest() {
    const picked = await pickFolder({ directory: true, defaultPath: dest || root });
    if (typeof picked !== "string") return;
    dest = picked;
    destTouched = true;
  }

  async function doClone() {
    const cloneUrl = mode === "url" ? url.trim() : protocol === "https" ? selected?.clone_http : selected?.clone_ssh;
    if (!cloneUrl || !dest.trim()) return;

    cloning = true;
    error = "";
    progress = null;
    cloneId = crypto.randomUUID();
    const unlisten = await listen<CloneProgress>(`clone:${cloneId}`, (e) => (progress = e.payload));
    try {
      const path = await forge.clone(cloneId, cloneUrl, dest.trim());
      onclone(path);
      open = false;
      reset();
    } catch (e) {
      error = String(e);
    } finally {
      unlisten();
      cloning = false;
    }
  }

  function reset() {
    mode = "browse";
    account = null;
    query = "";
    own = [];
    searched = null;
    selected = null;
    url = "";
    dest = "";
    destTouched = false;
    progress = null;
    error = "";
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>Repository klonen</Dialog.Title>
      <Dialog.Description>Von GitHub/GitLab suchen oder eine Git-URL einfügen.</Dialog.Description>
    </Dialog.Header>

    <div class="flex flex-wrap items-center gap-1.5">
      {#each accounts as a (a.kind + a.host + a.user)}
        <Button
          variant={mode === "browse" && account === a ? "default" : "outline"}
          size="sm"
          class="text-xs"
          onclick={() => selectAccount(a)}
        >
          {a.user}<span class="text-muted-foreground ml-1 font-mono text-[10px]">@{a.host}</span>
        </Button>
      {/each}
      <Button variant="outline" size="sm" class="text-xs" onclick={onaccounts}>Konto hinzufügen…</Button>
      <span class="flex-1"></span>
      <Button
        variant={mode === "url" ? "default" : "outline"}
        size="sm"
        class="text-xs"
        onclick={() => {
          mode = "url";
          selected = null;
        }}>URL einfügen</Button
      >
    </div>

    {#if mode === "url"}
      <Input bind:value={url} placeholder="https://… oder git@…" class="font-mono text-xs" />
    {:else if !account}
      <p class="text-muted-foreground px-1 py-6 text-center text-sm">
        {accounts.length ? "Konto oben wählen." : "Noch kein Konto verbunden."}
      </p>
    {:else}
      <div class="flex items-center gap-1.5">
        <div class="relative flex-1">
          <SearchIcon class="text-muted-foreground absolute top-1/2 left-2 size-3.5 -translate-y-1/2" />
          <Input
            bind:value={query}
            placeholder="Eigene Repos filtern…"
            class="pl-7 text-xs"
            onkeydown={(e) => e.key === "Enter" && searchRemote()}
          />
        </div>
        <Button variant="outline" size="sm" class="text-xs" disabled={!query.trim() || loading} onclick={searchRemote}
          >Überall suchen</Button
        >
      </div>

      <div class="border-border min-h-0 flex-1 overflow-y-auto rounded border">
        {#if loading && !results.length}
          <p class="text-muted-foreground px-3 py-6 text-center text-xs">Lade…</p>
        {:else if !results.length}
          <p class="text-muted-foreground px-3 py-6 text-center text-xs">Keine Treffer.</p>
        {:else}
          {#each results as r (r.full_name)}
            {@const isExisting = existingNames.has(r.name.toLowerCase())}
            <button
              class="border-border hover:bg-secondary flex w-full items-center gap-2 border-b px-2.5 py-1.5 text-left last:border-0 {selected ===
              r
                ? 'bg-secondary'
                : ''}"
              onclick={() => pick(r)}
            >
              <div class="min-w-0 flex-1">
                <div class="truncate text-xs font-medium">{r.full_name}</div>
                {#if r.description}
                  <div class="text-muted-foreground truncate text-[11px]">{r.description}</div>
                {/if}
              </div>
              {#if r.lang}<Badge variant="outline" class="text-muted-foreground text-[10px]">{r.lang}</Badge>{/if}
              {#if r.stars}
                <span class="text-muted-foreground flex items-center gap-0.5 text-[11px]"
                  ><StarIcon class="size-3" />{r.stars}</span
                >
              {/if}
              {#if r.private}<LockIcon class="text-muted-foreground size-3" />{/if}
              {#if r.fork}<GitForkIcon class="text-muted-foreground size-3" />{/if}
              {#if r.archived}<ArchiveIcon class="text-muted-foreground size-3" />{/if}
              <span class="text-muted-foreground w-8 text-right font-mono text-[10px]">{age(r.updated_at)}</span>
              {#if isExisting}
                <Badge variant="outline" class="text-primary gap-0.5 text-[10px]"><CheckIcon class="size-3" />lokal</Badge>
              {/if}
            </button>
          {/each}
          {#if !searched && own.length && !loading}
            <button
              class="text-muted-foreground hover:text-foreground w-full py-1.5 text-center text-[11px]"
              onclick={() => {
                page += 1;
                loadOwn();
              }}>Mehr laden…</button
            >
          {/if}
        {/if}
      </div>

      {#if selected}
        <div class="flex gap-1.5">
          <Button
            variant={protocol === "https" ? "default" : "outline"}
            size="sm"
            class="text-xs"
            onclick={() => (protocol = "https")}>HTTPS</Button
          >
          <Button variant={protocol === "ssh" ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (protocol = "ssh")}
            >SSH</Button
          >
        </div>
      {/if}
    {/if}

    {#if mode === "url" || selected}
      <div class="flex items-center gap-1.5">
        <Input
          bind:value={dest}
          oninput={() => (destTouched = true)}
          placeholder="Zielordner"
          class="font-mono text-xs"
        />
        <Button variant="outline" size="icon" class="shrink-0" onclick={browseDest} aria-label="Ordner wählen">
          <FolderIcon class="size-3.5" />
        </Button>
      </div>
    {/if}

    {#if progress}
      <div class="text-muted-foreground space-y-1">
        <div class="bg-secondary h-1.5 overflow-hidden rounded">
          <div class="bg-primary h-full transition-all" style="width:{progress.percent ?? 0}%"></div>
        </div>
        <div class="truncate font-mono text-[11px]">{progress.line}</div>
      </div>
    {/if}
    {#if error}<p class="text-destructive text-xs">{error}</p>{/if}

    <div class="flex justify-end gap-1.5">
      <Button variant="outline" size="sm" class="text-xs" onclick={() => (open = false)} disabled={cloning}>Abbrechen</Button>
      <Button
        size="sm"
        class="gap-1.5 text-xs"
        disabled={cloning || !dest.trim() || (mode === "url" ? !url.trim() : !selected)}
        onclick={doClone}
      >
        {#if cloning}<Loader2Icon class="size-3.5 animate-spin" />{/if}
        Klonen
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
