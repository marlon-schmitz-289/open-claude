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

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const pos = (p: PointerEvent) => (axis === "x" ? p.clientX : p.clientY);
    const start = pos(e);
    const from = size;
    // Der andere Bereich behaelt mindestens min.
    const box = el.parentElement!.getBoundingClientRect();
    const limit = Math.min(max, (axis === "x" ? box.width : box.height) - min);

    const move = (m: PointerEvent) => {
      const d = pos(m) - start;
      size = Math.round(Math.min(limit, Math.max(min, from + (invert ? -d : d))));
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      if (key)
        try {
          localStorage.setItem(key, String(size));
        } catch {}
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
</script>

<div
  role="separator"
  aria-orientation={axis === "x" ? "vertical" : "horizontal"}
  onpointerdown={down}
  class="hover:bg-primary/40 active:bg-primary/60 relative z-10 shrink-0 transition-colors {axis === 'x'
    ? '-mx-0.5 w-1 cursor-col-resize'
    : '-my-0.5 h-1 cursor-row-resize'}"
></div>
