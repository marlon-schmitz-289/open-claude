<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { untrack } from "svelte";
  import { Button } from "$lib/components/ui/button/index.js";
  import UnityCloseDialog from "./UnityCloseDialog.svelte";
  import { unity, verdict, formatDuration, type UnityInfo, type UnityTestMode, type TestReport } from "$lib/unity";
  import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
  import CircleXIcon from "@lucide/svelte/icons/circle-x";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  let { repo }: { repo: string } = $props();

  let info = $state<UnityInfo | null>(null);
  let loading = $state(true);
  let error = $state("");

  async function reload() {
    loading = true;
    error = "";
    try {
      info = await unity.info(repo);
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  // Neu laden, wenn ein anderes Repo in denselben Tab wechselt (die Git-Ansicht wird pro Repo neu erzeugt,
  // aber sicher ist sicher, falls das mal nicht so bleibt).
  $effect(() => {
    repo;
    untrack(reload);
  });

  let settingUp = $state(false);
  let setupLog = $state("");
  async function setup() {
    settingUp = true;
    error = "";
    try {
      setupLog = await unity.setup(repo);
    } catch (e) {
      error = String(e);
    }
    settingUp = false;
    await reload();
  }

  let opening = $state(false);
  async function openEditor() {
    opening = true;
    error = "";
    try {
      // Kehrt erst zurueck, wenn der Editor laeuft (bis 180 s).
      await unity.open(repo);
    } catch (e) {
      error = String(e);
    }
    opening = false;
    await reload();
  }

  let closePath = $state<string | null>(null);

  let testRunning = $state<UnityTestMode | null>(null);
  let report = $state<TestReport | null>(null);
  let testError = $state("");

  async function runTest(mode: UnityTestMode) {
    if (testRunning) return;
    testRunning = mode;
    testError = "";
    try {
      report = await unity.test(repo, mode);
    } catch (e) {
      testError = String(e);
    }
    testRunning = null;
    reload(); // Test kann den Editor geoeffnet/verbunden haben lassen bzw. hat ihn ggf. gestartet.
  }

  // Das Backend sperrt Test/Oeffnen/Einrichten je Projekt ohnehin; hier nur, damit man es gar nicht erst anklickt.
  // Schliessen bleibt frei: Notausgang fuer einen haengenden Testlauf im Editor.
  const busy = $derived(!!testRunning || opening || settingUp);

  const VERDICT_CLASS = { green: "text-success", yellow: "text-warning", red: "text-destructive" } as const;
</script>

{#snippet stat(label: string, ok: boolean)}
  <span class="flex items-center gap-1.5">
    {#if ok}
      <CircleCheckIcon class="size-3.5 text-success" />
    {:else}
      <CircleXIcon class="text-muted-foreground size-3.5 opacity-60" />
    {/if}
    {label}
  </span>
{/snippet}

<div class="flex h-full min-h-0 flex-col overflow-y-auto text-xs">
  <Notice bind:text={error} />

  {#if loading && !info}
    <p class="text-muted-foreground px-3 py-4">Lese Unity-Status …</p>
  {:else if info}
    <div class="border-border flex flex-wrap items-center gap-x-4 gap-y-1.5 border-b px-3 py-2">
      {@render stat("Unity-CLI", info.cli)}
      {@render stat("Editor installiert", info.editorInstalled)}
      {@render stat("Editor offen", info.editorOpen)}
      {@render stat("Pipeline-Paket", info.pipeline)}
      {@render stat("Verbunden", info.connected)}
      {@render stat("Skill installiert", info.skill)}
      <span class="flex-1"></span>
      <Button size="sm" variant="outline" class="h-6 text-[11px]" disabled={busy} onclick={setup}>
        {settingUp ? "Richte ein …" : "Unity-Support einrichten"}
      </Button>
      {#if info.editorOpen}
        <Button size="sm" variant="outline" class="h-6 text-[11px]" onclick={() => (closePath = repo)}>
          Editor schließen
        </Button>
      {:else}
        <Button size="sm" variant="outline" class="h-6 text-[11px]" disabled={busy} onclick={openEditor}>
          {opening ? "Öffnet …" : "Editor öffnen"}
        </Button>
      {/if}
      <button
        class="text-muted-foreground hover:text-foreground disabled:opacity-50"
        title="Neu laden"
        disabled={loading}
        onclick={reload}
      >
        <RefreshCwIcon class="size-3.5 {loading ? 'animate-spin' : ''}" />
      </button>
    </div>

    {#if setupLog}
      <pre
        class="border-border text-muted-foreground max-h-32 overflow-y-auto border-b px-3 py-2 font-mono text-[11px] whitespace-pre-wrap"
      >{setupLog}</pre>
    {/if}

    <div class="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-2">
      <div class="flex items-center gap-2">
        <Button size="sm" class="h-7 text-[11px]" disabled={busy} onclick={() => runTest("EditMode")}>
          {testRunning === "EditMode" ? "Läuft …" : "EditMode-Tests"}
        </Button>
        <Button size="sm" class="h-7 text-[11px]" disabled={busy} onclick={() => runTest("PlayMode")}>
          {testRunning === "PlayMode" ? "Läuft …" : "PlayMode-Tests"}
        </Button>
        {#if testRunning}<LoaderCircleIcon class="text-muted-foreground size-3.5 animate-spin" />{/if}
      </div>

      {#if testError}<p class="text-destructive text-[11px]">{testError}</p>{/if}

      {#if report}
        {@const v = verdict(report)}
        <div class="border-border rounded border p-2">
          <div class="flex flex-wrap items-center gap-3">
            <span class="flex items-center gap-1.5 font-semibold {VERDICT_CLASS[v]}">
              {#if v === "red"}
                <CircleXIcon class="size-4" />
              {:else if v === "yellow"}
                <TriangleAlertIcon class="size-4" />
              {:else}
                <CircleCheckIcon class="size-4" />
              {/if}
              {report.passed}/{report.total} bestanden
            </span>
            {#if report.failed}<span class="text-destructive">{report.failed} fehlgeschlagen</span>{/if}
            {#if report.skipped}<span class="text-warning">{report.skipped} übersprungen</span>{/if}
            {#if report.inconclusive}<span class="text-warning">{report.inconclusive} unklar</span>{/if}
            <span class="text-muted-foreground font-mono">{formatDuration(report.durationSecs)}</span>
            <span class="text-muted-foreground">
              über {report.via === "editor" ? "laufenden Editor" : "Batchmode"}
            </span>
          </div>
          {#if report.failures.length}
            <ul class="border-border mt-2 space-y-1.5 border-t pt-2">
              {#each report.failures as f, i (i)}
                <li>
                  <div class="font-mono text-[11px] font-semibold">{f.name}</div>
                  <div class="text-muted-foreground text-[11px] whitespace-pre-wrap select-text">{f.message}</div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<UnityCloseDialog bind:path={closePath} onclosed={reload} />
