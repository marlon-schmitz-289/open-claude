<script lang="ts">
  import { age } from "$lib/utils";
  import { tick, untrack } from "svelte";
  import { ContextMenu } from "bits-ui";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { git, type Commit, type CommitDetail, type ResetMode } from "$lib/git";
  import { layout, type Row } from "$lib/graph";
  import { parseDiff } from "$lib/diff";
  import DiffView from "$lib/components/git/DiffView.svelte";
  import Splitter, { stored } from "$lib/components/Splitter.svelte";
  import SearchIcon from "@lucide/svelte/icons/search";
  import CopyIcon from "@lucide/svelte/icons/copy";

  let { repo, onchange, jumpTo = null, refreshKey = 0 }: {
    repo: string;
    onchange: () => void;
    jumpTo?: string | null;
    /** Aendert der Aufrufer nach jeder Aktion, dann laedt der Verlauf neu. */
    refreshKey?: number;
  } = $props();

  const PAGE = 200;
  // Farben pro Graph-Spur, per Index zyklisch.
  const PALETTE = ["#5aa9e6", "#e6875a", "#7ec850", "#c85fd6", "#e6c15a", "#5ae6c1", "#e65a7c", "#a9a9e6"];
  // Muss der Zeilenhoehe (h-[26px]) entsprechen, sonst reissen die Linien zwischen den Zeilen ab.
  const ROW_H = 26;
  const LANE_W = 14;

  let commits = $state<Commit[]>([]);
  let skip = $state(0);
  let loading = $state(false);
  let done = $state(false);
  let error = $state("");
  let query = $state("");
  let list: HTMLDivElement | null = $state(null);
  let detailH = $state(stored("git.detail", 220));

  let selected = $state<string | null>(null);
  let detail = $state<CommitDetail | null>(null);
  let detailLoading = $state(false);
  let detailError = $state("");

  // gen verwirft Seiten, die ein reload() ueberholt hat; pending teilt den laufenden Request.
  let gen = 0;
  let pending: Promise<void> | null = null;
  let remoteNames = $state<string[]>([]);

  /** keep: so viele Commits wie vorher laden, damit der Verlauf nach einer Aktion nicht an den Anfang springt. */
  function reload(keep = 0) {
    gen++;
    pending = null;
    commits = [];
    skip = 0;
    done = false;
    git.remotes(repo).then((r) => (remoteNames = r.map((x) => x.name))).catch(() => {});
    return loadMore(Math.max(PAGE, keep));
  }

  function loadMore(n = PAGE): Promise<void> {
    if (done) return Promise.resolve();
    const g = gen;
    return (pending ??= (async () => {
      loading = true;
      error = "";
      try {
        const page = await git.log(repo, null, skip, n);
        if (g !== gen) return;
        commits = [...commits, ...page];
        skip += page.length;
        if (page.length < n) done = true;
      } catch (e) {
        if (g === gen) error = String(e);
      } finally {
        if (g === gen) {
          loading = false;
          pending = null;
        }
      }
    })());
  }

  $effect(() => {
    repo;
    refreshKey;
    // reload liest loading/done, sonst Endlosschleife
    untrack(() => {
      const top = list?.scrollTop ?? 0;
      reload(commits.length)
        .then(() => tick())
        .then(() => list && (list.scrollTop = top));
    });
  });

  // Suche sieht nur geladene Commits: dafuer automatisch nachladen (begrenzt, danach per Knopf weiter).
  const SEARCH_STEP = 5000;
  let searchUntil = $state(SEARCH_STEP);
  $effect(() => {
    if (filtered && filtered.length < 50 && !done && !loading && !error && skip < searchUntil) untrack(() => loadMore());
  });

  function onScroll() {
    if (!list) return;
    if (list.scrollTop + list.clientHeight >= list.scrollHeight - 400) loadMore();
  }

  let keyTimer: ReturnType<typeof setTimeout> | undefined;
  function onListKey(e: KeyboardEvent) {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const src = filtered ?? commits;
    const i = src.findIndex((c) => c.sha === selected);
    const next = src[i < 0 ? 0 : Math.max(0, Math.min(src.length - 1, i + step))];
    if (!next || next.sha === selected) return;
    // Markierung sofort, Detail erst wenn die Taste kurz ruht: Gedrueckthalten startet sonst ein git show pro Zeile.
    selected = next.sha;
    clearTimeout(keyTimer);
    keyTimer = setTimeout(() => selected === next.sha && select(next.sha), 120);
    document.getElementById(`commit-${next.sha}`)?.scrollIntoView({ block: "nearest" });
  }

  async function select(sha: string) {
    selected = sha;
    detail = null;
    detailError = "";
    detailLoading = true;
    try {
      const d = await git.show(repo, sha);
      if (selected !== sha) return; // inzwischen anderer Commit gewaehlt
      detail = d;
      selected = d.commit.sha;
    } catch (e) {
      if (selected !== sha) return;
      detailError = String(e);
    }
    detailLoading = false;
  }

  const JUMP_PAGES = 10;

  // Sprung aus der Sidebar: Ref/SHA aufloesen, begrenzt nachladen, dann anzeigen und hinscrollen.
  async function jump(ref: string) {
    await select(ref);
    const sha = selected;
    if (!sha || detailError || sha !== detail?.commit.sha) return;
    for (let i = 0; i < JUMP_PAGES && !done && !error && !commits.some((c) => c.sha === sha); i++) await loadMore();
    if (!commits.some((c) => c.sha === sha)) {
      if (!error) error = "Commit liegt weiter zurück – Details unten, zum Anzeigen im Verlauf weiter scrollen.";
      return;
    }
    await tick();
    document.getElementById(`commit-${sha}`)?.scrollIntoView({ block: "center" });
  }

  $effect(() => {
    const ref = jumpTo;
    if (ref) untrack(() => jump(ref));
  });

  const rows = $derived(layout(commits.map((c) => ({ sha: c.sha, parents: c.parents }))));
  const lanes = $derived(
    rows.reduce((m, r) => Math.max(m, r.lane, ...r.edges.flatMap((e) => [e.from, e.to])), 0) + 1,
  );

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return null;
    return commits.filter(
      (c) =>
        c.subject.toLowerCase().includes(q) ||
        c.author.toLowerCase().includes(q) ||
        c.sha.startsWith(q),
    );
  });

  const files = $derived(detail ? parseDiff(detail.diff) : []);

  function refBadges(refs: string[]) {
    return refs.map((r) => {
      const head = r.startsWith("HEAD");
      const tag = r.startsWith("tag: ");
      const remote = !tag && remoteNames.some((n) => r.startsWith(n + "/"));
      const label = tag ? r.slice(5) : r.includes(" -> ") ? r.split(" -> ")[1] : r;
      return { label, kind: head ? "head" : tag ? "tag" : remote ? "remote" : "local" };
    });
  }

  const STATUS_LABEL: Record<string, string> = {
    modified: "M",
    added: "A",
    deleted: "D",
    renamed: "R",
    copied: "C",
  };

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

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

  function copySha(sha: string) {
    navigator.clipboard?.writeText(sha);
  }

  function reset(sha: string, mode: ResetMode) {
    // Auch soft/mixed schreibt den Branch um und liegt im Menue direkt neben Revert.
    const [title, message] =
      mode === "hard"
        ? ["Hart zurücksetzen", `HEAD hart auf ${sha.slice(0, 8)} zurücksetzen? Nicht committete Änderungen gehen verloren.`]
        : [`Zurücksetzen (${mode})`, `Branch auf ${sha.slice(0, 8)} zurücksetzen? Die Änderungen bleiben erhalten.`];
    askConfirm(title, message, async () => {
      await git.reset(repo, sha, mode);
    });
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

{#snippet refBadge(b: { label: string; kind: string })}
  <Badge
    variant="outline"
    class="shrink-0 text-[10px] {b.kind === 'head'
      ? 'text-primary border-primary/50 font-semibold'
      : b.kind === 'tag'
        ? 'text-amber-400 border-amber-400/40'
        : b.kind === 'remote'
          ? 'text-muted-foreground'
          : ''}"
  >
    {b.label}
  </Badge>
{/snippet}

{#snippet commitRow(c: Commit, row: Row | null)}
  <ContextMenu.Root>
    <!-- Auswahl zeigt bg-accent, ein Fokusrahmen waere doppelt -->
    <ContextMenu.Trigger class="outline-none">
      <button
        id="commit-{c.sha}"
        tabindex="-1"
        role="option"
        aria-selected={selected === c.sha}
        class="hover:bg-accent flex h-[26px] w-full items-center gap-2 px-2 text-left outline-none {selected ===
        c.sha
          ? 'bg-accent'
          : ''}"
        onclick={() => select(c.sha)}
      >
        {#if row}
          <svg width={lanes * LANE_W} height={ROW_H} class="shrink-0">
            {#each row.edges as e (e.from + "-" + e.to + "-" + e.kind)}
              <line
                x1={e.from * LANE_W + LANE_W / 2}
                y1={e.kind === "out" ? ROW_H / 2 : 0}
                x2={e.to * LANE_W + LANE_W / 2}
                y2={e.kind === "in" ? ROW_H / 2 : ROW_H}
                stroke={PALETTE[e.color % PALETTE.length]}
                stroke-width="1.5"
              />
            {/each}
            <circle
              cx={row.lane * LANE_W + LANE_W / 2}
              cy={ROW_H / 2}
              r="3"
              fill={PALETTE[row.color % PALETTE.length]}
            />
          </svg>
        {/if}
        <span class="text-muted-foreground shrink-0 font-mono text-[11px]">{c.sha.slice(0, 8)}</span>
        {#each refBadges(c.refs) as b (b.label + b.kind)}
          {@render refBadge(b)}
        {/each}
        <span class="min-w-0 flex-1 truncate text-[11px]">{c.subject}</span>
        <span class="text-muted-foreground hidden shrink-0 truncate text-[11px] sm:block max-w-28">{c.author}</span>
        <span class="text-muted-foreground w-10 shrink-0 text-right font-mono text-[10px] tabular-nums">{age(c.date)}</span>
      </button>
    </ContextMenu.Trigger>
    <ContextMenu.Portal>
      <ContextMenu.Content class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-48 rounded-md p-1 text-xs ring-1">
        {@render menuItem("Branch hier erstellen", () =>
          askPrompt("Neuer Branch ab " + c.sha.slice(0, 8), "", async (v) => {
            await git.checkout(repo, v, true, c.sha);
          }),
        )}
        {@render menuItem("Tag erstellen", () =>
          askPrompt("Tag ab " + c.sha.slice(0, 8), "", async (v) => {
            await git.tagCreate(repo, v, c.sha, null);
          }),
        )}
        {@render menuItem("Auschecken (detached)", () =>
          askConfirm("Detached auschecken", `HEAD von ${c.sha.slice(0, 8)} lösen? Änderungen ohne Branch können verloren gehen.`, async () => {
            await git.checkout(repo, c.sha, false, null);
          }),
        )}
        {@render menuItem("Cherry-Pick", () => run(() => git.cherryPick(repo, c.sha)))}
        {@render menuItem("Revert", () => run(() => git.revert(repo, c.sha)))}
        {@render menuItem("Reset: soft", () => reset(c.sha, "soft"))}
        {@render menuItem("Reset: mixed", () => reset(c.sha, "mixed"))}
        {@render menuItem("Reset: hard", () => reset(c.sha, "hard"), true)}
        {@render menuItem("SHA kopieren", () => copySha(c.sha))}
      </ContextMenu.Content>
    </ContextMenu.Portal>
  </ContextMenu.Root>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class="border-border flex items-center gap-2 border-b px-2 py-1.5">
    <SearchIcon class="text-muted-foreground size-3.5 shrink-0" />
    <Input bind:value={query} placeholder="Verlauf durchsuchen" class="h-6! text-[11px]" />
  </div>

  {#if error}
    <p class="bg-destructive/15 text-destructive border-destructive/30 border-b px-2 py-1 text-[11px]">{error}</p>
  {/if}

  <!-- min-h-20: die Liste bleibt sichtbar, auch wenn die gespeicherte Detailhoehe nicht mehr ins Fenster passt -->
  <div class="flex min-h-20 flex-1 flex-col overflow-hidden">
    <!-- Liste selbst ist fokussierbar: Pfeiltasten wechseln den Commit statt zu scrollen -->
    <div
      bind:this={list}
      onscroll={onScroll}
      onkeydown={onListKey}
      tabindex="0"
      role="listbox"
      aria-label="Commits"
      class="min-h-0 flex-1 overflow-y-auto outline-none"
    >
      {#if filtered}
        {#each filtered as c (c.sha)}
          {@render commitRow(c, null)}
        {/each}
        {#if loading}
          <p class="text-muted-foreground px-2 py-2 text-center text-[11px]">Durchsucht {skip} Commits …</p>
        {:else if !done}
          <button
            class="text-muted-foreground hover:text-foreground w-full px-2 py-2 text-center text-[11px]"
            onclick={() => ((searchUntil = skip + SEARCH_STEP), loadMore())}
          >
            {filtered.length ? "" : "Keine Treffer. "}Nur die letzten {skip} Commits durchsucht – weiter suchen
          </button>
        {:else if filtered.length === 0}
          <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Keine Treffer.</p>
        {/if}
      {:else}
        {#each commits as c, i (c.sha)}
          {@render commitRow(c, rows[i])}
        {/each}
        {#if loading}
          <p class="text-muted-foreground px-2 py-2 text-center text-[11px]">Lädt …</p>
        {/if}
      {/if}
    </div>
  </div>

  <Splitter bind:size={detailH} axis="y" min={80} invert key="git.detail" />
  <div class="border-border min-h-0 overflow-y-auto border-t" style="height:{detailH}px">
    {#if detailLoading}
      <p class="text-muted-foreground px-2 py-2 text-[11px]">Lädt …</p>
    {:else if detailError}
      <p class="text-destructive px-2 py-2 text-[11px]">{detailError}</p>
    {:else if detail}
      <div class="border-border space-y-1 border-b px-2 py-2">
        <div class="flex items-center gap-2">
          <span class="font-mono text-[11px] font-semibold">{detail.commit.sha.slice(0, 10)}</span>
          <button
            class="text-muted-foreground hover:text-foreground"
            onclick={() => copySha(detail!.commit.sha)}
            title="SHA kopieren"
          >
            <CopyIcon class="size-3" />
          </button>
        </div>
        <p class="whitespace-pre-wrap text-[11px]">{detail.commit.subject}</p>
        {#if detail.body}<p class="text-muted-foreground whitespace-pre-wrap text-[11px]">{detail.body}</p>{/if}
        <p class="text-muted-foreground text-[10px]">
          {detail.commit.author} &lt;{detail.commit.email}&gt; · {new Date(detail.commit.date).toLocaleString("de-DE")}
        </p>
        {#if detail.commit.parents.length}
          <p class="text-muted-foreground font-mono text-[10px]">
            Eltern:
            {#each detail.commit.parents as p (p)}
              <button class="hover:text-primary underline-offset-2 hover:underline" onclick={() => select(p)}>{p.slice(0, 8)}</button
              >{" "}
            {/each}
          </p>
        {/if}
        {#if files.length}
          <ul class="font-mono text-[10px]">
            {#each files as f (f.newPath + f.oldPath)}
              <li class="text-muted-foreground truncate">
                <span class="text-primary">{STATUS_LABEL[f.status]}</span>
                {f.status === "renamed" ? `${f.oldPath} -> ${f.newPath}` : f.newPath}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <DiffView diff={detail.diff} />
    {:else}
      <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Commit wählen.</p>
    {/if}
  </div>
</div>

<Dialog.Root open={confirm !== null} onOpenChange={(o) => !o && (confirm = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{confirm?.title}</Dialog.Title>
      <Dialog.Description>{confirm?.message}</Dialog.Description>
    </Dialog.Header>
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (confirm = null)}>Abbrechen</Button>
      <Button variant="destructive" size="sm" onclick={doConfirm}>Bestätigen</Button>
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
