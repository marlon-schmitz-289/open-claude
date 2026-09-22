<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { load, type Store } from "@tauri-apps/plugin-store";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

  import * as Command from "$lib/components/ui/command/index.js";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Separator } from "$lib/components/ui/separator/index.js";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import RefreshIcon from "@lucide/svelte/icons/refresh-cw";
  import GitBranchIcon from "@lucide/svelte/icons/git-branch";
  import KeyboardIcon from "@lucide/svelte/icons/keyboard";
  import StarIcon from "@lucide/svelte/icons/star";
  import PowerIcon from "@lucide/svelte/icons/power";
  import MinusIcon from "@lucide/svelte/icons/minus";
  import SquareIcon from "@lucide/svelte/icons/square";
  import XIcon from "@lucide/svelte/icons/x";
  import { fuzzy } from "$lib/fuzzy";
  import IdleAmongUs from "$lib/components/IdleAmongUs.svelte";

  type Repo = {
    path: string;
    rel: string;
    branch: string;
    last_commit: string;
    langs: string[];
  };

  // Farben wie auf GitHub, damit die Sprache ohne Lesen erkennbar ist.
  const LANG_COLOR: Record<string, string> = {
    Rust: "#dea584",
    Go: "#00add8",
    "C#": "#178600",
    "F#": "#b845fc",
    Python: "#3572a5",
    Java: "#b07219",
    Kotlin: "#a97bff",
    PHP: "#4f5d95",
    Ruby: "#701516",
    Swift: "#f05138",
    "C++": "#f34b7d",
    TypeScript: "#3178c6",
    JavaScript: "#f1e05a",
  };
  type Cache = { root: string; repos: Repo[]; at: number };

  let store: Store;
  let root = $state("");
  let repos = $state<Repo[]>([]);
  let query = $state("");
  let selected = $state("");
  let error = $state("");
  let scanning = $state(true);
  let scannedAt = $state(0);
  let fromCache = $state(false);
  let help = $state(false);
  let pins = $state<string[]>([]);
  let autostart = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  async function toggleAutostart() {
    try {
      await (autostart ? disable() : enable());
      autostart = await isEnabled();
    } catch (e) {
      error = String(e);
    }
  }

  async function togglePin(path: string) {
    pins = pins.includes(path) ? pins.filter((p) => p !== path) : [...pins, path];
    await store.set("pins", pins);
  }

  /** Repo-Pfad in Ordner + Name trennen. */
  function split(rel: string): [string, string] {
    const i = Math.max(rel.lastIndexOf("/"), rel.lastIndexOf("\\"));
    return i < 0 ? ["", rel] : [rel.slice(0, i), rel.slice(i + 1)];
  }

  const hits = $derived.by(() => {
    const q = query.trim();
    return repos
      .map((repo) => {
        const [dir, name] = split(repo.rel);
        // Name zaehlt doppelt: wer "backend" tippt, meint das Projekt, nicht den Ordner.
        const onName = fuzzy(name, q);
        const onPath = fuzzy(repo.rel, q);
        // "rust" oder "c#" tippen soll die passenden Projekte zeigen, aber nie vor Namenstreffern.
        const onLang = q.length > 1 ? fuzzy((repo.langs ?? []).join(" "), q) : null;
        if (!onName && !onPath && !onLang) return null;
        return {
          repo,
          dir,
          name,
          marks: onName?.hits ?? [],
          score: Math.max(
            (onName?.score ?? -99) * 2,
            onPath?.score ?? -99,
            (onLang?.score ?? -99) - 10,
          ),
        };
      })
      .filter((m) => m !== null)
      .sort((a, b) => (q ? b.score - a.score : a.repo.rel.localeCompare(b.repo.rel)));
  });

  /** Angepinntes zuerst, sonst nach Ordner gruppiert — bei Suche eine Liste nach Relevanz. */
  const groups = $derived.by(() => {
    const pinned = hits.filter((h) => pins.includes(h.repo.path));
    const rest = hits.filter((h) => !pins.includes(h.repo.path));
    const head = pinned.length ? [{ heading: "Angepinnt", items: pinned }] : [];

    if (query.trim()) return [...head, { heading: "", items: rest }];

    const by = new Map<string, typeof hits>();
    for (const h of rest) {
      const top = h.dir.split(/[\\/]/)[0] || "Direkt im Ordner";
      (by.get(top) ?? by.set(top, []).get(top)!).push(h);
    }
    return [...head, ...[...by].map(([heading, items]) => ({ heading, items }))];
  });

  const flat = $derived(groups.flatMap((g) => g.items));
  const current = $derived(flat.find((h) => h.repo.path === selected) ?? flat[0]);

  onMount(async () => {
    store = await load("settings.json", { autoSave: true });
    root = (await store.get<string>("root")) ?? (await invoke<string>("default_root"));
    pins = (await store.get<string[]>("pins")) ?? [];
    autostart = await isEnabled().catch(() => false);

    // Zuerst den letzten Stand zeigen, dann im Hintergrund frisch einlesen.
    const cached = await store.get<Cache>("cache");
    if (cached?.root === root && cached.repos.length) {
      repos = cached.repos;
      scannedAt = cached.at;
      fromCache = true;
      scanning = false;
    }
    await rescan();
  });

  async function rescan() {
    scanning = true;
    error = "";
    try {
      repos = await invoke<Repo[]>("scan", { root });
      scannedAt = Date.now();
      fromCache = false;
      await store.set("cache", { root, repos, at: scannedAt } satisfies Cache);
    } catch (e) {
      repos = [];
      error = String(e);
    }
    scanning = false;
    input?.focus();
  }

  async function pickRoot() {
    const picked = await open({ directory: true, defaultPath: root });
    if (typeof picked !== "string") return;
    root = picked;
    await store.set("root", root);
    repos = [];
    await rescan();
  }

  async function run(cmd: "launch" | "reveal", repo?: Repo) {
    if (!repo) return;
    try {
      await invoke(cmd, { path: repo.path });
    } catch (e) {
      error = String(e);
    }
  }

  function onKey(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;

    if (ctrl && e.key >= "1" && e.key <= "9") {
      run("launch", flat[Number(e.key) - 1]?.repo);
    } else if (ctrl && e.key.toLowerCase() === "k") {
      input?.select();
    } else if (ctrl && e.key.toLowerCase() === "r") {
      rescan();
    } else if (ctrl && e.key.toLowerCase() === "o") {
      pickRoot();
    } else if (ctrl && e.key.toLowerCase() === "e") {
      run("reveal", current?.repo);
    } else if (ctrl && e.key.toLowerCase() === "p") {
      if (current) togglePin(current.repo.path);
    } else if (e.key === "F1" || (ctrl && e.key === "/")) {
      help = !help;
    } else if (e.key === "Escape" && !help) {
      if (query) query = "";
      else getCurrentWindow().close();
    } else {
      return;
    }
    e.preventDefault();
  }

  const STEPS: [number, string][] = [
    [31536e6, "J"],
    [2592e6, "Mo"],
    [864e5, "T"],
    [36e5, "h"],
    [6e4, "min"],
  ];

  function age(iso: string) {
    if (!iso) return "";
    const diff = Date.now() - Date.parse(iso);
    for (const [ms, unit] of STEPS) {
      if (diff >= ms) return `${Math.round(diff / ms)} ${unit}`;
    }
    return "jetzt";
  }

  const short = $derived(root.replace(/^[A-Za-z]:[\\/]Users[\\/][^\\/]+/, "~"));
