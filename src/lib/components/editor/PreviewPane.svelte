<script lang="ts">
  // Vorschau der aktiven oder angehefteten Datei. Der Renderer kommt aus files.PREVIEW_KINDS (nach Endung).
  // Haelt nur Anzeigezustand; pin und url liegen im Store und ueberleben das Umdocken.
  import Markdown from "$lib/components/Markdown.svelte";
  import { Empty, paneBar } from "$lib/components/kit";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import PinIcon from "@lucide/svelte/icons/pin";
  import PinOffIcon from "@lucide/svelte/icons/pin-off";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import SquareIcon from "@lucide/svelte/icons/square";
  import { forge } from "$lib/git";
  import { DOTNET_FEHLT, DOTNET_URL, NUR_WINDOWS, previewKind, previewUrl, xamlRender } from "$lib/files";
  import { allowWpf, editor, setPin, setUrl } from "$lib/editor.svelte";
  import { devUrl } from "$lib/editor.logic";
  import { devConfig } from "$lib/run.logic";
  import { detect, run, runStart, runStop } from "$lib/run.svelte";

  let { repo }: { repo: string } = $props();

  const s = $derived(editor(repo));
  const target = $derived(s.pin ?? s.active);
  const kind = $derived(target ? previewKind(target) : null);
  const f = $derived(target ? s.files[target] : undefined);
  // Gespeicherte URL nochmal pruefen: nur http(s) darf in den iframe.
  // Eine von Hand eingetragene Adresse gewinnt gegen die erkannte des eigenen Dev-Servers.
  const r = $derived(run(repo));
  const url = $derived(devUrl(s.url) || r.current?.url || "");

  // Dev-Server: was der Start-Knopf starten wuerde (liest nur). Ausgabe und Rueckfragen zeigt das Panel "Ausgabe".
  $effect(() => void detect(repo));
  const dev = $derived(devConfig(r.configs, target ?? ""));
  // Nur ein npm-Lauf ist der Dev-Server; cargo run/dotnet watch stoppt die Vorschau nicht.
  const busy = $derived(r.current?.state === "running");
  const running = $derived(busy && r.current?.cfg.target.kind === "npm");
  function toggleDev() {
    if (running) return void runStop(repo);
    if (dev && !busy) void runStart(repo, dev);
  }

  // Zaehler des Neu-laden-Knopfs: haengt an ?v= bzw. baut den iframe neu auf.
  let n = $state(0);
  // mtime aendert sich beim Speichern: damit laedt die WebView Seite und Bild neu. Eine Webseite bindet andere
  // Dateien ein (CSS, JS): bei ihr zaehlt die juengste aller offenen Dateien.
  const stamp = $derived(
    kind === "web" ? Math.max(0, ...Object.values(s.files).map((o) => o.mtime ?? 0)) : (f?.mtime ?? 0),
  );
  const src = $derived(target ? previewUrl(repo, target, `${stamp}-${n}`) : "");
  // XAML laden kann Code ausfuehren: unter Windows erst nach Freigabe des Projekts (anderswo rendert ohnehin nichts).
  const locked = $derived(navigator.userAgent.includes("Windows") && !s.wpf);

  // URL-Feld: erst Enter uebernimmt, sonst laedt der iframe bei jedem Tastendruck.
  let draft = $state("");
  let bad = $state(false);
  $effect(() => {
    draft = s.url;
  });
  function go() {
    const v = devUrl(draft);
    bad = v === null;
    if (v !== null) setUrl(repo, v);
  }

  // WPF: statisches Bild, beim Tippen verzoegert neu gerendert. Das letzte gute Bild bleibt bei Fehlern stehen.
  let png = $state("");
  let error = $state("");
  let shown: string | null = null;
  $effect(() => {
    const path = target;
    const text = f?.text;
    void n;
    if (kind !== "xaml" || !path || text === undefined || url || locked) return;
    if (shown !== path) {
      shown = path;
      png = "";
      error = "";
    }
    let stale = false;
    const timer = setTimeout(async () => {
      try {
        const out = await xamlRender(repo, path, text);
        if (stale) return;
        png = out;
        error = "";
      } catch (e) {
        if (!stale) error = String(e);
      }
    }, 400);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  });
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class={paneBar}>
    <Input
      bind:value={draft}
      placeholder={r.current?.url || (target ?? "http://localhost:5173")}
      title="Adresse eines laufenden Dev-Servers; leer = Datei bzw. gestarteten Dev-Server anzeigen (Enter übernimmt)"
      aria-label="Adresse der Vorschau"
      aria-invalid={bad}
      spellcheck={false}
      class="h-6! min-w-0 flex-1 font-mono text-[11px]"
      onkeydown={(e) => e.key === "Enter" && go()}
    />
    <Button
      variant="ghost"
      size="icon-xs"
      class={running ? "text-foreground" : "text-muted-foreground"}
      disabled={!running && (!dev || busy)}
      title={running
        ? `Stoppen (${r.current?.cfg.label})`
        : busy
          ? `Es läuft bereits ${r.current?.cfg.label}`
          : dev
          ? `Dev-Server starten: ${dev.label} – führt Code aus dem Projekt aus`
          : "Kein dev-, start- oder serve-Script in package.json gefunden"}
      onclick={toggleDev}
    >
      {#if running}<SquareIcon />{:else}<PlayIcon />{/if}
    </Button>
    <Button
      variant="ghost"
      size="icon-xs"
      class={s.pin ? "text-foreground" : "text-muted-foreground"}
      disabled={!target}
      aria-pressed={!!s.pin}
      title={s.pin ? `Angeheftet: ${s.pin} – lösen, um der aktiven Datei zu folgen` : "Vorschau an diese Datei heften"}
      onclick={() => setPin(repo, s.pin ? null : target)}
    >
      {#if s.pin}<PinOffIcon />{:else}<PinIcon />{/if}
    </Button>
    <Button variant="ghost" size="icon-xs" class="text-muted-foreground" title="Neu laden" onclick={() => n++}>
      <RotateCwIcon />
    </Button>
  </div>
  {#if bad}
    <div class="text-destructive border-border border-b px-2 py-1" role="alert">Nur http://- oder https://-Adressen.</div>
  {/if}
  <div class="min-h-0 flex-1 overflow-auto">
    {#if url}
      <!-- bg-white: Seiten ohne eigenen Hintergrund rechnen mit Weiss wie im Browser. -->
      <!-- sandbox ohne allow-top-navigation: die Seite darf das App-Fenster nicht ersetzen (dort haette sie alle Befehle der App). -->
      <!-- allow-modals/-downloads: confirm(), alert() und Downloads der laufenden App sollen wie im Browser gehen. -->
      {#key n}<iframe
          src={url}
          sandbox="allow-scripts allow-forms allow-same-origin allow-modals allow-downloads"
          title="Vorschau {url}"
          class="size-full border-0 bg-white"
        ></iframe>{/key}
    {:else if !target}
      <Empty>Keine Datei für die Vorschau.</Empty>
    {:else if kind === "web"}
      <!-- Zusaetzlich ohne allow-same-origin: alle Projekte liegen auf einem Origin, per fetch laese die Seite sonst fremde Dateien. -->
      <iframe {src} sandbox="allow-scripts allow-forms" title="Vorschau {target}" class="size-full border-0 bg-white"></iframe>
    {:else if kind === "image"}
      <div class="grid min-h-full place-items-center p-3">
        <img {src} alt={target} class="max-w-full" />
      </div>
    {:else if kind === null}
      <Empty>Keine Vorschau für {target}.</Empty>
    {:else if !f}
      <Empty>Lädt …</Empty>
    {:else if kind === "markdown"}
      <div class="p-3"><Markdown text={f.text} onerror={(e) => (s.error = e)} /></div>
    {:else if locked}
      <Empty>
        <div class="max-w-xs space-y-2">
          <p>Die WPF-Vorschau lädt XAML aus diesem Projekt. XAML kann dabei Code ausführen – nur für Projekte erlauben, denen du vertraust.</p>
          <Button size="xs" variant="outline" onclick={() => allowWpf(repo)}>WPF-Vorschau erlauben</Button>
        </div>
      </Empty>
    {:else if error === NUR_WINDOWS}
      <Empty>WPF-Vorschau nur unter Windows</Empty>
    {:else if error === DOTNET_FEHLT}
      <Empty>
        <div>
          Für die WPF-Vorschau fehlt die .NET 8 Desktop Runtime.<br />
          <button
            class="text-primary underline"
            onclick={() => forge.openUrl(DOTNET_URL).catch((e) => (s.error = String(e)))}>Bei Microsoft herunterladen</button
          >
        </div>
      </Empty>
    {:else}
      {#if error}
        <pre class="text-destructive border-border border-b px-2 py-1 font-mono text-[11px] whitespace-pre-wrap select-text" role="alert">{error}</pre>
      {/if}
      {#if png}
        <div class="grid place-items-center p-3">
          <img src="data:image/png;base64,{png}" alt="WPF-Vorschau {target}" class="max-w-full" />
        </div>
      {:else if !error}
        <Empty>Rendert …</Empty>
      {/if}
    {/if}
  </div>
</div>
