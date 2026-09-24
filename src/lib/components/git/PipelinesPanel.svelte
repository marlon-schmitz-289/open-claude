<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { age } from "$lib/utils";
  import { git, forge, type ForgeKind, type Job, type Run, type RunStatus } from "$lib/git";
  import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
  import CircleXIcon from "@lucide/svelte/icons/circle-x";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
  import CircleDashedIcon from "@lucide/svelte/icons/circle-dashed";
  import CircleSlashIcon from "@lucide/svelte/icons/circle-slash";
  import CircleMinusIcon from "@lucide/svelte/icons/circle-minus";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";

  let {
    repo,
    refreshKey,
    visible,
    kind = $bindable(null),
  }: { repo: string; refreshKey: number; visible: boolean; kind?: ForgeKind | null } = $props();

  const PAGE = 30;
  const POLL_MS = 10_000;
  const ICON = {
    success: [CircleCheckIcon, "text-green-500"],
    failure: [CircleXIcon, "text-red-500"],
    running: [LoaderCircleIcon, "animate-spin text-yellow-500"],
    queued: [CircleDashedIcon, "text-muted-foreground"],
    cancelled: [CircleSlashIcon, "text-muted-foreground opacity-60"],
    skipped: [CircleMinusIcon, "text-muted-foreground opacity-60"],
  } as const;

  let originUrl = $state<string | null>(null);
  let branch = $state<string | null>(null);
  let onlyBranch = $state(false);
  let noAccount = $state(false);
  let runs = $state<Run[]>([]);
  let page = $state(0);
  let done = $state(false);
  let loading = $state(true);
  let error = $state("");
  let open = $state<Record<number, boolean>>({});
  let jobs = $state<Record<number, Job[]>>({});
  let jobErr = $state<Record<number, string>>({});

  // gen verwirft Antworten, die ein reload() ueberholt hat; busy verhindert ueberlappende Requests.
  let gen = 0;
  let busy = false;

  const filter = () => (onlyBranch && branch ? branch : "");
  const active = $derived(runs.some((r) => r.status === "queued" || r.status === "running"));

  // "git@host:pfad", "ssh://git@host:22/pfad", "https://host/pfad" -> host
  const hostOf = (url: string) => url.match(/^(?:[a-z+]+:\/\/)?(?:[^@/]+@)?([^/:]+)/i)?.[1] ?? url;

  function duration(s: number | null) {
    if (s == null) return "";
    const pad = (n: number) => String(n).padStart(2, "0");
    return s < 3600 ? `${Math.floor(s / 60)}:${pad(s % 60)}` : `${Math.floor(s / 3600)}:${pad(Math.floor((s % 3600) / 60))} h`;
  }

  async function reload() {
    const g = ++gen;
    busy = true;
    loading = true;
    error = "";
    noAccount = false;
    runs = [];
    page = 0;
    done = false;
    open = {};
    jobs = {};
    jobErr = {};
    try {
      const [remotes, status] = await Promise.all([git.remotes(repo), git.status(repo)]);
      if (g !== gen) return;
      originUrl = remotes.find((r) => r.name === "origin")?.url ?? null;
      branch = status.branch;
      if (!originUrl) return;
      await fetchPage(g);
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) {
        loading = false;
        busy = false;
      }
    }
  }

  async function fetchPage(g: number) {
    const res = await forge.runs(originUrl!, filter(), page + 1);
    if (g !== gen) return;
    if (!res) {
      noAccount = true;
      return;
    }
    kind = res.kind;
    // Zwischen zwei Seiten neu gestartete Runs verschieben die Seiten: Dubletten weglassen.
    const seen = new Set(runs.map((r) => r.id));
    runs = [...runs, ...res.items.filter((r) => !seen.has(r.id))];
    page++;
    done = res.items.length < PAGE;
  }

  async function loadMore() {
    if (busy || done) return;
    const g = gen;
    busy = loading = true;
    try {
      await fetchPage(g);
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) busy = loading = false;
    }
  }

  async function loadJobs(id: number, g = gen) {
    try {
      const list = await forge.jobs(originUrl!, id);
      if (g !== gen) return;
      jobs[id] = list;
      delete jobErr[id];
    } catch (e) {
      if (g === gen) jobErr[id] = String(e);
    }
  }

  function toggle(r: Run) {
    open[r.id] = !open[r.id];
    if (open[r.id] && !jobs[r.id]) loadJobs(r.id);
  }

  // Erste Seite frisch holen, aeltere geladene Runs dahinter behalten; aufgeklappte Jobs mit.
  async function poll() {
    if (busy || !originUrl) return;
    const g = gen;
    busy = true;
    try {
      const [res] = await Promise.all([
        forge.runs(originUrl, filter(), 1),
        ...Object.keys(open).filter((id) => open[+id]).map((id) => loadJobs(+id, g)),
      ]);
      if (g !== gen || !res) return;
      const ids = new Set(res.items.map((r) => r.id));
      const last = res.items.at(-1)?.created_at ?? "";
      runs =
        res.items.length < PAGE
          ? res.items
          : [...res.items, ...runs.filter((r) => !ids.has(r.id) && r.created_at <= last)];
    } catch (e) {
      if (g === gen) error = String(e);
    } finally {
      if (g === gen) busy = false;
    }
  }

  $effect(() => {
    repo;
    refreshKey;
    onlyBranch;
    reload();
  });

  $effect(() => {
    if (!visible || !active) return;
    const t = setInterval(poll, POLL_MS);
    return () => clearInterval(t);
  });

  const label = $derived(kind === "gitlab" ? "Pipelines" : "Actions");
