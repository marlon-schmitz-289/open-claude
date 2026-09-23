<script lang="ts" module>
  /** Gespeicherte Groesse lesen; localStorage kann fehlen oder werfen. */
  export function stored(key: string, fallback: number): number {
    try {
      return Number(localStorage.getItem(key)) || fallback;
    } catch {
      return fallback;
    }
  }
</script>

<script lang="ts">
  // Ziehleiste zwischen zwei Bereichen. size ist die Groesse des Bereichs, den die Leiste begrenzt;
  // invert, wenn dieser Bereich hinter der Leiste liegt (rechts bzw. unten).
  let {
    size = $bindable(),
    axis = "x",
    min = 120,
    max = Infinity,
    invert = false,
    key,
  }: { size: number; axis?: "x" | "y"; min?: number; max?: number; invert?: boolean; key?: string } =
    $props();

  let el: HTMLDivElement;

  /** Tatsaechlich dargestellte Groesse (CSS kann size begrenzen) und Obergrenze, bei der der Nachbar min behaelt. */
  function bounds() {
    const dim = (n: Element | null) => (n ? (axis === "x" ? n.getBoundingClientRect().width : n.getBoundingClientRect().height) : 0);
    const [before, after] = [dim(el.previousElementSibling), dim(el.nextElementSibling)];
    const shown = invert ? after : before;
    return { shown, limit: Math.min(max, before + after - min) };
  }
  const clamp = (v: number, limit: number) => Math.round(Math.min(limit, Math.max(min, v)));
  function save() {
    if (key)
      try {
        localStorage.setItem(key, String(size));
      } catch {}
  }

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    const pos = (p: PointerEvent) => (axis === "x" ? p.clientX : p.clientY);
    const start = pos(e);
    // Ab der sichtbaren Groesse ziehen, sonst passiert bei zu grossem gespeichertem Wert erst mal nichts.
    const { shown: from, limit } = bounds();

    const move = (m: PointerEvent) => {
      const d = pos(m) - start;
      size = clamp(from + (invert ? -d : d), limit);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      save();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  function onkeydown(e: KeyboardEvent) {
    const d = { ArrowLeft: -16, ArrowUp: -16, ArrowRight: 16, ArrowDown: 16 }[e.key];
    if (!d) return;
    e.preventDefault();
    const { shown, limit } = bounds();
    size = clamp(shown + (invert ? -d : d), limit);
    save();
  }
</script>

<!-- Fokussierbarer separator ist das ARIA-Muster "Window Splitter"; svelte kennt es nicht als interaktiv. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={el}
  role="separator"
  tabindex="0"
  aria-orientation={axis === "x" ? "vertical" : "horizontal"}
  aria-valuenow={size}
  aria-valuemin={min}
  aria-valuemax={Number.isFinite(max) ? max : undefined}
  onpointerdown={down}
  {onkeydown}
  class="hover:bg-primary/40 active:bg-primary/60 focus-visible:bg-primary/60 relative z-10 shrink-0 outline-none transition-colors {axis === 'x'
    ? '-mx-0.5 w-1 cursor-col-resize'
    : '-my-0.5 h-1 cursor-row-resize'}"
></div>
