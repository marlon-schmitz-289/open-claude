<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Select } from "$lib/components/kit";
  import { Textarea } from "$lib/components/ui/textarea/index.js";
  import { EFFORTS, MODELS, fromScript, newAgent, newPhase, newTemplate, problems, toScript, type WfFile, type WfTemplate } from "$lib/workflow.logic";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";

  let {
    open = $bindable(false),
    cwd,
    entry = null,
    taken,
    onsaved,
  }: {
    open: boolean;
    cwd: string;
    /** null = neue Vorlage */
    entry?: WfFile | null;
    /** alle Vorlagen, fuer Namenskollisionen je Ort */
    taken: { scope: string; file: string }[];
    onsaved: () => void;
  } = $props();

  let t = $state<WfTemplate>(newTemplate());
  let scope = $state<"user" | "project">("user");
  let busy = $state(false);
  let error = $state("");

  // Beim Oeffnen frisch aus der Datei laden
  $effect.pre(() => {
    if (!open) return;
    untrack(() => {
      t = (entry && fromScript(entry.text)) || newTemplate();
      scope = entry?.scope ?? "user";
      error = "";
    });
  });

  const issues = $derived(
    problems(
      t,
      // APFS ist case-insensitiv: Review.js belegt auch review
      taken
        .filter((f) => f.scope === scope && !(entry && f.scope === entry.scope && f.file === entry.file))
        .map((f) => f.file.toLowerCase()),
    ),
  );
  // Kaputtes Schema wirft in toScript; issues zeigt den Grund
  const script = $derived.by(() => {
    try {
      return toScript(t);
    } catch {
      return "";
    }
  });
  // Nur gegen die geladene Datei pruefen, nicht gegen die laufenden Aenderungen
  const outdated = $derived.by(() => {
    const b = entry && fromScript(entry.text);
    if (!b) return false;
    try {
      return toScript(b) !== entry.text;
    } catch {
      return true;
    }
  });
  const flow = $derived(t.phases.map((p) => `${p.title || "?"} ${p.mode === "parallel" ? "‖" : ""}${p.agents.length}`).join(" → "));

  function move<T>(a: T[], i: number, d: number) {
    const j = i + d;
    if (j < 0 || j >= a.length) return;
    [a[i], a[j]] = [a[j], a[i]];
  }

  async function save() {
    if (busy || issues.length) return;
    busy = true;
    error = "";
    try {
      await invoke("workflows_save", { cwd, scope, file: t.name, text: toScript($state.snapshot(t)) });
      // Umbenannt oder Ort gewechselt: alte Datei weg
      if (entry && (entry.file.toLowerCase() !== t.name || entry.scope !== scope)) await invoke("workflows_delete", { cwd, scope: entry.scope, file: entry.file });
      open = false;
      onsaved();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open={() => open, (v) => { if (!busy) open = v; }}>
  <Dialog.Content class="flex max-h-[90vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>Workflow-Vorlage</Dialog.Title>
      <Dialog.Description>
        Phasen laufen nacheinander. In Prompts: <code>{"{{args}}"}</code> = Aufruf-Argumente, <code>{"{{prev}}"}</code> = Ergebnis des vorigen Schritts.
      </Dialog.Description>
    </Dialog.Header>

    <fieldset disabled={busy} class="min-h-0 flex-1 space-y-3 overflow-y-auto pr-1 text-xs">
      <div class="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-2">
        <span class="text-muted-foreground">Name</span>
        <div class="flex items-center gap-2">
          <Input bind:value={t.name} placeholder="code-review" class="font-mono text-xs" />
          {#each [["user", "Global"], ["project", "Projekt"]] as [s, label] (s)}
            <Button variant={scope === s ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (scope = s as typeof scope)}>{label}</Button>
          {/each}
        </div>
        <span class="text-muted-foreground">Beschreibung</span>
        <Input bind:value={t.description} placeholder="Was der Workflow tut" class="text-xs" />
        <span class="text-muted-foreground">Wann nutzen</span>
        <Input bind:value={t.whenToUse} placeholder="optional" class="text-xs" />
        <span class="text-muted-foreground">Ablauf</span>
        <span class="truncate font-mono">{flow}</span>
      </div>

      {#each t.phases as p, i (p)}
        <section class="border-border/60 space-y-2 rounded-lg border p-2">
          <div class="flex items-center gap-1.5">
            <span class="text-muted-foreground shrink-0">Phase {i + 1}</span>
            <Input bind:value={p.title} class="h-7 text-xs" />
            <Button variant={p.mode === "parallel" ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (p.mode = "parallel")}>parallel</Button>
            <Button variant={p.mode === "sequence" ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (p.mode = "sequence")}>nacheinander</Button>
            <Button variant="ghost" size="icon-sm" aria-label="Nach oben" disabled={i === 0} onclick={() => move(t.phases, i, -1)}><ArrowUpIcon class="size-3.5" /></Button>
            <Button variant="ghost" size="icon-sm" aria-label="Nach unten" disabled={i === t.phases.length - 1} onclick={() => move(t.phases, i, 1)}
              ><ArrowDownIcon class="size-3.5" /></Button
            >
            <Button variant="ghost" size="icon-sm" aria-label="Phase löschen" onclick={() => t.phases.splice(i, 1)}><Trash2Icon class="size-3.5" /></Button>
          </div>

          {#each p.agents as a, j (a)}
            <div class="bg-secondary/30 space-y-1.5 rounded-md p-2">
              <div class="flex items-center gap-1.5">
                <span class="text-muted-foreground shrink-0">Agent</span>
                <Input bind:value={a.label} placeholder="Label (optional)" class="h-7 text-xs" />
                <Button variant="ghost" size="icon-sm" aria-label="Agent duplizieren" onclick={() => p.agents.splice(j + 1, 0, { ...$state.snapshot(a) })}
                  ><CopyIcon class="size-3.5" /></Button
                >
                <Button variant="ghost" size="icon-sm" aria-label="Agent löschen" onclick={() => p.agents.splice(j, 1)}><Trash2Icon class="size-3.5" /></Button>
              </div>
              <Textarea bind:value={a.prompt} rows={3} placeholder={"Was soll der Agent tun? {{args}} {{prev}}"} class="font-mono text-xs" />
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-muted-foreground">Modell</span>
                {#each MODELS as m (m)}
                  <Button variant={a.model === m ? "default" : "outline"} size="sm" class="h-6 px-2 text-xs" onclick={() => (a.model = m)}>{m || "Session"}</Button>
                {/each}
                <span class="text-muted-foreground ml-2">Effort</span>
                <Select bind:value={a.effort} options={EFFORTS.map((e) => [e || "Session", e] as const)} aria-label="Effort" />
              </div>
              <details open={!!a.schema}>
                <summary class="text-muted-foreground cursor-pointer">Strukturierte Ausgabe (JSON-Schema)</summary>
                <Textarea
                  bind:value={a.schema}
                  rows={4}
                  placeholder={'{"type":"object","properties":{"findings":{"type":"array","items":{"type":"string"}}},"required":["findings"]}'}
                  class="mt-1 font-mono text-xs"
                />
              </details>
            </div>
          {/each}
          <Button variant="outline" size="sm" class="text-xs" onclick={() => p.agents.push(newAgent())}>+ Agent</Button>
        </section>
      {/each}
      <Button variant="outline" size="sm" class="text-xs" onclick={() => t.phases.push(newPhase(t.phases.length + 1))}>+ Phase</Button>

      <details>
        <summary class="text-muted-foreground cursor-pointer">Skript-Vorschau</summary>
        <pre class="bg-background/60 mt-1 max-h-64 overflow-auto rounded p-2 font-mono text-[11px]">{script}</pre>
      </details>
    </fieldset>

    {#if issues.length}
      <ul class="text-warning max-h-24 list-inside list-disc overflow-y-auto text-xs">
        {#each issues as m, i (i)}<li>{m}</li>{/each}
      </ul>
    {/if}
    {#if outdated}
      <p class="text-muted-foreground text-xs">Das Skript wurde außerhalb geändert – Speichern erzeugt es neu.</p>
    {/if}
    {#if error}<p class="text-destructive text-xs whitespace-pre-wrap">{error}</p>{/if}

    <div class="flex justify-end gap-1.5">
      <Button variant="outline" size="sm" class="text-xs" disabled={busy} onclick={() => (open = false)}>Abbrechen</Button>
      <Button size="sm" class="gap-1.5 text-xs" disabled={busy || !!issues.length} onclick={save}>
        {#if busy}<LoaderCircleIcon class="size-3.5 animate-spin" />{/if}
        Speichern
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
