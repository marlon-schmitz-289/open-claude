<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Switch } from "$lib/components/kit";
  import TrashIcon from "@lucide/svelte/icons/trash-2";
  import { editor, setProfile, setProfiles } from "$lib/editor.svelte";
  import { BUILTIN, parseArgs, type Profile } from "$lib/run.logic";

  let { open = $bindable(false), repo }: { open: boolean; repo: string } = $props();

  const s = $derived(editor(repo));
  let name = $state("");
  let release = $state(false);
  let args = $state("");

  const parsed = $derived(parseArgs(args));
  const taken = $derived([...BUILTIN, ...s.profiles].some((p) => p.name === name.trim()));
  const error = $derived(parsed === null ? "Argumente ohne Leerzeichen, Quotes oder Shell-Zeichen (; & | $ % …)" : taken ? "Name gibt es schon" : "");

  const describe = (p: Profile) =>
    [p.release ? "Release (--release / -c Release)" : "Debug", ...(p.args.length ? [p.args.join(" ")] : [])].join(" · ");

  function add() {
    if (!name.trim() || error || !parsed) return;
    setProfiles(repo, [...s.profiles, { name: name.trim(), release, args: parsed }]);
    setProfile(repo, name.trim());
    name = args = "";
    release = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Build-Profile</Dialog.Title>
      <Dialog.Description
        >Release hängt bei Cargo <code>--release</code> und bei .NET <code>-c Release</code> an. Zusatzargumente kommen hinten dran, z. B.
        <code>--features x</code> oder <code>-- --port=3000</code>. Gilt für dieses Projekt.</Dialog.Description
      >
    </Dialog.Header>

    <ul class="border-border divide-border divide-y rounded border text-xs">
      {#each [...BUILTIN, ...s.profiles] as p, i (p.name)}
        <li class="flex items-center gap-2 px-2.5 py-1.5">
          <span class="font-medium">{p.name}</span>
          <span class="text-muted-foreground min-w-0 flex-1 truncate font-mono text-[11px]" title={describe(p)}>{describe(p)}</span>
          {#if i >= BUILTIN.length}
            <Button
              variant="ghost"
              size="icon"
              class="text-muted-foreground hover:text-destructive size-6"
              aria-label="Profil {p.name} löschen"
              onclick={() => setProfiles(repo, s.profiles.filter((x) => x.name !== p.name))}><TrashIcon class="size-3.5" /></Button
            >
          {/if}
        </li>
      {/each}
    </ul>

    <div class="flex flex-col gap-2">
      <Input bind:value={name} placeholder="Name, z. B. Profiling" class="text-xs" />
      <label class="flex items-center gap-2 text-xs">
        <Switch on={release} label="Release" onclick={() => (release = !release)} />
        Release
      </label>
      <Input bind:value={args} placeholder="Zusatzargumente (optional)" class="font-mono text-xs" />
      {#if error}<p class="text-destructive text-xs">{error}</p>{/if}
      <Button size="sm" class="text-xs" disabled={!name.trim() || !!error} onclick={add}>Profil anlegen</Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
