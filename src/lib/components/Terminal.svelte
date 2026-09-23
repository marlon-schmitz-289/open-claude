<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";

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
    if (e.type !== "keydown" || !e.ctrlKey) return true;
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
      fontFamily: css.getPropertyValue("--font-mono") || "monospace",
      fontSize: 13,
      theme: {
        background: "#00000000",
        foreground: css.getPropertyValue("--foreground"),
        cursor: css.getPropertyValue("--primary"),
      },
    });
    term = t;
    t.loadAddon(fit);
    t.open(el);
    fit.fit();
    t.attachCustomKeyEventHandler(onKeyEvent);
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
<div class="h-full p-2 pr-0 {visible ? '' : 'hidden'}">
  <div bind:this={el} class="h-full"></div>
</div>
