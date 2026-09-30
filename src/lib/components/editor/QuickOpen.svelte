<script lang="ts">
  // Strg+P: Datei des Projekts per Fuzzy-Suche oeffnen. Die Liste kommt bei jedem Oeffnen frisch (git ls-files, ohne Git alle Dateien).
  import * as Command from "$lib/components/ui/command/index.js";
  import { fuzzy } from "$lib/fuzzy";
  import { fs } from "$lib/files";
  import { editor, open as openFile } from "$lib/editor.svelte";

  let { repo, open = $bindable(false) }: { repo: string; open: boolean } = $props();

  let files = $state<string[]>([]);
  let query = $state("");

  $effect(() => {
    if (!open) return;
    query = "";
    fs.files(repo).then(
      (f) => (files = f),
      (e) => (editor(repo).error = String(e)),
    );
  });

  // ponytail: jede Eingabe sucht linear ueber alle Dateien; bei sehr grossen Repos entprellen oder in Rust suchen.
  const hits = $derived.by(() => {
    const q = query.trim();
    if (!q) return files.slice(0, 50);
    return files
      .flatMap((path) => {
        const m = fuzzy(path, q);
        return m ? [{ path, score: m.score }] : [];
      })
      .sort((a, b) => b.score - a.score)
      .slice(0, 50)
      .map((h) => h.path);
  });

  function pick(path: string) {
    open = false;
    openFile(repo, path);
  }
</script>

<Command.Dialog bind:open shouldFilter={false} title="Datei öffnen" description="Datei im Projekt suchen">
  <Command.Input bind:value={query} placeholder="Datei öffnen" />
  <Command.List>
    <Command.Empty>Keine Datei gefunden.</Command.Empty>
    {#each hits as path (path)}
      {@const cut = path.lastIndexOf("/") + 1}
      <Command.Item value={path} onSelect={() => pick(path)}>
        <span class="truncate">{path.slice(cut)}</span>
        <span class="text-muted-foreground min-w-0 flex-1 truncate text-xs">{path.slice(0, cut)}</span>
      </Command.Item>
    {/each}
  </Command.List>
</Command.Dialog>
