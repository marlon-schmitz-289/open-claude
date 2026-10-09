<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import SendIcon from "@lucide/svelte/icons/send-horizontal";
  import SquareIcon from "@lucide/svelte/icons/square";
  import XIcon from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button/index.js";
  import ClaudeLoader from "./ClaudeLoader.svelte";
  import Markdown from "./Markdown.svelte";
  import { CONTROL, LOCAL, fmt, route, type Cmd } from "$lib/chat.logic";

  let {
    id,
    cwd,
    visible,
    resume,
    onresume,
    onexit,
  }: { id: string; cwd: string; visible: boolean; resume?: string; onresume: (sid: string) => void; onexit: () => void } = $props();

  type Past = { id: string; title: string; modified: number };

  type Item =
    | { kind: "user"; text: string; imgs?: string[] }
    | { kind: "text"; text: string }
    | { kind: "tool"; id: string; name: string; detail: string; state: "run" | "ok" | "err" }
    | { kind: "ask"; rid: string; name: string; input: Record<string, unknown>; answer?: "allow" | "deny" }
    | { kind: "info"; text: string; error?: boolean }
    | { kind: "pick"; list: Past[] };

  let items = $state<Item[]>([]);
  let draft = $state("");
  // Angehaengte Bilder als data:-URL, gehen mit der naechsten Nachricht raus.
  let imgs = $state<string[]>([]);
  // Slash-Commands und Skills, kommen per initialize von claude; lokale nur, wo claude headless keinen hat.
  let commands = $state<Cmd[]>([]);
  const all = $derived([...commands, ...LOCAL.filter((l) => !commands.some((c) => c.name === l.name))]);
  let sel = $state(0);
  // Exakt vor Praefix vor Teilstring, sonst vervollstaendigt Enter "/review" zu einem anderen Command.
  const rank = (c: Cmd, q: string) => Math.min(...[c.name, ...(c.aliases ?? [])].map((n) => (n === q ? 0 : n.startsWith(q) ? 1 : n.includes(q) ? 2 : 3)));
  const matches = $derived.by(() => {
    const q = /^\/(\S*)$/.exec(draft)?.[1];
    return q === undefined ? [] : all.filter((c) => rank(c, q) < 3).sort((a, b) => rank(a, q) - rank(b, q)).slice(0, 8);
  });
  // Session-id von claude, fuer /continue und um die eigene Session in /resume auszublenden.
  let sid: string | null = null;
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

  const note = (text: string, error = false) => items.push({ kind: "info", text, error });
  const write = (msg: object) => invoke("chat_send", { id, line: JSON.stringify(msg) }).catch((e) => note(String(e), true));

  // Offene control_requests nach request_id.
  const pending = new Map<string, (r: any, err?: string) => void>();
  const control = (subtype: string, extra = {}) =>
    new Promise<any>((res, rej) => {
      const rid = crypto.randomUUID();
      pending.set(rid, (r, err) => (err ? rej(err) : res(r)));
      write({ type: "control_request", request_id: rid, request: { subtype, ...extra } });
    });

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
    if (m.type === "system" && m.subtype === "init") {
      booting = false;
      sid = m.session_id ?? sid;
    } else if (m.type === "control_response") {
      const r = m.response ?? {};
      pending.get(r.request_id)?.(r.response, r.subtype === "error" ? r.error || "Fehler" : undefined);
      pending.delete(r.request_id);
    } else if (m.type === "conversation_reset") {
      // /clear: neue Session, alter Verlauf weg
      items = [];
      sid = m.new_conversation_id ?? sid;
    }
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

  function prompt(text: string, pics: string[] = []) {
    // data:<typ>;base64,<daten> -> Bild-Block der API
    const blocks = pics.map((u) => ({ type: "image", source: { type: "base64", media_type: u.slice(5, u.indexOf(";")), data: u.slice(u.indexOf(",") + 1) } }));
    const content = blocks.length ? [...blocks, ...(text ? [{ type: "text", text }] : [])] : text;
    write({ type: "user", message: { role: "user", content } });
    busy = true;
  }

  // Ungetrimmt routen: wer Text mit "/" senden will, stellt ein Leerzeichen davor.
  function send() {
    const text = draft.trim();
    if ((!text && !imgs.length) || dead) return;
    // Mit Bildern ist es immer eine Nachricht, kein Command.
    const r = imgs.length ? null : route(draft, commands);
    const pics = imgs;
    items.push({ kind: "user", text, imgs: pics });
    stick = true;
    draft = "";
    imgs = [];
    tick().then(grow);
    if (r?.kind === "local") local(r.name, r.args);
    else if (r?.kind === "unknown") note(`/${r.name} gibt es hier nicht, /help zeigt alle`);
    else prompt(text, pics);
  }

  /** Commands, die claude headless nicht kennt (siehe chat.logic.ts). */
  async function local(name: string, args: string) {
    try {
      if (CONTROL[name]) note(fmt(await control(CONTROL[name])));
      else if (name === "export") {
        // WebKit erlaubt die Zwischenablage nur in der Tasten-Geste: ClipboardItem mit Promise statt writeText nach await.
        const blob = control("export_conversation").then((r) => new Blob([r?.text ?? ""], { type: "text/plain" }));
        await navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
        note((await blob).size ? "Verlauf in der Zwischenablage" : "Noch nichts zu exportieren");
      } else if (name === "plan") {
        await control("set_permission_mode", { mode: "plan" });
        note("Plan-Modus an");
        if (args) prompt(args);
      } else if (name === "copy") {
        const t = items.findLast((i) => i.kind === "text");
        if (t?.kind === "text") await navigator.clipboard.writeText(t.text).then(() => note("Antwort in der Zwischenablage"));
        else note("Noch keine Antwort");
      } else if (name === "help") {
        note(all.map((c) => `/${c.name}${c.argumentHint ? ` ${c.argumentHint}` : ""}  ${c.description.split("\n")[0].slice(0, 100)}`).join("\n"));
      } else if (name === "exit") onexit();
      else if (name === "login" || name === "logout") note(`Im Terminal: claude auth ${name}`);
      else if (name === "resume" || name === "continue") {
        const list = (await invoke<Past[]>("chat_sessions", { cwd })).filter((p) => p.id !== sid);
        const a = args.toLowerCase();
        const hit =
          name === "continue" ? list[0] : args && (list.find((p) => p.id.startsWith(a)) ?? list.find((p) => p.title.toLowerCase().includes(a)));
        if (hit) onresume(hit.id);
        else if (name === "resume" && !args && list.length) items.push({ kind: "pick", list });
        else note(args ? `Keine Sitzung zu „${args}“` : "Keine frühere Sitzung");
      }
    } catch (e) {
      note(String(e), true);
    }
  }

  function answer(it: Item & { kind: "ask" }, allow: boolean) {
    it.answer = allow ? "allow" : "deny";
    const response = allow ? { behavior: "allow", updatedInput: it.input } : { behavior: "deny", message: "Vom Benutzer abgelehnt" };
    write({ type: "control_response", response: { subtype: "success", request_id: it.rid, response } });
  }

  const stop = () => write({ type: "control_request", request_id: crypto.randomUUID(), request: { subtype: "interrupt" } });

  function pick(name: string) {
    draft = `/${name} `;
    field?.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    const m = matches[sel];
    if (m && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      sel = (sel + (e.key === "ArrowDown" ? 1 : matches.length - 1)) % matches.length;
    } else if (m && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && draft !== `/${m.name}`))) {
      // Enter vervollstaendigt erst; ist der Command schon ausgeschrieben, wird gesendet.
      e.preventDefault();
      pick(m.name);
    } else if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    } else if (e.key === "Escape" && !e.shiftKey && busy) {
      e.preventDefault();
      stop();
    }
  }

  // Lange Kante max. 1568 px (Empfehlung der API) und unter 5 MB, sonst lehnt die API ab bzw. rechnet selbst runter.
  const MAX_EDGE = 1568;
  const MAX_URL = 5 * 1024 * 1024 * 1.37;
  async function attach(url: string) {
    try {
      const img = new Image();
      img.src = url;
      await img.decode();
      const k = Math.min(1, MAX_EDGE / Math.max(img.naturalWidth, img.naturalHeight));
      if (k < 1 || url.length > MAX_URL) {
        const c = document.createElement("canvas");
        c.width = Math.round(img.naturalWidth * k);
        c.height = Math.round(img.naturalHeight * k);
        c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
        url = c.toDataURL("image/png");
        if (url.length > MAX_URL) url = c.toDataURL("image/jpeg", 0.85);
      }
      imgs.push(url);
    } catch (e) {
      note(`Bild nicht lesbar: ${e}`, true);
    }
  }

  function onPaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length) return;
    e.preventDefault();
    for (const f of files) {
      const r = new FileReader();
      r.onload = () => attach(String(r.result));
      r.readAsDataURL(f);
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
      // Tauri faengt Datei-Drops ab (kein HTML-drop); Bilder anhaengen, andere Pfade ans Eingabefeld.
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type !== "drop" || !visible || !e.payload.paths.length) return;
        const pics = e.payload.paths.filter((p) => /\.(png|jpe?g|gif|webp)$/i.test(p));
        for (const p of pics) invoke<string>("chat_image", { path: p }).then(attach, (err) => note(String(err), true));
        const rest = e.payload.paths.filter((p) => !pics.includes(p));
        if (rest.length) draft += (draft && !draft.endsWith(" ") ? " " : "") + rest.join(" ") + " ";
        field?.focus();
        tick().then(grow);
      }),
    ];
    sid = resume ?? null;
    Promise.all(unlisten)
      .then(() => (resume ? replay(resume) : undefined))
      .then(() => (closed ? undefined : invoke("chat_open", { id, cwd, resume })))
      .then(() => {
        booting = false;
        if (closed) return invoke("chat_close", { id });
        control("initialize").then((r) => (commands = r?.commands ?? []), () => {});
      })
      .catch((e) => {
        booting = false;
        dead = true;
        items.push({ kind: "info", text: String(e), error: true });
      });
  });

  /** Alter Verlauf aus der jsonl (claude --resume spielt ihn nicht ab); gleiches Format wie stream-json. */
  async function replay(resume: string) {
    const lines = await invoke<string[]>("chat_history", { cwd, sid: resume }).catch((e) => (note(String(e), true), []));
    for (const l of lines) {
      const m = JSON.parse(l);
      const c = m.message?.content;
      const text = m.type === "user" && (typeof c === "string" ? c : Array.isArray(c) && c.find((b: any) => b.type === "text")?.text);
      const pics: string[] =
        m.type === "user" && Array.isArray(c)
          ? c.filter((b: any) => b.type === "image" && b.source?.type === "base64").map((b: any) => `data:${b.source.media_type};base64,${b.source.data}`)
          : [];
      if (text || pics.length) items.push({ kind: "user", text: text || "", imgs: pics });
      else onLine(l);
    }
    // Abgebrochene Tools ohne Ergebnis nicht ewig laufen lassen.
    for (const i of items) if (i.kind === "tool" && i.state === "run") i.state = "err";
  }

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
          <div class="bg-primary/15 flex max-w-[85%] flex-col gap-2 self-end rounded-lg px-3 py-2">
            {#if it.imgs?.length}
              <div class="flex flex-wrap justify-end gap-1">
                {#each it.imgs as u, j (j)}<img src={u} alt="Bild {j + 1}" class="max-h-40 rounded" />{/each}
              </div>
            {/if}
            {#if it.text}<p class="whitespace-pre-wrap select-text">{it.text}</p>{/if}
          </div>
        {:else if it.kind === "text"}
          <Markdown text={it.text} root={cwd} size="text-sm" />
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
        {:else if it.kind === "pick"}
          <div class="flex flex-col gap-1">
            {#each it.list as p (p.id)}
              <button
                type="button"
                class="hover:bg-accent flex min-w-0 items-baseline gap-2 rounded px-2 py-1 text-left text-xs"
                onclick={() => onresume(p.id)}
              >
                <span class="truncate">{p.title}</span>
                <span class="text-muted-foreground ml-auto shrink-0">{new Date(p.modified).toLocaleString()}</span>
              </button>
            {/each}
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
    class="bg-card relative flex shrink-0 items-end gap-2 rounded-xl py-2 pr-2 pl-4 shadow-lg ring-1 shadow-black/10 ring-foreground/5 transition-shadow focus-within:ring-primary/40"
  >
    {#if imgs.length}
      <div class="bg-popover absolute inset-x-0 bottom-full mb-2 flex flex-wrap gap-2 rounded-xl p-2 shadow-lg ring-1 ring-foreground/10 {matches.length ? 'hidden' : ''}">
        {#each imgs as u, i (i)}
          <div class="relative">
            <img src={u} alt="Anhang {i + 1}" class="h-16 rounded" />
            <button
              type="button"
              class="bg-background/90 absolute -top-1.5 -right-1.5 rounded-full p-0.5 shadow ring-1 ring-foreground/10"
              title="Entfernen"
              aria-label="Bild entfernen"
              onclick={() => imgs.splice(i, 1)}><XIcon class="size-3" /></button
            >
          </div>
        {/each}
      </div>
    {/if}
    {#if matches.length}
      <div class="bg-popover absolute inset-x-0 bottom-full mb-2 overflow-hidden rounded-xl py-1 text-sm shadow-lg ring-1 ring-foreground/10">
        {#each matches as c, i (c.name)}
          <button
            type="button"
            class="flex w-full min-w-0 items-baseline gap-2 px-3 py-1 text-left {i === sel ? 'bg-accent' : ''}"
            onmousedown={(e) => e.preventDefault()}
            onclick={() => pick(c.name)}
            onmouseenter={() => (sel = i)}
          >
            <span class="shrink-0 font-mono">/{c.name}</span>
            {#if c.argumentHint}<span class="text-muted-foreground shrink-0 font-mono text-xs">{c.argumentHint}</span>{/if}
            <span class="text-muted-foreground truncate text-xs">{c.description}</span>
          </button>
        {/each}
      </div>
    {/if}
    <textarea
      bind:this={field}
      bind:value={draft}
      oninput={() => {
        sel = 0;
        grow();
      }}
      onkeydown={onKeydown}
      onpaste={onPaste}
      rows="1"
      disabled={dead}
      placeholder={dead ? "Sitzung beendet" : "Nachricht an Claude …"}
      title="Enter senden, Umschalt+Enter neue Zeile"
      class="placeholder:text-muted-foreground block h-8 max-h-48 flex-1 resize-none overflow-x-hidden overflow-y-auto bg-transparent py-1 text-sm leading-6 outline-none"
    ></textarea>
    {#if busy}
      <Button size="icon" variant="outline" class="shrink-0" onclick={stop} title="Abbrechen" aria-label="Abbrechen"><SquareIcon /></Button>
    {:else}
      <Button size="icon" class="shrink-0" onclick={send} disabled={dead || (!draft.trim() && !imgs.length)} title="Senden" aria-label="Senden"><SendIcon /></Button>
    {/if}
  </div>
</div>