</script>

<svelte:window onkeydown={onKey} />

<IdleAmongUs />

<div
  class="bg-chrome border-border flex h-9 items-center border-b pl-3.5"
  data-tauri-drag-region
>
  <img
    src="/logo.png"
    alt=""
    class="mr-2 size-4 [image-rendering:pixelated]"
    data-tauri-drag-region
  />
  <span class="text-muted-foreground flex-1 text-xs font-semibold" data-tauri-drag-region>
    Open Claude
  </span>
  <button
    class="text-muted-foreground hover:bg-secondary hover:text-foreground grid h-9 w-11 place-items-center"
    onclick={() => getCurrentWindow().minimize()}
    aria-label="Minimieren"><MinusIcon class="size-3.5" /></button
  >
  <button
    class="text-muted-foreground hover:bg-secondary hover:text-foreground grid h-9 w-11 place-items-center"
    onclick={() => getCurrentWindow().toggleMaximize()}
    aria-label="Maximieren"><SquareIcon class="size-3" /></button
  >
  <button
    class="text-muted-foreground grid h-9 w-11 place-items-center hover:bg-[#b4404a] hover:text-white"
    onclick={() => getCurrentWindow().close()}
    aria-label="Schließen"><XIcon class="size-3.5" /></button
  >
</div>

