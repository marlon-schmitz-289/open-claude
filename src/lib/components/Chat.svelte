<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import SendIcon from "@lucide/svelte/icons/send-horizontal";
  import SquareIcon from "@lucide/svelte/icons/square";
  import { Button } from "$lib/components/ui/button/index.js";
  import ClaudeLoader from "./ClaudeLoader.svelte";
  import Markdown from "./Markdown.svelte";

  let { id, cwd, visible }: { id: string; cwd: string; visible: boolean } = $props();

  type Item =
    | { kind: "user"; text: string }
    | { kind: "text"; text: string }
    | { kind: "tool"; id: string; name: string; detail: string; state: "run" | "ok" | "err" }
    | { kind: "ask"; rid: string; name: string; input: Record<string, unknown>; answer?: "allow" | "deny" }
    | { kind: "info"; text: string; error?: boolean };

  let items = $state<Item[]>([]);
  let draft = $state("");
  // Bis der Prozess laeuft, laufen die Clawds. system/init kommt im stream-json-Modus erst nach dem ersten Prompt.
  let booting = $state(true);
  let busy = $state(false);
  let dead = $state(false);
  let log: HTMLDivElement;
  let field: HTMLTextAreaElement;
  // Nur mitscrollen, wenn der User unten ist; wer hochscrollt, liest in Ruhe.
  let stick = true;
  // stderr nur beim Ende zeigen, claude schreibt dort auch harmlose Warnungen.
  const errs: string[] = [];
  let unlisten: Promise<UnlistenFn>[] = [];
  let closed = false;

  const write = (msg: object) =>
    invoke("chat_send", { id, line: JSON.stringify(msg) }).catch((e) => items.push({ kind: "info", text: String(e), error: true }));

  // Kurzbeschreibung eines Tool-Aufrufs wie im Session-Panel der Mod.
  const detail = (input: Record<string, unknown>) =>
    String(["file_path", "pattern", "command", "description", "url", "query"].map((k) => input?.[k]).find((v) => typeof v === "string") ?? "");

  function onLine(line: string) {
    let m: any;
    try {
      m = JSON.parse(line);
    } catch {
      return; // Ausgaben der Login-Shell (.zshrc) vor claude
    }
    // Interna von Sub-Agents nicht in den Verlauf.
    if (m.parent_tool_use_id) return;
    if (m.type === "system" && m.subtype === "init") booting = false;
    else if (m.type === "assistant") {
      for (const b of m.message?.content ?? []) {
        if (b.type === "text" && b.text.trim()) items.push({ kind: "text", text: b.text });
        else if (b.type === "tool_use") items.push({ kind: "tool", id: b.id, name: b.name, detail: detail(b.input), state: "run" });
      }
    } else if (m.type === "user" && Array.isArray(m.message?.content)) {
      for (const b of m.message.content) {
        const t = b.type === "tool_result" && items.find((i) => i.kind === "tool" && i.id === b.tool_use_id);
        if (t && t.kind === "tool") t.state = b.is_error ? "err" : "ok";
      }
    } else if (m.type === "control_request" && m.request?.subtype === "can_use_tool") {
      items.push({ kind: "ask", rid: m.request_id, name: m.request.tool_name, input: m.request.input });
    } else if (m.type === "result") {
      busy = false;
      // Abgebrochen oder Fehler: offene Abfragen sind hinfaellig.
      for (const i of items) if (i.kind === "ask" && !i.answer) i.answer = "deny";
      if (m.is_error) items.push({ kind: "info", text: m.result || (m.errors ?? []).join("\n") || "Fehler", error: true });
    }
  }

  function send() {
    const text = draft.trim();
    if (!text || dead) return;
    items.push({ kind: "user", text });
    stick = true;
    write({ type: "user", message: { role: "user", content: text } });
    draft = "";
    busy = true;
    tick().then(grow);
  }

  function answer(it: Item & { kind: "ask" }, allow: boolean) {
    it.answer = allow ? "allow" : "deny";
    const response = allow ? { behavior: "allow", updatedInput: it.input } : { behavior: "deny", message: "Vom Benutzer abgelehnt" };
    write({ type: "control_response", response: { subtype: "success", request_id: it.rid, response } });
  }

  const stop = () => write({ type: "control_request", request_id: crypto.randomUUID(), request: { subtype: "interrupt" } });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    } else if (e.key === "Escape" && !e.shiftKey && busy) {
      e.preventDefault();
      stop();
    }
  }

  // Waechst mit dem Text bis max-h, dann scrollt das Feld.
  function grow() {
    field.style.height = "";
    if (field.value) field.style.height = `${field.scrollHeight}px`;
  }

  $effect(() => {
    items.length;
    if (stick) tick().then(() => log && (log.scrollTop = log.scrollHeight));
  });

  $effect(() => {
    if (visible) field?.focus();
  });

  onMount(() => {
    // Listener vor chat_open, damit keine fruehe Zeile verloren geht.
    unlisten = [
      listen<string>(`chat:${id}`, (e) => onLine(e.payload)),
      listen<string>(`chat-err:${id}`, (e) => {
        errs.push(e.payload);
        if (errs.length > 20) errs.shift();
      }),
      listen<number | null>(`chat-exit:${id}`, (e) => {
        booting = busy = false;
        dead = true;
        items.push({ kind: "info", text: [`claude beendet (Code ${e.payload ?? "?"})`, ...errs].join("\n"), error: e.payload !== 0 });
      }),
      // Tauri faengt Datei-Drops ab (kein HTML-drop); Pfade ans Eingabefeld haengen.
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type !== "drop" || !visible || !e.payload.paths.length) return;
        draft += (draft && !draft.endsWith(" ") ? " " : "") + e.payload.paths.join(" ") + " ";
        field?.focus();
        tick().then(grow);
      }),
    ];
    Promise.all(unlisten)
      .then(() => (closed ? undefined : invoke("chat_open", { id, cwd })))
      .then(() => {
        booting = false;
        if (closed) return invoke("chat_close", { id });
      })
      .catch((e) => {
        booting = false;
        dead = true;
        items.push({ kind: "info", text: String(e), error: true });
      });
  });

  onDestroy(() => {
    closed = true;
    for (const u of unlisten) u.then((f) => f());
    invoke("chat_close", { id }).catch(() => {});
  });
