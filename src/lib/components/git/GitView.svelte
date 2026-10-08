<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { ConfirmDialog, PromptDialog, Tabs, menuContent, menuItem, menuSeparator } from "$lib/components/kit";
  import { tick, untrack } from "svelte";
  import { DropdownMenu } from "bits-ui";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { git, forge, accountId, type Account, type ForgeKind, type RepoAccount, type Status } from "$lib/git";
  import Sidebar from "./Sidebar.svelte";
  import AskDialog from "./AskDialog.svelte";
  import ChangesPanel from "./ChangesPanel.svelte";
  import HistoryPanel from "./HistoryPanel.svelte";
  import ReleasesPanel from "./ReleasesPanel.svelte";
  import PipelinesPanel from "./PipelinesPanel.svelte";
  import UnityPanel from "$lib/components/unity/UnityPanel.svelte";
  import SkillsPanel from "$lib/components/skills/SkillsPanel.svelte";
  import type { SkillStore, ProjectSkills } from "$lib/skills";
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
  import UserIcon from "@lucide/svelte/icons/user";
  import CheckIcon from "@lucide/svelte/icons/check";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";

  let {
    repo,
    accounts,
    onaccounts,
    unity = false,
    kind = null,
    skillStore,
    onskills,
    onedit,
  }: {
    repo: string;
    accounts: Account[];
    onaccounts: () => void;
    unity?: boolean;
    kind?: string | null;
    skillStore: SkillStore;
    onskills: (ps: ProjectSkills) => void;
    onedit?: (path: string) => void;
  } = $props();

  const isUnity = $derived(unity);

  let status = $state<Status | null>(null);
  let refreshKey = $state(0);
  // Forge-Panels kosten API-Calls: nur nachziehen, wenn sich HEAD/Branch bewegt oder das Remote angefasst wurde.
  let forgeKey = $state(0);
  let side = $state(stored("git.side", 240));
  let tab = $state<"changes" | "history" | "releases" | "ci" | "unity" | "skills">("changes");
  // Forge-Tabs erst beim ersten Oeffnen mounten: sonst API-Calls, obwohl nie hingeschaut wird.
  let seen = $state({ releases: false, ci: false, unity: false, skills: false });
  let ciKind = $state<ForgeKind | null>(null);
  $effect(() => {
    if (tab !== "changes" && tab !== "history") seen[tab] = true;
  });
  let conflictPath = $state<string | null>(null);
  let jumpTo = $state<string | null>(null);
  let busy = $state("");
  let error = $state("");
  let note = $state("");
  let shown = $state<{ title: string; diff: string } | null>(null);
  let confirm = $state<{ title: string; message: string; label: string; run: () => Promise<unknown> } | null>(null);
  // Offen-Flag getrennt, weil PromptDialog einen String bindet
  let branchOpen = $state(false);
  let branchName = $state("");
  let stashOpen = $state(false);
  let stash = $state({ message: "", untracked: true });

  const conflicts = $derived(status?.files.filter((f) => f.conflict).length ?? 0);
  const STATE_LABEL = { merge: "Merge", rebase: "Rebase", "cherry-pick": "Cherry-Pick", revert: "Revert" };

  /** Status neu lesen; refreshKey laesst Sidebar und Verlauf nachziehen. Beim Fokus nur, wenn HEAD sich bewegt hat. */
  async function refresh(force = true, remote = false) {
    const before = `${status?.head}|${status?.branch}`;
    try {
      status = await git.status(repo);
      // Nach Abbrechen/Fortsetzen (auch extern) steht die Datei nicht mehr im Konflikt: Editor zu, sonst zeigt er Altes.
      if (conflictPath && !status.files.some((f) => f.conflict && f.path === conflictPath)) conflictPath = null;
    } catch (e) {
      error = `Status: ${e}`;
    }
    const moved = before !== `${status?.head}|${status?.branch}`;
    if (force || moved) refreshKey++;
    if (remote || moved) forgeKey++;
  }

  /** Fuer den Aktualisieren-Knopf in der Titelleiste und F5; alte Meldungen gelten dann nicht mehr. */
  export const reload = () => {
    error = note = "";
    return refresh(true, true);
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

  // Konto des Repos (git config ocui.account) fuer Push/Pull/API und die Commit-Identitaet.
  let acct = $state<RepoAccount | null>(null);
  const acctFor = $derived(accounts.find((a) => accountId(a) === acct?.effective) ?? null);
  const hostAccounts = $derived(accounts.filter((a) => a.host === acct?.host));

  async function loadAccount() {
    const r = repo;
    try {
      const res = await forge.repoAccount(r);
      if (r === repo) acct = res;
    } catch (e) {
      if (r === repo) error = `Konto: ${e}`;
    }
  }

  $effect(() => {
    repo;
    acct = null;
    loadAccount();
  });

  /** Neues Konto setzen; Forge-Panels laden danach mit dessen Token nach. */
  async function setAccount(id: string | null) {
    error = "";
    try {
      acct = await forge.setRepoAccount(repo, id);
    } catch (e) {
      error = String(e);
    }
    await refresh(true, true);
  }

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
    await refresh(true, ["Fetch", "Pull", "Push", "Force-Push"].includes(label));
  }

  /** Ladeanzeige fuer Aktionen aus Sidebar und Verlauf; Fehler und Refresh bleiben beim Aufrufer. */
  async function track<T>(label: string, action: () => Promise<T>): Promise<T> {
    busy = label;
    try {
      return await action();
    } finally {
      busy = "";
    }
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
    if (!branchOpen) return; // doppeltes Enter
    const name = branchName.trim();
    branchOpen = false;
    if (name) await run("Branch", () => git.checkout(repo, name, true, null));
  }

  async function doStash() {
    if (!stashOpen) return; // doppeltes Enter
    const s = stash;
    stashOpen = false;
    await run("Stash", () => git.stashPush(repo, s.message.trim() || null, s.untracked));
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
    else if (ctrl && !e.shiftKey && e.key === "3") tab = "releases";
    else if (ctrl && !e.shiftKey && e.key === "4") tab = "ci";
    else if (ctrl && !e.shiftKey && e.key === "5" && isUnity) tab = "unity";
    else if (ctrl && !e.shiftKey && e.key === "6") tab = "skills";
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
        class={menuContent}
      >
        {#each items as [label, onSelect] (label)}
          <DropdownMenu.Item class={menuItem} {onSelect}
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
        {#if busy}
          <LoaderCircleIcon class="text-primary size-3.5 animate-spin" />
        {:else}
          <GitBranchIcon class="text-muted-foreground size-3.5" />
        {/if}
        <span class="font-semibold">{status.branch ?? `detached ${status.head.slice(0, 7)}`}</span>
        {#if status.ahead}<span class="text-primary">↑{status.ahead}</span>{/if}
        {#if status.behind}<span class="text-warning">↓{status.behind}</span>{/if}
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
    {@render tool("Stash", "Änderungen stashen", () => ((stash = { message: "", untracked: true }), (stashOpen = true)), ArchiveIcon)}
    {@render tool("Branch", "Neuer Branch ab HEAD", () => ((branchName = ""), (branchOpen = true)), GitBranchPlusIcon)}
    <span class="flex-1"></span>
    {#if acct}
      <DropdownMenu.Root onOpenChange={(o) => o && loadAccount()}>
        <DropdownMenu.Trigger
          class="text-muted-foreground hover:bg-secondary hover:text-foreground flex h-7 items-center gap-1.5 rounded px-2 text-[11px]"
          title={acct.name || acct.email ? `Commit als: ${acct.name ?? "?"} <${acct.email ?? "?"}>` : "Keine Commit-Identität gesetzt"}
        >
          {#if acctFor?.avatar}
            <img src={acctFor.avatar} alt="" class="size-4 rounded-full" />
          {:else}
            <UserIcon class="size-3.5" />
          {/if}
          {#if acct.effective}
            {acct.effective.slice(acct.effective.lastIndexOf(":") + 1)}
            {#if !acct.account}<span class="opacity-60">(auto)</span>{/if}
          {:else}
            System-Git
          {/if}
          <ChevronDownIcon class="size-3" />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            align="end"
            class={menuContent}
          >
            {#each hostAccounts as a (accountId(a))}
              <DropdownMenu.Item
                class={menuItem}
                onSelect={() => setAccount(accountId(a))}
              >
                <CheckIcon class="size-3 {acct.account === accountId(a) ? '' : 'invisible'}" />
                {a.user}<span class="text-muted-foreground font-mono text-[10px]">@{a.host}</span>
              </DropdownMenu.Item>
            {/each}
            <DropdownMenu.Item
              class={menuItem}
              onSelect={() => setAccount(null)}
            >
              <CheckIcon class="size-3 {acct.account ? 'invisible' : ''}" />
              Standard (System-Git)
            </DropdownMenu.Item>
            <DropdownMenu.Separator class={menuSeparator} />
            <DropdownMenu.Item class={menuItem} onSelect={onaccounts}
              >Konten verwalten …</DropdownMenu.Item
            >
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    {/if}
    {#if busy}
      <span class="text-muted-foreground mr-2 flex items-center gap-1.5 text-[11px]" role="status">
        <LoaderCircleIcon class="size-3.5 animate-spin" />{busy} läuft …
      </span>
    {/if}
  </div>

  {#if status && status.state !== "clean"}
    <div class="border-border flex items-center gap-2 border-b bg-warning/10 px-3 py-1.5 text-warning">
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

  <Notice bind:text={error} />
  {#if !error}<Notice bind:text={note} kind="note" />{/if}

  {#if status}
    <div class="flex min-h-0 flex-1">
      <div class="border-border shrink-0 border-r" style="width:{side}px; max-width:40%">
        <Sidebar
          {repo}
          {status}
          {refreshKey}
          busy={!!busy}
          {track}
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
          <Tabs
            bind:value={tab}
            tabs={[
              { id: "changes", label: "Änderungen", title: "Strg+1", count: status.files.length },
              { id: "history", label: "Verlauf", title: "Strg+2" },
              { id: "releases", label: "Releases", title: "Strg+3" },
              { id: "ci", label: ciKind === "gitlab" ? "Pipelines" : ciKind === "github" ? "Actions" : "CI", title: "Strg+4" },
              ...(isUnity ? [{ id: "unity" as const, label: "Unity", title: "Strg+5" }] : []),
              { id: "skills", label: "Skills", title: "Strg+6" },
            ]}
          />
          <div class="min-h-0 flex-1">
            <!-- Einmal geoeffnet bleiben alle gemountet: Auswahl und Scrollstand ueberleben den Tabwechsel. -->
            <div class="h-full {tab === 'changes' ? '' : 'hidden'}">
              <ChangesPanel {repo} {status} {onedit} onchange={refresh} onconflict={(p) => (conflictPath = p)} />
            </div>
            <div class="h-full {tab === 'history' ? '' : 'hidden'}">
              <HistoryPanel {repo} {jumpTo} {refreshKey} busy={!!busy} {track} onchange={refresh} />
            </div>
            {#if seen.releases}
              <div class="h-full {tab === 'releases' ? '' : 'hidden'}">
                <ReleasesPanel {repo} refreshKey={forgeKey} />
              </div>
            {/if}
            {#if seen.ci}
              <div class="h-full {tab === 'ci' ? '' : 'hidden'}">
                <PipelinesPanel {repo} refreshKey={forgeKey} visible={tab === "ci"} bind:kind={ciKind} />
              </div>
            {/if}
            {#if seen.unity}
              <div class="h-full {tab === 'unity' ? '' : 'hidden'}">
                <UnityPanel {repo} />
              </div>
            {/if}
            {#if seen.skills}
              <div class="h-full {tab === 'skills' ? '' : 'hidden'}">
                <SkillsPanel {repo} {kind} store={skillStore} onproject={onskills} />
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {:else if !error}
    <p class="text-muted-foreground px-4 py-6">Lese Status …</p>
  {/if}
</div>

<AskDialog />

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

<ConfirmDialog
  open={confirm !== null}
  title={confirm?.title ?? ""}
  message={confirm?.message}
  action={confirm?.label ?? ""}
  onconfirm={doConfirm}
  oncancel={() => (confirm = null)}
/>

<PromptDialog
  open={branchOpen}
  title="Neuer Branch"
  description="Wird ab HEAD angelegt und ausgecheckt."
  bind:value={branchName}
  placeholder="feature/…"
  action="Anlegen"
  mono
  onsubmit={createBranch}
  oncancel={() => (branchOpen = false)}
/>

<PromptDialog
  open={stashOpen}
  title="Änderungen stashen"
  bind:value={stash.message}
  placeholder="Nachricht (optional)"
  action="Stashen"
  onsubmit={doStash}
  oncancel={() => (stashOpen = false)}
>
  <label class="flex items-center gap-2 text-xs">
    <input type="checkbox" bind:checked={stash.untracked} class="accent-primary" />
    Neue (untracked) Dateien einschließen
  </label>
</PromptDialog>
