<script lang="ts">
  // Portiert aus akiosk-frontend/src/components/IdleAmongUs.vue — gleiche Timings und Physik.
  import { onMount } from "svelte";

  const FIRST_SPAWN = 15000;
  const NEXT_MIN = 5000;
  const NEXT_MAX = 12000;
  const IDLE_SPEED_MIN = 0.15;
  const IDLE_SPEED_MAX = 0.35;
  const PANIC_MULT = 2.2;
  const BODY_LINGER = 6000;
  const MIN_Y_GAP = 8;
  const FADE_EDGE = 15; // Prozent vom Rand, ab dem Bild und Ton ausblenden

  type Crewmate = {
    id: number;
    x: number;
    y: number;
    speed: number;
    goingRight: boolean;
    dead: boolean;
    deadAt: number;
    bobOffset: number;
    bobSpeed: number;
    walkSource: AudioBufferSourceNode | null;
    walkGain: GainNode | null;
  };

  let crew = $state<Crewmate[]>([]);
  let nextId = 0;
  let idleTimer: ReturnType<typeof setTimeout> | null = null;
  let animFrame: number | null = null;
  let userActive = false;
  let audioCtx: AudioContext | null = null;
  let killBuffer: AudioBuffer | null = null;
  let walkBuffer: AudioBuffer | null = null;

  function onActivity() {
    userActive = true;
    if (idleTimer) clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      userActive = false;
      scheduleNext();
    }, FIRST_SPAWN);
  }

  function scheduleNext() {
    if (userActive) return;
    idleTimer = setTimeout(spawn, NEXT_MIN + Math.random() * (NEXT_MAX - NEXT_MIN));
  }

  function findSafeY(): number {
    const taken = crew.filter((c) => !c.dead).map((c) => c.y);
    for (let attempt = 0; attempt < 20; attempt++) {
      const y = 20 + Math.random() * 60;
      if (taken.every((oy) => Math.abs(oy - y) >= MIN_Y_GAP)) return y;
    }
    return 20 + Math.random() * 60;
  }

  function startWalkSound(c: Crewmate) {
    if (!audioCtx || !walkBuffer) return;
    const source = audioCtx.createBufferSource();
    source.buffer = walkBuffer;
    source.loop = true;
    source.playbackRate.value = 0.9 + Math.random() * 0.2;
    const gain = audioCtx.createGain();
    gain.gain.value = 0;
    source.connect(gain);
    gain.connect(audioCtx.destination);
    source.start(0, Math.random() * walkBuffer.duration);
    c.walkSource = source;
    c.walkGain = gain;
  }

  function stopWalkSound(c: Crewmate) {
    if (c.walkSource) {
      try {
        c.walkSource.stop();
      } catch {
        /* schon gestoppt */
      }
      c.walkSource = null;
    }
    c.walkGain = null;
  }

  /** Lautstaerke und Sichtbarkeit haengen am Abstand zum Fensterrand. */
  function edgeFade(x: number): number {
    if (x < 0 || x > 100) return 0;
    if (x < FADE_EDGE) return x / FADE_EDGE;
    if (x > 100 - FADE_EDGE) return (100 - x) / FADE_EDGE;
    return 1;
  }

  function spawn() {
    if (userActive) return;
    const goingRight = Math.random() > 0.5;
    const c: Crewmate = {
      id: nextId++,
      x: goingRight ? -8 : 108,
      y: findSafeY(),
      speed: IDLE_SPEED_MIN + Math.random() * (IDLE_SPEED_MAX - IDLE_SPEED_MIN),
      goingRight,
      dead: false,
      deadAt: 0,
      bobOffset: Math.random() * Math.PI * 2, // zufaellige Phase, sonst wippen alle im Takt
      bobSpeed: 0.06 + Math.random() * 0.03,
      walkSource: null,
      walkGain: null,
    };
    crew.push(c);
    startWalkSound(c);
    if (animFrame === null) animFrame = requestAnimationFrame(move);
    scheduleNext();
  }

  let frame = 0;
  function move() {
    const now = Date.now();
    frame++;

    for (const c of crew) {
      if (c.dead) continue;
      c.x += (c.goingRight ? 1 : -1) * (userActive ? c.speed * PANIC_MULT : c.speed);
      c.bobOffset += c.bobSpeed * (userActive ? PANIC_MULT : 1);
      if (c.walkGain) c.walkGain.gain.value = edgeFade(c.x) * 0.6;
      if (c.walkSource && frame % 30 === 0) {
        c.walkSource.playbackRate.value = userActive
          ? 1.3 + Math.random() * 0.1
          : 0.9 + Math.random() * 0.05;
      }
    }

    for (let i = crew.length - 1; i >= 0; i--) {
      const c = crew[i];
      const gone = c.goingRight ? c.x > 110 : c.x < -10;
      if ((c.dead && now - c.deadAt > BODY_LINGER) || (!c.dead && gone)) {
        stopWalkSound(c);
        crew.splice(i, 1);
      }
    }

    animFrame = crew.length > 0 ? requestAnimationFrame(move) : null;
  }

  function kill(c: Crewmate) {
    if (c.dead) return;
    c.dead = true;
    c.deadAt = Date.now();
    stopWalkSound(c);
    if (audioCtx && killBuffer) {
      const source = audioCtx.createBufferSource();
      source.buffer = killBuffer;
      source.connect(audioCtx.destination);
      source.start(0, 0.25);
    }
  }

  async function preload(url: string) {
    const buf = await fetch(url).then((r) => r.arrayBuffer());
    return audioCtx!.decodeAudioData(buf);
  }

  onMount(() => {
    audioCtx = new AudioContext();
    preload("/easteregg/killsound.mp3").then((b) => (killBuffer = b), () => {});
    preload("/easteregg/walking.mp3").then((b) => (walkBuffer = b), () => {});

    const events = ["mousemove", "mousedown", "keydown", "touchstart", "scroll"];
    // Capture: xterm stoppt die Propagation seiner Tasten-Events, sonst zaehlt Tippen im Terminal nicht.
    for (const e of events) window.addEventListener(e, onActivity, { passive: true, capture: true });
    idleTimer = setTimeout(spawn, FIRST_SPAWN);

    return () => {
      for (const e of events) window.removeEventListener(e, onActivity, { capture: true });
      if (idleTimer) clearTimeout(idleTimer);
      if (animFrame !== null) cancelAnimationFrame(animFrame);
      crew.forEach(stopWalkSound);
      audioCtx?.close();
    };
  });
</script>

<!-- Laufbahn: unterhalb von Titel- und Suchleiste, oberhalb der Fusszeile. -->
<div class="pointer-events-none fixed inset-x-0 top-[85px] bottom-[41px] z-[998]">
{#each crew as c (c.id)}
  {#if c.dead}
    <img
      src="/easteregg/body.png"
      alt=""
      class="pointer-events-none absolute w-[55px] select-none transition-opacity duration-1500"
      style="left:{c.x}%; top:{c.y}%; transform:scaleX({c.goingRight ? -1 : 1}); opacity:{Date.now() -
        c.deadAt >
      BODY_LINGER - 1500
        ? 0.4
        : 1}"
    />
  {:else}
    <!-- Reine Deko: nichts geht verloren, wenn man es nicht anklickt. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <img
      src="/easteregg/among-us.gif"
      alt=""
      draggable="false"
      class="pointer-events-auto absolute z-[1] w-[60px] cursor-pointer select-none"
      style="left:{c.x}%; top:calc({c.y}% + {Math.sin(c.bobOffset) *
        3}px); transform:scaleX({c.goingRight ? -1 : 1}); opacity:{edgeFade(c.x)}"
      onmousedown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        kill(c);
      }}
    />
  {/if}
{/each}
</div>