</script>

<div class="flex h-full flex-col gap-2 p-2 {visible ? '' : 'hidden'}">
  <!-- Logo kommt aus static/logo.svg; eigenes Logo: Datei ersetzen. -->
  <header class="flex shrink-0 justify-center py-1">
    <img src="/logo.svg" alt="Logo" class="h-10 select-none" draggable="false" />
  </header>

  <div
    class="relative min-h-0 flex-1 overflow-hidden rounded-xl shadow-lg ring-1 shadow-black/10 dark:shadow-black/30 ring-foreground/5"
    style="background: radial-gradient(ellipse at top right, color-mix(in oklch, var(--primary) 8%, transparent), transparent 60%), var(--card);"
  >
    <div
      bind:this={log}
      onscroll={() => (stick = log.scrollHeight - log.scrollTop - log.clientHeight < 40)}
      class="flex h-full flex-col gap-3 overflow-y-auto px-4 py-3 text-sm"
    >
      {#each items as it, i (i)}
        {#if it.kind === "user"}
          <div class="bg-primary/15 max-w-[85%] self-end rounded-lg px-3 py-2 whitespace-pre-wrap select-text">{it.text}</div>
        {:else if it.kind === "text"}
          <Markdown text={it.text} size="text-sm" />
        {:else if it.kind === "tool"}
          <div class="text-muted-foreground flex min-w-0 items-center gap-2 font-mono text-xs">
            <span class={it.state === "err" ? "text-destructive" : it.state === "ok" ? "text-green-500" : "text-primary animate-pulse"}
              >{it.state === "err" ? "✗" : it.state === "ok" ? "✓" : "●"}</span
            >
            <span class="text-foreground shrink-0">{it.name}</span>
            <span class="truncate" title={it.detail}>{it.detail}</span>
          </div>
        {:else if it.kind === "ask"}
          <div class="border-primary/40 rounded-lg border px-3 py-2">
            <p class="mb-1 text-xs font-medium">Claude möchte <span class="font-mono">{it.name}</span> ausführen</p>
            <pre class="bg-muted max-h-40 overflow-auto rounded p-2 font-mono text-xs whitespace-pre-wrap select-text">{detail(it.input) ||
                JSON.stringify(it.input, null, 2)}</pre>
            {#if it.answer}
              <p class="text-muted-foreground mt-1 text-xs">{it.answer === "allow" ? "Erlaubt" : "Abgelehnt"}</p>
            {:else}
              <div class="mt-2 flex gap-2">
                <Button size="sm" onclick={() => answer(it, true)}>Erlauben</Button>
                <Button size="sm" variant="outline" onclick={() => answer(it, false)}>Ablehnen</Button>
              </div>
            {/if}
          </div>
        {:else}
          <p class="font-mono text-xs whitespace-pre-wrap {it.error ? 'text-destructive' : 'text-muted-foreground'}">{it.text}</p>
        {/if}
      {/each}
      {#if busy}
        <p class="text-muted-foreground text-xs">Claude arbeitet … <span class="opacity-60">(Esc bricht ab)</span></p>
      {/if}
    </div>
    {#if booting}<ClaudeLoader />{/if}
  </div>

  <div
    class="bg-card flex shrink-0 items-end gap-2 rounded-xl py-2 pr-2 pl-4 shadow-lg ring-1 shadow-black/10 ring-foreground/5 transition-shadow focus-within:ring-primary/40"
  >
    <textarea
      bind:this={field}
      bind:value={draft}
      oninput={grow}
      onkeydown={onKeydown}
      rows="1"
      disabled={dead}
      placeholder={dead ? "Sitzung beendet" : "Nachricht an Claude …"}
      title="Enter senden, Umschalt+Enter neue Zeile"
      class="placeholder:text-muted-foreground block h-8 max-h-48 flex-1 resize-none overflow-x-hidden overflow-y-auto bg-transparent py-1 text-sm leading-6 outline-none"
    ></textarea>
    {#if busy}
      <Button size="icon" variant="outline" class="shrink-0" onclick={stop} title="Abbrechen" aria-label="Abbrechen"><SquareIcon /></Button>
    {:else}
      <Button size="icon" class="shrink-0" onclick={send} disabled={dead || !draft.trim()} title="Senden" aria-label="Senden"><SendIcon /></Button>
    {/if}
  </div>
</div>
