<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { ContextItem, ConfirmDialog, DiffDialog, PromptDialog, fileIcon, menuContent } from "$lib/components/kit";
  import { age, cn } from "$lib/utils";
  import { tick, untrack } from "svelte";
  import { ContextMenu } from "bits-ui";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { git, type Commit, type CommitDetail, type ResetMode } from "$lib/git";
  import { layout, type Row } from "$lib/graph";
  import { fileDiff, parseDiff, stat, type FileStatus } from "$lib/diff";
  import { dirname } from "$lib/tree";
  import { ask } from "$lib/ask.svelte";
  import { theme } from "$lib/theme.svelte";
  import Splitter, { stored } from "$lib/components/Splitter.svelte";
  import SearchIcon from "@lucide/svelte/icons/search";
  import CopyIcon from "@lucide/svelte/icons/copy";

  let { repo, onchange, busy, track, jumpTo = null, refreshKey = 0 }: {
    repo: string;
    /** Laeuft gerade eine Git-Aktion: keine zweite starten. Anzeige macht GitView ueber track. */
    busy: boolean;
    track: <T>(label: string, action: () => Promise<T>) => Promise<T>;
    onchange: () => void;
    jumpTo?: string | null;
    /** Aendert der Aufrufer nach jeder Aktion, dann laedt der Verlauf neu. */
    refreshKey?: number;
  } = $props();

  const PAGE = 200;
  // Farben pro Graph-Spur, per Index zyklisch. Hell: gleiche Hues, oklch L 0.55.
  const DARK = ["#5aa9e6", "#e6875a", "#7ec850", "#c85fd6", "#e6c15a", "#5ae6c1", "#e65a7c", "#a9a9e6"];
  const LIGHT = ["#0077bd", "#b64e10", "#478416", "#964bb4", "#916a00", "#008474", "#bf395b", "#6568b6"];
  const PALETTE = $derived(theme.dark ? DARK : LIGHT);
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
  /** Index der Datei im Diff-Dialog; null = zu. full = Body ganz zeigen. */
  let shown = $state<number | null>(null);
  let full = $state(false);

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
    shown = null;
    full = false;
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
  const stats = $derived(files.map(stat));
  const total = $derived(stats.reduce((t, s) => ({ a: t.a + s.a, d: t.d + s.d }), { a: 0, d: 0 }));
  const cur = $derived(shown === null ? null : (files[shown] ?? null));
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  function refBadges(refs: string[]) {
    return refs.map((r) => {
      const head = r.startsWith("HEAD");
      const tag = r.startsWith("tag: ");
      const remote = !tag && remoteNames.some((n) => r.startsWith(n + "/"));
      const label = tag ? r.slice(5) : r.includes(" -> ") ? r.split(" -> ")[1] : r;
      return { label, kind: head ? "head" : tag ? "tag" : remote ? "remote" : "local" };
    });
  }

  // Kuerzel und Farbe wie im ChangesPanel
  const STATUS: Record<FileStatus, [string, string]> = {
    modified: ["M", "text-warning"],
    added: ["A", "text-success"],
    deleted: ["D", "text-destructive"],
    renamed: ["R", "text-info"],
    copied: ["C", "text-info"],
  };

  async function run(label: string, action: () => Promise<unknown>) {
    if (busy) return;
    error = "";
    try {
      await track(label, action);
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
    const { title, run: action } = confirm;
    confirm = null;
    await run(title, action);
  }

  let prompt = $state<{ title: string; run: (v: string) => Promise<void> } | null>(null);
  let promptValue = $state("");
  function askPrompt(title: string, value: string, action: (v: string) => Promise<void>) {
    prompt = { title, run: action };
    promptValue = value;
  }
  async function doPrompt() {
    if (!prompt) return;
    const { title, run: action } = prompt;
    const value = promptValue.trim();
    prompt = null;
    if (value) await run(title, () => action(value));
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

{#snippet refBadge(b: { label: string; kind: string })}
  <Badge
    variant="outline"
    class="shrink-0 text-[10px] {b.kind === 'head'
      ? 'text-primary border-primary/50 font-semibold'
      : b.kind === 'tag'
        ? 'text-warning border-warning/40'
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
      <ContextMenu.Content class={cn(menuContent, "min-w-48")}>
        <ContextItem
          label="Branch hier erstellen"
          onSelect={() =>
            askPrompt("Neuer Branch ab " + c.sha.slice(0, 8), "", async (v) => {
              await git.checkout(repo, v, true, c.sha);
            })}
        />
        <ContextItem
          label="Tag erstellen"
          onSelect={() =>
            askPrompt("Tag ab " + c.sha.slice(0, 8), "", async (v) => {
              await git.tagCreate(repo, v, c.sha, null);
            })}
        />
        <ContextItem
          label="Auschecken (detached)"
          onSelect={() =>
            askConfirm("Detached auschecken", `HEAD von ${c.sha.slice(0, 8)} lösen? Änderungen ohne Branch können verloren gehen.`, async () => {
              await git.checkout(repo, c.sha, false, null);
            })}
        />
        <ContextItem
          label="Cherry-Pick"
          onSelect={() =>
            ask("pick", "Cherry-Pick", `${c.sha.slice(0, 8)} auf den aktuellen Branch übernehmen?`, () =>
              run("Cherry-Pick", () => git.cherryPick(repo, c.sha)),
            )}
        />
        <ContextItem
          label="Revert"
          onSelect={() =>
            ask("pick", "Revert", `${c.sha.slice(0, 8)} mit einem neuen Commit rückgängig machen?`, () =>
              run("Revert", () => git.revert(repo, c.sha)),
            )}
        />
        <ContextItem label="Reset: soft" onSelect={() => reset(c.sha, "soft")} />
        <ContextItem label="Reset: mixed" onSelect={() => reset(c.sha, "mixed")} />
        <ContextItem label="Reset: hard" onSelect={() => reset(c.sha, "hard")} destructive />
        <ContextItem label="SHA kopieren" onSelect={() => copySha(c.sha)} />
      </ContextMenu.Content>
    </ContextMenu.Portal>
  </ContextMenu.Root>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class="border-border flex items-center gap-2 border-b px-2 py-1.5">
    <SearchIcon class="text-muted-foreground size-3.5 shrink-0" />
    <Input bind:value={query} placeholder="Verlauf durchsuchen" class="h-6! text-[11px]" />
  </div>

  <Notice bind:text={error} />

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
      <div class="space-y-1 px-3 pt-2 pb-1.5">
        <p class="truncate text-xs font-medium" title={detail.commit.subject}>{detail.commit.subject}</p>
        <div class="text-muted-foreground flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[10px]">
          <button class="hover:text-foreground flex items-center gap-1 font-mono" title="SHA kopieren" onclick={() => copySha(detail!.commit.sha)}
            >{detail.commit.sha.slice(0, 8)}<CopyIcon class="size-2.5" /></button
          >
          <span>·</span><span title={detail.commit.email}>{detail.commit.author}</span>
          <span>·</span><span title={new Date(detail.commit.date).toLocaleString("de-DE")}>{age(detail.commit.date)}</span>
          {#if detail.commit.parents.length}
            <span>·</span><span>Eltern</span>
            {#each detail.commit.parents as p (p)}
              <button class="hover:text-primary font-mono underline-offset-2 hover:underline" onclick={() => select(p)}>{p.slice(0, 8)}</button>
            {/each}
          {/if}
          <span class="ml-auto font-mono tabular-nums">
            {files.length}
            {files.length === 1 ? "Datei" : "Dateien"}
            {#if total.a}<span class="text-success">+{total.a}</span>{/if}
            {#if total.d}<span class="text-destructive">−{total.d}</span>{/if}
          </span>
        </div>
        {#if detail.body}
          <button
            class="text-muted-foreground hover:text-foreground w-full text-left text-[11px] whitespace-pre-wrap {full ? 'block' : 'line-clamp-2'}"
            title={full ? "Weniger" : "Ganz anzeigen"}
            onclick={() => (full = !full)}>{detail.body}</button
          >
        {/if}
      </div>
      {#if files.length}
        <div class="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-1.5 px-3 pb-3">
          {#each files as f, i (i)}
            {@const st = stats[i]}
            {@const ic = fileIcon(f.newPath)}
            {@const [label, tone] = STATUS[f.status]}
            <button
              class="bg-card border-border/60 hover:bg-accent/60 hover:border-border flex min-w-0 flex-col gap-0.5 rounded-lg border px-2 py-1.5 text-left transition-colors"
              title={f.oldPath !== f.newPath ? `${f.oldPath} → ${f.newPath}` : f.newPath}
              onclick={() => (shown = i)}
            >
              <span class="flex min-w-0 items-center gap-1.5">
                <ic.icon class="size-3.5 shrink-0 {ic.tint}" />
                <span class="min-w-0 flex-1 truncate text-[11px] font-medium">{base(f.newPath)}</span>
                <span class="shrink-0 font-mono text-[10px] font-semibold {tone}">{label}</span>
              </span>
              <span class="flex min-w-0 items-center gap-2 pl-5 text-[10px]">
                <span class="text-muted-foreground min-w-0 flex-1 truncate">{dirname(f.newPath)}</span>
                {#if f.binary}<span class="text-muted-foreground shrink-0">binär</span>
                {:else}<span class="shrink-0 font-mono tabular-nums"
                    >{#if st.a}<span class="text-success">+{st.a}</span>{/if}
                    {#if st.d}<span class="text-destructive">−{st.d}</span>{/if}</span
                  >{/if}
              </span>
            </button>
          {/each}
        </div>
      {/if}
    {:else}
      <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Commit wählen.</p>
    {/if}
  </div>
</div>

<ConfirmDialog
  open={confirm !== null}
  title={confirm?.title ?? ""}
  message={confirm?.message}
  action="Bestätigen"
  onconfirm={doConfirm}
  oncancel={() => (confirm = null)}
/>

<PromptDialog
  open={prompt !== null}
  title={prompt?.title ?? ""}
  bind:value={promptValue}
  onsubmit={doPrompt}
  oncancel={() => (prompt = null)}
/>

<DiffDialog
  diff={cur && fileDiff(cur)}
  title={cur ? base(cur.newPath) : ""}
  onclose={() => (shown = null)}
  step={files.length > 1 ? (d) => (shown = (shown! + d + files.length) % files.length) : undefined}
>
  {#if cur && shown !== null}
    {@const [label, tone] = STATUS[cur.status]}
    <span class="shrink-0 font-mono text-xs font-semibold {tone}">{label}</span>
    <span class="shrink-0 font-mono text-xs tabular-nums">
      {#if stats[shown].a}<span class="text-success">+{stats[shown].a}</span>{/if}
      {#if stats[shown].d}<span class="text-destructive">−{stats[shown].d}</span>{/if}
    </span>
    <span class="text-muted-foreground shrink-0 text-xs font-normal tabular-nums">{shown + 1} / {files.length}</span>
  {/if}
</DiffDialog>
