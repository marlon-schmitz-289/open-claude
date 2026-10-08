<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ContextMenu } from "bits-ui";
  import Notice from "$lib/components/Notice.svelte";
  import { ContextItem, PromptDialog, menuContent } from "$lib/components/kit";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import FilePlusIcon from "@lucide/svelte/icons/file-plus";
  import FolderPlusIcon from "@lucide/svelte/icons/folder-plus";
  import { fs, type Entry } from "$lib/files";
  import { editor, open, setExpanded, renamed, removed, poll } from "$lib/editor.svelte";
  import { rows, join, dirname, type Row } from "$lib/tree";
  import { under } from "$lib/editor.logic";

  let { repo }: { repo: string } = $props();

  const s = $derived(editor(repo));
  const mac = navigator.userAgent.includes("Mac");
  let error = $state("");

  // Listing je Ordner, eine Ebene. Nur Projektordner und aufgeklappte Ordner werden gelesen.
  let lists = $state<Record<string, Entry[]>>({});
  // mtime, mit der ein Ordner zuletzt gelistet wurde (nicht reaktiv): weicht s.dirs ab, wird neu gelesen.
  const seen = new Map<string, number | null | undefined>();

  $effect(() => {
    const watched = ["", ...s.expanded];
    for (const dir of watched) {
      const mtime = s.dirs[dir];
      if (seen.has(dir) && seen.get(dir) === mtime) continue;
      seen.set(dir, mtime);
      load(dir);
    }
    // Zugeklappte werden nicht mehr beobachtet: beim naechsten Aufklappen frisch lesen.
    for (const dir of seen.keys()) if (!watched.includes(dir)) seen.delete(dir);
  });

  async function load(dir: string) {
    try {
      lists[dir] = await fs.list(repo, dir);
    } catch (e) {
      // Aufgeklappter Ordner ist weg (geloescht, Branch gewechselt): still zuklappen.
      if (dir) setExpanded(repo, dir, false);
      else error = String(e);
    }
  }

  const visible = $derived(rows(lists, s.expanded));

  function click(r: Row) {
    if (r.dir) setExpanded(repo, r.path, !r.open);
    else open(repo, r.path);
  }

  async function run(action: () => Promise<void>) {
    error = "";
    try {
      await action();
    } catch (e) {
      error = String(e);
    }
    // Auch nach einem Fehler: die Platte kann sich trotzdem geaendert haben.
    await poll(repo).catch(() => {});
  }

  // Texteingabe fuer Neue Datei / Neuer Ordner / Umbenennen.
  let prompt = $state<{ title: string; run: (v: string) => Promise<void> } | null>(null);
  let promptValue = $state("");
  function askPrompt(title: string, value: string, action: (v: string) => Promise<void>) {
    prompt = { title, run: action };
    promptValue = value;
  }
  function doPrompt() {
    if (!prompt) return;
    const action = prompt.run;
    const value = promptValue.trim();
    prompt = null;
    if (value) run(() => action(value));
  }

  function create(dir: string, isDir: boolean) {
    askPrompt(`${isDir ? "Neuer Ordner" : "Neue Datei"} in ${dir || "Projektordner"}`, "", async (name) => {
      const path = join(dir, name);
      await fs.create(repo, path, isDir);
      if (dir) setExpanded(repo, dir, true);
      if (!isDir) await open(repo, path);
    });
  }

  function rename(r: Row) {
    askPrompt(`${r.name} umbenennen`, r.name, async (name) => {
      const to = join(dirname(r.path), name);
      if (to === r.path) return;
      await fs.rename(repo, r.path, to);
      renamed(repo, r.path, to);
    });
  }

  function remove(r: Row) {
    const dirty = Object.values(s.files).some((f) => f.dirty && under(f.path, r.path));
    const message = `„${r.path}“ in den Papierkorb legen?${dirty ? "\nUngespeicherte Änderungen im Editor gehen verloren." : ""}`;
    if (!window.confirm(message)) return;
    run(async () => {
      await fs.delete(repo, r.path);
      removed(repo, r.path);
    });
  }

  // reveal oeffnet Ordner: bei Dateien den, in dem sie liegen.
  function reveal(dir: string) {
    const path = dir ? `${repo}/${dir}` : repo;
    run(() => invoke("reveal", { path: repo.includes("\\") ? path.replaceAll("/", "\\") : path }));
  }
</script>

<div class="flex h-full flex-col text-xs">
  <div class="border-border flex items-center border-b py-0.5 pr-1 pl-2">
    <span class="text-muted-foreground min-w-0 flex-1 truncate text-[10px]" title={repo}
      >{repo.split(/[\\/]/).pop()}</span
    >
    <button
      class="text-muted-foreground hover:text-foreground grid size-5 place-items-center rounded"
      title="Neue Datei im Projektordner"
      aria-label="Neue Datei"
      onclick={() => create("", false)}><FilePlusIcon class="size-3" /></button
    >
    <button
      class="text-muted-foreground hover:text-foreground grid size-5 place-items-center rounded"
      title="Neuer Ordner im Projektordner"
      aria-label="Neuer Ordner"
      onclick={() => create("", true)}><FolderPlusIcon class="size-3" /></button
    >
  </div>
  <Notice bind:text={error} />

  <div class="min-h-0 flex-1 overflow-y-auto py-1">
    {#each visible as r (r.path)}
      <!-- Neues landet im Ordner selbst bzw. neben der Datei. -->
      {@const dir = r.dir ? r.path : dirname(r.path)}
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          <button
            class="hover:bg-accent flex w-full items-center gap-1 py-0.5 pr-2 text-left text-[11px] {r.ignored
              ? 'opacity-50'
              : ''} {r.path === s.active ? 'bg-accent text-primary' : ''}"
            style="padding-left:{8 + r.depth * 12}px"
            onclick={() => click(r)}
            aria-expanded={r.dir ? r.open : undefined}
            title={r.path}
          >
            <ChevronRightIcon
              class="size-3 shrink-0 transition-transform {r.open ? 'rotate-90' : ''} {r.dir ? '' : 'invisible'}"
            />
            <span class="min-w-0 flex-1 truncate font-mono">{r.name}</span>
          </button>
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content class={menuContent}>
            <ContextItem label="Neue Datei" onSelect={() => create(dir, false)} />
            <ContextItem label="Neuer Ordner" onSelect={() => create(dir, true)} />
            <ContextItem label="Umbenennen" onSelect={() => rename(r)} />
            <ContextItem label={mac ? "Im Finder zeigen" : "Im Explorer zeigen"} onSelect={() => reveal(dir)} />
            <ContextItem label="Löschen" onSelect={() => remove(r)} destructive />
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
    {/each}
  </div>
</div>

<PromptDialog
  open={prompt !== null}
  title={prompt?.title ?? ""}
  bind:value={promptValue}
  onsubmit={doPrompt}
  oncancel={() => (prompt = null)}
/>
