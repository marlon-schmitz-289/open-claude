<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import SendIcon from "@lucide/svelte/icons/send-horizontal";
  import SquareIcon from "@lucide/svelte/icons/square";
  import XIcon from "@lucide/svelte/icons/x";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import Undo2Icon from "@lucide/svelte/icons/undo-2";
  import ZapIcon from "@lucide/svelte/icons/zap";
  import { Select } from "$lib/components/kit";
  import { Button } from "$lib/components/ui/button/index.js";
  import ClaudeLoader from "./ClaudeLoader.svelte";
  import Markdown from "./Markdown.svelte";
  import McpDialog from "./McpDialog.svelte";
  import PlugIcon from "@lucide/svelte/icons/plug";
  import { confirm } from "$lib/confirm.svelte";
  import { CONTROL, LOCAL, MODES, answers, fmt, nextMode, resultText, route, suggestionLabel, type Cmd, type Question, type Suggestion } from "$lib/chat.logic";

  let {
    id,
    cwd,
    visible,
    resume,
    onresume,
    onexit,
  }: { id: string; cwd: string; visible: boolean; resume?: string; onresume: (sid: string) => void; onexit: () => void } = $props();

  type Past = { id: string; title: string; modified: number };

  type Input = Record<string, any>;
  type Item =
    | { kind: "user"; text: string; imgs?: string[]; uuid?: string }
    | { kind: "text"; text: string }
    | { kind: "think"; text: string }
    | { kind: "tool"; id: string; name: string; detail: string; state: "run" | "ok" | "err"; input: Input; result?: string }
    | { kind: "ask"; rid: string; name: string; input: Input; suggestions?: Suggestion[]; answer?: string }
    | { kind: "question"; rid: string; input: Input; questions: Question[]; picks: string[][]; other: string[]; answer?: string }
    | { kind: "plan"; rid: string; input: Input; plan: string; answer?: string }
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
  // Berechtigungsmodus; kommt per initialize, Umschalt+Tab wechselt wie im Terminal.
  let mode = $state("default");
  // Modelle samt Effort-Stufen kommen per initialize; leer = unbekannt, bis system/init das Modell nennt.
  type Model = { value: string; displayName: string; resolvedModel?: string; supportedEffortLevels?: string[]; supportsFastMode?: boolean };
  let models = $state<Model[]>([]);
  let model = $state("");
  let effort = $state("");
  let fast = $state(false);
  const current = $derived(models.find((x) => x.value === model) ?? models[0]);
  const EFFORT: Record<string, string> = { low: "Niedrig", medium: "Mittel", high: "Hoch", xhigh: "Sehr hoch", max: "Max" };
  // Laeuft der Prozess mit --dangerously-skip-permissions; restarting schluckt das gewollte Ende beim Neustart.
  let bypassed = false;
  let restarting = false;
  let mcpOpen = $state(false);
  let modeKey = $state(0);
  // Zusatz zur "arbeitet"-Zeile, z. B. beim Komprimieren.
  let phase = $state("");
  // Live-Text aus stream_event je Nachricht:Index; die fertige assistant-Nachricht kommt danach nur noch fuer Tools.
  const live = new Map<string, { text: string }>();
  const streamed = new Set<string>();
  let log: HTMLDivElement;
  let content: HTMLDivElement;
  let field: HTMLTextAreaElement;
  // Nur mitscrollen, wenn der User unten ist; wer hochscrollt, liest in Ruhe.
  let stick = $state(true);
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
      mode = m.permissionMode ?? mode;
      model ||= models.find((x) => x.value !== "default" && x.resolvedModel === m.model)?.value ?? "";
      fast = m.fast_mode_state === "on";
    } else if (m.type === "system" && m.subtype === "status") {
      phase = m.status === "compacting" ? "Komprimiert den Verlauf …" : "";
    } else if (m.type === "system" && m.subtype === "compact_boundary") {
      note("Verlauf komprimiert");
    } else if (m.type === "system" && m.subtype === "api_retry") {
      note(`API-Fehler, neuer Versuch${m.attempt ? ` (${m.attempt}${m.max_retries ? `/${m.max_retries}` : ""})` : ""} …`, true);
    } else if (m.type === "stream_event") {
      const e = m.event;
      const key = `${m.api_message_id}:${e?.index}`;
      const type = e?.content_block?.type;
      if (e?.type === "content_block_start" && (type === "text" || type === "thinking")) {
        items.push({ kind: type === "text" ? "text" : "think", text: "" });
        live.set(key, items[items.length - 1] as { text: string });
        streamed.add(m.api_message_id);
      } else if (e?.type === "content_block_delta") {
        const it = live.get(key);
        if (it) it.text += e.delta?.text ?? e.delta?.thinking ?? "";
      }
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
      // Gestreamt: Text und Gedanken stehen schon da (Verlauf aus der jsonl kommt ohne Stream).
      const seen = streamed.has(m.message?.id);
      for (const b of m.message?.content ?? []) {
        if (b.type === "text" && b.text.trim() && !seen) items.push({ kind: "text", text: b.text });
        else if (b.type === "thinking" && b.thinking?.trim() && !seen) items.push({ kind: "think", text: b.thinking });
        else if (b.type === "tool_use") items.push({ kind: "tool", id: b.id, name: b.name, detail: detail(b.input), state: "run", input: b.input ?? {} });
      }
    } else if (m.type === "user" && Array.isArray(m.message?.content)) {
      for (const b of m.message.content) {
        const t = b.type === "tool_result" && items.find((i) => i.kind === "tool" && i.id === b.tool_use_id);
        if (t && t.kind === "tool") {
          t.state = b.is_error ? "err" : "ok";
          t.result = resultText(b.content);
        }
      }
    } else if (m.type === "control_request" && m.request?.subtype === "can_use_tool") {
      const r = m.request;
      const input = r.input ?? {};
      if (r.tool_name === "AskUserQuestion") items.push({ kind: "question", rid: m.request_id, input, questions: input.questions ?? [], picks: [], other: [] });
      else if (r.tool_name === "ExitPlanMode") items.push({ kind: "plan", rid: m.request_id, input, plan: String(input.plan ?? "") });
      else items.push({ kind: "ask", rid: m.request_id, name: r.tool_name, input, suggestions: r.permission_suggestions });
    } else if (m.type === "result") {
      busy = false;
      phase = "";
      live.clear();
      streamed.clear();
      // Abgebrochen oder Fehler: offene Abfragen sind hinfaellig.
      for (const i of items) if ((i.kind === "ask" || i.kind === "question" || i.kind === "plan") && !i.answer) i.answer = "Abgebrochen";
      if (m.is_error) items.push({ kind: "info", text: m.result || (m.errors ?? []).join("\n") || "Fehler", error: true });
    }
  }

  // Ausgabe von !-Befehlen: geht wie bei claude im Terminal als Kontext mit der naechsten Nachricht raus.
  let shell: string[] = [];
  // Wie claudes Bash-Modus: lange Ausgaben gekuerzt, damit der Kontext nicht volllaeuft.
  const cut = (t: string) => (t.length > 30000 ? `${t.slice(0, 30000)}\n… (gekürzt)` : t).trimEnd();

  async function bash(cmd: string) {
    try {
      const o = await invoke<{ stdout: string; stderr: string; code: number | null }>("chat_bash", { cwd, cmd });
      const out = [o.stdout, o.stderr].map(cut).filter(Boolean).join("\n");
      note(out || `(keine Ausgabe, Exit ${o.code ?? "?"})`, o.code !== 0);
      shell.push(`<bash-input>${cmd}</bash-input>\n<bash-stdout>${cut(o.stdout)}</bash-stdout><bash-stderr>${cut(o.stderr)}</bash-stderr>`);
    } catch (e) {
      note(String(e), true);
    }
  }

  function prompt(text: string, pics: string[] = [], uuid: string = crypto.randomUUID()) {
    if (shell.length) (text = [...shell, text].filter(Boolean).join("\n\n")), (shell = []);
    // data:<typ>;base64,<daten> -> Bild-Block der API
    const blocks = pics.map((u) => ({ type: "image", source: { type: "base64", media_type: u.slice(5, u.indexOf(";")), data: u.slice(u.indexOf(",") + 1) } }));
    const content = blocks.length ? [...blocks, ...(text ? [{ type: "text", text }] : [])] : text;
    // Eigene uuid: darauf kann rewind_conversation spaeter zurueckspulen.
    write({ type: "user", uuid, message: { role: "user", content } });
    busy = true;
  }

  // Ungetrimmt routen: wer Text mit "/" senden will, stellt ein Leerzeichen davor.
  function send() {
    const text = draft.trim();
    if ((!text && !imgs.length) || dead) return;
    // Headless gibt /mcp nur Text aus; hier gibt es die Verwaltung.
    if (text === "/mcp" && !imgs.length) {
      draft = "";
      mcpOpen = true;
      return;
    }
    // Mit Bildern ist es immer eine Nachricht, kein Command.
    const r = imgs.length ? null : route(draft, commands);
    const pics = imgs;
    items.push({ kind: "user", text, imgs: pics });
    const sent = items[items.length - 1] as Item & { kind: "user" };
    stick = true;
    draft = "";
    imgs = [];
    tick().then(grow);
    if (!pics.length && text.startsWith("!")) {
      if (text.slice(1).trim()) bash(text.slice(1).trim());
    } else if (r?.kind === "local") local(r.name, r.args);
    else if (r?.kind === "unknown") note(`/${r.name} gibt es hier nicht, /help zeigt alle`);
    else {
      sent.uuid = crypto.randomUUID();
      prompt(text, pics, sent.uuid);
    }
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
        mode = "plan";
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

  const respond = (rid: string, response: object) => write({ type: "control_response", response: { subtype: "success", request_id: rid, response } });

  /** always: die Vorschlaege von claude mitschicken (Regel oder Modus fuer die Sitzung), dann fragt es nicht mehr. */
  function answer(it: Item & { kind: "ask" }, allow: boolean, always = false) {
    it.answer = always ? `Erlaubt · ${suggestionLabel(it.suggestions ?? [])}` : allow ? "Erlaubt" : "Abgelehnt";
    const next = always ? it.suggestions?.find((s) => s.type === "setMode")?.mode : undefined;
    if (next) mode = next;
    respond(
      it.rid,
      allow
        ? { behavior: "allow", updatedInput: it.input, ...(always ? { updatedPermissions: it.suggestions } : {}) }
        : { behavior: "deny", message: "Vom Benutzer abgelehnt" },
    );
  }

  /** Eine Frage mit Einfachauswahl antwortet sofort, sonst sammeln bis "Antworten". */
  function choose(it: Item & { kind: "question" }, qi: number, label: string) {
    const q = it.questions[qi];
    const cur = it.picks[qi] ?? [];
    it.picks[qi] = q.multiSelect ? (cur.includes(label) ? cur.filter((x) => x !== label) : [...cur, label]) : [label];
    if (!q.multiSelect && it.questions.length === 1) reply(it);
  }

  function reply(it: Item & { kind: "question" }, skip = false) {
    if (skip) {
      it.answer = "Übersprungen";
      return respond(it.rid, { behavior: "deny", message: "Der Benutzer hat die Frage übersprungen" });
    }
    const a = answers(it.questions, it.picks, it.other);
    it.answer = Object.values(a).filter(Boolean).join(" · ") || "Keine Auswahl";
    respond(it.rid, { behavior: "allow", updatedInput: { ...it.input, answers: a } });
  }

  /** Plan annehmen und im gewaehlten Modus umsetzen; null = weiterplanen, Text im Eingabefeld geht als Feedback mit. */
  function approve(it: Item & { kind: "plan" }, next: string | null) {
    if (!next) {
      it.answer = "Weiter planen";
      respond(it.rid, { behavior: "deny", message: draft.trim() || "Der Benutzer möchte weiter planen." });
      draft = "";
      tick().then(grow);
      return;
    }
    it.answer = `Umsetzen · ${MODES.find(([, v]) => v === next)?.[0] ?? next}`;
    mode = next;
    respond(it.rid, { behavior: "allow", updatedInput: it.input, updatedPermissions: [{ type: "setMode", mode: next, destination: "session" }] });
  }

  /** Gespraech bis vor diese Nachricht zurueckspulen; Dateien bleiben, der Text kommt zurueck ins Eingabefeld. */
  async function rewind(i: number) {
    const it = items[i];
    if (it.kind !== "user" || !it.uuid || busy) return;
    try {
      const r = await control("rewind_conversation", { target_message_uuid: it.uuid });
      if (!r?.rewound) return note(`Zurückspulen nicht möglich${r?.error ? `: ${r.error}` : ""}`, true);
      items.splice(i);
      draft = r.prefillText ?? it.text;
      tick().then(grow);
      field?.focus();
    } catch (e) {
      note(String(e), true);
    }
  }

  function pickModel(v: string) {
    const prev = model;
    model = v;
    control("set_model", { model: v }).catch((e) => ((model = prev), note(String(e), true)));
  }

  function pickEffort(v: string) {
    const prev = effort;
    effort = v;
    control("apply_flag_settings", { settings: { effortLevel: v } }).catch((e) => ((effort = prev), note(String(e), true)));
  }

  function toggleFast() {
    fast = !fast;
    control("apply_flag_settings", { settings: { fastMode: fast } }).catch((e) => ((fast = !fast), note(String(e), true)));
  }

  /** Eingabe eines Tools lesbar: Befehl, Diff oder Inhalt statt JSON, wo es das gibt. */
  function toolInput(it: Item & { kind: "tool" }) {
    const i = it.input;
    if (typeof i.command === "string") return i.command;
    if (typeof i.old_string === "string") return `- ${i.old_string.replaceAll("\n", "\n- ")}\n+ ${String(i.new_string ?? "").replaceAll("\n", "\n+ ")}`;
    if (typeof i.content === "string") return i.content;
    return JSON.stringify(i, null, 2);
  }

  async function setMode(next: string) {
    // Alles erlauben braucht einen Neustart mit Flag; gilt nur fuer diesen Prozess, die naechste Sitzung startet wieder normal.
    if (next === "bypassPermissions" && !bypassed) {
      // Das Dropdown zeigt die Wahl schon an; ohne Neustart wieder auf den echten Modus zuruecksetzen.
      modeKey++;
      if (busy) return note("Erst warten oder abbrechen, dann Alles erlauben einschalten", true);
      const ok = await confirm(
        "Claude führt dann jeden Befehl und jede Dateiänderung ohne Rückfrage aus. Die Sitzung startet dafür neu (der Verlauf bleibt). Gilt nur für diese Sitzung, beim nächsten Start ist es wieder aus.",
        "Alles erlauben",
        "Ohne Rückfragen arbeiten?",
      );
      if (ok) relaunch(true);
      return;
    }
    const prev = mode;
    mode = next;
    control("set_permission_mode", { mode: next }).catch((e) => ((mode = prev), note(String(e), true)));
  }

  /** Erste Infos von claude: Commands, Modus, Modelle. Nach jedem (Neu-)Start. */
  function init() {
    return control("initialize").then(
      (r) => {
        commands = r?.commands ?? [];
        mode = r?.current_permission_mode ?? mode;
        models = r?.models ?? [];
        fast = r?.fast_mode_state === "on";
      },
      () => {},
    );
  }

  /** claude neu starten und die Session fortsetzen (Bypass an/aus, neue MCP-Server). Der Verlauf hier bleibt stehen. */
  async function relaunch(bypass = bypassed) {
    if (busy) return note("Erst warten oder abbrechen", true);
    // Das Ende des alten Prozesses ist gewollt, nicht als "beendet" melden.
    restarting = !dead;
    booting = true;
    await invoke("chat_close", { id }).catch(() => {});
    try {
      await invoke("chat_open", { id, cwd, resume: sid ?? undefined, bypass });
      bypassed = bypass;
      dead = false;
      await init();
      // Modell und Effort gelten nur pro Prozess: gewaehlte Werte wieder setzen.
      if (model) await control("set_model", { model }).catch(() => {});
      if (effort) await control("apply_flag_settings", { settings: { effortLevel: effort } }).catch(() => {});
      note(bypass ? "Neu gestartet, Alles erlauben ist an" : "Neu gestartet");
    } catch (e) {
      restarting = false;
      dead = true;
      note(String(e), true);
    }
    booting = false;
  }

  function bottom() {
    stick = true;
    log.scrollTop = log.scrollHeight;
  }

  const stop = () => write({ type: "control_request", request_id: crypto.randomUUID(), request: { subtype: "interrupt" } });

  function pick(name: string) {
    draft = `/${name} `;
    field?.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    const m = matches[sel];
    if (e.key === "Tab" && e.shiftKey) {
      e.preventDefault();
      if (!dead) setMode(nextMode(mode));
    } else if (m && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
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

  // Waechst der Verlauf (neue Nachricht, Markdown, Bilder, Tool-Status), unten bleiben.
  $effect(() => {
    const ro = new ResizeObserver(() => stick && (log.scrollTop = log.scrollHeight));
    ro.observe(content);
    return () => ro.disconnect();
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
        if (restarting) return void (restarting = false);
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
        init();
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
      if (text || pics.length) items.push({ kind: "user", text: text || "", imgs: pics, uuid: m.uuid });
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
      class="h-full overflow-y-auto px-4 py-3 text-sm"
    >
      <div bind:this={content} class="flex flex-col gap-3">
      {#each items as it, i (i)}
        {#if it.kind === "user"}
          <div class="group flex max-w-[85%] items-start gap-1 self-end">
          {#if it.uuid && !busy && !dead}
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground mt-2 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
              title="Bis vor diese Nachricht zurückspulen (Dateien bleiben, der Text kommt ins Eingabefeld)"
              aria-label="Zurückspulen"
              onclick={() => rewind(i)}><Undo2Icon class="size-3.5" /></button
            >
          {/if}
          <div class="bg-primary/15 flex min-w-0 flex-col gap-2 rounded-lg px-3 py-2">
            {#if it.imgs?.length}
              <div class="flex flex-wrap justify-end gap-1">
                {#each it.imgs as u, j (j)}<img src={u} alt="Bild {j + 1}" class="max-h-40 rounded" />{/each}
              </div>
            {/if}
            {#if it.text}<p class="whitespace-pre-wrap select-text">{it.text}</p>{/if}
          </div>
          </div>
        {:else if it.kind === "text"}
          {#if it.text.trim()}<Markdown text={it.text} root={cwd} size="text-sm" />{/if}
        {:else if it.kind === "think"}
          {#if it.text.trim()}
            <details class="text-muted-foreground text-xs">
              <summary class="cursor-pointer select-none">Gedanken</summary>
              <p class="mt-1 border-l-2 pl-2 whitespace-pre-wrap select-text">{it.text}</p>
            </details>
          {/if}
        {:else if it.kind === "tool"}
          <details class="min-w-0">
            <summary class="text-muted-foreground flex min-w-0 cursor-pointer list-none items-center gap-2 font-mono text-xs">
              <span class={it.state === "err" ? "text-destructive" : it.state === "ok" ? "text-green-500" : "text-primary animate-pulse"}
                >{it.state === "err" ? "✗" : it.state === "ok" ? "✓" : "●"}</span
              >
              <span class="text-foreground shrink-0">{it.name}</span>
              <span class="truncate" title={it.detail}>{it.detail}</span>
            </summary>
            <div class="mt-1 ml-4 flex flex-col gap-1">
              <pre class="bg-muted max-h-60 overflow-auto rounded p-2 font-mono text-xs whitespace-pre-wrap select-text">{toolInput(it)}</pre>
              {#if it.result}
                <pre
                  class="bg-muted max-h-60 overflow-auto rounded p-2 font-mono text-xs whitespace-pre-wrap select-text {it.state === 'err'
                    ? 'text-destructive'
                    : 'text-muted-foreground'}">{it.result.length > 5000 ? `${it.result.slice(0, 5000)}\n… (gekürzt)` : it.result}</pre>
              {/if}
            </div>
          </details>
        {:else if it.kind === "ask"}
          <div class="border-primary/40 rounded-lg border px-3 py-2">
            <p class="mb-1 text-xs font-medium">Claude möchte <span class="font-mono">{it.name}</span> ausführen</p>
            <pre class="bg-muted max-h-40 overflow-auto rounded p-2 font-mono text-xs whitespace-pre-wrap select-text">{detail(it.input) ||
                JSON.stringify(it.input, null, 2)}</pre>
            {#if it.answer}
              <p class="text-muted-foreground mt-1 text-xs">{it.answer}</p>
            {:else}
              <div class="mt-2 flex flex-wrap gap-2">
                <Button size="sm" onclick={() => answer(it, true)}>Erlauben</Button>
                {#if it.suggestions?.length}
                  <Button size="sm" variant="secondary" onclick={() => answer(it, true, true)}>{suggestionLabel(it.suggestions)}</Button>
                {/if}
                <Button size="sm" variant="outline" onclick={() => answer(it, false)}>Ablehnen</Button>
              </div>
            {/if}
          </div>
        {:else if it.kind === "question"}
          <div class="border-primary/40 flex flex-col gap-3 rounded-lg border px-3 py-2">
            {#each it.questions as q, qi (qi)}
              <div class="flex flex-col gap-1.5">
                <p class="text-xs font-medium">
                  {#if q.header}<span class="text-muted-foreground">{q.header} · </span>{/if}{q.question}
                </p>
                <div class="flex flex-wrap gap-1.5">
                  {#each q.options as o (o.label)}
                    <Button
                      size="sm"
                      variant={it.picks[qi]?.includes(o.label) ? "default" : "outline"}
                      title={o.description}
                      disabled={!!it.answer}
                      onclick={() => choose(it, qi, o.label)}>{o.label}</Button
                    >
                  {/each}
                </div>
                {#if !it.answer}
                  <input
                    bind:value={it.other[qi]}
                    placeholder="Andere Antwort …"
                    class="border-border placeholder:text-muted-foreground focus:border-primary/60 rounded border bg-transparent px-2 py-1 text-xs outline-none"
                    onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), reply(it))}
                  />
                {/if}
              </div>
            {/each}
            {#if it.answer}
              <p class="text-muted-foreground text-xs">Antwort: {it.answer}</p>
            {:else}
              <div class="flex gap-2">
                <Button size="sm" onclick={() => reply(it)}>Antworten</Button>
                <Button size="sm" variant="outline" onclick={() => reply(it, true)}>Überspringen</Button>
              </div>
            {/if}
          </div>
        {:else if it.kind === "plan"}
          <div class="border-primary/40 rounded-lg border px-3 py-2">
            <p class="mb-1 text-xs font-medium">Plan</p>
            <Markdown text={it.plan} root={cwd} size="text-sm" />
            {#if it.answer}
              <p class="text-muted-foreground mt-1 text-xs">{it.answer}</p>
            {:else}
              <div class="mt-2 flex flex-wrap gap-2">
                <Button size="sm" onclick={() => approve(it, "auto")}>Umsetzen (Auto)</Button>
                <Button size="sm" variant="outline" onclick={() => approve(it, "acceptEdits")}>Umsetzen, Edits annehmen</Button>
                <Button size="sm" variant="outline" onclick={() => approve(it, "default")}>Umsetzen, manuell</Button>
                <Button size="sm" variant="ghost" title="Feedback vorher ins Eingabefeld schreiben, es geht mit" onclick={() => approve(it, null)}
                  >Weiter planen</Button
                >
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
        <p class="text-muted-foreground text-xs">{phase || "Claude arbeitet …"} <span class="opacity-60">(Esc bricht ab)</span></p>
      {/if}
      </div>
    </div>
    {#if !stick}
      <Button
        size="icon"
        variant="secondary"
        class="absolute right-3 bottom-3 size-8 rounded-full shadow-lg"
        onclick={bottom}
        title="Ganz nach unten"
        aria-label="Ganz nach unten"><ArrowDownIcon /></Button
      >
    {/if}
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
  <div class="text-muted-foreground flex shrink-0 items-center gap-2 px-1 text-xs">
    {#key modeKey}
      <Select
        value={mode}
        options={[...MODES.map(([label, v]) => [label, v] as const), ["Alles erlauben", "bypassPermissions"] as const]}
        onchange={setMode}
        disabled={dead}
        class={mode === "bypassPermissions" ? "border-destructive/60 text-destructive" : ""}
        title={mode === "bypassPermissions" ? "Keine Rückfragen, nur für diese Sitzung" : MODES.find(([, v]) => v === mode)?.[2]}
        aria-label="Berechtigungsmodus"
      />
    {/key}
    <button
      type="button"
      class="border-border hover:text-foreground flex h-6 items-center gap-1 rounded border px-1.5"
      disabled={dead}
      title="MCP-Server (/mcp)"
      onclick={() => (mcpOpen = true)}><PlugIcon class="size-3" /> MCP</button
    >
    {#if models.length}
      <Select
        value={model}
        options={models.map((x) => [x.displayName, x.value] as const)}
        onchange={pickModel}
        disabled={dead}
        placeholder="Modell"
        title="Modell für diese Sitzung"
        aria-label="Modell"
      />
    {/if}
    {#if current?.supportedEffortLevels?.length}
      <Select
        value={effort}
        options={current.supportedEffortLevels.map((v) => [EFFORT[v] ?? v, v] as const)}
        onchange={pickEffort}
        disabled={dead}
        placeholder="Effort"
        title="Wie gründlich Claude nachdenkt"
        aria-label="Effort"
      />
    {/if}
    {#if current?.supportsFastMode}
      <button
        type="button"
        class="flex h-6 items-center gap-1 rounded border px-1.5 {fast ? 'border-primary/60 text-primary' : 'border-border hover:text-foreground'}"
        aria-pressed={fast}
        disabled={dead}
        title="Fast-Modus: schnellere Antworten, kostet mehr"
        onclick={toggleFast}><ZapIcon class="size-3" /> Fast</button
      >
    {/if}
    <span class="opacity-60">Umschalt+Tab wechselt den Modus</span>
  </div>
</div>
<McpDialog bind:open={mcpOpen} {cwd} {control} onrestart={() => relaunch()} />
