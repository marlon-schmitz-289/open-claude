<script lang="ts">
  // Ausgabe der Run-Leiste: eigenes xterm, haengt sich an den Puffer des Projekts (Replay beim Mount).
  // Haelt keinen Zustand: Prozess und Ausgabe liegen in $lib/run.svelte und ueberleben das Umdocken.
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { Button } from "$lib/components/ui/button/index.js";
  import { paneBar } from "$lib/components/kit";
  import EraserIcon from "@lucide/svelte/icons/eraser";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import SquareIcon from "@lucide/svelte/icons/square";
  import { ptyResize } from "$lib/files";
  import { DARK, LIGHT, LINKS } from "$lib/termtheme";
  import { theme } from "$lib/theme.svelte";
  import { attach, clearOutput, run, runRestart, runStart, runStop, runWrite } from "$lib/run.svelte";

  let { repo }: { repo: string } = $props();

  const r = $derived(run(repo));
  const cur = $derived(r.current);
  let el: HTMLDivElement;
  let term: Terminal | undefined;
  const fit = new FitAddon();
  // PTY, der die aktuelle Groesse schon gemeldet wurde.
  let sized = "";

  function refit() {
    if (!term || !el.clientWidth || !el.clientHeight) return;
    fit.fit();
    const c = r.current;
    if (c?.state !== "running") return;
    sized = c.id;
    ptyResize(c.id, term.cols, term.rows).catch(() => {});
  }

  // Laufzeit im Sekundentakt, nur waehrend ein Prozess laeuft.
  let now = $state(Date.now());
  $effect(() => {
    if (cur?.state !== "running") return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const clock = (ms: number) => {
    const s = Math.max(0, Math.floor(ms / 1000));
    return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  };

  $effect(() => {
    const t = theme.dark ? DARK : LIGHT;
    if (term) term.options.theme = t;
  });

  onMount(() => {
    const css = getComputedStyle(document.documentElement);
    const t = new Terminal({
      allowTransparency: true,
      cursorStyle: "bar",
      cursorInactiveStyle: "none",
      linkHandler: LINKS,
      fontFamily: css.getPropertyValue("--font-mono") || "monospace",
      fontSize: 12,
      lineHeight: 1.2,
      scrollback: 5000,
      theme: theme.dark ? DARK : LIGHT,
    });
    term = t;
    t.loadAddon(fit);
    t.open(el);
    // F5 & Co. gehoeren der Run-Leiste (EditorView), nicht dem Prozess.
    t.attachCustomKeyEventHandler((e) => e.key !== "F5");
    for (const c of attach(repo, (b) => {
      t.write(b);
      // Ein neuer PTY startet mit 120x30: beim ersten Output (dann gibt es ihn sicher) die echte Groesse melden.
      if (r.current && sized !== r.current.id) refit();
    }))
      t.write(c);
    // Strg+C, Eingaben und Antworten auf Terminal-Abfragen gehen an den Prozess.
    t.onData((d) => runWrite(repo, d));
    const ro = new ResizeObserver(refit);
    ro.observe(el);
    return () => {
      ro.disconnect();
      attach(repo, null);
      t.dispose();
      term = undefined;
    };
  });
</script>

<div class="flex h-full min-h-0 flex-col text-xs">
  <div class={paneBar}>
    {#if cur}
      <span
        class="size-1.5 shrink-0 rounded-full {cur.state === 'running'
          ? 'bg-success animate-pulse'
          : cur.state === 'exited' && cur.exit !== 0
            ? 'bg-destructive'
            : 'bg-muted-foreground'}"
      ></span>
      <span class="truncate font-mono text-[11px]" title={cur.cfg.label}>{cur.cfg.label}</span>
      <span class="text-muted-foreground shrink-0 text-[11px]" role="status">
        {#if cur.state === "running"}
          läuft {clock(now - cur.started)}
        {:else if cur.state === "idle"}
          gestoppt
        {:else if cur.exit === null}
          <span class="text-destructive">beendet</span>
        {:else}
          <span class={cur.exit === 0 ? "text-success" : "text-destructive"}>beendet (Exit {cur.exit})</span>
        {/if}
      </span>
    {:else}
      <span class="text-muted-foreground">Noch nichts gestartet (F5).</span>
    {/if}
    <span class="flex-1"></span>
    {#if cur?.state === "running"}
      <Button variant="ghost" size="icon-xs" title="Stoppen (Umschalt+F5)" onclick={() => runStop(repo)}><SquareIcon /></Button>
    {:else}
      <Button
        variant="ghost"
        size="icon-xs"
        title="Starten (F5) – führt Code aus dem Projekt aus"
        onclick={() => runStart(repo, cur?.cfg)}><PlayIcon /></Button
      >
    {/if}
    <Button variant="ghost" size="icon-xs" disabled={!cur} title="Neu starten (Strg+Umschalt+F5)" onclick={() => runRestart(repo)}>
      <RotateCwIcon />
    </Button>
    <Button variant="ghost" size="icon-xs" title="Ausgabe leeren" onclick={() => clearOutput(repo)}><EraserIcon /></Button>
  </div>
  <div bind:this={el} class="min-h-0 flex-1 px-2 py-1"></div>
</div>

