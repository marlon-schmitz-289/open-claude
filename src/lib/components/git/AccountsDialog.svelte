<script lang="ts">
  import type { Store } from "@tauri-apps/plugin-store";
  import { forge, type Account, type ForgeKind } from "$lib/git";
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Badge } from "$lib/components/ui/badge/index.js";
  import TrashIcon from "@lucide/svelte/icons/trash-2";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import GlobeIcon from "@lucide/svelte/icons/globe";

  let {
    open = $bindable(false),
    store,
    accounts = $bindable<Account[]>([]),
  }: { open: boolean; store: Store; accounts: Account[] } = $props();

  let kind = $state<ForgeKind>("github");
  let host = $state("github.com");
  let token = $state("");
  let busy = $state(false);
  let error = $state("");

  // Host-Feld beim Wechsel des Anbieters auf den ueblichen Default setzen.
  $effect(() => {
    if (kind === "github" && host === "gitlab.com") host = "github.com";
    if (kind === "gitlab" && host === "github.com") host = "gitlab.com";
  });

  const bare = $derived(host.trim().replace(/^https?:\/\//i, "").replace(/\/+$/, ""));
  const tokenUrl = $derived(
    kind === "github"
      ? `https://${bare}/settings/tokens/new?scopes=repo,read:org&description=ocui`
      : `https://${bare}/-/user_settings/personal_access_tokens?name=ocui&scopes=read_api,read_repository`,
  );

  async function persist(next: Account[]) {
    accounts = next;
    await store.set("accounts", accounts);
  }

  function upsert(list: Account[], a: Account): Account[] {
    const rest = list.filter((x) => !(x.kind === a.kind && x.host === a.host));
    return [...rest, a];
  }

  async function login() {
    if (!host.trim() || !token.trim()) return;
    busy = true;
    error = "";
    try {
      const a = await forge.login(kind, host.trim(), token.trim());
      await persist(upsert(accounts, a));
      host = a.host;
      token = "";
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }

  async function importCli() {
    busy = true;
    error = "";
    try {
      const a = await forge.importCli(kind, host.trim());
      await persist(upsert(accounts, a));
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }

  async function logout(a: Account) {
    busy = true;
    error = "";
    try {
      await forge.logout(a.kind, a.host);
      await persist(accounts.filter((x) => !(x.kind === a.kind && x.host === a.host)));
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Konten</Dialog.Title>
      <Dialog.Description>GitHub- und GitLab-Zugänge für Klonen und Pull Requests.</Dialog.Description>
    </Dialog.Header>

    {#if accounts.length}
      <ul class="border-border divide-border divide-y rounded border text-sm">
        {#each accounts as a (a.kind + a.host + a.user)}
          <li class="flex items-center gap-2 px-2.5 py-1.5">
            <GlobeIcon class="size-3.5 shrink-0" />
            <span class="min-w-0 flex-1 truncate">
              <span class="font-medium">{a.user}</span>
              <span class="text-muted-foreground font-mono text-[11px]">@{a.host}</span>
              <Badge variant="outline" class="text-muted-foreground ml-1 text-[10px]"
                >{a.kind === "github" ? "GitHub" : "GitLab"}</Badge
              >
            </span>
            <Button
              variant="ghost"
              size="icon"
              class="text-muted-foreground hover:text-destructive size-6"
              disabled={busy}
              onclick={() => logout(a)}
              aria-label="Konto entfernen"
            >
              <TrashIcon class="size-3.5" />
            </Button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="flex flex-col gap-2">
      <div class="flex gap-1.5">
        <Button
          variant={kind === "github" ? "default" : "outline"}
          size="sm"
          class="flex-1 text-xs"
          onclick={() => (kind = "github")}>GitHub</Button
        >
        <Button
          variant={kind === "gitlab" ? "default" : "outline"}
          size="sm"
          class="flex-1 text-xs"
          onclick={() => (kind = "gitlab")}>GitLab</Button
        >
      </div>
      <Input bind:value={host} placeholder="Host, z. B. github.com" class="font-mono text-xs" />
      <Input bind:value={token} type="password" placeholder="Access Token" class="font-mono text-xs" />
      <!-- Die Webview oeffnet target=_blank nicht, daher ueber den Standardbrowser. -->
      <button
        type="button"
        onclick={() => forge.openUrl(tokenUrl).catch((e) => (error = String(e)))}
        class="text-muted-foreground hover:text-primary inline-flex items-center gap-1 self-start text-[11px]"
      >
        <ExternalLinkIcon class="size-3" /> Token mit den nötigen Rechten erstellen ({kind === "github"
          ? "repo, read:org"
          : "read_api, read_repository"})
      </button>
      {#if error}<p class="text-destructive text-xs">{error}</p>{/if}
      <div class="flex gap-1.5">
        <Button size="sm" class="flex-1 text-xs" disabled={busy || !host.trim() || !token.trim()} onclick={login}
          >Anmelden</Button
        >
        <Button variant="outline" size="sm" class="text-xs" disabled={busy} onclick={importCli}
          >Von {kind === "github" ? "gh" : "glab"} übernehmen</Button
        >
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>