</script>

{#snippet statusIcon(s: RunStatus)}
  {@const [Icon, cls] = ICON[s]}
  <Icon class="size-3.5 shrink-0 {cls}" aria-label={s} />
{/snippet}

{#snippet browser(url: string)}
  <button
    class="text-muted-foreground hover:text-foreground shrink-0"
    title="Im Browser öffnen"
    onclick={(e) => {
      e.stopPropagation();
      forge.openUrl(url).catch((err) => (error = String(err)));
    }}
  >
    <ExternalLinkIcon class="size-3" />
  </button>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class="border-border flex items-center gap-2 border-b px-2 py-1.5">
    <div class="border-border flex overflow-hidden rounded border text-[11px]">
      <button
        class="px-2 py-0.5 {onlyBranch ? 'text-muted-foreground hover:text-foreground' : 'bg-accent'}"
        onclick={() => (onlyBranch = false)}>Alle Branches</button
      >
      <button
        class="px-2 py-0.5 disabled:opacity-50 {onlyBranch ? 'bg-accent' : 'text-muted-foreground hover:text-foreground'}"
        disabled={!branch}
        title={branch ?? "Kein Branch ausgecheckt"}
        onclick={() => (onlyBranch = true)}>Aktueller Branch</button
      >
    </div>
    <span class="text-muted-foreground min-w-0 flex-1 truncate text-[11px]">
      {kind ? label : ""}{active && visible ? " · aktualisiert alle 10 s" : ""}
    </span>
    <button
      class="text-muted-foreground hover:text-foreground disabled:opacity-50"
      title="Neu laden"
      disabled={loading}
      onclick={reload}
    >
      <RefreshCwIcon class="size-3.5 {loading ? 'animate-spin' : ''}" />
    </button>
  </div>

  <Notice bind:text={error} />

  <div class="min-h-0 flex-1 overflow-y-auto">
    {#if !loading && !originUrl && !error}
      <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Kein Remote „origin“.</p>
    {:else if noAccount && originUrl}
      <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">
        Kein GitHub-/GitLab-Konto für {hostOf(originUrl)} – unter Konten anmelden.
      </p>
    {:else}
      {#each runs as r (r.id)}
        <div
          role="button"
          tabindex="0"
          class="hover:bg-accent flex w-full cursor-pointer items-center gap-2 px-2 py-1 text-left outline-none"
          onclick={() => toggle(r)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggle(r))}
        >
          <ChevronRightIcon class="text-muted-foreground size-3 shrink-0 transition-transform {open[r.id] ? 'rotate-90' : ''}" />
          {@render statusIcon(r.status)}
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="shrink-0 font-medium">{r.name}</span>
              <span class="min-w-0 truncate text-[11px]">{r.title}</span>
            </div>
            <div class="text-muted-foreground flex gap-2 truncate text-[10px]">
              <span class="truncate">{r.branch}</span>
              <span>{r.event}</span>
              <span class="font-mono">{r.sha.slice(0, 7)}</span>
            </div>
          </div>
          <span class="text-muted-foreground shrink-0 font-mono text-[10px] tabular-nums">{duration(r.duration_s)}</span>
          <span class="text-muted-foreground w-10 shrink-0 text-right font-mono text-[10px] tabular-nums">{age(r.created_at)}</span>
          {@render browser(r.web_url)}
        </div>
        {#if open[r.id]}
          <div class="border-border ml-5 border-l py-0.5">
            {#if jobErr[r.id]}
              <p class="text-destructive px-2 py-1 text-[11px]">{jobErr[r.id]}</p>
            {/if}
            {#if jobs[r.id]}
              {#each jobs[r.id] as j (j.id)}
                <div class="flex items-center gap-2 px-2 py-0.5">
                  {@render statusIcon(j.status)}
                  {#if j.stage}<span class="text-muted-foreground shrink-0 text-[10px]">{j.stage}</span>{/if}
                  <span class="min-w-0 flex-1 truncate text-[11px]">{j.name}</span>
                  <span class="text-muted-foreground shrink-0 font-mono text-[10px] tabular-nums">{duration(j.duration_s)}</span>
                  {@render browser(j.web_url)}
                </div>
              {:else}
                <p class="text-muted-foreground px-2 py-1 text-[11px]">Keine Jobs.</p>
              {/each}
            {:else if !jobErr[r.id]}
              <p class="text-muted-foreground px-2 py-1 text-[11px]">Lädt …</p>
            {/if}
          </div>
        {/if}
      {/each}
      {#if loading}
        <p class="text-muted-foreground px-2 py-2 text-center text-[11px]">Lädt …</p>
      {:else if !done && runs.length}
        <button class="text-muted-foreground hover:text-foreground w-full px-2 py-2 text-center text-[11px]" onclick={loadMore}>
          Mehr laden
        </button>
      {:else if !runs.length && !error}
        <p class="text-muted-foreground px-2 py-4 text-center text-[11px]">Keine {label}.</p>
      {/if}
    {/if}
  </div>
</div>
