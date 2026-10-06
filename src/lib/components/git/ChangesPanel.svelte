<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { ContextMenu } from "bits-ui";
  import { invoke } from "@tauri-apps/api/core";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { git, type FileChange, type Status } from "$lib/git";
  import { hunkPatch, linesPatch, type DiffFile, type Hunk } from "$lib/diff";
  import { pruneSel, rangeSel, toggleSel } from "$lib/selection";
  import DiffView, { type HunkAction } from "./DiffView.svelte";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  let {
    repo,
    status,
    onchange,
    onconflict,
    onedit,
  }: {
    repo: string;
    status: Status;
    onchange: () => void;
    onconflict: (path: string) => void;
    onedit?: (path: string) => void;
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

  // Mehrfachauswahl: immer nur in einer der beiden Listen. Konflikte stehen in keiner davon.
  let pick = $state<{ staged: boolean; paths: string[] }>({ staged: false, paths: [] });
  let anchor = ""; // Startpunkt fuer Umschalt+Klick
  const paths = (fs: FileChange[]) => fs.map((f) => f.path);

  // Was aus dem Status verschwindet, faellt aus der Auswahl (sonst waere es beim Wiederauftauchen wieder gewaehlt).
  $effect(() => {
    const kept = pruneSel(pick.paths, paths(pick.staged ? staged : unstaged));
    if (kept.length !== pick.paths.length) pick.paths = kept;
  });

  // Wirksame Auswahl; ohne eigene Auswahl zaehlt die Datei, deren Diff gerade gezeigt wird.
  const chosen = $derived.by(() => {
    const kept = pruneSel(pick.paths, paths(pick.staged ? staged : unstaged));
    if (kept.length || !sel) return { staged: pick.staged, paths: kept };
    return { staged: sel.staged, paths: [sel.path] };
  });
  const chosenSet = $derived(new Set(chosen.paths));
  const isChosen = (f: FileChange, isStaged: boolean) => chosen.staged === isStaged && chosenSet.has(f.path);
  const chosenIn = (files: FileChange[], isStaged: boolean) =>
    chosen.staged === isStaged ? files.filter((f) => chosenSet.has(f.path)) : [];

  function pickRow(e: MouseEvent, f: FileChange, files: FileChange[], isStaged: boolean) {
    // WebKit fokussiert Buttons beim Klick nicht; ohne Fokus kaemen Leertaste/Strg+A/Esc nicht an.
    (e.currentTarget as HTMLElement).focus();
    const same = chosen.staged === isStaged;
    let next = [f.path];
    if (e.shiftKey && same) next = rangeSel(paths(files), anchor, f.path);
    else if ((e.ctrlKey || e.metaKey) && same) next = toggleSel(chosen.paths, f.path);
    if (!e.shiftKey || !same) anchor = f.path;
    pick = { staged: isStaged, paths: next };
    if (next.includes(f.path)) sel = { path: f.path, staged: isStaged };
    else if (sel?.path === f.path && sel.staged === isStaged)
      sel = next.length ? { path: next[next.length - 1], staged: isStaged } : null;
  }

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

  // busy sperrt den Commit, solange eine Aktion laeuft (sonst zwei Commits per doppeltem Strg+Enter).
  let busy = $state(false);
  async function run(action: () => Promise<unknown>) {
    error = "";
    busy = true;
    try {
      await action();
      onchange();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const move = (fs: FileChange[], isStaged: boolean) =>
    run(() => (isStaged ? git.unstage(repo, paths(fs)) : git.stage(repo, paths(fs))));

  // Nach Stagen per Leertaste: Fokus auf die Datei, die jetzt an derselben Stelle der Liste steht.
  let refocus: { staged: boolean; index: number } | null = null;
  $effect(() => {
    status;
    if (!refocus) return;
    const { staged: s, index } = refocus;
    refocus = null;
    const here = document.querySelectorAll<HTMLElement>(`[data-file-list="${s}"]`);
    (here[Math.min(index, here.length - 1)] ?? document.querySelector<HTMLElement>("[data-file-list]"))?.focus();
  });

  // Bestaetigung fuer Verwerfen.
  let confirm = $state<{
    title: string;
    message: string;
    files?: FileChange[];
    run: () => Promise<unknown>;
  } | null>(null);
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

  function discard(fs: FileChange[]) {
    if (fs.length === 0) return;
    const [f] = fs;
    const loose = fs.filter((x) => x.worktree === "?").length;
    const go = () => git.discard(repo, paths(fs));
    confirm =
      fs.length === 1
        ? {
            title: "Änderungen verwerfen?",
            message:
              f.worktree === "?"
                ? `${f.path} ist nicht versioniert und wird gelöscht.`
                : `Alle nicht gestageten Änderungen in ${f.path} gehen verloren.`,
            run: go,
          }
        : {
            title: `Änderungen in ${fs.length} Dateien verwerfen?`,
            message:
              `Alle nicht gestageten Änderungen in diesen ${fs.length} Dateien gehen verloren.` +
              (loose
                ? ` ${loose === fs.length ? "Alle" : loose} davon ${loose === 1 ? "ist" : "sind"} nicht versioniert (?) und ${loose === 1 ? "wird" : "werden"} gelöscht.`
                : "") +
              " Das lässt sich nicht rückgängig machen.",
            files: fs,
            run: go,
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
  const canCommit = $derived(!busy && message.trim() !== "" && (amend || staged.length > 0));
  // Fuer Amend geladene Nachricht; beim Abwaehlen wieder weg, sofern unveraendert.
  let amendMsg = "";

  async function toggleAmend() {
    if (!amend) {
      if (message === amendMsg) message = "";
      amendMsg = "";
      return;
    }
    if (message.trim()) return;
    try {
      const d = await git.show(repo, "HEAD");
      if (!amend || message.trim()) return;
      message = amendMsg = d.body.trim() ? `${d.commit.subject}\n\n${d.body.trim()}` : d.commit.subject;
    } catch (e) {
      error = String(e);
    }
  }

  function commit(push = false) {
    if (!canCommit) return;
    run(async () => {
      await git.commit(repo, message, amend);
      message = amendMsg = "";
      amend = false;
      if (!push) return;
      // Der Commit steht schon: Liste sofort aktualisieren, ein Push-Fehler darf ihn nicht verstecken.
      onchange();
      await git.push(repo, false).catch((e) => {
        throw `Commit erstellt, Push fehlgeschlagen: ${e}`;
      });
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
  {@const picked = chosenIn(files, isStaged)}
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="bg-chrome border-border flex items-center gap-1 border-b px-2 py-1">
      <span class="flex-1 font-semibold">{title}</span>
      <span class="text-muted-foreground font-mono text-[10px]">{files.length}</span>
      <Button
        variant="ghost"
        size="xs"
        class="h-5 text-[11px]"
        disabled={files.length === 0}
        onclick={() => move(files, isStaged)}
      >
        {isStaged ? "Alles unstagen" : "Alles stagen"}
      </Button>
    </div>
    {#if picked.length > 1}
      <div class="bg-chrome border-border flex flex-wrap items-center gap-1 border-b px-2 py-0.5">
        <span class="text-muted-foreground flex-1 text-[11px] whitespace-nowrap">{picked.length} ausgewählt</span>
        <Button
          variant="ghost"
          size="xs"
          class="h-5 text-[11px]"
          disabled={busy}
          title={isStaged ? "Ausgewählte unstagen" : "Ausgewählte stagen"}
          onclick={() => move(picked, isStaged)}
        >
          {isStaged ? "Unstagen" : "Stagen"}
        </Button>
        {#if !isStaged}
          <Button
            variant="ghost"
            size="xs"
            class="text-destructive h-5 text-[11px]"
            disabled={busy}
            title="Ausgewählte verwerfen …"
            onclick={() => discard(picked)}
          >
            Verwerfen …
          </Button>
        {/if}
      </div>
    {/if}
    <div class="min-h-0 flex-1 overflow-y-auto">
      {#each files as f (f.path)}
        {@const code = isStaged ? f.index : f.worktree}
        <!-- Wie im Dateimanager: Menue wirkt auf die ganze Auswahl, wenn die Zeile dazugehoert, sonst nur auf sie. -->
        {@const multi = isChosen(f, isStaged) && picked.length > 1}
        <ContextMenu.Root>
          <ContextMenu.Trigger>
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 px-2 py-0.5 text-left text-[11px] select-none {isChosen(
                f,
                isStaged,
              ) ||
              (sel?.path === f.path && sel.staged === isStaged)
                ? 'bg-accent'
                : ''}"
              title={f.orig ? `${f.orig} → ${f.path}` : f.path}
              data-file-list={isStaged}
              onclick={(e) => pickRow(e, f, files, isStaged)}
              ondblclick={() => move([f], isStaged)}
              onkeydown={(e) => {
                if (e.key === " ") {
                  e.preventDefault();
                  refocus = { staged: isStaged, index: files.indexOf(f) };
                  move([f], isStaged);
                } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
                  e.preventDefault();
                  pick = { staged: isStaged, paths: paths(files) };
                  if (sel?.staged !== isStaged) sel = { path: f.path, staged: isStaged };
                } else if (e.key === "Escape" && picked.length > 1) {
                  // Nur die Mehrfachauswahl aufheben (Diff bleibt stehen) und die Ansicht nicht verlassen;
                  // ohne Mehrfachauswahl geht Esc wie bisher ans Fenster (+page.svelte: zurueck).
                  e.stopPropagation();
                  pick = { staged: isStaged, paths: [] };
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
              {#if multi}
                {@render menuItem(
                  `Ausgewählte ${isStaged ? "unstagen" : "stagen"} (${picked.length})`,
                  () => move(picked, isStaged),
                )}
                {#if !isStaged}
                  {@render menuItem(
                    `Ausgewählte verwerfen (${picked.length}) …`,
                    () => discard(picked),
                    true,
                  )}
                {/if}
              {:else}
                {@render menuItem(isStaged ? "Unstagen" : "Stagen", () => move([f], isStaged))}
                {#if !isStaged}
                  {@render menuItem("Änderungen verwerfen …", () => discard([f]), true)}
                {/if}
              {/if}
              {#if onedit && code !== "D"}
                {@render menuItem("Im Editor öffnen", () => onedit(f.path))}
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
  <Notice bind:text={error} />
  <div class="flex min-h-0 flex-1">
    <!-- Schrumpft mit, damit auch im kleinen Fenster Platz fuer den Diff bleibt -->
    <div class="border-border flex w-2/5 max-w-80 min-w-48 shrink-0 flex-col border-r">
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
          placeholder="Commit-Nachricht (Strg+Enter, mit Umschalt: + Push)"
          bind:value={message}
          onkeydown={(e) => {
            if (e.key === "Enter" && e.ctrlKey) {
              e.preventDefault();
              // Amend braeuchte nach einem Push Force: das bleibt dem Force-Push in der Toolbar.
              commit(e.shiftKey && !amend);
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
          <Button size="xs" disabled={!canCommit} onclick={() => commit()}>
            {amend ? "Amend" : "Commit"}{staged.length ? ` (${staged.length})` : ""}
          </Button>
          {#if !amend}
            <Button size="xs" variant="outline" disabled={!canCommit} onclick={() => commit(true)} title="Strg+Umschalt+Enter">
              Commit + Push
            </Button>
          {/if}
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
    {#if confirm?.files}
      <ul class="border-border max-h-40 overflow-y-auto rounded-md border py-1 font-mono text-[11px]">
        {#each confirm.files as f (f.path)}
          <li class="flex gap-1.5 px-2" title={f.path}>
            <span class="w-3 shrink-0 text-center font-bold {color[f.worktree] ?? ''}">{f.worktree}</span>
            <span class="min-w-0 flex-1 truncate">{f.path}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => (confirm = null)}>Abbrechen</Button>
      <Button variant="destructive" size="sm" onclick={doConfirm}>Verwerfen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
