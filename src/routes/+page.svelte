<script lang="ts">
  import { age } from "$lib/utils";
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { load, type Store } from "@tauri-apps/plugin-store";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";

  import * as Command from "$lib/components/ui/command/index.js";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import RefreshIcon from "@lucide/svelte/icons/refresh-cw";
  import GitBranchIcon from "@lucide/svelte/icons/git-branch";
  import KeyboardIcon from "@lucide/svelte/icons/keyboard";
  import StarIcon from "@lucide/svelte/icons/star";
  import PowerIcon from "@lucide/svelte/icons/power";
  import InboxIcon from "@lucide/svelte/icons/inbox";
  import GhostIcon from "@lucide/svelte/icons/ghost";
  import VolumeIcon from "@lucide/svelte/icons/volume-2";
  import MinusIcon from "@lucide/svelte/icons/minus";
  import SquareIcon from "@lucide/svelte/icons/square";
  import XIcon from "@lucide/svelte/icons/x";
  import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import { ContextMenu, DropdownMenu } from "bits-ui";
  import Notice from "$lib/components/Notice.svelte";
  import TerminalIcon from "@lucide/svelte/icons/terminal";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import { fuzzy } from "$lib/fuzzy";
  import IdleAmongUs from "$lib/components/IdleAmongUs.svelte";
  import Terminal from "$lib/components/Terminal.svelte";
  import GitView from "$lib/components/git/GitView.svelte";
  import CloneDialog from "$lib/components/git/CloneDialog.svelte";
  import AccountsDialog from "$lib/components/git/AccountsDialog.svelte";
  import { git, forge, type Account } from "$lib/git";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UsersIcon from "@lucide/svelte/icons/users";

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

  // raw: die Store-Instanz nicht proxien; null bis onMount geladen hat.
  let store = $state.raw<Store>(null!);
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
  let tray = $state(true);
  // Among-Us-Easteregg: rate ist der Faktor auf die Wartezeiten (groesser = seltener).
  let amongUs = $state({ on: true, rate: 1, volume: 1 });
  const RATES: [string, number][] = [["Selten", 3], ["Normal", 1], ["Oft", 0.4]];
  let input = $state<HTMLInputElement | null>(null);
  // ponytail: keine Tabs-UI, eine Session pro Projekt; Tab-Leiste wenn mehrere parallel sichtbar sein sollen.
  let sessions = $state<{ id: string; repo: Repo }[]>([]);
  let active = $state<string | null>(null);
  const activeSession = $derived(sessions.find((s) => s.id === active));
  const running = $derived(new Set(sessions.map((s) => s.repo.path)));
  // Git-Ansicht ueberdeckt Liste und Terminal; Terminals laufen darunter weiter.
  let gitRepo = $state<Repo | null>(null);
  let gitView = $state<GitView>();
  const view = $derived(gitRepo ? "git" : active ? "term" : "list");
  const viewRepo = $derived(gitRepo ?? activeSession?.repo);
  let cloneOpen = $state(false);
  let accountsOpen = $state(false);
  let accounts = $state<Account[]>([]);

  async function toggleAutostart() {
    try {
      await (autostart ? disable() : enable());
      autostart = await isEnabled();
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleTray() {
    tray = !tray;
    await invoke("set_tray", { on: tray });
    await store.set("tray", tray);
  }

  function saveAmongUs() {
    store.set("amongUs", $state.snapshot(amongUs));
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

  // Maximiert-Zustand fuers Icon; aendert sich auch per Doppelklick auf die Titelleiste oder Win+Pfeil.
  let maximized = $state(false);
  // macOS: native Ampeln im Overlay-Titelbalken (tauri.macos.conf.json) statt eigener Knoepfe.
  const mac = navigator.userAgent.includes("Mac");
  onMount(() => {
    const win = getCurrentWindow();
    const sync = async () => (maximized = await win.isMaximized());
    sync();
    const off = win.onResized(sync);
    return () => off.then((f) => f());
  });

  // Neue Version aus GitHub-Releases; Fehler (offline, Dev-Build) still ignorieren.
  let update = $state<Update | null>(null);
  let updating = $state(false);
  onMount(() => {
    check().then((u) => (update = u), () => {});
  });

  async function installUpdate() {
    updating = true;
    try {
      await update!.downloadAndInstall();
      await relaunch();
    } catch (e) {
      error = String(e);
      updating = false;
    }
  }

  onMount(async () => {
    store = await load("settings.json", { autoSave: true });
    const saved = await store.get<string>("root");
    root = saved ?? (await invoke<string>("default_root"));
    pins = (await store.get<string[]>("pins")) ?? [];
    accounts = (await store.get<Account[]>("accounts")) ?? [];
    autostart = await isEnabled().catch(() => false);
    tray = (await store.get<boolean>("tray")) ?? true;
    amongUs = { ...amongUs, ...(await store.get<typeof amongUs>("amongUs")) };
    if (!mac) await invoke("set_tray", { on: tray });

    // Erster Start: Dev-Ordner erst bestaetigen lassen, dann einlesen.
    if (!saved) {
      setup = true;
      scanning = false;
      return;
    }

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

  // Nur der zuletzt gestartete Scan zaehlt, sonst ueberschreibt ein alter Ordner den neuen.
  let scanGen = 0;

  async function rescan() {
    const g = ++scanGen;
    const r = root;
    scanning = true;
    error = "";
    try {
      const found = await invoke<Repo[]>("scan", { root: r });
      if (g !== scanGen) return;
      repos = found;
      scannedAt = Date.now();
      fromCache = false;
      await store.set("cache", { root: r, repos, at: scannedAt } satisfies Cache);
    } catch (e) {
      if (g !== scanGen) return;
      repos = [];
      error = String(e);
    }
    scanning = false;
    input?.focus();
  }

  let setup = $state(false);

  async function finishSetup() {
    root = root.trim();
    if (!root) return;
    await store.set("root", root);
    setup = false;
    await rescan();
  }

  async function pickRoot() {
    const picked = await open({ directory: true, defaultPath: root });
    if (typeof picked !== "string") return;
    root = picked;
    if (setup) return; // im Erststart-Dialog erst mit "Übernehmen" speichern
    await store.set("root", root);
    repos = [];
    await rescan();
  }

  /** Offene Session des Projekts zeigen, sonst eine neue starten. */
  function launch(repo?: Repo) {
    if (!repo) return;
    if (gitRepo && gitView && !gitView.canLeave()) return;
    leaveGit();
    let s = sessions.find((s) => s.repo.path === repo.path);
    // Eindeutig pro Seitenladung: nach einem Neuladen laufen alte PTYs evtl. noch unter "t1"
    if (!s) sessions.push((s = { id: crypto.randomUUID(), repo }));
    active = s.id;
  }

  /** Git-Ansicht schliessen; der Branch kann sich dort geaendert haben, Liste und Titel nachziehen. */
  function leaveGit() {
    const r = gitRepo;
    gitRepo = null;
    if (r)
      git.status(r.path).then(
        (st) => (r.branch = st.branch ?? st.head.slice(0, 7)),
        () => {},
      );
  }

  /** Git-Ansicht fuer das Projekt; ein laufendes Terminal bleibt im Hintergrund. */
  function openGit(repo?: Repo) {
    if (!repo) return;
    active = null;
    gitRepo = repo;
  }

  /** Nach dem Klonen neu einlesen und das neue Projekt markieren. */
  async function cloned(path: string) {
    await rescan();
    const norm = (p: string) => p.replace(/\\/g, "/").toLowerCase();
    const hit = repos.find((r) => norm(r.path) === norm(path));
    if (hit) {
      query = "";
      // Der Suchwechsel waehlt per afterTick (Microtasks) den ersten Eintrag, daher danach setzen.
      await new Promise((r) => setTimeout(r));
      selected = hit.path;
    }
  }

  async function reveal(repo?: Repo) {
    if (!repo) return;
    try {
      await invoke("reveal", { path: repo.path });
    } catch (e) {
      error = String(e);
    }
  }

  // Fehler aus dem Kontextmenue als Leiste, error wuerde die ganze Liste ersetzen.
  let notice = $state("");

  async function browse(repo: Repo) {
    try {
      const remotes = await git.remotes(repo.path);
      const r = remotes.find((r) => r.name === "origin") ?? remotes[0];
      const url = r && (await forge.webUrl(r.url));
      if (!url) throw "Kein Remote mit Web-Adresse.";
      await forge.openUrl(url);
    } catch (e) {
      notice = String(e);
    }
  }

  // Loeschen: vorher zeigen, was nur auf diesem Rechner liegt.
  let doomed = $state<{ repo: Repo; risks: string[] } | null>(null);
  let trashing = $state(false);

  async function askTrash(repo: Repo) {
    const risks: string[] = [];
    try {
      const [st, branches, stashes, remotes] = await Promise.all([
        git.status(repo.path),
        git.branches(repo.path),
        git.stashes(repo.path),
        git.remotes(repo.path),
      ]);
      const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;
      if (!remotes.length) risks.push("Kein Remote: alles liegt nur hier.");
      if (st.files.length) risks.push(n(st.files.length, "nicht committete Änderung", "nicht committete Änderungen"));
      // Nie gepushte Branches zaehlen nur, wenn sie nicht schon in der Basis stecken.
      const unpushed = branches.filter((b) => !b.remote && (b.ahead > 0 || (!b.upstream && b.unmerged !== 0)));
      if (remotes.length && unpushed.length)
        risks.push(`Nicht gepusht: ${unpushed.map((b) => b.name + (b.ahead ? ` (+${b.ahead})` : "")).join(", ")}`);
      if (stashes.length) risks.push(n(stashes.length, "Stash", "Stashes"));
    } catch (e) {
      risks.push(`Status unbekannt: ${e}`);
    }
    doomed = { repo, risks };
  }

  async function trashRepo() {
    if (!doomed) return;
    const { repo } = doomed;
    trashing = true;
    try {
      // Laufende Sitzung zuerst beenden, unter Windows sperrt ihr cwd sonst den Ordner.
      sessions = sessions.filter((s) => s.repo.path !== repo.path);
      await tick();
      await invoke("trash_repo", { path: repo.path, root });
      repos = repos.filter((r) => r.path !== repo.path);
      if (pins.includes(repo.path)) await togglePin(repo.path);
      await store.set("cache", { root, repos, at: scannedAt } satisfies Cache);
      doomed = null;
    } catch (e) {
      notice = String(e);
      doomed = null;
    }
    trashing = false;
  }

  async function back() {
    if (gitRepo && gitView && !gitView.canLeave()) return;
    active = null;
    leaveGit();
    await tick();
    input?.focus();
  }

  /** Per Klick beenden: claude arbeitet evtl. noch, daher nachfragen. */
  function endSession(id: string) {
    if (window.confirm("Sitzung beenden? Ein laufender claude-Prozess wird abgebrochen.")) closeSession(id);
  }

  /** Aus der Liste nehmen; der Terminal-Unmount schliesst die PTY. */
  function closeSession(id: string) {
    sessions = sessions.filter((s) => s.id !== id);
    if (active === id) back();
  }

  // Klick/Enter oeffnet Git, mit Strg Claude. onSelect der Liste kennt kein Event,
  // daher Strg in der Capture-Phase merken, bevor die Liste reagiert.
  let withCtrl = false;
  // Esc gehoert offenen Dialogen/Menues. Die schliessen schon vor onKey, daher hier vorher nachsehen.
  let overlayOnEsc = false;
  const noteCtrl = (e: KeyboardEvent | PointerEvent) => {
    withCtrl = e.ctrlKey || e.metaKey;
    if (!(e instanceof KeyboardEvent)) return;
    if (e.key === "Escape")
      overlayOnEsc = !!document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]');
    // WebView2 wuerde neu laden: Sitzungen verwaisen. Im Terminal gehoert Strg+R/F5 der Shell (xterm verhindert selbst).
    const reload = e.key === "F5" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r");
    if (reload && !(e.target instanceof Element && e.target.closest(".xterm"))) e.preventDefault();
  };

  function onKey(e: KeyboardEvent) {
    // Im Terminal gehoeren fast alle Kuerzel der Shell (Readline: Strg+R/K/E/P/O/G ...).
    // Die Git-Ansicht hat ihre eigenen Kuerzel in GitView.svelte.
    if (view !== "list") {
      const alt = e.altKey && !e.ctrlKey;
      if ((e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "w") || (e.shiftKey && e.key === "Escape")) {
        e.preventDefault();
        back();
      } else if (e.key === "Escape" && view === "git") {
        // Offene Dialoge/Menues schliessen sich selbst, Eingabefelder verlieren nur den Fokus.
        const el = document.activeElement;
        if (overlayOnEsc) return;
        e.preventDefault();
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) el.blur();
        else back();
      } else if (alt && e.key.toLowerCase() === "e") {
        e.preventDefault();
        reveal(viewRepo);
      } else if (alt && e.key.toLowerCase() === "g" && view === "term") {
        e.preventDefault();
        openGit(activeSession?.repo);
      }
      return;
    }
    // Esc hat schon ein Dialog oder Menue geschlossen, nicht auch noch das Fenster.
    if (e.key === "Escape" && (overlayOnEsc || e.defaultPrevented)) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === "F1" || (ctrl && e.key === "/")) {
      if (!cloneOpen && !accountsOpen && !setup) help = !help;
      e.preventDefault();
      return;
    }
    // Offene Dialoge (Hilfe, Klonen, Konten) bekommen ihre Tasten selbst.
    if (help || cloneOpen || accountsOpen || setup || doomed) return;

    if (ctrl && e.key >= "1" && e.key <= "9") {
      launch(flat[Number(e.key) - 1]?.repo);
    } else if (ctrl && e.key.toLowerCase() === "k") {
      input?.select();
    } else if (ctrl && e.key.toLowerCase() === "r") {
      rescan();
    } else if (ctrl && e.key.toLowerCase() === "o") {
      pickRoot();
    } else if (ctrl && e.key.toLowerCase() === "e") {
      reveal(current?.repo);
    } else if (ctrl && e.key.toLowerCase() === "p") {
      if (current) togglePin(current.repo.path);
    } else if (ctrl && e.key.toLowerCase() === "g") {
      openGit(current?.repo);
    } else if (ctrl && e.key.toLowerCase() === "n") {
      cloneOpen = true;
    } else if (e.key === "Escape") {
      if (query) query = "";
      else getCurrentWindow().close();
    } else {
      return;
    }
    e.preventDefault();
  }

  const short = $derived(root.replace(/^[A-Za-z]:[\\/]Users[\\/][^\\/]+/, "~"));
</script>

<svelte:window onkeydown={onKey} onkeydowncapture={noteCtrl} onpointerdowncapture={noteCtrl} />

{#if amongUs.on}<IdleAmongUs rate={amongUs.rate} volume={amongUs.volume} />{/if}

<div
  class="bg-chrome border-border flex h-9 items-center border-b {mac ? 'pl-20' : 'pl-3.5'}"
  data-tauri-drag-region
>
  {#if viewRepo}
    <button
      class="text-muted-foreground hover:bg-secondary hover:text-foreground {mac ? '' : '-ml-3.5'} mr-1 grid h-9 w-10 place-items-center"
      onclick={back}
      aria-label="Zurück zur Liste (Strg+Umschalt+W)"
      title="Zurück zur Liste (Strg+Umschalt+W)"><ArrowLeftIcon class="size-3.5" /></button
    >
    <div class="flex min-w-0 flex-1 items-center gap-2" data-tauri-drag-region>
      <span class="truncate text-xs font-semibold" data-tauri-drag-region>
        {split(viewRepo.rel)[1]}
      </span>
      {#if split(viewRepo.rel)[0]}
        <span
          class="text-muted-foreground hidden truncate font-mono text-[11px] sm:block"
          data-tauri-drag-region
        >
          {split(viewRepo.rel)[0]}
        </span>
      {/if}
      {#if view === "term"}
        <Badge variant="outline" class="text-muted-foreground shrink-0 gap-1 font-mono text-[11px]">
          <GitBranchIcon class="size-3" />
          {viewRepo.branch}
        </Badge>
      {/if}
      {#each viewRepo.langs ?? [] as lang (lang)}
        <Badge variant="outline" class="text-muted-foreground shrink-0 gap-1.5 text-[11px]">
          <span
            class="size-2 rounded-full"
            style="background:{LANG_COLOR[lang] ?? 'var(--muted-foreground)'}"
          ></span>
          {lang}
        </Badge>
      {/each}
    </div>
    {#if view === "git" && gitRepo}
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground h-6 gap-1.5 text-[11px]"
        onclick={() => launch(gitRepo!)}
        title="Claude im Projekt öffnen"><TerminalIcon class="size-3.5" /> Claude</Button
      >
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground h-6 gap-1.5 text-[11px]"
        onclick={() => reveal(gitRepo!)}
        title="Ordner im Explorer öffnen (Alt+E)"><FolderOpenIcon class="size-3.5" /> Explorer</Button
      >
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground mr-1 h-6 text-[11px]"
        onclick={() => gitView?.reload()}
        title="Aktualisieren (F5)"
        aria-label="Aktualisieren"><RefreshIcon class="size-3.5" /></Button
      >
    {:else if activeSession}
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground h-6 gap-1.5 text-[11px]"
        onclick={() => openGit(activeSession.repo)}
        title="Git-Ansicht (Alt+G)"><GitBranchIcon class="size-3.5" /> Git</Button
      >
      <Button
        variant="ghost"
        size="sm"
        class="text-muted-foreground mr-1 h-6 text-[11px]"
        onclick={() => endSession(activeSession.id)}>Sitzung beenden</Button
      >
    {/if}
  {:else}
    <img
      src="/logo.png"
      alt=""
      class="mr-2 size-4 [image-rendering:pixelated]"
      data-tauri-drag-region
    />
    <span class="text-muted-foreground flex-1 text-xs font-semibold" data-tauri-drag-region>
      Open Claude
    </span>
  {/if}
  {#if update}
    <Button
      variant="ghost"
      size="sm"
      class="text-primary mr-1 h-6 gap-1.5 text-[11px]"
      disabled={updating}
      onclick={installUpdate}
      title={`Update installieren und neu starten${update.body ? `\n\n${update.body}` : ""}`}
      ><DownloadIcon class="size-3.5" /> {updating ? "Aktualisiere…" : `Update ${update.version}`}</Button
    >
  {/if}
  {#if !mac}
    <!-- Fensterknoepfe wie bei Windows: nicht per Tab erreichbar, kein Fokusrahmen, Fokus bleibt im Terminal/der Suche. -->
    <button
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
      class="text-muted-foreground hover:bg-secondary hover:text-foreground grid h-9 w-11 place-items-center outline-none"
      onclick={() => getCurrentWindow().minimize()}
      aria-label="Minimieren"><MinusIcon class="size-3.5" /></button
    >
    <button
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
      class="text-muted-foreground hover:bg-secondary hover:text-foreground grid h-9 w-11 place-items-center outline-none"
      onclick={() => getCurrentWindow().toggleMaximize()}
      aria-label={maximized ? "Wiederherstellen" : "Maximieren"}
    >
      {#if maximized}
        <!-- Wiederherstellen: vorderes Quadrat unten links, hinteres oben rechts -->
        <svg class="size-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1">
          <rect x="0.5" y="2.5" width="9" height="9" rx="1" />
          <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h7a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-1" />
        </svg>
      {:else}
        <SquareIcon class="size-3" />
      {/if}
    </button>
    <button
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
      class="text-muted-foreground grid h-9 w-11 place-items-center outline-none hover:bg-[#b4404a] hover:text-white"
      onclick={() => getCurrentWindow().close()}
      aria-label="Schließen"><XIcon class="size-3.5" /></button
    >
  {/if}
</div>

<div class="flex h-[calc(100vh-2.25rem)] flex-col">
  <Command.Root
    shouldFilter={false}
    bind:value={selected}
    loop
    class="bg-background flex-1 rounded-none! p-0 {view !== 'list' ? 'hidden!' : ''}"
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
      <Button
        variant="ghost"
        size="sm"
        onclick={pickRoot}
        title="Dev-Ordner wechseln (Strg+O): {root}"
        class="text-muted-foreground font-mono text-xs"
      >
        <FolderIcon class="size-3.5" />
        <span class="max-w-44 truncate">{short}</span>
      </Button>
      <!-- Aktionen als Icons: dort, wo man ohnehin arbeitet, statt in der Fusszeile -->
      <div class="text-muted-foreground flex items-center">
        <Button variant="ghost" size="icon" onclick={rescan} title="Neu einlesen (Strg+R)" aria-label="Neu einlesen">
          <RefreshIcon class="size-4 {scanning ? 'animate-spin' : ''}" />
        </Button>
        <Button variant="ghost" size="icon" onclick={() => (cloneOpen = true)} title="Repo klonen (Strg+N)" aria-label="Repo klonen">
          <DownloadIcon class="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="relative"
          onclick={() => (accountsOpen = true)}
          title="GitHub- und GitLab-Konten"
          aria-label="Konten"
        >
          <UsersIcon class="size-4" />
          {#if accounts.length}
            <span class="bg-primary absolute top-1.5 right-1.5 size-1.5 rounded-full"></span>
          {/if}
        </Button>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger
            class="hover:bg-accent hover:text-foreground grid size-9 place-items-center rounded-md"
            title="Einstellungen"
            aria-label="Einstellungen"><SettingsIcon class="size-4" /></DropdownMenu.Trigger
          >
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              align="end"
              sideOffset={4}
              class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-56 rounded-lg p-1 text-xs shadow-lg ring-1"
            >
              <!-- macOS: Dock statt Tray, Autostart ueber die Systemeinstellungen. -->
              {#if !mac}
                {@render setting("Autostart", autostart, toggleAutostart, PowerIcon)}
                {@render setting("Schließen legt ins Tray", tray, toggleTray, InboxIcon)}
                <DropdownMenu.Separator class="bg-border my-1 h-px" />
              {/if}
              {@render setting("Among-Us-Easteregg", amongUs.on, () => ((amongUs.on = !amongUs.on), saveAmongUs()), GhostIcon)}
              {#if amongUs.on}
                <div class="flex items-center gap-2 px-2 py-1.5">
                  <span class="text-muted-foreground w-3.5"></span>
                  <span class="flex-1">Häufigkeit</span>
                  <div class="bg-secondary flex rounded p-0.5">
                    {#each RATES as [label, rate] (rate)}
                      <button
                        class="rounded px-1.5 py-0.5 text-[10px] {amongUs.rate === rate
                          ? 'bg-primary text-primary-foreground'
                          : 'text-muted-foreground hover:text-foreground'}"
                        onclick={() => ((amongUs.rate = rate), saveAmongUs())}>{label}</button
                      >
                    {/each}
                  </div>
                </div>
                <label class="flex items-center gap-2 px-2 py-1.5">
                  <VolumeIcon class="text-muted-foreground size-3.5" />
                  <span class="flex-1">Sound</span>
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    class="accent-primary w-24"
                    aria-label="Easteregg-Lautstärke"
                    bind:value={amongUs.volume}
                    onchange={saveAmongUs}
                  />
                </label>
              {/if}
              <DropdownMenu.Separator class="bg-border my-1 h-px" />
              <DropdownMenu.Item
                class="data-highlighted:bg-accent flex cursor-pointer items-center gap-2 rounded px-2 py-1.5"
                onSelect={() => (help = true)}
              >
                <KeyboardIcon class="text-muted-foreground size-3.5" />
                <span class="flex-1">Tastenkürzel</span>
                <kbd class="text-muted-foreground font-mono text-[10px]">F1</kbd>
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
    </div>

    <Notice bind:text={notice} />
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
              {@const pinned = pins.includes(item.repo.path)}
              <!-- Rechtsklick waehlt die Zeile mit aus, damit klar ist, wofuer das Menue gilt -->
              <ContextMenu.Root onOpenChange={(o) => o && (selected = item.repo.path)}>
                <ContextMenu.Trigger>
                  {#snippet child({ props })}
                    <Command.Item
                      {...props}
                      value={item.repo.path}
                      onSelect={() => (withCtrl ? launch(item.repo) : openGit(item.repo))}
                      class="gap-3 px-3 py-2"
                    >
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

                      {#if running.has(item.repo.path)}
                        <Badge variant="outline" class="text-primary gap-1.5 pr-0.5 text-[11px]">
                          <span class="size-2 animate-pulse rounded-full bg-current"></span>
                          läuft
                          <button
                            class="text-muted-foreground hover:bg-destructive/20 hover:text-destructive grid size-4 place-items-center rounded"
                            onpointerdown={(e) => e.stopPropagation()}
                            onclick={(e) => {
                              e.stopPropagation();
                              const s = sessions.find((s) => s.repo.path === item.repo.path);
                              if (s) endSession(s.id);
                            }}
                            aria-label="Sitzung beenden"
                            title="Sitzung beenden"><XIcon class="size-3" /></button
                          >
                        </Badge>
                      {/if}

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
                  {/snippet}
                </ContextMenu.Trigger>
                <ContextMenu.Portal>
                  <ContextMenu.Content
                    class="bg-popover text-popover-foreground ring-foreground/10 z-50 min-w-52 rounded-lg p-1 text-xs shadow-lg ring-1"
                  >
                    {@render repoItem("Git-Ansicht", "Strg+G", () => openGit(item.repo))}
                    {@render repoItem(running.has(item.repo.path) ? "Zur Claude-Sitzung" : "Claude starten", "Strg+⏎", () => launch(item.repo))}
                    <ContextMenu.Separator class="bg-border my-1 h-px" />
                    {@render repoItem(mac ? "Im Finder zeigen" : "Im Explorer zeigen", "Strg+E", () => reveal(item.repo))}
                    {@render repoItem("Im Browser öffnen", "", () => browse(item.repo))}
                    {@render repoItem("Pfad kopieren", "", () => navigator.clipboard.writeText(item.repo.path))}
                    {@render repoItem(pinned ? "Nicht mehr anpinnen" : "Anpinnen", "Strg+P", () => togglePin(item.repo.path))}
                    <ContextMenu.Separator class="bg-border my-1 h-px" />
                    {@render repoItem("Vom Gerät löschen …", "", () => askTrash(item.repo), true)}
                  </ContextMenu.Content>
                </ContextMenu.Portal>
              </ContextMenu.Root>
            {/each}
          </Command.Group>
        {/each}
      {/if}
    </Command.List>
  </Command.Root>

  <!-- Alle Terminals bleiben gemountet, sonst stirbt claude beim Zurueckgehen. -->
  <div class="min-h-0 flex-1 {view === 'term' ? '' : 'hidden'}">
    {#each sessions as s (s.id)}
      <Terminal
        id={s.id}
        cwd={s.repo.path}
        visible={s.id === active}
        onexit={() => closeSession(s.id)}
      />
    {/each}
  </div>

  {#if gitRepo}
    <div class="min-h-0 flex-1">
      {#key gitRepo.path}
        <GitView bind:this={gitView} repo={gitRepo.path} />
      {/key}
    </div>
  {/if}

  <!-- Nur Status und Tastenhinweise; Aktionen und Einstellungen sitzen oben in der Suchleiste -->
  <footer
    class="bg-chrome border-border text-muted-foreground flex items-center gap-4 border-t px-3.5 py-1.5 text-[11px] whitespace-nowrap {view !== 'list'
      ? 'hidden'
      : ''}"
  >
    <span class="tabular-nums">
      {flat.length === repos.length ? `${repos.length} Projekte` : `${flat.length} von ${repos.length}`}
    </span>
    <span class="opacity-70">
      {#if scanning}Lese ein …{:else if fromCache}Stand gespeichert{:else if scannedAt}aktualisiert
        {new Date(scannedAt).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" })}{/if}
    </span>
    <span class="flex-1"></span>
    {#each [["⏎", "Git"], ["Strg ⏎", "Claude"]] as [key, what] (key)}
      <span class="flex items-center gap-1.5">{@render kbd(key)} {what}</span>
    {/each}
    <button class="hover:text-foreground flex items-center gap-1.5" onclick={() => (help = true)}>
      {@render kbd("F1")} Hilfe
    </button>
  </footer>
</div>

{#snippet repoItem(label: string, key: string, onSelect: () => void, destructive = false)}
  <ContextMenu.Item
    class="data-highlighted:bg-accent flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 {destructive
      ? 'text-destructive'
      : ''}"
    {onSelect}
  >
    <span class="flex-1">{label}</span>
    {#if key}<kbd class="text-muted-foreground font-mono text-[10px]">{key}</kbd>{/if}
  </ContextMenu.Item>
{/snippet}

{#snippet kbd(key: string)}
  <kbd class="bg-secondary text-foreground/80 rounded px-1.5 py-px font-mono text-[10px]">{key}</kbd>
{/snippet}

{#snippet setting(label: string, on: boolean, toggle: () => void, Icon: typeof PowerIcon)}
  <!-- Schalter bleiben im offenen Menue, damit man beide nacheinander umlegen kann -->
  <DropdownMenu.Item
    class="data-highlighted:bg-accent flex cursor-pointer items-center gap-2 rounded px-2 py-1.5"
    closeOnSelect={false}
    onSelect={toggle}
  >
    <Icon class="text-muted-foreground size-3.5" />
    <span class="flex-1">{label}</span>
    <span
      class="relative h-3.5 w-6 rounded-full transition-colors {on ? 'bg-primary' : 'bg-secondary ring-border ring-1'}"
      aria-hidden="true"
      ><span
        class="bg-foreground absolute top-0.5 size-2.5 rounded-full transition-all {on ? 'left-3' : 'left-0.5'}"
      ></span></span
    >
  </DropdownMenu.Item>
{/snippet}

{#snippet keys(title: string, list: string[][])}
  <div>
    <h3 class="text-muted-foreground mb-2 text-[11px] font-semibold uppercase tracking-wide">{title}</h3>
    <dl class="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 text-sm">
      {#each list as [key, what] (key)}
        <dt class="border-border rounded border px-1.5 py-0.5 text-center font-mono text-[11px]">
          {key}
        </dt>
        <dd class="text-muted-foreground">{what}</dd>
      {/each}
    </dl>
  </div>
{/snippet}

<Dialog.Root bind:open={help}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>Tastenkürzel</Dialog.Title>
      <Dialog.Description>Alles lässt sich ohne Maus bedienen.</Dialog.Description>
    </Dialog.Header>
    <div class="grid gap-6 sm:grid-cols-2">
      {@render keys("Liste", [
        ["⏎ / Klick", "Git-Ansicht öffnen"],
        ["Strg + ⏎ / Strg + Klick", "Claude im Projekt starten"],
        ["↑ ↓", "Projekt wählen"],
        ["Strg + 1 … 9", "Claude im Treffer starten"],
        ["Strg + G", "Git-Ansicht öffnen"],
        ["Strg + N", "Repo klonen"],
        ["Strg + K", "Suche fokussieren"],
        ["Strg + P", "Projekt anpinnen"],
        ["Strg + E", "Ordner im Explorer öffnen"],
        ["Strg + R", "Neu einlesen"],
        ["Strg + O", "Dev-Ordner wechseln"],
        ["Rechtsklick", "Weitere Aktionen, z. B. vom Gerät löschen"],
        ["Esc", "Suche leeren, sonst schließen (bzw. ins Tray)"],
      ])}
      <div class="grid content-start gap-6">
        {@render keys("Terminal und Git", [
          ["⇧ + Esc / Strg + ⇧ + W", "Zurück zur Liste, Sitzung läuft weiter"],
          ["Alt + E", "Ordner im Explorer öffnen"],
          ["Alt + G", "Im Terminal: Git-Ansicht öffnen"],
          ["Strg + ⇧ + C / Strg + V", "Im Terminal: kopieren / einfügen"],
        ])}
        {@render keys("Git-Ansicht", [
          ["Esc", "Zurück zur Liste"],
          ["Strg + 1 … 4", "Änderungen / Verlauf / Releases / Actions"],
          ["Strg + ⇧ + F", "Fetch"],
          ["Strg + ⇧ + L", "Pull"],
          ["Strg + ⇧ + P", "Push"],
          ["F5", "Aktualisieren"],
          ["Strg + ⏎", "Commit"],
          ["Leertaste", "Datei stagen / unstagen"],
          ["F7 / ⇧ + F7", "Merge-Editor: nächster / vorheriger Konflikt"],
          ["Strg + S", "Merge-Editor: speichern"],
        ])}
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>

<!-- Erster Start: nicht wegklickbar, ohne Ordner gibt es nichts einzulesen. -->
<Dialog.Root open={setup}>
  <Dialog.Content
    class="sm:max-w-md"
    showCloseButton={false}
    escapeKeydownBehavior="ignore"
    interactOutsideBehavior="ignore"
  >
    <Dialog.Header>
      <Dialog.Title>Willkommen bei Open Claude</Dialog.Title>
      <Dialog.Description>
        Wo liegen deine Projekte? Alle Git-Repos darunter erscheinen in der Liste. Ändern kannst du das später mit Strg+O.
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex gap-2">
      <Input
        bind:value={root}
        class="font-mono text-xs"
        onkeydown={(e) => e.key === "Enter" && finishSetup()}
      />
      <Button variant="outline" size="icon" onclick={pickRoot} aria-label="Ordner wählen">
        <FolderIcon class="size-4" />
      </Button>
    </div>
    <div class="flex justify-end">
      <Button onclick={finishSetup} disabled={!root.trim()}>Übernehmen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root open={doomed !== null} onOpenChange={(o) => !o && !trashing && (doomed = null)}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>„{doomed && split(doomed.repo.rel)[1]}“ vom Gerät löschen?</Dialog.Title>
      <Dialog.Description>
        Der Ordner wandert in den Papierkorb, auf dem Server bleibt alles. Eine laufende Claude-Sitzung wird beendet.
      </Dialog.Description>
    </Dialog.Header>
    <p class="text-muted-foreground truncate font-mono text-[11px]" title={doomed?.repo.path}>{doomed?.repo.path}</p>
    {#if doomed?.risks.length}
      <ul class="bg-destructive/10 text-destructive list-disc rounded-md py-2 pr-3 pl-7 text-xs">
        {#each doomed.risks as r (r)}<li>{r}</li>{/each}
      </ul>
    {/if}
    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" disabled={trashing} onclick={() => (doomed = null)}>Abbrechen</Button>
      <Button variant="destructive" size="sm" disabled={trashing} onclick={trashRepo}>
        {trashing ? "Lösche …" : "In den Papierkorb"}
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>

{#if store}
  <AccountsDialog bind:open={accountsOpen} {store} bind:accounts />
{/if}
<CloneDialog
  bind:open={cloneOpen}
  {accounts}
  {root}
  existing={repos.map((r) => r.path)}
  onclone={cloned}
  onaccounts={() => {
    cloneOpen = false;
    accountsOpen = true;
  }}
/>
