<script lang="ts" module>
  import XIcon from "@lucide/svelte/icons/x";
  export type TabInfo = { title: string; hint?: string; dirty?: boolean; closable?: boolean; icon?: typeof XIcon; tint?: string };
</script>

<script lang="ts">
  // Andockbare Tabs: zeichnet den Layout-Baum aus $lib/dock (Splits mit Trennleisten, Gruppen mit Tab-Leiste).
  // Das Layout ist kontrolliert: jede Aenderung geht als neuer Baum ueber onlayout nach aussen, geschlossen wird nur ueber onclose.
  // Ziehen laeuft ueber Pointer-Events; HTML5-Drag-and-drop geht in WebView2 nicht, solange Tauris Drop-Handler an ist.
  import type { Snippet } from "svelte";
  import Splitter from "$lib/components/Splitter.svelte";
  import { activate, edgeAt, groupOf, moveTab, resize, splitAt, type Edge, type Group, type Layout, type Node, type PanelId } from "$lib/dock";

  let {
    layout,
    onlayout,
    panel,
    tab,
    onclose,
    onfocus,
    empty,
  }: {
    layout: Layout;
    onlayout: (layout: Layout) => void;
    panel: Snippet<[PanelId]>;
    tab: (id: PanelId) => TabInfo;
    onclose: (id: PanelId) => void;
    onfocus?: (panel: PanelId, group: string) => void;
    empty?: Snippet;
  } = $props();

  /** Ziel unter dem Zeiger: index = Platz in der Tab-Leiste, sonst Kante (null = Mitte, als Tab ans Ende). */
  type Hint = { group: string; index?: number; edge?: Edge | null; style: string };

  let root = $state<HTMLDivElement>();
  /** Gezogener Tab; on erst nach 4 px, damit ein Klick kein Ziehen ist. */
  let drag = $state<{ id: PanelId; x: number; y: number; on: boolean } | null>(null);
  let hint = $state<Hint | null>(null);
  /** Eine Trennleiste wird gezogen. */
  let sizing = $state(false);
  // Solange gezogen wird, nehmen die Inhalte keine Zeiger an, sonst schluckt ein iframe die Bewegung.
  const busy = $derived(sizing || !!drag?.on);

  /** Pixelgroesse der Kinder i und i + 1 des Splits an path (Trennleisten liegen dazwischen, daher 2 * i). */
  function pair(path: number[], i: number, axis: "x" | "y") {
    const kids = root?.querySelector(`[data-split="${path.join(".")}"]`)?.children;
    const dim = (el?: Element) => {
      const r = el?.getBoundingClientRect();
      return r ? (axis === "x" ? r.width : r.height) : 0;
    };
    return [dim(kids?.[2 * i]), dim(kids?.[2 * i + 2])];
  }

  function select(id: PanelId, group: string) {
    if (groupOf(layout, id)?.active !== id) onlayout(activate(layout, id));
    onfocus?.(id, group);
  }

  function aim(x: number, y: number): Hint | null {
    if (!root || !drag) return null;
    const box = root.getBoundingClientRect();
    const inside = (r: DOMRect) => x >= r.left && x < r.right && y >= r.top && y < r.bottom;
    const rect = (l: number, t: number, w: number, h: number) =>
      `left:${l - box.left}px;top:${t - box.top}px;width:${w}px;height:${h}px`;
    for (const bar of root.querySelectorAll<HTMLElement>("[data-bar]")) {
      const r = bar.getBoundingClientRect();
      if (!inside(r)) continue;
      const tabs = [...bar.querySelectorAll("[data-tab]")].map((t) => t.getBoundingClientRect());
      const index = tabs.filter((t) => x > t.left + t.width / 2).length;
      const left = tabs[index]?.left ?? tabs.at(-1)?.right ?? r.left;
      return { group: bar.dataset.bar!, index, style: rect(left - 1, r.top, 2, r.height) };
    }
    for (const body of root.querySelectorAll<HTMLElement>("[data-body]")) {
      const r = body.getBoundingClientRect();
      // Treffer samt Abstand um die Karte (Split-Kind), sonst faellt der Drop in der Luecke weg.
      if (!inside(body.parentElement!.parentElement!.getBoundingClientRect())) continue;
      const group = body.dataset.body!;
      const edge = edgeAt(x - r.left, y - r.top, r.width, r.height);
      // Mitte der eigenen Gruppe: der Tab liegt schon dort.
      if (!edge && groupOf(layout, drag.id)?.id === group) return null;
      const w = edge === "left" || edge === "right" ? r.width / 2 : r.width;
      const h = edge === "top" || edge === "bottom" ? r.height / 2 : r.height;
      return { group, edge, style: rect(edge === "right" ? r.left + w : r.left, edge === "bottom" ? r.top + h : r.top, w, h) };
    }
    return null;
  }

  function down(e: PointerEvent, id: PanelId) {
    dragged = false;
    // Mittelklick schliesst (onauxclick); ohne preventDefault startet Windows den Autoscroll.
    if (e.button === 1) return e.preventDefault();
    // Auf dem x nicht fangen, sonst landet dessen Klick auf dem Tab.
    if (e.button !== 0 || (e.target as Element).closest("button")) return;
    // Sonst startet der Browser eine Textauswahl, die beim Ziehen ueber Editor und Panels mitlaeuft.
    // Damit entfaellt auch der Fokus beim Druecken, daher selbst setzen (Pfeiltasten im Tablist).
    e.preventDefault();
    const tab = e.currentTarget as HTMLElement;
    tab.focus();
    tab.setPointerCapture(e.pointerId);
    drag = { id, x: e.clientX, y: e.clientY, on: false };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    if (!drag.on && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 4) return;
    drag.on = true;
    hint = aim(e.clientX, e.clientY);
  }

  /** Nach echtem Ziehen den folgenden click schlucken, sonst meldet select() die alte Gruppe. */
  let dragged = false;

  function up() {
    const [d, h] = [drag, hint];
    drag = hint = null;
    dragged = !!d?.on;
    if (!d?.on || !h) return;
    const next =
      h.index !== undefined
        ? moveTab(layout, d.id, h.group, h.index)
        : h.edge
          ? splitAt(layout, d.id, h.group, h.edge)
          : moveTab(layout, d.id, h.group, Infinity);
    if (next === layout) return;
    onlayout(next);
    const g = groupOf(next, d.id);
    if (g) onfocus?.(d.id, g.id);
  }

  function key(e: KeyboardEvent, g: Group, id: PanelId) {
    if (e.target !== e.currentTarget) return;
    const i = g.tabs.indexOf(id);
    const to = { ArrowLeft: g.tabs[i - 1], ArrowRight: g.tabs[i + 1], Home: g.tabs[0], End: g.tabs.at(-1) }[e.key];
    if (e.key === "Enter" || e.key === " ") select(id, g.id);
    else if (e.key === "Delete" && tab(id).closable !== false) onclose(id);
    else if (to) {
      select(to, g.id);
      ((e.currentTarget as HTMLElement).parentElement!.children[g.tabs.indexOf(to)] as HTMLElement).focus();
    } else return;
    e.preventDefault();
  }
