<script lang="ts">
  import { fade } from "svelte/transition";

  // Clawds mit eigener Groesse, Geschwindigkeit und Richtung; negativer delay = schon unterwegs.
  const CRITTERS = [
    { size: 30, dur: 7, delay: 0, back: false, x: 10 },
    { size: 20, dur: 10, delay: -3.5, back: true, x: 30 },
    { size: 25, dur: 6, delay: -4.5, back: false, x: 50 },
    { size: 16, dur: 12, delay: -8, back: true, x: 70 },
    { size: 22, dur: 8.5, delay: -1.5, back: false, x: 85 },
  ];
</script>

<div out:fade={{ duration: 250 }} class="pointer-events-none absolute inset-0 grid place-items-center">
  <p class="text-muted-foreground font-mono text-xs">
    Claude startet<span class="dot">.</span><span class="dot">.</span><span class="dot">.</span>
  </p>
  <div class="absolute inset-x-6 bottom-8 h-10 overflow-hidden">
    {#each CRITTERS as c, i (i)}
      <div
        class="walker {c.back ? 'back' : ''}"
        style="--dur:{c.dur}s; --delay:{c.delay}s; --x:{c.x}%; height:{c.size}px"
      >
        <!-- Pixel-Clawd wie im Claude-Code-Logo, Beine in zwei Gruppen fuer den Laufschritt -->
        <svg class="clawd" viewBox="0 -1 12 7" height={c.size} shape-rendering="crispEdges" aria-hidden="true">
          <rect x="2" y="0" width="8" height="5" fill="#d97757" />
          <rect x="0" y="2" width="2" height="2" fill="#d97757" />
          <rect x="10" y="2" width="2" height="2" fill="#d97757" />
          <rect x="4" y="1.5" width="1" height="1.5" fill="#1f1611" />
          <rect x="7" y="1.5" width="1" height="1.5" fill="#1f1611" />
          <!-- Zwei Fussstellungen im Wechsel (aussen / innen), alle gleich kurz: trippeln statt Beine heben -->
          <!-- Nie zwei Fuesse nebeneinander, sonst wird daraus ein Stummel in der Mitte -->
          <g class="step-a" fill="#d97757">
            <rect x="2" y="5" width="1" height="1" />
            <rect x="4" y="5" width="1" height="1" />
            <rect x="7" y="5" width="1" height="1" />
            <rect x="9" y="5" width="1" height="1" />
          </g>
          <g class="step-b" fill="#d97757">
            <rect x="3" y="5" width="1" height="1" />
            <rect x="8" y="5" width="1" height="1" />
          </g>
        </svg>
      </div>
    {/each}
  </div>
</div>

<style>
  .walker {
    position: absolute;
    bottom: 0;
    animation: walk var(--dur) linear var(--delay) infinite;
  }
  /* Rueckwaerts laufen und dabei in Laufrichtung schauen. */
  .walker.back {
    animation-direction: reverse;
  }
  .walker.back .clawd {
    scale: -1 1;
  }
  @keyframes walk {
    from {
      left: -40px;
    }
    to {
      left: 100%;
    }
  }

  .clawd {
    display: block;
    animation: bob 0.36s steps(1) infinite;
  }
  @keyframes bob {
    50% {
      translate: 0 -1px;
    }
  }
  .step-a,
  .step-b {
    animation: stride 0.36s steps(1) infinite;
  }
  .step-b {
    animation-delay: -0.18s;
  }
  @keyframes stride {
    50% {
      opacity: 0;
    }
  }

  .dot {
    animation: blink 1.2s infinite;
  }
  .dot:nth-child(2) {
    animation-delay: 0.2s;
  }
  .dot:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes blink {
    0%,
    100% {
      opacity: 0.2;
    }
    40% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .walker,
    .clawd,
    .step-a,
    .step-b,
    .dot {
      animation: none;
    }
    .step-b {
      display: none;
    }
    .walker {
      left: var(--x);
    }
  }
</style>
