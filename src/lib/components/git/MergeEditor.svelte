<script lang="ts">
  import Notice from "$lib/components/Notice.svelte";
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { git, type Conflict } from "$lib/git";
  import { diffLines, parseConflicts, render, type Choice, type Mark, type Segment } from "$lib/conflict";
  import { Button } from "$lib/components/ui/button/index.js";
  import { DropdownMenu } from "bits-ui";
  import { cn } from "$lib/utils";
  import { menuContent, menuItem } from "$lib/components/kit";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
  import ChevronsLeftIcon from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRightIcon from "@lucide/svelte/icons/chevrons-right";
  import UndoIcon from "@lucide/svelte/icons/undo-2";
  import PencilIcon from "@lucide/svelte/icons/pencil";

  let {
    repo,
    path,
    onclose,
    dirty = $bindable(false),
  }: { repo: string; path: string; onclose: (resolved: boolean) => void; dirty?: boolean } = $props();

  let conflict = $state.raw<Conflict | null>(null);
  // raw: grosse Dateien nicht tief proxien
  let segments = $state.raw<Segment[]>([]);
  let choices = $state<(Choice | null)[]>([]);
  let expanded = new SvelteSet<number>();
  let current = $state(0);
  let editing = $state(false);
  // Per "Übernehmen" eingelesene Handarbeit; load() setzt choices zurueck, dirty muss sie trotzdem sehen.
  let modified = $state(false);
  let draft = $state("");
  let busy = $state(false);
  let error = $state("");
  let scroller = $state<HTMLDivElement>();
  // Echte Namen statt "Ours"/"Theirs", z. B. main / feature/x
  let sides = $state({ ours: "Ours", theirs: "Theirs" });

  const CONTEXT = 4;

  // Segmentindex -> Konfliktnummer
  const nums = $derived.by(() => {
    let n = 0;
    return segments.map((s) => (s.kind === "conflict" ? n++ : -1));
  });
  const total = $derived(nums.filter((n) => n >= 0).length);
  const open = $derived(Array.from({ length: total }, (_, i) => i).filter((i) => !choices[i]));

  const lines = (t: string) => (t ? t.replace(/\n$/, "").split("\n") : []);
  // Je Segment einmal zerlegen (unabhaengig von choices): Kontextzeilen ...
  const plains = $derived(segments.map((s) => (s.kind === "plain" ? lines(s.text) : [])));
  // ... bzw. je Konflikt beide Seiten samt Marken gegen die Basis (ohne Basis gegen die jeweils andere Seite).
  const parts = $derived(
    segments.map((s) => {
      if (s.kind === "plain") return null;
      const [ours, theirs] = [lines(s.ours), lines(s.theirs)];
      const base = s.base === null ? null : lines(s.base);
      return { ours, theirs, base, oursMarks: diffLines(ours, base ?? theirs), theirsMarks: diffLines(theirs, base ?? ours) };
    }),
  );
  // Zeilen jedes Segments im gerenderten Ergebnis (offene Konflikte samt Markern) ...
  const results = $derived(segments.map((s, i) => (s.kind === "plain" ? plains[i] : lines(render([s], [choices[nums[i]]])))));
  // ... und daraus die erste Zeilennummer jedes Segments.
  const starts = $derived.by(() => {
    let at = 1;
    return results.map((r) => (at += r.length) - r.length);
  });

  function load(text: string) {
    segments = parseConflicts(text);
    choices = [];
    expanded.clear();
    current = 0;
  }

  $effect(() => {
    const [r, p] = [repo, path];
    conflict = null;
    error = "";
    editing = false;
    modified = false;
    git.conflictSides(r).then((s) => (sides = s)).catch(() => {});
    git
      .conflict(r, p)
      .then((c) => {
        conflict = c;
        load(c.merged);
      })
      .catch((e) => (error = String(e)));
  });

  function jump(dir: 1 | -1) {
    if (!total) return;
    // bevorzugt offene Konflikte, sonst reihum
    const pool = open.length ? open : Array.from({ length: total }, (_, i) => i);
    const next = dir > 0 ? pool.find((i) => i > current) ?? pool[0] : pool.findLast((i) => i < current) ?? pool[pool.length - 1];
    current = next;
    tick().then(() => scroller?.querySelector(`[data-conflict="${next}"]`)?.scrollIntoView({ block: "center" }));
  }

  function choose(n: number, c: Choice | null) {
    choices[n] = c;
    current = n;
  }

  // Entscheidungen oder Handarbeit, die ein Schliessen/Ueberschreiben verwerfen wuerde.
  $effect(() => {
    dirty = editing || modified || choices.some(Boolean);
  });
  const discardOk = () => !dirty || confirm("Entscheidungen und Änderungen im Merge-Editor verwerfen?");

  function cancel() {
    if (discardOk()) onclose(false);
  }

  function whole(text: string | null) {
    if (text === null || !discardOk()) return;
    editing = false;
    modified = false;
    load(text);
  }

  function startEdit() {
    draft = render(segments, choices);
    editing = true;
  }

  // Manuelle Aenderungen neu einlesen; verbliebene Marker werden wieder zu Konflikten.
  function applyEdit() {
    load(draft);
    editing = false;
    modified = true;
  }

  async function save() {
    if (editing) applyEdit();
    if (open.length || busy) return;
    busy = true;
    error = "";
    try {
      await git.resolve(repo, path, render(segments, choices));
      modified = false;
      onclose(true);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // Datei laesst sich nicht als Text laden (binaer): nur eine Seite komplett nehmen.
  async function side(theirs: boolean) {
    if (busy) return;
    busy = true;
    try {
      await git.resolveSide(repo, path, theirs);
      onclose(true);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "F7") {
      e.preventDefault();
      jump(e.shiftKey ? -1 : 1);
    } else if (e.ctrlKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      save();
    }
  }

  const label = (c: Choice) =>
    ({
      ours: sides.ours,
      theirs: sides.theirs,
      "ours-theirs": `Beide (${sides.ours} zuerst)`,
      "theirs-ours": `Beide (${sides.theirs} zuerst)`,
      base: "Basis",
    })[c];
</script>

<svelte:window {onkeydown} />

<!-- Zeilen mit Nummern im Ergebnis; zwei <pre> statt ein Element pro Zeile (grosse Dateien) -->
{#snippet code(ls: string[], from: number)}
  <div class="flex overflow-x-auto">
    <pre class="text-muted-foreground/60 bg-background sticky left-0 w-10 shrink-0 pr-1 text-right select-none">{ls.map((_, k) => from + k).join("\n")}</pre>
    <pre class="pr-3">{ls.join("\n")}</pre>
  </div>
{/snippet}

<!-- Eine Seite: unveraenderte Zeilen gedimmt, geaenderte in Seitenfarbe, abweichender Teil kraeftiger -->
{#snippet column(ls: string[], marks: Mark[], dim: boolean, tint: string, strong: string)}
  <div class="overflow-x-auto {dim ? 'opacity-50' : ''}">
    <div class="w-max min-w-full">
      {#each ls as l, k (k)}
        {@const m = marks[k]}
        <div class="min-h-5 px-3 whitespace-pre {m ? tint : 'text-muted-foreground'}">{#if m}{l.slice(0, m[0])}<span class={strong}>{l.slice(m[0], m[1])}</span>{l.slice(m[1])}{:else}{l}{/if}</div>
      {:else}
        <div class="text-muted-foreground px-3">(leer)</div>
      {/each}
    </div>
  </div>
{/snippet}

<div class="@container flex h-full min-h-0 flex-col text-xs">
  <!-- Immer einzeilig: Pfad kuerzt sich, bei wenig Platz (Containerbreite, nicht Fenster) nur Icons -->
  <div class="bg-chrome border-border flex items-center gap-1 border-b px-3 py-1.5">
    <span class="mr-1 min-w-0 flex-1 truncate font-mono text-[11px]" title={path}>{path}</span>
    <span
      class="shrink-0 font-mono text-[11px] whitespace-nowrap tabular-nums {open.length ? 'text-destructive' : 'text-primary'}"
      title={total ? `${total - open.length} von ${total} Konflikten gelöst` : undefined}
    >
      {#if total}{total - open.length}/{total}{:else}keine Konfliktmarker{/if}
    </span>
    <Button variant="ghost" size="sm" class="h-6 px-1.5" onclick={() => jump(-1)} disabled={!total || editing} title="Vorheriger Konflikt (Umschalt+F7)">
      <ChevronUpIcon class="size-3.5" />
    </Button>
    <Button variant="ghost" size="sm" class="h-6 px-1.5" onclick={() => jump(1)} disabled={!total || editing} title="Nächster Konflikt (F7)">
      <ChevronDownIcon class="size-3.5" />
    </Button>
    <span class="bg-border h-4 w-px"></span>
    <!-- Ganze Datei von einer Seite: selten gebraucht, darum im Menue statt zwei breiter Knoepfe -->
    <DropdownMenu.Root>
      <DropdownMenu.Trigger
        class="hover:bg-accent flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[11px] font-medium whitespace-nowrap disabled:opacity-50"
        disabled={!conflict}
        >Ganze Datei <ChevronDownIcon class="size-3" /></DropdownMenu.Trigger
      >
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={4}
          class={cn(menuContent, "max-w-80 min-w-48 rounded-lg shadow-lg")}
        >
          {#each [[sides.ours, conflict?.ours], [sides.theirs, conflict?.theirs]] as [name, text], k (k)}
            <DropdownMenu.Item
              class={cn(menuItem, "gap-1 py-1.5 data-disabled:opacity-50 data-disabled:hover:bg-transparent")}
              disabled={text == null}
              onSelect={() => whole(text ?? null)}
              >wie auf <span class="truncate font-mono">{name}</span></DropdownMenu.Item
            >
          {/each}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
    <Button variant="ghost" size="sm" class="h-6 shrink-0 text-[11px]" disabled={!conflict} onclick={() => whole(conflict!.merged)} title="Zurücksetzen: alle Entscheidungen verwerfen">
      <UndoIcon class="size-3.5" /> <span class="hidden @xl:inline">Zurücksetzen</span>
    </Button>
    <Button variant="ghost" size="sm" class="h-6 shrink-0 text-[11px] {editing ? 'text-primary' : ''}" disabled={!conflict} onclick={() => (editing ? applyEdit() : startEdit())} title={editing ? "Übernehmen" : "Von Hand bearbeiten"}>
      <PencilIcon class="size-3.5" /> <span class="hidden @xl:inline">{editing ? "Übernehmen" : "Bearbeiten"}</span>
    </Button>
  </div>

  <!-- bleibt stehen: erklaert z. B., warum eine Binaerdatei nur komplett uebernommen werden kann -->
  <Notice bind:text={error} timeout={0} />

  {#if !conflict}
    {#if !error}
      <p class="text-muted-foreground px-3 py-6">Lade Konflikt …</p>
    {:else}
      <div class="flex min-h-0 flex-1 items-start gap-2 px-3 py-4">
        <Button variant="outline" size="sm" class="text-xs" disabled={busy} onclick={() => side(false)}>{sides.ours} komplett übernehmen</Button>
        <Button variant="outline" size="sm" class="text-xs" disabled={busy} onclick={() => side(true)}>{sides.theirs} komplett übernehmen</Button>
      </div>
    {/if}
  {:else if editing}
    <textarea
      class="bg-background min-h-0 flex-1 resize-none p-3 font-mono text-xs leading-5 outline-none"
      spellcheck="false"
      bind:value={draft}
    ></textarea>
  {:else}
    <div class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto font-mono leading-5" bind:this={scroller}>
      {#each segments as seg, i (i)}
        {#if seg.kind === "plain"}
          {@const ls = plains[i]}
          {@const head = i === 0 ? 0 : CONTEXT}
          {@const tail = i === segments.length - 1 ? 0 : CONTEXT}
          {#if ls.length > head + tail + CONTEXT && !expanded.has(i)}
            {#if head}{@render code(ls.slice(0, head), starts[i])}{/if}
            <button
              class="text-muted-foreground hover:text-foreground bg-secondary/40 w-full px-3 py-0.5 text-left text-[11px]"
              onclick={() => expanded.add(i)}>… {ls.length - head - tail} unveränderte Zeilen einblenden</button
            >
            {#if tail}{@render code(ls.slice(-tail), starts[i] + ls.length - tail)}{/if}
          {:else}
            {@render code(ls, starts[i])}
          {/if}
        {:else}
          {@const n = nums[i]}
          {@const c = choices[n]}
          {@const p = parts[i]!}
          {@const hasOurs = !!c?.includes("ours")}
          {@const hasTheirs = !!c?.includes("theirs")}
          {@const result = c ? results[i] : []}
          <div
            data-conflict={n}
            class="my-1 grid grid-cols-1 border-l-4 @3xl:grid-cols-3 {c ? 'border-primary/60' : 'border-destructive'} {n === current ? 'ring-primary/50 ring-1' : ''}"
          >
            <div class="min-w-0">
              <div class="text-muted-foreground flex h-6 items-center gap-1 bg-emerald-500/10 pr-1 pl-3 font-sans text-[11px]">
                <span class="min-w-0 flex-1 truncate" title={sides.ours}>{sides.ours}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-5 px-1"
                  disabled={hasOurs}
                  onclick={() => choose(n, c === "theirs" ? "theirs-ours" : "ours")}
                  title={c === "theirs" ? `${sides.ours} zusätzlich anhängen` : `${sides.ours} übernehmen`}
                >
                  <ChevronsRightIcon class="size-3.5" />
                </Button>
              </div>
              {@render column(p.ours, p.oursMarks, !!c && !hasOurs, "bg-emerald-500/10", "bg-emerald-500/35")}
            </div>
            <div class="border-border min-w-0 border-y @3xl:border-x @3xl:border-y-0">
              <div class="bg-secondary/60 flex h-6 items-center gap-1 px-2 font-sans">
                <span class="min-w-0 flex-1 truncate text-[11px] font-semibold {c ? 'text-primary' : 'text-destructive'}">
                  Konflikt {n + 1}{c ? ` · ${label(c)}` : ""}
                </span>
                {#if c}
                  <Button variant="ghost" size="sm" class="h-5 px-1.5 text-[11px]" onclick={() => choose(n, null)}>
                    <UndoIcon class="size-3" /> Rückgängig
                  </Button>
                {:else if seg.base !== null}
                  <Button variant="outline" size="sm" class="h-5 px-1.5 text-[11px]" onclick={() => choose(n, "base")}>Basis übernehmen</Button>
                {/if}
              </div>
              {#if result.length}
                <div class="bg-primary/5">{@render code(result, starts[i])}</div>
              {:else if c}
                <div class="text-muted-foreground bg-primary/5 px-3">(leer)</div>
              {:else if p.base}
                <div class="text-muted-foreground px-3 font-sans text-[11px]">Basis</div>
                {#if p.base.length}
                  <pre class="text-muted-foreground min-h-5 overflow-x-auto px-3 opacity-60">{p.base.join("\n")}</pre>
                {:else}
                  <div class="text-muted-foreground px-3 opacity-60">(leer)</div>
                {/if}
              {:else}
                <div class="text-muted-foreground px-3 font-sans text-[11px] italic">offen – Seite wählen</div>
              {/if}
            </div>
            <div class="min-w-0">
              <div class="text-muted-foreground flex h-6 items-center gap-1 bg-sky-500/10 pr-3 pl-1 font-sans text-[11px]">
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-5 px-1"
                  disabled={hasTheirs}
                  onclick={() => choose(n, c === "ours" ? "ours-theirs" : "theirs")}
                  title={c === "ours" ? `${sides.theirs} zusätzlich anhängen` : `${sides.theirs} übernehmen`}
                >
                  <ChevronsLeftIcon class="size-3.5" />
                </Button>
                <span class="min-w-0 flex-1 truncate" title={sides.theirs}>{sides.theirs}</span>
              </div>
              {@render column(p.theirs, p.theirsMarks, !!c && !hasTheirs, "bg-sky-500/10", "bg-sky-500/35")}
            </div>
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  <div class="bg-chrome border-border flex items-center justify-end gap-2 border-t px-3 py-1.5">
    <span class="text-muted-foreground mr-auto min-w-0 truncate text-[11px] whitespace-nowrap">F7 / Umschalt+F7 navigieren · Strg+S speichern</span>
    <Button variant="ghost" size="sm" class="h-7 text-xs" onclick={cancel}>Abbrechen</Button>
    <Button size="sm" class="h-7 text-xs" disabled={!conflict || busy || (!editing && open.length > 0)} onclick={save}>
      Speichern &amp; als gelöst markieren
    </Button>
  </div>
</div>