<div class="flex h-[calc(100vh-2.25rem)] flex-col">
  <Command.Root
    shouldFilter={false}
    bind:value={selected}
    loop
    class="bg-background flex-1 rounded-none! p-0"
  >
    <div class="border-border flex items-center gap-2 border-b pr-2">
      <div class="flex-1 [&_[data-slot=command-input-wrapper]]:p-0">
        <Command.Input
          bind:ref={input}
          bind:value={query}
          placeholder="Projekt suchen"
          autofocus
          class="h-12! text-base!"
        />
      </div>
      <Button variant="ghost" size="sm" onclick={pickRoot} title={root} class="font-mono text-xs">
        <FolderIcon class="size-3.5" />
        <span class="max-w-44 truncate">{short}</span>
      </Button>
      <Button variant="ghost" size="icon" onclick={rescan} aria-label="Neu einlesen (Strg+R)">
        <RefreshIcon class="size-4 {scanning ? 'animate-spin' : ''}" />
      </Button>
    </div>

    <Command.List class="max-h-none flex-1 overflow-y-auto">
      {#if error}
        <p class="text-destructive px-4 py-6 text-sm">{error}</p>
      {:else if scanning && repos.length === 0}
        <p class="text-muted-foreground px-4 py-6 text-sm">Lese Git-Repos …</p>
      {:else}
        <Command.Empty class="px-4 py-6 text-left text-sm">
          {repos.length ? `Kein Projekt passt zu „${query.trim()}“.` : "Hier liegen keine Git-Repos."}
        </Command.Empty>
        {#each groups as group (group.heading)}
          <Command.Group heading={group.heading || undefined}>
            {#each group.items as item (item.repo.path)}
              {@const nr = flat.indexOf(item) + 1}
              <Command.Item
                value={item.repo.path}
                onSelect={() => run("launch", item.repo)}
                class="gap-3 px-3 py-2"
              >
                {@const pinned = pins.includes(item.repo.path)}
                <button
                  class="text-muted-foreground hover:text-primary -m-1 grid size-6 place-items-center rounded p-1 {pinned
                    ? 'text-primary'
                    : 'opacity-50'}"
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={(e) => {
                    e.stopPropagation();
                    togglePin(item.repo.path);
                  }}
                  aria-label={pinned ? "Nicht mehr anpinnen" : "Anpinnen"}
                  aria-pressed={pinned}
                >
                  <StarIcon class="size-4 {pinned ? 'fill-current' : ''}" />
                </button>

                <div class="min-w-0 flex-1">
                  <div class="truncate text-sm font-semibold">
                    {#each item.name.split("") as ch, i}<span
                        class={item.marks.includes(i) ? "text-primary" : ""}>{ch}</span
                      >{/each}
                  </div>
                  {#if item.dir}
                    <div class="text-muted-foreground truncate font-mono text-[11px]">
                      {item.dir}
                    </div>
                  {/if}
                </div>

                {#each item.repo.langs ?? [] as lang (lang)}
                  <Badge variant="outline" class="text-muted-foreground gap-1.5 text-[11px]">
                    <span
                      class="size-2 rounded-full"
                      style="background:{LANG_COLOR[lang] ?? 'var(--muted-foreground)'}"
                    ></span>
                    {lang}
                  </Badge>
                {/each}

                <Badge variant="outline" class="text-muted-foreground gap-1 font-mono text-[11px]">
                  <GitBranchIcon class="size-3" />
                  {item.repo.branch}
                </Badge>
                <span class="text-muted-foreground w-12 text-right font-mono text-[11px] tabular-nums">
                  {age(item.repo.last_commit)}
                </span>
                {#if nr <= 9}
                  <kbd
                    class="text-muted-foreground border-border hidden w-10 rounded border px-1 py-0.5 text-center font-mono text-[10px] sm:block"
                    >^{nr}</kbd
                  >
                {:else}
                  <span class="hidden w-10 sm:block"></span>
                {/if}
              </Command.Item>
            {/each}
          </Command.Group>
        {/each}
      {/if}
    </Command.List>
  </Command.Root>

  <footer
    class="bg-chrome border-border text-muted-foreground flex items-center gap-3 border-t px-3.5 py-2 text-[11px]"
  >
    <span>{flat.length} von {repos.length}</span>
    <Separator orientation="vertical" class="h-3.5!" />
    <span>
      {#if scanning}Lese ein …{:else if fromCache}Stand gespeichert{:else if scannedAt}Aktualisiert
        {new Date(scannedAt).toLocaleTimeString("de-DE", {
          hour: "2-digit",
          minute: "2-digit",
        })}{/if}
    </span>
    <span class="flex-1"></span>
    <Button
      variant="ghost"
      size="sm"
      class="h-6 gap-1.5 text-[11px] {autostart ? 'text-primary' : ''}"
      onclick={toggleAutostart}
      aria-pressed={autostart}
      title={autostart ? "Startet mit Windows" : "Beim Anmelden automatisch starten"}
    >
      <PowerIcon class="size-3.5" /> Autostart {autostart ? "an" : "aus"}
    </Button>
    <Separator orientation="vertical" class="h-3.5!" />
    <span class="flex items-center gap-1.5">
      <kbd class="border-border rounded border px-1.5 py-0.5 font-mono">⏎</kbd> Claude starten
    </span>
    <Button variant="ghost" size="sm" class="h-6 gap-1.5 text-[11px]" onclick={() => (help = true)}>
      <KeyboardIcon class="size-3.5" /> F1
    </Button>
  </footer>
</div>

<Dialog.Root bind:open={help}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>Tastenkürzel</Dialog.Title>
      <Dialog.Description>Alles lässt sich ohne Maus bedienen.</Dialog.Description>
    </Dialog.Header>
    <dl class="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2.5 text-sm">
      {#each [["⏎", "Claude im Projekt starten"], ["↑ ↓", "Projekt wählen"], ["Strg + 1 … 9", "Treffer direkt starten"], ["Strg + K", "Suche fokussieren"], ["Strg + P", "Projekt anpinnen"], ["Strg + E", "Ordner im Explorer öffnen"], ["Strg + R", "Neu einlesen"], ["Strg + O", "Dev-Ordner wechseln"], ["Esc", "Suche leeren, sonst schließen"]] as [key, what]}
        <dt class="border-border rounded border px-1.5 py-0.5 text-center font-mono text-[11px]">
          {key}
        </dt>
        <dd class="text-muted-foreground">{what}</dd>
      {/each}
    </dl>
  </Dialog.Content>
</Dialog.Root>
