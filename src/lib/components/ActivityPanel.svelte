<script lang="ts" module>
  // Bilddaten sind gross und unveraenderlich: einmal laden, fuer alle Panels behalten.
  // Schluessel `${session}:${id}` — ids sind nur innerhalb einer Session eindeutig.
  const images = new Map<string, string>();
</script>

<script lang="ts">
  import { confirm } from "$lib/confirm.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import Markdown from "$lib/components/Markdown.svelte";
  import ZoomImage from "$lib/components/ZoomImage.svelte";
  import { Tabs } from "$lib/components/kit";
  import { age } from "$lib/utils";
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import PlayIcon from "@lucide/svelte/icons/play";
  import PencilIcon from "@lucide/svelte/icons/pencil";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import WorkflowDialog from "$lib/components/WorkflowDialog.svelte";
  import { ptyWrite } from "$lib/files";
  import { fromScript, metaOf, type WfFile } from "$lib/workflow.logic";

  type Step = { at: number; tool: string; summary: string };
  type Agent = {
    id: string;
    label: string;
    phase: string;
    state: string;
    started: number;
    last_at: number;
    tokens: number;
    tool_calls: number;
    steps: Step[];
    text: string | null;
    result: string | null;
  };
  type Run = {
    id: string;
    name: string;
    status: string;
    started: number;
    duration_ms: number;
    tokens: number;
    phases: string[];
    agents: Agent[];
  };
  type Img = { id: number; at: number; kind: string; label: string };
  type Entry = { at: number; kind: "prompt" | "text" | "tool"; tool: string; summary: string; body: string; output: string | null; error: boolean };
  // Kopie aus src-tauri/mods/open-claude/types/index.d.ts (Import scheitert an `declare module 'claude-code'`).
  type Limit = { kind: string; percentUsed: number; resetsAt?: string };
  type Panel = {
    startedAt: number;
    context: { tokens: number | null; window: number; percent: number | null };
    compactLeft: number | null;
    limits: Limit[];
    cost: number | null;
    model: string;
    effort: string | null;
    tokens: { input: number; output: number; cacheRead: number; cacheWrite: number };
    turns: number;
    active: { id: string; tool: string; target: string; state: "run" | "ok" | "err" }[];
    todos: { content: string; status: string }[];
  };
  type Mode = { name: string; level: string };
  type Activity = { session: string | null; runs: Run[]; images: Img[]; panel: Panel | null; modes: Mode[] };

  let { cwd, id }: { cwd: string; id: string } = $props();

  let data = $state<Activity | null>(null);
  let error = $state("");
  let tplError = $state("");
  let tab = $state<"session" | "wf" | "img">("session");
  // Session-Daten der Mod im Terminal; Countdowns rechnen gegen now.
  let panel = $state<Panel | null>(null);
  let now = $state(Date.now());
  // Offen/zu je Run; ohne Eintrag sind laufende Runs offen.
  let openRuns = $state<Record<string, boolean>>({});
  // Geoeffneter Agent: volle Zeitleiste statt Liste.
  let focus = $state<{ run: string; agent: string } | null>(null);
  let log = $state<Entry[] | null>(null);
  let scroller = $state<HTMLElement>();
  let urls = $state<Record<string, string>>({});
  // Bild-id in der Lightbox (nicht Index: neue Bilder verschieben die Liste)
  let shown = $state<number | null>(null);
  // Workflow-Vorlagen (global + Projekt); Claude kann neue schreiben, darum mitpollen
  let templates = $state<WfFile[]>([]);
  let wfOpen = $state(false);
  let wfEntry = $state<WfFile | null>(null);

  const STATUS: Record<string, [string, string, string]> = {
    running: ["◐", "text-primary animate-pulse", "läuft"],
    done: ["✓", "text-success", "fertig"],
    completed: ["✓", "text-success", "fertig"],
    error: ["✕", "text-destructive", "Fehler"],
    failed: ["✕", "text-destructive", "Fehler"],
    aborted: ["⊘", "text-muted-foreground", "abgebrochen"],
    killed: ["⊘", "text-muted-foreground", "abgebrochen"],
  };
  const status = (s: string) => STATUS[s] ?? ["○", "text-muted-foreground", s];

  const live = $derived((data?.runs.some((r) => r.status === "running") || panel?.active.some((a) => a.state === "run")) ?? false);
  // Neueste Bilder zuerst
  const imgs = $derived([...(data?.images ?? [])].reverse());

  let busy = false;
  async function poll() {
    if (busy || document.visibilityState !== "visible") return;
    busy = true;
    const c = cwd,
      i = id;
    try {
      const d = await invoke<Activity>("claude_activity", { cwd: c, id: i });
      // Halb geschriebene Datei liefert null: letzten Wert behalten.
      if (c === cwd && i === id) (data = d), (panel = d.panel ?? panel), (now = Date.now()), (error = "");
      // Laufender Agent: Zeitleiste mitziehen; fertiger wird nur einmal geladen.
      if (focus && (!log || focused?.a.state === "running")) loadLog();
      if (tab === "wf") loadTemplates();
    } catch (e) {
      if (c === cwd && i === id) error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    const t = setInterval(poll, 1500);
    return () => clearInterval(t);
  });

  $effect(() => {
    void cwd;
    void id;
    data = null;
    panel = null;
    urls = {};
    shown = null;
    focus = null;
    log = null;
    templates = [];
    tplError = "";
    busy = false; // alte Anfrage wird per c !== cwd verworfen
    poll();
  });

  async function loadTemplates() {
    const c = cwd;
    try {
      const l = await invoke<WfFile[]>("workflows_list", { cwd: c });
      if (c === cwd) (templates = l), (tplError = "");
    } catch (e) {
      if (c === cwd) tplError = String(e);
    }
  }
  $effect(() => {
    if (tab === "wf") loadTemplates();
  });

  const tpls = $derived(
    templates.map((f) => {
      const b = fromScript(f.text);
      const m = b ? null : metaOf(f.text);
      return { f, builder: !!b, name: b?.name ?? m?.name ?? f.file, description: b?.description ?? m?.description ?? "" };
    }),
  );
  const shadowed = (scope: string, name: string) => scope === "project" && tpls.some((x) => x.f.scope === "user" && x.name === name);

  function editTemplate(f: WfFile | null) {
    wfEntry = f;
    wfOpen = true;
  }
  async function deleteTemplate(f: WfFile, name: string) {
    if (!(await confirm(`Vorlage „${name}“ löschen?`, "Löschen"))) return;
    try {
      await invoke("workflows_delete", { cwd, scope: f.scope, file: f.file });
      loadTemplates();
    } catch (e) {
      tplError = String(e);
    }
  }
  // Ohne Enter: Args dahinter tippen, dann selbst abschicken; startet nie versehentlich einen teuren Lauf
  const startTemplate = (f: WfFile) =>
    ptyWrite(id, `Fuehre den Workflow mit dem Workflow-Tool aus, scriptPath: ${JSON.stringify(f.path)}. Args: `).catch((e) => (tplError = String(e)));

  const focused = $derived.by(() => {
    const r = data?.runs.find((r) => r.id === focus?.run);
    const a = r?.agents.find((a) => a.id === focus?.agent);
    return r && a ? { r, a } : null;
  });

  async function loadLog() {
    const f = focus,
      sid = data?.session;
    if (!f || !sid) return;
    try {
      const l = await invoke<Entry[]>("claude_agent_log", { cwd, sid, run: f.run, agent: f.agent });
      if (f !== focus) return;
      // Nur mitscrollen, wenn man schon unten war; sonst springt es beim Lesen weg.
      const el = scroller;
      const stick = !!log && !!el && el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      log = l;
      if (stick) requestAnimationFrame(() => el!.scrollTo({ top: el!.scrollHeight }));
    } catch (e) {
      if (f === focus) (log = []), (error = String(e));
    }
  }

  function openAgent(r: Run, a: Agent) {
    focus = { run: r.id, agent: a.id };
    log = null;
    scroller?.scrollTo({ top: 0 });
    loadLog();
  }

  const key = (id: number) => `${data?.session}:${id}`;
  async function load(id: number) {
    const sid = data?.session;
    if (!sid) return;
    const k = `${sid}:${id}`;
    if (images.has(k)) return void (urls[k] = images.get(k)!);
    try {
      const url = await invoke<string>("claude_image", { sid, id });
      images.set(k, url);
      if (sid === data?.session) urls[k] = url;
    } catch {}
  }

  // Thumbnail erst laden, wenn es in Sicht kommt.
  function lazy(node: HTMLElement, id: number) {
    const io = new IntersectionObserver((es) => {
      if (es.some((e) => e.isIntersecting)) {
        load(id);
        io.disconnect();
      }
    });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }

  const tokens = (n: number) => (n < 1000 ? String(n) : n < 1e6 ? `${Math.round(n / 1000)}k` : `${(n / 1e6).toFixed(1)}M`);
  function dur(ms: number) {
    const s = Math.max(0, Math.round(ms / 1000));
    return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m`;
  }
  const clock = (ms: number) => new Date(ms).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const ago = (ms: number) => (ms ? age(new Date(ms).toISOString()) : "");

  // Wie register.tsx der Mod: <50 primary, <80 gelb, sonst rot.
  const HOUR = 3_600_000;
  const WIN = { five_hour: ["Session 5h", 5 * HOUR], seven_day: ["Woche 7d", 168 * HOUR] } as const;
  const DAYS = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
  const lvl = (p: number) => (p < 50 ? "bg-primary" : p < 80 ? "bg-warning" : "bg-destructive");
  const txt = (p: number) => (p < 50 ? "text-primary" : p < 80 ? "text-warning" : "text-destructive");
  // Prognose in % bei Reset (> 100 = zu schnell); undefined, solange das Fenster nicht laeuft.
  const pace = (used: number, iso: string | undefined, ms: number) => {
    const f = (now - (Date.parse(iso ?? "") - ms)) / ms;
    // Nach dem Reset ohne neue Daten waere die Prognose Unsinn.
    return Number.isNaN(f) || f <= 0 || f > 1 ? undefined : used / f;
  };
  const hm = (d: Date) => d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  const mins = (ms: number) => {
    const m = Math.max(0, Math.floor(ms / 60_000));
    return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
  };
  const reset = (iso?: string) => {
    const at = new Date(iso ?? "");
    if (Number.isNaN(at.getTime())) return "";
    const left = at.getTime() - now;
    if (left <= 0) return "Reset erfolgt – aktualisiert mit nächster Antwort";
    return left < 24 * HOUR ? `Reset ${hm(at)} · noch ${mins(left)}` : `Reset ${DAYS[at.getDay()]} ${hm(at)}`;
  };
  function details(s: Panel): [string, string][] {
    const t = s.tokens;
    const seen = t.input + t.cacheRead + t.cacheWrite;
    const hours = (now - s.startedAt) / HOUR;
    const rows: [string, unknown][] = [
      ["Modell", s.model && [s.model.replace(/^claude-/, ""), s.effort].filter(Boolean).join(" · ")],
      // Im Abo bleibt cost 0, dann sagt die Zeile nichts.
      ["Kosten", !!s.cost && `$${s.cost.toFixed(2)}${hours > 0.05 ? ` · $${(s.cost / hours).toFixed(2)}/h` : ""}`],
      ["Tokens", !!(seen || t.output) && `${tokens(seen)} in · ${tokens(t.output)} out`],
      ["Cache", !!seen && `${Math.round((t.cacheRead / seen) * 100)}% Hit`],
      ["Dauer", !!s.startedAt && `${mins(now - s.startedAt)} · ${s.turns} Turns`],
    ];
    return rows.filter((r): r is [string, string] => typeof r[1] === "string" && !!r[1]);
  }
  const ACT = { run: "running", ok: "done", err: "error" } as const;

  const phaseState = (r: Run, p: string) =>
    r.agents.some((a) => a.phase === p && a.state === "running")
      ? "active"
      : r.agents.some((a) => a.phase === p) && r.agents.filter((a) => a.phase === p).every((a) => a.state === "done")
        ? "done"
        : "open";

  const agentDur = (r: Run, a: Agent) => (a.started ? (a.state === "running" ? Date.now() : a.last_at) - a.started : 0);

  const index = $derived(imgs.findIndex((m) => m.id === shown));
  const current = $derived(index < 0 ? null : imgs[index]);
  function step(d: number) {
    if (index < 0) return;
    shown = imgs[(index + d + imgs.length) % imgs.length].id;
    load(shown);
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowLeft") step(-1);
    else if (e.key === "ArrowRight") step(1);
    else return;
    e.preventDefault();
  }
</script>

<div class="bg-card flex h-full flex-col overflow-hidden rounded-xl text-xs ring-1 ring-foreground/5">
  <Tabs
    bind:value={tab}
    tabs={[
      { id: "session", label: "Session" },
      { id: "wf", label: "Workflows" },
      { id: "img", label: "Bilder", count: imgs.length },
    ]}
  >
    {#if live}
      <span class="text-muted-foreground ml-auto flex items-center gap-1.5 pr-1 text-[11px]">
        <span class="bg-primary size-1.5 animate-pulse rounded-full"></span>live
      </span>
    {/if}
  </Tabs>

  {#if tab === "wf" && focus}
    <!-- Ausserhalb des Scrollbereichs: sticky lag dort ueber breitem Inhalt. -->
    <div class="border-border shrink-0 border-b px-2 py-1.5 text-xs">
      <button class="text-muted-foreground hover:text-foreground flex max-w-full items-center gap-0.5" onclick={() => (focus = null)}>
        <ChevronLeftIcon class="size-3.5 shrink-0" /><span class="truncate">{focused?.r.name ?? "Zurück"}</span>
      </button>
      {#if focused}
        {@const { r, a } = focused}
        {@const [ai, ac, at] = status(a.state)}
        <div class="mt-1 flex items-center gap-2">
          <span class={ac} title={at}>{ai}</span>
          <span class="min-w-0 flex-1 truncate font-medium">{a.label || a.id}</span>
        </div>
        <div class="text-muted-foreground mt-0.5 truncate pl-5 tabular-nums">
          {[at, a.phase, dur(agentDur(r, a)), `${a.tool_calls} Tools`, a.tokens ? tokens(a.tokens) : ""].filter(Boolean).join(" · ")}
        </div>
      {/if}
    </div>
  {/if}

  <div bind:this={scroller} class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-2 text-xs">
    {#if tab === "wf" && !focus}
      <!-- Vor der if-Kette: auch ohne laufende Session sichtbar -->
      <section class="border-border/60 mb-2 rounded-lg border">
        <div class="flex items-center justify-between px-2 py-1">
          <span class="text-muted-foreground">Vorlagen</span>
          <button class="text-muted-foreground hover:text-foreground" onclick={() => editTemplate(null)}>+ Neu</button>
        </div>
        {#each tpls as x (x.f.path)}
          <div class="border-border/60 flex items-center gap-1.5 border-t px-2 py-1">
            <span class="max-w-32 shrink-0 truncate font-medium" title={x.f.path}>{x.name}</span>
            <span class="text-muted-foreground shrink-0 text-[10px]">{x.f.scope === "user" ? "Global" : "Projekt"}</span>
            {#if !x.builder}<span class="bg-secondary shrink-0 rounded px-1 text-[10px]" title="Freies Skript: nur starten und löschen">Skript</span>{/if}
            {#if shadowed(x.f.scope, x.name)}<span class="text-warning shrink-0 text-[10px]">Projekt überschreibt Global</span>{/if}
            <span class="text-muted-foreground min-w-0 flex-1 truncate" title={x.description}>{x.description}</span>
            <button
              class="hover:text-foreground text-muted-foreground disabled:opacity-40"
              disabled={!data?.session}
              title={data?.session ? "In die Claude-Session schreiben (Enter selbst drücken)" : "Keine laufende Claude-Session"}
              aria-label="Starten"
              onclick={() => startTemplate(x.f)}><PlayIcon class="size-3.5" /></button
            >
            <button
              class="hover:text-foreground text-muted-foreground disabled:invisible"
              disabled={!x.builder}
              aria-label="Bearbeiten"
              onclick={() => editTemplate(x.f)}><PencilIcon class="size-3.5" /></button
            >
            <button
              class="hover:text-destructive text-muted-foreground disabled:opacity-40"
              disabled={!/^[A-Za-z0-9_-]+$/.test(x.f.file)}
              aria-label="Löschen"
              onclick={() => deleteTemplate(x.f, x.name)}><Trash2Icon class="size-3.5" /></button
            >
          </div>
        {:else}
          {#if !tplError}<p class="text-muted-foreground border-border/60 border-t px-2 py-1">Noch keine. Oder Claude sagen: „erstell einen Workflow für … und speicher ihn als Vorlage“.</p>{/if}
        {/each}
        {#if tplError}<p class="text-destructive border-border/60 border-t px-2 py-1">{tplError}</p>{/if}
      </section>
    {/if}
    {#if error}
      <p class="text-destructive p-2">{error}</p>
    {:else if !data}
      <p class="text-muted-foreground p-2">Lade …</p>
    {:else if tab === "session"}
      {#if !panel}
        <p class="text-muted-foreground p-2">Noch keine Session-Daten – wartet auf Claude …</p>
      {:else}
        {@const ctx = panel.context}
        {#if ctx.window > 0}
          {@const p = ctx.percent ?? ((ctx.tokens ?? 0) / ctx.window) * 100}
          {@const sub = [
            ctx.tokens === null ? "" : `${tokens(ctx.tokens)} / ${tokens(ctx.window)}`,
            panel.compactLeft === null ? "" : `Compact in ${tokens(Math.max(0, panel.compactLeft))}`,
          ]
            .filter(Boolean)
            .join(" · ")}
          <section class="border-border/60 mb-2 rounded-lg border p-2">
            {@render gauge("Kontext", p, sub)}
          </section>
        {/if}
        {@const lims = (["five_hour", "seven_day"] as const).flatMap((k) => panel!.limits.filter((l) => l.kind === k).slice(0, 1).map((l) => [k, l] as const))}
        {#if lims.length}
          <section class="border-border/60 mb-2 divide-y divide-foreground/5 rounded-lg border">
            {#each lims as [k, l] (k)}
              {@const [label, ms] = WIN[k]}
              {@const f = k === "five_hour" ? pace(l.percentUsed, l.resetsAt, ms) : undefined}
              <div class="p-2">
                {@render gauge(label, l.percentUsed, reset(l.resetsAt))}
                {#if f !== undefined}
                  <div class="mt-0.5 {f > 100 ? 'text-warning' : 'text-muted-foreground'}">
                    {f > 100 ? "◆ zu schnell → 100% vor Reset" : `◇ im Plan → ~${Math.round(f)}% bei Reset`}
                  </div>
                {/if}
              </div>
            {/each}
          </section>
        {/if}
        {@const rows = details(panel)}
        {#if rows.length}
          <section class="border-border/60 mb-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 rounded-lg border p-2">
            {#each rows as [k, v] (k)}
              <span class="text-muted-foreground">{k}</span><span class="truncate tabular-nums" title={v}>{v}</span>
            {/each}
          </section>
        {/if}
        {#if data?.modes.length}
          <!-- Plugin-Modi (caveman, ponytail, ...): eigene kleine Karte unter den Details -->
          <section class="border-border/60 mb-2 rounded-lg border p-2">
            <div class="text-muted-foreground mb-1">Modi</div>
            <div class="flex flex-wrap gap-1">
              {#each data.modes as m (m.name)}
                <span class="bg-primary/10 text-primary rounded-full px-2 py-px" title="Plugin-Modus {m.name}"
                  >{m.name}{#if m.level}<span class="text-primary/70">&nbsp;· {m.level}</span>{/if}</span
                >
              {/each}
            </div>
          </section>
        {/if}
        {#if panel.active.length}
          <section class="border-border/60 mb-2 rounded-lg border p-2">
            <div class="text-muted-foreground mb-1">Aktiv</div>
            {#each panel.active as a, i (i)}
              {@const [icon, color, text] = status(ACT[a.state])}
              <div class="flex items-center gap-2">
                <span class={color} title={text}>{icon}</span>
                <span class="max-w-24 min-w-10 shrink-0 truncate" title={a.tool}>{a.tool.replace(/^mcp__.*?__/, "")}</span>
                <span class="text-muted-foreground min-w-0 flex-1 truncate font-mono text-[11px]" title={a.target}>{a.target}</span>
              </div>
            {/each}
          </section>
        {/if}
        {#if panel.todos.length}
          <section class="border-border/60 mb-2 rounded-lg border p-2">
            <div class="text-muted-foreground mb-1 flex justify-between">
              <span>Todos</span><span class="tabular-nums">{panel.todos.filter((t) => t.status === "completed").length}/{panel.todos.length}</span>
            </div>
            {#each panel.todos as t, i (i)}
              <div class="flex gap-2">
                {#if t.status === "completed"}
                  <span class="text-success">✓</span><span class="text-muted-foreground line-through">{t.content}</span>
                {:else if t.status === "in_progress"}
                  <span class="text-primary">◐</span><span>{t.content}</span>
                {:else}
                  <span class="text-muted-foreground">○</span><span>{t.content}</span>
                {/if}
              </div>
            {/each}
          </section>
        {/if}
      {/if}
    {:else if !data.session}
      <p class="text-muted-foreground p-2">Keine laufende Claude-Session gefunden.</p>
    {:else if tab === "wf" && focus}
      {#if !focused}
        <p class="text-muted-foreground p-2">Agent nicht mehr in der Liste.</p>
      {:else if !log}
        <p class="text-muted-foreground p-2">Lade Verlauf …</p>
      {:else}
        {@const a = focused.a}
        <ol class="space-y-1">
          {#each log as e, i (i)}
            <li>
              {#if e.kind === "prompt"}
                <details class="bg-secondary/50 rounded-md px-2 py-1">
                  <summary class="text-muted-foreground cursor-pointer">Auftrag</summary>
                  <pre class="mt-1 max-h-64 overflow-auto font-mono text-[11px] whitespace-pre-wrap">{e.body}</pre>
                </details>
              {:else if e.kind === "text"}
                <!-- Breite Tabellen/Code scrollen hier, nicht das ganze Panel unter die Kopfzeile. -->
                <div class="border-primary/40 min-w-0 overflow-x-auto border-l-2 py-0.5 pl-2"><Markdown text={e.body} /></div>
              {:else}
                {@const [si, sc, st] =
                  e.output !== null
                    ? e.error
                      ? ["✕", "text-destructive", "Fehler"]
                      : ["✓", "text-success", "fertig"]
                    : a.state === "running"
                      ? ["◐", "text-primary animate-pulse", "läuft"]
                      : ["⊘", "text-muted-foreground", "ohne Ergebnis"]}
                <details class="hover:bg-accent/40 open:bg-accent/30 rounded-md">
                  <summary class="flex cursor-pointer list-none items-center gap-1.5 px-1 py-0.5 font-mono text-[11px]">
                    <span class="text-muted-foreground shrink-0 tabular-nums">{e.at ? clock(e.at) : ""}</span>
                    <span class="shrink-0 {sc}" title={st}>{si}</span>
                    <span class="text-foreground shrink-0">{e.tool}</span>
                    <span class="text-muted-foreground min-w-0 truncate">{e.summary}</span>
                  </summary>
                  <div class="space-y-1 px-1 pb-1.5">
                    <pre class="bg-background/60 max-h-48 overflow-auto rounded p-1.5 font-mono text-[11px] whitespace-pre-wrap">{e.body}</pre>
                    {#if e.output !== null}
                      <pre
                        class="bg-background/60 max-h-64 overflow-auto rounded p-1.5 font-mono text-[11px] whitespace-pre-wrap {e.error
                          ? 'text-destructive'
                          : ''}">{e.output || "(leer)"}</pre>
                    {/if}
                  </div>
                </details>
              {/if}
            </li>
          {:else}
            <li class="text-muted-foreground p-2">Kein Verlauf gefunden.</li>
          {/each}
        </ol>
        {#if a.result}
          <div class="border-border/60 mt-2 rounded-lg border p-2">
            <div class="text-muted-foreground mb-1">Ergebnis</div>
            <div class="min-w-0 overflow-x-auto"><Markdown text={a.result} /></div>
          </div>
        {/if}
      {/if}
    {:else if tab === "wf"}
      {#each data.runs as r (r.id)}
        {@const open = openRuns[r.id] ?? r.status === "running"}
        {@const [icon, color, text] = status(r.status)}
        <section class="border-border/60 mb-2 rounded-lg border">
          <button
            class="hover:bg-accent/40 flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left"
            aria-expanded={open}
            onclick={() => (openRuns[r.id] = !open)}
          >
            <span class={color} title={text}>{icon}</span>
            <span class="min-w-0 flex-1 truncate font-medium">{r.name}</span>
            <span class="text-muted-foreground shrink-0 tabular-nums">{text} · {dur(r.duration_ms)} · {tokens(r.tokens)}</span>
          </button>
          {#if open}
            {#if r.phases.length}
              <div class="flex flex-wrap gap-1 px-2 pb-1.5">
                {#each r.phases as p, i (i)}
                  {@const ps = phaseState(r, p)}
                  <span
                    class="rounded-full px-1.5 py-px text-[10px] ring-1 {ps === 'active'
                      ? 'text-primary ring-primary/40'
                      : ps === 'done'
                        ? 'text-foreground ring-foreground/10'
                        : 'text-muted-foreground/60 ring-foreground/5'}">{ps === "done" ? "✓ " : ""}{p}</span
                  >
                {/each}
              </div>
            {/if}
            <ul class="border-border/60 border-t">
              {#each r.agents as a, i (i)}
                {@const [ai, ac, at] = status(a.state)}
                {@const last = a.steps.at(-1)}
                <li>
                  <button
                    class="hover:bg-accent/40 group/agent w-full px-2 py-1 text-left disabled:cursor-default"
                    disabled={!a.id}
                    title={a.id ? "Verlauf anzeigen" : undefined}
                    onclick={() => openAgent(r, a)}
                  >
                    <div class="flex items-center gap-2">
                      <span class={ac} title={at}>{ai}</span>
                      <span class="min-w-0 flex-1 truncate">{a.label || a.id}</span>
                      <span class="text-muted-foreground shrink-0 tabular-nums"
                        >{dur(agentDur(r, a))}{#if a.tokens} · {tokens(a.tokens)}{/if}</span
                      >
                      {#if a.id}<ChevronRightIcon class="text-muted-foreground size-3.5 shrink-0 opacity-0 group-hover/agent:opacity-100" />{/if}
                    </div>
                    {#if last}
                      <div class="text-muted-foreground truncate pl-5 font-mono text-[11px]">{last.tool} {last.summary}</div>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {:else}
        <p class="text-muted-foreground p-2">Keine Workflows in dieser Session.</p>
      {/each}
    {:else if imgs.length}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-2">
        {#each imgs as m (key(m.id))}
          <button class="group min-w-0 text-left" onclick={() => ((shown = m.id), load(m.id))} title={m.label}>
            <div use:lazy={m.id} class="bg-background/40 group-hover:ring-primary/40 aspect-video overflow-hidden rounded-md ring-1 ring-foreground/5">
              {#if urls[key(m.id)]}<img src={urls[key(m.id)]} alt={m.label} class="aspect-video w-full rounded-md object-cover" />{/if}
            </div>
            <div class="mt-1 truncate text-xs">{m.label}</div>
            <div class="text-muted-foreground truncate text-[10px]">{ago(m.at)}</div>
          </button>
        {/each}
      </div>
    {:else}
      <p class="text-muted-foreground p-2">Noch keine Bilder in dieser Session.</p>
    {/if}
  </div>
</div>

{#snippet gauge(label: string, p: number, sub: string)}
  <div class="flex justify-between"><span>{label}</span><span class="tabular-nums {txt(p)}">{Math.round(p)}%</span></div>
  <div class="my-1 h-1 overflow-hidden rounded-full bg-foreground/5">
    <div class="h-full rounded-full {lvl(p)}" style:width="{Math.min(100, Math.max(0, p))}%"></div>
  </div>
  {#if sub}<div class="text-muted-foreground truncate">{sub}</div>{/if}
{/snippet}

<WorkflowDialog bind:open={wfOpen} {cwd} entry={wfEntry} taken={templates} onsaved={loadTemplates} />

<Dialog.Root open={current !== null} onOpenChange={(o) => !o && (shown = null)}>
  <Dialog.Content class="max-h-[90vh] sm:max-w-[min(90vw,1200px)]" {onkeydown}>
    {#if current}
      <Dialog.Title class="sr-only">{current.label}</Dialog.Title>
      <div class="flex min-h-0 items-center gap-2">
        <button class="hover:bg-accent rounded-md p-1" aria-label="Vorheriges Bild" onclick={() => step(-1)}
          ><ChevronLeftIcon class="size-5" /></button
        >
        <div class="flex min-h-0 min-w-0 flex-1 justify-center">
          {#if urls[key(current.id)]}
            <ZoomImage src={urls[key(current.id)]} alt={current.label} onswipe={step} />
          {:else}
            <p class="text-muted-foreground p-8">Lade …</p>
          {/if}
        </div>
        <button class="hover:bg-accent rounded-md p-1" aria-label="Nächstes Bild" onclick={() => step(1)}
          ><ChevronRightIcon class="size-5" /></button
        >
      </div>
      <div class="text-muted-foreground flex items-center gap-2 text-xs">
        <span class="min-w-0 flex-1 truncate tabular-nums"
          >{index + 1} / {imgs.length} · {current.label} · {current.at ? clock(current.at) : ""}</span
        >
        {#if current.kind === "read"}
          <button class="hover:text-foreground shrink-0 underline-offset-2 hover:underline" onclick={() => navigator.clipboard.writeText(current.label)}
            >Pfad kopieren</button
          >
        {/if}
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>
