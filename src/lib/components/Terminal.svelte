<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import ClaudeLoader from "./ClaudeLoader.svelte";

  let {
    id,
    cwd,
    visible,
    onexit,
  }: { id: string; cwd: string; visible: boolean; onexit: () => void } = $props();

  let el: HTMLDivElement;
  let term: Terminal | undefined;
  const fit = new FitAddon();
  // Zuletzt an die PTY gemeldete Groesse; leer bis pty_open durch ist.
  let sent = "";
  let unlisten: Promise<UnlistenFn>[] = [];
  // Unmount vor pty_open-Antwort: sonst bliebe eine PTY ohne UI zurueck.
  let dead = false;
  // claude braucht ~2 s bis zur ersten Ausgabe; solange laufen die Clawds.
  let booting = $state(true);

  function refit() {
    // Versteckt liefert der Container 0x0 und wuerde die PTY auf 0 Spalten setzen.
    if (!term || !visible || !el.clientWidth || !el.clientHeight) return;
    fit.fit();
    const size = `${term.cols}x${term.rows}`;
    if (!sent || size === sent) return;
    sent = size;
    invoke("pty_resize", { id, cols: term.cols, rows: term.rows }).catch(() => {});
  }

  function onKeyEvent(e: KeyboardEvent) {
    if (e.type !== "keydown") return true;
    // Alt+E oeffnet den Explorer, das macht der App-Handler.
    if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "e") return false;
    // Umschalt+Esc fuehrt zur Liste zurueck; einfaches Esc bleibt bei claude (abbrechen, zurueckspringen).
    if (e.shiftKey && e.key === "Escape") return false;
    if (!e.ctrlKey) return true;
    const key = e.key.toLowerCase();
    // false = xterm ignoriert das Event, es blubbert zum App-Handler am window.
    if (e.shiftKey && key === "w") return false;
    if (e.shiftKey && key === "c") {
      const text = term!.getSelection();
      if (!text) return true;
      navigator.clipboard.writeText(text);
    } else if (key === "v") {
      navigator.clipboard.readText().then((t) => term?.paste(t));
    } else {
      return true;
    }
    // Sonst feuert zusaetzlich das native paste-Event und es kommt doppelt an.
    e.preventDefault();
    return false;
  }

  onMount(() => {
    // Listener vor pty_open, damit kein frueher Output verloren geht.
    unlisten = [
      listen<number[]>(`pty:${id}`, (e) => term?.write(new Uint8Array(e.payload))),
      listen(`pty-exit:${id}`, () => onexit()),
    ];
    // Farben aus der App; Hintergrund transparent, damit er von der Seite kommt.
    const css = getComputedStyle(document.documentElement);
    const t = new Terminal({
      allowTransparency: true,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: css.getPropertyValue("--font-mono") || "monospace",
      fontSize: 13,
      lineHeight: 1.2,
      letterSpacing: 0.3,
      fontWeightBold: "600",
      scrollback: 10000,
      smoothScrollDuration: 90,
      // Palette Richtung Tokyo Night, magenta nah am Primary.
      theme: {
        background: "#00000000",
        foreground: css.getPropertyValue("--foreground"),
        cursor: css.getPropertyValue("--primary"),
        cursorAccent: "#1a1b26",
        selectionBackground: "#a78bfa40",
        black: "#2a2b3d",
        red: "#f7768e",
        green: "#9ece6a",
        yellow: "#e0af68",
        blue: "#7aa2f7",
        magenta: "#a78bfa",
        cyan: "#7dcfff",
        white: "#c0caf5",
        brightBlack: "#565f89",
        brightRed: "#ff8fa3",
        brightGreen: "#b9f27c",
        brightYellow: "#ffc777",
        brightBlue: "#8db0ff",
        brightMagenta: "#c4b5fd",
        brightCyan: "#a4e2ff",
        brightWhite: "#e5e9f7",
      },
    });
    term = t;
    t.loadAddon(fit);
    t.open(el);
    fit.fit();
    t.attachCustomKeyEventHandler(onKeyEvent);
    // ConPTY schickt vorab nur Steuersequenzen; erst sichtbarer Text heisst, claude ist da.
    const ready = t.onWriteParsed(() => {
      const b = t.buffer.active;
      for (let y = 0; y < b.length; y++) {
        if (b.getLine(y)?.translateToString(true).trim()) {
          booting = false;
          ready.dispose();
          return;
        }
      }
    });
    t.onData((data) => invoke("pty_write", { id, data }).catch(() => {}));
    t.focus();

    const { cols, rows } = t;
    Promise.all(unlisten)
      .then(() => (dead ? undefined : invoke("pty_open", { id, cwd, cols, rows })))
      .then(() => {
        if (dead) return invoke("pty_close", { id });
        sent = `${cols}x${rows}`;
        refit();
      })
      .catch((e) => dead || t.write(`\x1b[31m${e}\x1b[0m\r\n`));

    const ro = new ResizeObserver(refit);
    ro.observe(el);
    return () => ro.disconnect();
  });

  $effect(() => {
    if (visible && term) {
      refit();
      term.focus();
    }
  });

  onDestroy(() => {
    dead = true;
    for (const u of unlisten) u.then((f) => f());
    invoke("pty_close", { id }).catch(() => {});
    term?.dispose();
  });
</script>

<!-- ponytail: kein Scrollback ueber Neustart, Output lebt nur im xterm-Buffer. -->
<div class="h-full p-2 {visible ? '' : 'hidden'}">
  <!-- Dezenter Primary-Schimmer, scheint durchs transparente Terminal. -->
  <div
    class="h-full overflow-hidden rounded-xl px-3 py-2.5 shadow-lg ring-1 shadow-black/30 ring-white/5 transition-shadow duration-300 focus-within:ring-primary/40 focus-within:shadow-primary/10"
    style="background: radial-gradient(ellipse at top right, color-mix(in oklch, var(--primary) 8%, transparent), transparent 60%), var(--card);"
  >
    <div class="relative h-full">
      <div bind:this={el} class="h-full"></div>
      {#if booting}<ClaudeLoader />{/if}
    </div>
  </div>
</div>

<style>
  /* xterm.css gibt dem Viewport schwarz, das verdeckt den Verlauf. */
  :global(.xterm .xterm-viewport) {
    background-color: transparent !important;
  }
</style>
