<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import ClaudeLoader from "./ClaudeLoader.svelte";
  import { evenCell } from "$lib/termcell";
  import { theme } from "$lib/theme.svelte";
  import { DARK, LIGHT, LINKS } from "$lib/termtheme";

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

  // Zeilenhoehe und Zeichenabstand fuer den DOM-Renderer; WebGL rundet den Abstand auf ganze Geraetepixel.
  const base = { lineHeight: 1.2, letterSpacing: 0.3 };
  let webgl = false;

  // WebGL zeichnet Block- und Quadrantenzeichen in Achteln der Zelle. Bei ungerader Zellgroesse in
  // Geraetepixeln (haengt an Zoom mal Bildschirmskalierung) liegen die Kanten auf halben Pixeln:
  // Haarlinien im Logo, etwa ueber den Augen. Darum die Zelle je DPR auf gerade Pixel bringen.
  function snap() {
    if (!term || !webgl) return;
    // Private API wie im FitAddon; die Zeichengroesse gibt xterm nicht oeffentlich heraus.
    const cs = (term as any)._core?._charSizeService;
    if (cs?.width) Object.assign(term.options, evenCell(cs.width, cs.height, devicePixelRatio, base.lineHeight));
  }

  function refit() {
    // Versteckt liefert der Container 0x0 und wuerde die PTY auf 0 Spalten setzen.
    if (!term || !visible || !el.clientWidth || !el.clientHeight) return;
    snap();
    fit.fit();
    const size = `${term.cols}x${term.rows}`;
    if (!sent || size === sent) return;
    sent = size;
    invoke("pty_resize", { id, cols: term.cols, rows: term.rows }).catch(() => {});
  }

  // Tauri faengt Datei-Drops ab (kein HTML-drop), darum der Webview-Event. Pfade fuer die Shell quoten.
  const win = navigator.userAgent.includes("Windows");
  const quote = (p: string) => (win ? `"${p}"` : `'${p.replaceAll("'", "'\\''")}'`);

  function onKeyEvent(e: KeyboardEvent) {
    if (e.type !== "keydown") return true;
    // Alt+E (Explorer) und Alt+G (Git-Ansicht) macht der App-Handler.
    if (e.altKey && !e.ctrlKey && ["e", "g"].includes(e.key.toLowerCase())) return false;
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
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type !== "drop" || !visible || !e.payload.paths.length) return;
        term?.paste(e.payload.paths.map(quote).join(" ") + " ");
        term?.focus();
      }),
    ];
    // Farben aus der App; Hintergrund transparent, damit er von der Seite kommt.
    const css = getComputedStyle(document.documentElement);
    const t = new Terminal({
      allowTransparency: true,
      cursorBlink: true,
      cursorStyle: "bar",
      // Unfokussiert sonst ein Umriss-Kasten ueber dem Zeichen (v. a. auf dem Mac auffaellig).
      cursorInactiveStyle: "none",
      linkHandler: LINKS,
      fontFamily: css.getPropertyValue("--font-mono") || "monospace",
      fontSize: 13,
      ...base,
      fontWeightBold: "600",
      scrollback: 10000,
      smoothScrollDuration: 90,
      drawBoldTextInBrightColors: false,
      // Nur beim Start: claude laeuft mit festem Hell/Dunkel-Theme, ein Live-Wechsel machte den Text unlesbar.
      theme: theme.dark ? DARK : LIGHT,
    });
    term = t;
    t.loadAddon(fit);
    t.open(el);
    // Der DOM-Renderer nimmt Block- und Linienzeichen aus der Schrift: Logo mit Luecken,
    // Trennlinien mit Zacken (Windows). WebGL zeichnet sie selbst, zellfuellend.
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => {
        gl.dispose();
        webgl = false;
        Object.assign(t.options, base);
      });
      t.loadAddon(gl);
      webgl = true;
    } catch {
      // Kein WebGL2: DOM-Renderer bleibt.
    }
    snap();
    fit.fit();
    // Anderer Monitor/andere Skalierung bei gleicher Fenstergroesse: kein ResizeObserver, aber neue DPR.
    // Nach open() angemeldet, also nach xterms eigenem Listener (misst neu); endet mit dem Terminal.
    (t as any)._core?._coreBrowserService?.onDprChange(refit);
    t.attachCustomKeyEventHandler(onKeyEvent);
    // Trackpad unter macOS: Im Verlauf teilt xterms Scrollable die Deltas und glaettet je nach
    // Heuristik, fuer Maus-Apps/Alt-Screen drosselt xterm Deltas < 50 px auf 30 %. Langsam wischen
    // kriecht, schnell rast. Darum selbst: Verlauf 1:1 in Pixeln, sonst gesammelt als 50-px-Events.
    // Capture auf dem Container, sonst hat xterms Scrollable das Event schon.
    let wheelPx = 0;
    if (navigator.userAgent.includes("Mac")) {
      el.addEventListener(
        "wheel",
        (e) => {
          if (!e.isTrusted || e.ctrlKey || !e.deltaY || e.deltaMode !== WheelEvent.DOM_DELTA_PIXEL) return;
          e.preventDefault();
          e.stopPropagation();
          if (t.modes.mouseTrackingMode === "none" && t.buffer.active.type === "normal") {
            // scrollLines nimmt Bruchteile, Viewport rechnet in Pixel zurueck.
            t.scrollLines(e.deltaY / (el.clientHeight / t.rows));
            return;
          }
          wheelPx += e.deltaY;
          while (Math.abs(wheelPx) >= 50) {
            const deltaY = Math.sign(wheelPx) * 50;
            wheelPx -= deltaY;
            const { clientX, clientY } = e;
            e.target?.dispatchEvent(new WheelEvent("wheel", { deltaY, clientX, clientY, bubbles: true, cancelable: true }));
          }
        },
        { capture: true, passive: false },
      );
    }
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
      .then(() => (dead ? undefined : invoke("pty_open", { id, cwd, cols, rows, theme: theme.dark ? "dark" : "light" })))
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
    class="h-full overflow-hidden rounded-xl px-3 py-2.5 shadow-lg ring-1 shadow-black/10 dark:shadow-black/30 ring-foreground/5 transition-shadow duration-300 focus-within:ring-primary/40 focus-within:shadow-primary/10"
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
