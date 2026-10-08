<script lang="ts">
  // Bild mit eigenem Zoom fuers Touchpad: Pinch bzw. Strg/Cmd+Rad zoomt um den Zeiger, zwei Finger oder Ziehen
  // verschieben, Doppelklick wechselt 1x/2.5x, waagerechtes Wischen blaettert (ungezoomt).
  // data-own-zoom: der App-Zoom in +page.svelte laesst Ereignisse ueber dieser Flaeche in Ruhe.
  let { src, alt, onswipe }: { src: string; alt: string; onswipe?: (dir: number) => void } = $props();

  let box: HTMLDivElement;
  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);

  $effect(() => {
    void src;
    scale = 1;
    x = y = 0;
  });

  /** Um Faktor f zoomen; der Punkt (cx, cy) bleibt unter dem Zeiger (transform-origin ist die Mitte). */
  function zoomAt(f: number, cx: number, cy: number) {
    const r = box.getBoundingClientRect();
    const next = Math.min(8, Math.max(1, scale * f));
    const k = next / scale;
    const px = cx - r.left - r.width / 2;
    const py = cy - r.top - r.height / 2;
    x = next === 1 ? 0 : px - (px - x) * k;
    y = next === 1 ? 0 : py - (py - y) * k;
    scale = next;
  }

  // Touchpad-Wischen hat Nachlauf: nach einem Blaettern kurz nichts mehr zaehlen.
  let swipe = 0;
  let quietUntil = 0;
  function wheel(e: WheelEvent) {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) return zoomAt(Math.exp(-e.deltaY * 0.01), e.clientX, e.clientY);
    if (scale > 1) {
      x -= e.deltaX;
      y -= e.deltaY;
      return;
    }
    if (e.timeStamp < quietUntil || Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return;
    swipe += e.deltaX;
    if (Math.abs(swipe) < 60) return;
    onswipe?.(Math.sign(swipe));
    swipe = 0;
    quietUntil = e.timeStamp + 500;
  }

  // WebKit (macOS) meldet Pinch zusaetzlich als GestureEvent mit kumulativem scale.
  type Gesture = UIEvent & { scale: number; clientX: number; clientY: number };
  let gestureScale = 1;
  function gesture(e: Event) {
    const g = e as Gesture;
    e.preventDefault();
    if (e.type === "gesturestart") gestureScale = 1;
    else if (e.type === "gesturechange") {
      zoomAt(g.scale / gestureScale, g.clientX, g.clientY);
      gestureScale = g.scale;
    }
  }

  let drag = $state<{ id: number; x: number; y: number } | null>(null);
  function down(e: PointerEvent) {
    if (scale === 1 || e.button !== 0) return;
    e.preventDefault();
    box.setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, x: e.clientX - x, y: e.clientY - y };
  }
  function move(e: PointerEvent) {
    if (drag?.id !== e.pointerId) return;
    x = e.clientX - drag.x;
    y = e.clientY - drag.y;
  }

  function gestures(node: HTMLElement) {
    // Nicht passiv, sonst wirkt preventDefault nicht.
    const types = ["wheel", "gesturestart", "gesturechange", "gestureend"] as const;
    const fn = (e: Event) => (e.type === "wheel" ? wheel(e as WheelEvent) : gesture(e));
    for (const t of types) node.addEventListener(t, fn, { passive: false });
    return { destroy: () => types.forEach((t) => node.removeEventListener(t, fn)) };
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={box}
  data-own-zoom
  use:gestures
  class="relative flex h-[75vh] w-full touch-none items-center justify-center overflow-hidden rounded-md select-none {scale > 1
    ? drag
      ? 'cursor-grabbing'
      : 'cursor-grab'
    : ''}"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={() => (drag = null)}
  onpointercancel={() => (drag = null)}
  ondblclick={(e) => zoomAt(scale > 1 ? 1 / scale : 2.5, e.clientX, e.clientY)}
>
  <img
    {src}
    {alt}
    draggable="false"
    class="max-h-full max-w-full object-contain will-change-transform"
    style="transform: translate({x}px, {y}px) scale({scale})"
  />
  {#if scale > 1}
    <span class="bg-background/80 text-muted-foreground pointer-events-none absolute right-2 bottom-2 rounded px-1.5 py-0.5 text-[11px] tabular-nums"
      >{Math.round(scale * 100)}%</span
    >
  {/if}
</div>
