<script lang="ts">
  import { tick, untrack } from "svelte";
  import { DropdownMenu } from "bits-ui";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { git, type Status } from "$lib/git";
  import Sidebar from "./Sidebar.svelte";
  import ChangesPanel from "./ChangesPanel.svelte";
  import HistoryPanel from "./HistoryPanel.svelte";
  import MergeEditor from "./MergeEditor.svelte";
  import DiffView from "./DiffView.svelte";
  import Splitter, { stored } from "$lib/components/Splitter.svelte";
  import GitBranchIcon from "@lucide/svelte/icons/git-branch";
  import GitBranchPlusIcon from "@lucide/svelte/icons/git-branch-plus";
  import CloudDownloadIcon from "@lucide/svelte/icons/cloud-download";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  let { repo }: { repo: string } = $props();

  let status = $state<Status | null>(null);
  let refreshKey = $state(0);
  let side = $state(stored("git.side", 240));
  let tab = $state<"changes" | "history">("changes");
  let conflictPath = $state<string | null>(null);
  let jumpTo = $state<string | null>(null);
  let busy = $state("");
  let error = $state("");
  let note = $state("");
  let shown = $state<{ title: string; diff: string } | null>(null);
  let confirm = $state<{ title: string; message: string; label: string; run: () => Promise<unknown> } | null>(null);
  let branchName = $state<string | null>(null);
  let stash = $state<{ message: string; untracked: boolean } | null>(null);

  const conflicts = $derived(status?.files.filter((f) => f.conflict).length ?? 0);
  const STATE_LABEL = { merge: "Merge", rebase: "Rebase", "cherry-pick": "Cherry-Pick", revert: "Revert" };

  /** Status neu lesen; refreshKey laesst Sidebar und Verlauf nachziehen. Beim Fokus nur, wenn HEAD sich bewegt hat. */
  async function refresh(force = true) {
    const before = `${status?.head}|${status?.branch}`;
    try {
      status = await git.status(repo);
      // Nach Abbrechen/Fortsetzen (auch extern) steht die Datei nicht mehr im Konflikt: Editor zu, sonst zeigt er Altes.
      if (conflictPath && !status.files.some((f) => f.conflict && f.path === conflictPath)) conflictPath = null;
    } catch (e) {
      error = `Status: ${e}`;
    }
    if (force || before !== `${status?.head}|${status?.branch}`) refreshKey++;
  }

  /** Fuer den Aktualisieren-Knopf in der Titelleiste und F5; alte Meldungen gelten dann nicht mehr. */
  export const reload = () => {
    error = note = "";
    return refresh();
  };

  let mergeDirty = $state(false);
  /** Offener Merge-Editor mit Entscheidungen: erst nachfragen. */
  export function canLeave() {
    return !conflictPath || !mergeDirty || window.confirm("Merge-Editor schließen? Entscheidungen gehen verloren.");
  }

  $effect(() => {
    repo;
    untrack(refresh); // refresh liest status, sonst laedt jeder neue Status erneut: Endlosschleife
  });

  /** Aktion mit Ladeanzeige; Ausgabe von git als Hinweis, Fehler rot. */
  async function run(label: string, action: () => Promise<unknown>) {
    if (busy) return;
    busy = label;
    error = "";
    note = "";
    try {
      const out = await action();
      if (typeof out === "string" && out.trim()) note = out.trim().split("\n").slice(-2).join(" · ");
    } catch (e) {
      error = String(e);
    }
    busy = "";
    await refresh();
  }

  const doFetch = () => run("Fetch", () => git.fetch(repo));
  const pull = (rebase = false) => run("Pull", () => git.pull(repo, rebase));
  const push = () => run("Push", () => git.push(repo, false));
  const forcePush = () =>
    (confirm = {
      title: "Force-Push",
      message: "Remote-Branch mit dem lokalen Stand überschreiben (--force-with-lease)? Fremde Commits, die du schon kennst, gehen dort verloren.",
      label: "Force-Push",
      run: () => git.push(repo, true),
    });

  async function doConfirm() {
    const c = confirm;
    confirm = null;
    if (c) await run(c.title, c.run);
  }

  async function createBranch() {
    const name = branchName?.trim();
    branchName = null;
    if (name) await run("Branch", () => git.checkout(repo, name, true, null));
  }

  async function doStash() {
    const s = stash;
    stash = null;
    if (s) await run("Stash", () => git.stashPush(repo, s.message.trim() || null, s.untracked));
  }

  /** Gleiche Ref zweimal anklicken soll erneut springen, daher kurz auf null. */
  async function selectRef(ref: string) {
    if (!canLeave()) return;
    conflictPath = null;
    tab = "history";
    jumpTo = null;
    await tick();
    jumpTo = ref;
  }

  function onkeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (ctrl && !e.shiftKey && e.key === "1") tab = "changes";
    else if (ctrl && !e.shiftKey && e.key === "2") tab = "history";
    else if (ctrl && e.shiftKey && k === "f") doFetch();
    else if (ctrl && e.shiftKey && k === "p") push();
    else if (ctrl && e.shiftKey && k === "l") pull();
    else if (e.key === "F5") reload();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} onfocus={() => refresh(false)} />

