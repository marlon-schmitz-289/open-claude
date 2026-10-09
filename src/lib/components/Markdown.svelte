<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  // Pfad -> data:-URL; beim Streamen rendert der Text staendig neu, die Datei wird trotzdem nur einmal gelesen.
  const local = new Map<string, Promise<string>>();
</script>

<script lang="ts">
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { forge } from "$lib/git";

  /** base: Seite, gegen die relative Links aufgeloest werden (z. B. Release-URL). size: Schriftgroesse (Tailwind).
   *  root: Ordner, gegen den relative Bildpfade aufgeloest werden (Projekt des Chats). */
  let {
    text,
    base,
    root,
    onerror,
    size = "text-[11px]",
  }: { text: string; base?: string; root?: string; onerror?: (e: string) => void; size?: string } = $props();

  // Fremdes Markdown: ohne DOMPurify waere {@html} XSS mit Zugriff auf alle Tauri-Commands.
  const html = $derived(DOMPurify.sanitize(marked.parse(text, { gfm: true, breaks: true, async: false })));

  // Claude verlinkt Bilder als lokale Pfade (/Users/..., C:\..., file://, out/x.png); die WebView kann die nicht laden.
  let el: HTMLDivElement;
  $effect(() => {
    void html;
    if (base) return;
    for (const img of el.querySelectorAll("img")) {
      // file:///C:/x -> C:/x, file:///Users/x -> /Users/x
      const src = (img.getAttribute("src") ?? "").replace(/^file:\/\/(\/(?=[A-Za-z]:))?/, "");
      let path: string;
      try {
        path = decodeURI(src);
      } catch {
        continue;
      }
      // Relativ (ohne Schema wie https:/data:) gegen root, absolut direkt.
      if (!/^(\/(?!\/)|[A-Za-z]:[\\/])/.test(path)) {
        if (!root || /^([a-z][\w+.-]*:|\/\/)/i.test(path)) continue;
        path = `${root.replace(/[\\/]+$/, "")}/${path}`;
      }
      // Fehlschlag nicht merken: das Bild entsteht evtl. erst noch.
      if (!local.has(path)) local.set(path, invoke<string>("chat_image", { path }).catch((e) => (local.delete(path), Promise.reject(e))));
      local.get(path)!.then((u) => (img.src = u)).catch(() => {});
    }
  });

  // Links nie in der App-Webview navigieren, sondern im System-Browser oeffnen.
  function click(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href");
    if (!href || href.startsWith("#")) return;
    let url: string;
    try {
      url = new URL(href, base).href;
    } catch {
      return;
    }
    forge.openUrl(url).catch((err) => onerror?.(String(err)));
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div bind:this={el} class="md {size} break-words select-text" onclick={click}>{@html html}</div>

<style>
  .md :global(:where(h1, h2, h3, h4, h5, h6)) {
    font-weight: 600;
    margin: 0.9em 0 0.4em;
  }
  .md :global(h1) { font-size: 1.4em; }
  .md :global(h2) { font-size: 1.25em; }
  .md :global(h3) { font-size: 1.1em; }
  .md :global(:where(h1, h2)) {
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.2em;
  }
  .md :global(:where(p, ul, ol, pre, blockquote, table)) { margin: 0.5em 0; }
  .md :global(:first-child) { margin-top: 0; }
  .md :global(ul) { list-style: disc; padding-left: 1.5em; }
  .md :global(ol) { list-style: decimal; padding-left: 1.5em; }
  .md :global(li > :where(ul, ol)) { margin: 0.1em 0; }
  .md :global(a) { color: var(--primary); text-decoration: underline; cursor: pointer; }
  .md :global(code) {
    font-family: var(--font-mono, ui-monospace, monospace);
    background: var(--muted);
    border-radius: 3px;
    padding: 0.1em 0.3em;
  }
  .md :global(pre) {
    background: var(--muted);
    border-radius: 4px;
    padding: 0.6em 0.8em;
    overflow-x: auto;
  }
  .md :global(pre code) { background: none; padding: 0; }
  .md :global(blockquote) {
    border-left: 3px solid var(--border);
    padding-left: 0.8em;
    color: var(--muted-foreground);
  }
  .md :global(table) { border-collapse: collapse; }
  .md :global(:where(th, td)) { border: 1px solid var(--border); padding: 0.2em 0.5em; }
  .md :global(hr) { border-color: var(--border); margin: 0.8em 0; }
  .md :global(img) { max-width: 100%; }
  .md :global(input[type="checkbox"]) { margin-right: 0.3em; }
</style>
