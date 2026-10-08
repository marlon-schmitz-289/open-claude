<script lang="ts">
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { forge, accountId, type Account, type RepoLang } from "$lib/git";
  import { FolderField } from "$lib/components/kit";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";

  let {
    open = $bindable(false),
    accounts,
    root,
    oncreate,
    onaccounts,
  }: {
    open: boolean;
    accounts: Account[];
    root: string;
    oncreate: (path: string) => void;
    onaccounts: () => void;
  } = $props();

  // Gleiche Regel wie valid_name in create.rs
  const NAME = /^[A-Za-z0-9_](?:[A-Za-z0-9._-]{0,98}[A-Za-z0-9_-])?$/;

  let name = $state("");
  let dest = $state("");
  let destTouched = $state(false); // sobald der Nutzer den Zielordner selbst aendert, nicht mehr automatisch mitfuehren
  let lang = $state("none");
  let langs = $state<RepoLang[]>([]);
  let mit = $state(false);
  let account = $state<Account | null>(null); // null = nur lokal
  let priv = $state(true);
  let busy = $state(false);
  let status = $state("");
  let error = $state("");
  let done = $state(false); // lokal angelegt, Remote gescheitert: nur noch schliessen

  const valid = $derived(NAME.test(name) && !!dest.trim());
  const missing = $derived(langs.find((l) => l.id === lang && l.tool && !l.found));

  $effect(() => {
    if (open && !langs.length) forge.langs().then((l) => (langs = l), (e) => (error = String(e)));
  });

  // Nach "Lokal angelegt, Remote fehlgeschlagen" beim Schliessen frisch machen, egal wie geschlossen.
  $effect(() => {
    if (!open && done) reset();
  });

  // Zielordner nachfuehren, solange der Nutzer ihn nicht selbst angefasst hat.
  $effect(() => {
    if (destTouched) return;
    dest = name ? `${root}/${name}` : "";
  });

  async function browseDest() {
    const picked = await pickFolder({ directory: true, defaultPath: dest || root });
    if (typeof picked !== "string") return;
    dest = picked;
    destTouched = true;
  }

  async function doCreate() {
    if (busy || done || !valid) return;
    busy = true;
    error = "";
    status = "Lege Repo an…";
    // Auswahl festhalten, Controls sind waehrend busy zwar gesperrt, aber sicher ist sicher.
    const acc = account, p = priv;
    try {
      const id = acc ? accountId(acc) : null;
      const path = await forge.create(dest.trim(), name, lang, mit, id);
      let failed = "";
      if (acc) {
        status = `Lege auf ${acc.host} an…`;
        failed = await forge.createRemote(path, name, p).then(() => "", String);
      }
      oncreate(path);
      // Lokal steht; erneutes Anlegen scheiterte an "nicht leer", daher nur noch schliessen.
      if (failed) {
        error = `Lokal angelegt, Remote fehlgeschlagen: ${failed}`;
        done = true;
        return;
      }
      open = false;
      reset();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      status = "";
    }
  }

  function reset() {
    name = "";
    dest = "";
    destTouched = false;
    lang = "none";
    mit = false;
    account = null;
    priv = true;
    error = "";
    done = false;
  }
</script>

<!-- Waehrend busy nicht schliessbar, sonst ginge ein Remote-Fehler verloren. -->
<Dialog.Root bind:open={() => open, (v) => { if (!busy) open = v; }}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Neues Repository</Dialog.Title>
      <Dialog.Description>Lokal anlegen, optional auf GitHub/GitLab.</Dialog.Description>
    </Dialog.Header>

    <fieldset disabled={busy} class="grid grid-cols-[auto_1fr] items-start gap-x-3 gap-y-2.5 text-xs">
      <span class="text-muted-foreground pt-2">Name</span>
      <div>
        <Input
          bind:value={name}
          placeholder="my-app"
          class="font-mono text-xs"
          onkeydown={(e) => e.key === "Enter" && doCreate()}
        />
        {#if name && !NAME.test(name)}
          <p class="text-destructive mt-1 text-[11px]">Nur Buchstaben, Ziffern, . _ -</p>
        {/if}
      </div>

      <span class="text-muted-foreground pt-2">Ordner</span>
      <FolderField bind:value={dest} oninput={() => (destTouched = true)} onbrowse={browseDest} />

      <span class="text-muted-foreground pt-2">Sprache</span>
      <div>
        <div class="flex flex-wrap gap-1.5">
          {#each langs as l (l.id)}
            {@const off = l.tool && !l.found}
            <Button
              variant={lang === l.id ? "default" : "outline"}
              size="sm"
              class="text-xs {off && lang !== l.id ? 'text-muted-foreground' : ''}"
              title={off ? `${l.tool} nicht gefunden: nur .gitignore + README` : undefined}
              onclick={() => (lang = l.id)}>{l.label}</Button
            >
          {/each}
        </div>
        {#if missing}
          <p class="text-muted-foreground mt-1 text-[11px]">{missing.tool} nicht gefunden: nur .gitignore + README</p>
        {/if}
      </div>

      <span></span>
      <label class="flex items-center gap-2">
        <input type="checkbox" bind:checked={mit} class="accent-primary" />
        MIT-Lizenz
      </label>

      <span class="text-muted-foreground pt-2">Remote</span>
      <div class="space-y-1.5">
        <div class="flex flex-wrap gap-1.5">
          <Button variant={account ? "outline" : "default"} size="sm" class="text-xs" onclick={() => (account = null)}
            >Nur lokal</Button
          >
          {#each accounts as a (a.kind + a.host + a.user)}
            <Button variant={account === a ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (account = a)}>
              {a.user}<span class="text-muted-foreground ml-1 font-mono text-[10px]">@{a.host}</span>
            </Button>
          {/each}
          <Button variant="outline" size="sm" class="text-xs" onclick={onaccounts}>Konto hinzufügen…</Button>
        </div>
        {#if account}
          <div class="flex gap-1.5">
            <Button variant={priv ? "default" : "outline"} size="sm" class="text-xs" onclick={() => (priv = true)}
              >Privat</Button
            >
            <Button variant={priv ? "outline" : "default"} size="sm" class="text-xs" onclick={() => (priv = false)}
              >Öffentlich</Button
            >
          </div>
        {/if}
      </div>
    </fieldset>

    {#if status}
      <p class="text-muted-foreground flex items-center gap-1.5 text-xs">
        <LoaderCircleIcon class="size-3.5 animate-spin" />{status}
      </p>
    {/if}
    {#if error}<p class="text-destructive text-xs whitespace-pre-wrap">{error}</p>{/if}

    <div class="flex justify-end gap-1.5">
      <Button
        variant="outline"
        size="sm"
        class="text-xs"
        onclick={() => (open = false)}
        disabled={busy}>{done ? "Schließen" : "Abbrechen"}</Button
      >
      <Button size="sm" class="gap-1.5 text-xs" disabled={busy || done || !valid} onclick={doCreate}>
        {#if busy}<LoaderCircleIcon class="size-3.5 animate-spin" />{/if}
        Anlegen
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