</script>

<svelte:window
  onkeydowncapture={(e) => {
    if (!drag || e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    drag = hint = null;
  }}
/>

{#snippet node(n: Node, path: number[], grow: number)}
  {#if n.kind === "split"}
    <div
      data-split={path.join(".")}
      class="flex min-h-0 min-w-0 {n.axis === 'y' ? 'flex-col' : ''}"
      style="flex:{grow} 1 0"
    >
      {#each n.children as child, i}
        {#if i}
          <Splitter
            axis={n.axis}
            min={60}
            bind:size={
              () => Math.round(pair(path, i - 1, n.axis)[0]),
              (px) => {
                const [a, b] = pair(path, i - 1, n.axis);
                if (a + b) onlayout(resize(layout, path, i - 1, px / (a + b)));
              }
            }
          />
        {/if}
        {@render node(child, [...path, i], n.sizes[i])}
      {/each}
    </div>
  {:else}
    <!-- Aeusseres div bleibt Split-Kind (pair() misst es samt Abstand), die Karte liegt darin. -->
    <div class="flex min-h-0 min-w-0 flex-col p-1" style="flex:{grow} 1 0">
      <div
        class="bg-card ring-foreground/5 focus-within:ring-primary/30 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg shadow-sm ring-1 shadow-black/5 transition-shadow dark:shadow-black/20"
      >
        <div
          data-bar={n.id}
          role="tablist"
          class="bg-chrome/50 border-border/60 flex h-8 shrink-0 items-center gap-0.5 overflow-x-auto overflow-y-hidden border-b px-1 select-none"
        >
          {#each n.tabs as id (id)}
            {@const t = tab(id)}
            <div
              data-tab
              role="tab"
              tabindex={n.active === id ? 0 : -1}
              aria-selected={n.active === id}
              title={t.hint}
              class="group/tab focus-visible:ring-ring/50 flex h-6 shrink-0 cursor-default items-center gap-1.5 rounded-md pr-1 pl-2 outline-none focus-visible:ring-2 {n.active ===
              id
                ? 'bg-card text-foreground ring-foreground/10 shadow-xs ring-1'
                : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground'} {drag?.on && drag.id === id ? 'opacity-50' : ''}"
              onpointerdown={(e) => down(e, id)}
              onpointermove={move}
              onpointerup={up}
              onclick={() => (dragged ? (dragged = false) : select(id, n.id))}
              onauxclick={(e) => e.button === 1 && t.closable !== false && onclose(id)}
              onkeydown={(e) => key(e, n, id)}
            >
              {#if t.icon}<t.icon class="size-3.5 shrink-0 {t.tint ?? ''}" />{/if}
              <span class="whitespace-nowrap">{t.title}</span>
              {#if t.closable !== false}
                <!-- Ungespeichert: Punkt, der beim Ueberfahren zum x wird. Per Tastatur schliesst Entf auf dem Tab. -->
                <button
                  tabindex="-1"
                  aria-label="{t.title} schließen"
                  class="hover:bg-secondary grid size-4 place-items-center rounded {t.dirty || n.active === id
                    ? ''
                    : 'opacity-0 group-hover/tab:opacity-100'}"
                  onclick={(e) => {
                    e.stopPropagation();
                    onclose(id);
                  }}
                >
                  {#if t.dirty}<span class="bg-primary size-1.5 rounded-full group-hover/tab:hidden"></span>{/if}
                  <XIcon class="size-3 {t.dirty ? 'hidden group-hover/tab:block' : ''}" />
                </button>
              {:else}
                <span class="w-1"></span>
              {/if}
            </div>
          {/each}
        </div>
        <!-- Der Bereich meldet nur, welche Gruppe zuletzt benutzt wurde; bedient wird der Inhalt. -->
        <!-- svelte-ignore a11y_interactive_supports_focus -->
        <div
          data-body={n.id}
          role="tabpanel"
          class="relative min-h-0 flex-1 overflow-hidden {busy ? 'pointer-events-none' : ''}"
          onpointerdown={() => n.active && onfocus?.(n.active, n.id)}
          onfocusin={() => n.active && onfocus?.(n.active, n.id)}
        >
          {#if n.active}
            {#key n.active}{@render panel(n.active)}{/key}
          {:else}
            {@render empty?.()}
          {/if}
        </div>
      </div>
    </div>
  {/if}
{/snippet}

<!-- Nur Buchhaltung fuer das Ziehen (Ereignisse steigen von Tabs und Trennleisten auf), keine eigene Bedienung. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={root}
  class="relative flex h-full min-h-0 w-full text-xs {drag?.on ? 'cursor-grabbing select-none' : ''}"
  onpointerdown={(e) => (sizing = !!(e.target as Element).closest('[role="separator"]'))}
  onlostpointercapture={() => {
    drag = hint = null;
    sizing = false;
  }}
>
  {@render node(layout, [], 1)}
  {#if hint}
    <div class="bg-primary/15 border-primary/60 pointer-events-none absolute z-20 rounded-md border" style={hint.style}></div>
  {/if}
</div>
