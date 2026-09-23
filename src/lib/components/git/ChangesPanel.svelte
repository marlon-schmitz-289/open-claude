<script lang="ts">
  import { ContextMenu } from "bits-ui";
  import { invoke } from "@tauri-apps/api/core";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { git, type FileChange, type Status } from "$lib/git";
  import { hunkPatch, linesPatch, type DiffFile, type Hunk } from "$lib/diff";
  import DiffView, { type HunkAction } from "./DiffView.svelte";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  let {
    repo,
    status,
    onchange,
    onconflict,
  }: {
    repo: string;
    status: Status;
    onchange: () => void;
    onconflict: (path: string) => void;
  } = $props();

  const conflicts = $derived(status.files.filter((f) => f.conflict));
  const staged = $derived(status.files.filter((f) => !f.conflict && f.index !== "." && f.index !== "?"));
  const unstaged = $derived(status.files.filter((f) => !f.conflict && f.worktree !== "."));

  let sel = $state<{ path: string; staged: boolean } | null>(null);
  let diff = $state("");
  let error = $state("");

  // Nach Statuswechsel: Datei auf der anderen Seite weiter zeigen, falls sie nur dort noch liegt.
  $effect(() => {
    if (!sel) return;
    const { path, staged: s } = sel;
    const here = (s ? staged : unstaged).some((f) => f.path === path);
    const there = (s ? unstaged : staged).some((f) => f.path === path);
    if (!here) sel = there ? { path, staged: !s } : null;
  });

  // Alter Diff darf nicht mit den Aktionen der neuen Seite (staged/unstaged) angewendet werden.
  let shownKey = "";
  $effect(() => {
    status; // bei jedem neuen Status neu laden
    const cur = sel;
    const key = cur ? `${cur.staged}:${cur.path}` : "";
    if (key !== shownKey) diff = "";
    shownKey = key;
    if (!cur) return;
    git
      .diff(repo, cur.path, cur.staged)
      .then((d) => sel === cur && (diff = d))
      .catch((e) => (error = String(e)));
  });

  async function run(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  const toggle = (f: FileChange, isStaged: boolean) =>
    run(() => (isStaged ? git.unstage(repo, [f.path]) : git.stage(repo, [f.path])));

  // Bestaetigung fuer Verwerfen.
  let confirm = $state<{ title: string; message: string; run: () => Promise<unknown> } | null>(null);
  function doConfirm() {
    const action = confirm!.run;
    confirm = null;
    run(action);
  }

  const patch = (file: DiffFile, hunk: Hunk, lines: Set<number>, reverse: boolean) =>
    lines.size ? linesPatch(file, hunk, lines, reverse) : hunkPatch(file, hunk);

  function apply(p: string, cached: boolean, reverse: boolean) {
    if (!p) error = "Keine geänderte Zeile gewählt.";
    else run(() => git.apply(repo, p, cached, reverse));
  }

  const actions = $derived<HunkAction[]>(
    sel?.staged
      ? [
          {
            label: "Hunk unstagen",
            lineLabel: "Zeilen unstagen",
            run: (f, h, l) => apply(patch(f, h, l, true), true, true),
          },
        ]
      : [
          {
            label: "Hunk stagen",
            lineLabel: "Zeilen stagen",
            run: (f, h, l) => apply(patch(f, h, l, false), true, false),
          },
          {
            label: "Hunk verwerfen",
            lineLabel: "Zeilen verwerfen",
            destructive: true,
            run: (f, h, l) => {
              const p = patch(f, h, l, true);
              confirm = {
                title: l.size ? "Zeilen verwerfen?" : "Hunk verwerfen?",
                message: `Die Änderungen in ${f.newPath} gehen verloren.`,
                run: () => git.apply(repo, p, false, true),
              };
            },
          },
        ],
  );

  function discard(f: FileChange) {
    confirm = {
      title: "Änderungen verwerfen?",
      message:
        f.worktree === "?"
          ? `${f.path} ist nicht versioniert und wird gelöscht.`
          : `Alle nicht gestageten Änderungen in ${f.path} gehen verloren.`,
      run: () => git.discard(repo, [f.path]),
    };
  }

  function reveal(f: FileChange) {
    const dir = f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/")) : "";
    run(() => invoke("reveal", { path: (dir ? `${repo}/${dir}` : repo).replaceAll("/", "\\") }));
  }

  // ---------- Commit ----------
  let message = $state("");
  let amend = $state(false);
  const subjectLen = $derived(message.split("\n")[0].length);
  const canCommit = $derived(message.trim() !== "" && (amend || staged.length > 0));

  async function toggleAmend() {
    if (!amend || message.trim()) return;
    try {
      const d = await git.show(repo, "HEAD");
      message = d.body.trim() ? `${d.commit.subject}\n\n${d.body.trim()}` : d.commit.subject;
    } catch (e) {
      error = String(e);
    }
  }

  function commit() {
    if (!canCommit) return;
    run(async () => {
      await git.commit(repo, message, amend);
      message = "";
      amend = false;
    });
  }

  const color: Record<string, string> = {
    M: "text-amber-400",
    A: "text-emerald-400",
    D: "text-red-400",
    R: "text-sky-400",
    C: "text-sky-400",
    "?": "text-muted-foreground",
  };
</script>

{#snippet menuItem(label: string, onSelect: () => void, destructive = false)}
  <ContextMenu.Item
    class="hover:bg-accent flex cursor-pointer items-center rounded px-2 py-1 {destructive ? 'text-destructive' : ''}"
    {onSelect}
  >
    {label}
  </ContextMenu.Item>
{/snippet}

{#snippet list(title: string, files: FileChange[], isStaged: boolean)}
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="bg-chrome border-border flex items-center gap-1 border-b px-2 py-1">
      <span class="flex-1 font-semibold">{title}</span>
      <span class="text-muted-foreground font-mono text-[10px]">{files.length}</span>
      <Button
        variant="ghost"
        size="xs"
        class="h-5 text-[11px]"
        disabled={files.length === 0}
        onclick={() =>
          run(() =>
            isStaged
              ? git.unstage(repo, files.map((f) => f.path))
              : git.stage(repo, files.map((f) => f.path)),
          )}
      >
        {isStaged ? "Alles unstagen" : "Alles stagen"}
      </Button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto">
      {#each files as f (f.path)}
        {@const code = isStaged ? f.index : f.worktree}
        <ContextMenu.Root>
          <ContextMenu.Trigger>
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-0.5 text-left text-[11px] {sel?.path ===
                f.path && sel.staged === isStaged
                ? 'bg-accent'
                : ''}"
              title={f.orig ? `${f.orig} → ${f.path}` : f.path}
              onclick={() => (sel = { path: f.path, staged: isStaged })}
              ondblclick={() => toggle(f, isStaged)}
              onkeydown={(e) => {
                if (e.key === " ") {
                  e.preventDefault();
                  toggle(f, isStaged);
                }
              }}
            >
              <span class="w-3 shrink-0 text-center font-mono font-bold {color[code] ?? ''}">{code}</span>
              <span class="min-w-0 flex-1 truncate font-mono">{f.path}</span>
            </button>
          </ContextMenu.Trigger>
          <ContextMenu.Portal>
            <ContextMenu.Content
              class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-44 rounded-md p-1 text-xs ring-1"
            >
              {@render menuItem(isStaged ? "Unstagen" : "Stagen", () => toggle(f, isStaged))}
              {#if !isStaged}
                {@render menuItem("Änderungen verwerfen …", () => discard(f), true)}
              {/if}
              {@render menuItem("Im Explorer zeigen", () => reveal(f))}
              {@render menuItem("Pfad kopieren", () => navigator.clipboard.writeText(f.path))}
            </ContextMenu.Content>
          </ContextMenu.Portal>
        </ContextMenu.Root>
      {:else}
        <p class="text-muted-foreground px-2 py-1 text-[11px]">Keine.</p>
      {/each}
    </div>
  </div>
{/snippet}

<div class="flex h-full min-h-0 flex-col text-xs">
  {#if error}
    <button
      class="bg-destructive/15 text-destructive border-destructive/30 border-b px-2 py-1 text-left text-[11px] whitespace-pre-wrap"
      title="Ausblenden"
      onclick={() => (error = "")}>{error}</button
    >
  {/if}
  <div class="flex min-h-0 flex-1">
    <div class="border-border flex w-80 shrink-0 flex-col border-r">
      {#if conflicts.length}
        <div class="border-border max-h-40 overflow-y-auto border-b">
          <div class="bg-destructive/15 text-destructive flex items-center gap-1.5 px-2 py-1 font-semibold">
            <TriangleAlertIcon class="size-3.5" />
            <span class="flex-1">Konflikte</span>
            <span class="font-mono text-[10px]">{conflicts.length}</span>
          </div>
          {#each conflicts as f (f.path)}
            <button
              class="text-destructive hover:bg-accent flex w-full items-center gap-1.5 px-2 py-0.5 text-left text-[11px]"
              title="Im Merge-Editor öffnen"
              onclick={() => onconflict(f.path)}
            >
              <span class="w-3 shrink-0 text-center font-mono font-bold">U</span>
              <span class="min-w-0 flex-1 truncate font-mono">{f.path}</span>
            </button>
          {/each}
        </div>
      {/if}
      {@render list("Gestaged", staged, true)}
      <div class="border-border flex min-h-0 flex-1 flex-col border-t">{@render list("Nicht gestaged", unstaged, false)}</div>

      <!-- Commit -->
      <div class="border-border flex flex-col gap-1.5 border-t p-2">
        <textarea
          class="border-input bg-background focus-visible:ring-ring/50 h-24 resize-none rounded-md border px-2 py-1 font-mono text-[11px] outline-none focus-visible:ring-2"
          placeholder="Commit-Nachricht (Strg+Enter)"
          bind:value={message}
          onkeydown={(e) => {
            if (e.key === "Enter" && e.ctrlKey) {
              e.preventDefault();
              commit();
            }
          }}
        ></textarea>
        <div class="flex items-center gap-2">
          <label class="flex cursor-pointer items-center gap-1 text-[11px]">
            <input type="checkbox" bind:checked={amend} onchange={toggleAmend} />
            Amend
          </label>
          <span
            class="flex-1 font-mono text-[10px] {subjectLen > 72 ? 'text-amber-400' : 'text-muted-foreground'}"
            title={subjectLen > 72 ? "Betreffzeile länger als 72 Zeichen" : "Länge der Betreffzeile"}
          >
            {subjectLen}/72
          </span>
          <Button size="xs" disabled={!canCommit} onclick={commit}>
            {amend ? "Amend" : "Commit"}{staged.length ? ` (${staged.length})` : ""}
          </Button>
        </div>
      </div>
    </div>

    <div class="min-w-0 flex-1">
      {#if sel}
        <DiffView {diff} {actions} />
      {:else}
        <p class="text-muted-foreground p-3">Datei links auswählen.</p>
      {/if}
    </div>
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
      <Button variant="destructive" size="sm" onclick={doConfirm}>Verwerfen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