{#snippet tool(label: string, title: string, onclick: () => void, Icon: typeof CloudDownloadIcon)}
  <Button variant="ghost" size="sm" class="h-7 gap-1.5 px-2 text-[11px]" {onclick} {title} disabled={!!busy}>
    <Icon class="size-3.5" />
    {label}
  </Button>
{/snippet}

{#snippet menu(items: [string, () => void][])}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger
      class="text-muted-foreground hover:bg-secondary hover:text-foreground -ml-1 grid h-7 w-4 place-items-center rounded disabled:opacity-50"
      disabled={!!busy}
      aria-label="Weitere Optionen"><ChevronDownIcon class="size-3" /></DropdownMenu.Trigger
    >
    <DropdownMenu.Portal>
      <DropdownMenu.Content
        align="start"
        class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
      >
        {#each items as [label, onSelect] (label)}
          <DropdownMenu.Item class="hover:bg-accent data-highlighted:bg-accent cursor-pointer rounded px-2 py-1" {onSelect}
            >{label}</DropdownMenu.Item
          >
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class="bg-chrome border-border flex items-center gap-0.5 border-b px-2 py-1">
    {#if status}
      <span class="mr-2 flex items-center gap-1.5 font-mono text-[11px]" title={status.upstream ?? "Kein Upstream"}>
        <GitBranchIcon class="text-muted-foreground size-3.5" />
        <span class="font-semibold">{status.branch ?? `detached ${status.head.slice(0, 7)}`}</span>
        {#if status.ahead}<span class="text-primary">↑{status.ahead}</span>{/if}
        {#if status.behind}<span class="text-amber-400">↓{status.behind}</span>{/if}
      </span>
    {/if}
    {@render tool("Fetch", "Fetch (Strg+Umschalt+F)", doFetch, CloudDownloadIcon)}
    {@render tool("Pull", "Pull (Strg+Umschalt+L)", () => pull(), DownloadIcon)}
    {@render menu([
      ["Pull (Merge)", () => pull(false)],
      ["Pull (Rebase)", () => pull(true)],
    ])}
    {@render tool("Push", "Push (Strg+Umschalt+P)", push, UploadIcon)}
    {@render menu([["Force-Push …", forcePush]])}
    {@render tool("Stash", "Änderungen stashen", () => (stash = { message: "", untracked: true }), ArchiveIcon)}
    {@render tool("Branch", "Neuer Branch ab HEAD", () => (branchName = ""), GitBranchPlusIcon)}
    <span class="flex-1"></span>
    {#if busy}<span class="text-muted-foreground mr-2 text-[11px]">{busy} läuft …</span>{/if}
  </div>

  {#if status && status.state !== "clean"}
    <div class="border-border flex items-center gap-2 border-b bg-amber-400/10 px-3 py-1.5 text-amber-300">
      <TriangleAlertIcon class="size-3.5" />
      <span class="flex-1">
        {STATE_LABEL[status.state]} läuft{conflicts
          ? ` – ${conflicts} ${conflicts === 1 ? "Konflikt" : "Konflikte"} offen`
          : " – keine Konflikte mehr"}
      </span>
      <Button size="sm" class="h-6 text-[11px]" disabled={!!busy} onclick={() => run("Fortsetzen", () => git.continue(repo))}
        >Fortsetzen</Button
      >
      <Button
        variant="ghost"
        size="sm"
        class="h-6 text-[11px]"
        disabled={!!busy}
        onclick={() =>
          (confirm = {
            title: `${STATE_LABEL[status!.state as keyof typeof STATE_LABEL]} abbrechen`,
            message: "Zum Stand vor dem Vorgang zurückkehren? Bereits gelöste Konflikte gehen verloren.",
            label: "Abbrechen",
            run: () => git.abort(repo),
          })}>Abbrechen</Button
      >
    </div>
  {/if}

  {#if error}
    <button class="bg-destructive/15 text-destructive border-border border-b px-3 py-1.5 text-left whitespace-pre-wrap" onclick={() => (error = "")}
      >{error}</button
    >
  {:else if note}
    <button class="text-muted-foreground border-border truncate border-b px-3 py-1 text-left font-mono text-[11px]" onclick={() => (note = "")} title={note}
      >{note}</button
    >
  {/if}

  {#if status}
    <div class="flex min-h-0 flex-1">
      <div class="border-border shrink-0 border-r" style="width:{side}px; max-width:40%">
        <Sidebar
          {repo}
          {status}
          {refreshKey}
          onchange={refresh}
          onselectref={selectRef}
          onshowdiff={(title, diff) => (shown = { title, diff })}
        />
      </div>
      <Splitter bind:size={side} min={160} max={480} key="git.side" />
      <div class="flex min-w-0 flex-1 flex-col">
        {#if conflictPath}
          <MergeEditor
            {repo}
            path={conflictPath}
            bind:dirty={mergeDirty}
            onclose={() => {
              conflictPath = null;
              refresh();
            }}
          />
        {:else}
          <div class="border-border flex gap-1 border-b px-2">
            {#each [["changes", "Änderungen", "Strg+1"], ["history", "Verlauf", "Strg+2"]] as [id, label, key] (id)}
              <button
                class="-mb-px border-b-2 px-2 py-1.5 {tab === id
                  ? 'border-primary text-foreground'
                  : 'text-muted-foreground hover:text-foreground border-transparent'}"
                onclick={() => (tab = id as typeof tab)}
                title={key}
              >
                {label}{#if id === "changes" && status.files.length}<span class="text-muted-foreground ml-1.5 tabular-nums"
                    >{status.files.length}</span
                  >{/if}
              </button>
            {/each}
          </div>
          <div class="min-h-0 flex-1">
            <!-- Beide bleiben gemountet: Auswahl und Scrollstand ueberleben den Tabwechsel. -->
            <div class="h-full {tab === 'changes' ? '' : 'hidden'}">
              <ChangesPanel {repo} {status} onchange={refresh} onconflict={(p) => (conflictPath = p)} />
            </div>
            <div class="h-full {tab === 'history' ? '' : 'hidden'}">
              <HistoryPanel {repo} {jumpTo} {refreshKey} onchange={refresh} />
            </div>
          </div>
        {/if}
      </div>
    </div>
  {:else if !error}
    <p class="text-muted-foreground px-4 py-6">Lese Status …</p>
  {/if}
</div>

<Dialog.Root open={shown !== null} onOpenChange={(o) => !o && (shown = null)}>
  <Dialog.Content class="flex h-[80vh] flex-col sm:max-w-4xl">
    <Dialog.Header>
      <Dialog.Title>{shown?.title}</Dialog.Title>
    </Dialog.Header>
    <div class="border-border min-h-0 flex-1 rounded border">
      <DiffView diff={shown?.diff ?? ""} />
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={confirm !== null} onOpenChange={(o) => !o && (confirm = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{confirm?.title}</Dialog.Title>
      <Dialog.Description>{confirm?.message}</Dialog.Description>
    </Dialog.Header>
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (confirm = null)}>Zurück</Button>
      <Button variant="destructive" size="sm" onclick={doConfirm}>{confirm?.label}</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={branchName !== null} onOpenChange={(o) => !o && (branchName = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Neuer Branch</Dialog.Title>
      <Dialog.Description>Wird ab HEAD angelegt und ausgecheckt.</Dialog.Description>
    </Dialog.Header>
    {#if branchName !== null}
      <Input bind:value={branchName} placeholder="feature/…" class="font-mono" onkeydown={(e) => e.key === "Enter" && createBranch()} autofocus />
    {/if}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (branchName = null)}>Abbrechen</Button>
      <Button size="sm" onclick={createBranch}>Anlegen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={stash !== null} onOpenChange={(o) => !o && (stash = null)}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Änderungen stashen</Dialog.Title>
    </Dialog.Header>
    {#if stash}
      <Input bind:value={stash.message} placeholder="Nachricht (optional)" onkeydown={(e) => e.key === "Enter" && doStash()} autofocus />
      <label class="flex items-center gap-2 text-xs">
        <input type="checkbox" bind:checked={stash.untracked} class="accent-primary" />
        Neue (untracked) Dateien einschließen
      </label>
    {/if}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (stash = null)}>Abbrechen</Button>
      <Button size="sm" onclick={doStash}>Stashen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
