// Vertrag zwischen Frontend und src-tauri/src/git.rs + forge.rs.
// Namen und Felder hier sind verbindlich; Rust serialisiert exakt diese Formen (snake_case-Felder).
import { invoke } from "@tauri-apps/api/core";

/** Eine Datei aus `git status --porcelain=v2`. index/worktree = XY-Zeichen ("." = unveraendert, "?" = untracked). */
export type FileChange = {
  path: string;
  /** Alter Pfad bei Umbenennung. */
  orig: string | null;
  index: string;
  worktree: string;
  conflict: boolean;
};

export type RepoState = "clean" | "merge" | "rebase" | "cherry-pick" | "revert";

export type Status = {
  /** null bei detached HEAD. */
  branch: string | null;
  head: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  state: RepoState;
  files: FileChange[];
};

export type Commit = {
  sha: string;
  parents: string[];
  author: string;
  email: string;
  /** ISO 8601 (Author-Datum). */
  date: string;
  subject: string;
  /** Dekorationen wie "HEAD -> main", "origin/main", "tag: v1". */
  refs: string[];
};

export type CommitDetail = { commit: Commit; body: string; diff: string };

export type Branch = {
  /** Lokal "feature/x", remote "origin/feature/x". */
  name: string;
  remote: boolean;
  current: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
  sha: string;
  date: string;
  subject: string;
  /** Commits, die noch nicht in `base` (origin/HEAD bzw. main/master) sind; null ohne Basis oder bei git < 2.41. */
  unmerged: number | null;
  base: string | null;
};

export type Stash = { index: number; message: string; date: string; branch: string };
export type Tag = { name: string; sha: string; date: string; message: string };
export type Remote = { name: string; url: string };

/** Drei Staende einer Konfliktdatei plus die Arbeitsdatei mit Markern. */
export type Conflict = {
  path: string;
  base: string | null;
  ours: string | null;
  theirs: string | null;
  merged: string;
};

export type ResetMode = "soft" | "mixed" | "hard";

const call = <T>(cmd: string, args: Record<string, unknown>) => invoke<T>(cmd, args);

export const git = {
  status: (repo: string) => call<Status>("git_status", { repo }),
  /** rev null = alle Refs (--all). */
  log: (repo: string, rev: string | null, skip: number, limit: number) =>
    call<Commit[]>("git_log", { repo, rev, skip, limit }),
  show: (repo: string, sha: string) => call<CommitDetail>("git_show", { repo, sha }),
  /** Unified Diff. path null = alles. Untracked Dateien kommen als neue Datei. */
  diff: (repo: string, path: string | null, staged: boolean) =>
    call<string>("git_diff", { repo, path, staged }),
  stage: (repo: string, paths: string[]) => call<void>("git_stage", { repo, paths }),
  unstage: (repo: string, paths: string[]) => call<void>("git_unstage", { repo, paths }),
  /** Aenderungen verwerfen; untracked Dateien werden geloescht. */
  discard: (repo: string, paths: string[]) => call<void>("git_discard", { repo, paths }),
  /** Patch anwenden (Hunk stagen: cached; unstagen: cached+reverse; verwerfen: reverse). */
  apply: (repo: string, patch: string, cached: boolean, reverse: boolean) =>
    call<void>("git_apply", { repo, patch, cached, reverse }),
  /** Liefert die neue SHA. */
  commit: (repo: string, message: string, amend: boolean) =>
    call<string>("git_commit", { repo, message, amend }),

  branches: (repo: string) => call<Branch[]>("git_branches", { repo }),
  /** create = neuer Branch `target` ab `start` (null = HEAD). Remote-Branch auschecken legt Tracking-Branch an. */
  checkout: (repo: string, target: string, create: boolean, start: string | null) =>
    call<string>("git_checkout", { repo, target, create, start }),
  branchDelete: (repo: string, name: string, force: boolean, remote: boolean) =>
    call<string>("git_branch_delete", { repo, name, force, remote }),
  branchRename: (repo: string, from: string, to: string) =>
    call<void>("git_branch_rename", { repo, from, to }),
  merge: (repo: string, rev: string, noFf: boolean) => call<string>("git_merge", { repo, rev, noFf }),
  rebase: (repo: string, onto: string) => call<string>("git_rebase", { repo, onto }),
  /** Laufenden merge/rebase/cherry-pick/revert abbrechen bzw. fortsetzen (je nach Status.state). */
  abort: (repo: string) => call<string>("git_abort", { repo }),
  continue: (repo: string) => call<string>("git_continue", { repo }),
  cherryPick: (repo: string, sha: string) => call<string>("git_cherry_pick", { repo, sha }),
  revert: (repo: string, sha: string) => call<string>("git_revert", { repo, sha }),
  reset: (repo: string, sha: string, mode: ResetMode) => call<string>("git_reset", { repo, sha, mode }),

  fetch: (repo: string) => call<string>("git_fetch", { repo }),
  pull: (repo: string, rebase: boolean) => call<string>("git_pull", { repo, rebase }),
  /** Ohne Upstream wird automatisch `-u origin <branch>` gesetzt. force = --force-with-lease. */
  push: (repo: string, force: boolean) => call<string>("git_push", { repo, force }),
  remotes: (repo: string) => call<Remote[]>("git_remotes", { repo }),

  stashes: (repo: string) => call<Stash[]>("git_stashes", { repo }),
  stashPush: (repo: string, message: string | null, untracked: boolean) =>
    call<string>("git_stash_push", { repo, message, untracked }),
  stashApply: (repo: string, index: number, pop: boolean) =>
    call<string>("git_stash_apply", { repo, index, pop }),
  stashDrop: (repo: string, index: number) => call<void>("git_stash_drop", { repo, index }),
  stashShow: (repo: string, index: number) => call<string>("git_stash_show", { repo, index }),

  tags: (repo: string) => call<Tag[]>("git_tags", { repo }),
  tagCreate: (repo: string, name: string, sha: string, message: string | null) =>
    call<void>("git_tag_create", { repo, name, sha, message }),
  tagDelete: (repo: string, name: string) => call<void>("git_tag_delete", { repo, name }),

  /** Welche der names stecken schon in anderen aus names? Name -> enthaltende Namen (nur enthaltene als Key). */
  nesting: (repo: string, names: string[]) =>
    call<Record<string, string[]>>("git_nesting", { repo, names }),
  /** Fest eingestellter Production-Branch des Repos (git config ocui.base); null = automatisch. */
  getBase: (repo: string) => call<string | null>("git_get_base", { repo }),
  setBase: (repo: string, base: string | null) => call<void>("git_set_base", { repo, base }),

  conflict: (repo: string, path: string) => call<Conflict>("git_conflict", { repo, path }),
  /** Schreibt den aufgeloesten Inhalt und staged die Datei. */
  resolve: (repo: string, path: string, content: string) =>
    call<void>("git_resolve", { repo, path, content }),
  /** Ganze Seite uebernehmen und stagen, auch fuer Binaerdateien. */
  resolveSide: (repo: string, path: string, theirs: boolean) =>
    call<void>("git_resolve_side", { repo, path, theirs }),
  /** Lesbare Namen der beiden Seiten im laufenden Vorgang, z. B. { ours: "main", theirs: "feature/x" }. */
  conflictSides: (repo: string) => call<{ ours: string; theirs: string }>("git_conflict_sides", { repo }),
};

// ---------- GitHub / GitLab ----------

export type ForgeKind = "github" | "gitlab";

/** Token liegt im Windows-Anmeldeinfo-Speicher, nie im Store. */
export type Account = { kind: ForgeKind; host: string; user: string; avatar: string | null };

/** Konto-ID "kind:host:user", z. B. "github:github.com:octocat" (host darf einen Port enthalten). */
export const accountId = (a: Account) => `${a.kind}:${a.host}:${a.user}`;

/** Konto eines lokalen Repos (git config ocui.account) und die daraus folgende Commit-Identitaet. */
export type RepoAccount = {
  /** Explizit gesetzte Konto-ID; null = automatisch. */
  account: string | null;
  /** Konto, das API-Aufrufe nutzen (explizit oder erstes passendes); null = keins. */
  effective: string | null;
  /** Host des origin-Remotes. */
  host: string | null;
  /** Effektives user.name / user.email (lokal oder global). */
  name: string | null;
  email: string | null;
};

export type RemoteRepo = {
  kind: ForgeKind;
  host: string;
  /** "owner/name" bzw. GitLab "group/sub/name". */
  full_name: string;
  name: string;
  description: string | null;
  private: boolean;
  fork: boolean;
  archived: boolean;
  stars: number;
  lang: string | null;
  updated_at: string;
  default_branch: string | null;
  clone_http: string;
  clone_ssh: string;
  web_url: string;
};

export type PullRequest = {
  number: number;
  title: string;
  author: string;
  source_branch: string;
  target_branch: string;
  draft: boolean;
  updated_at: string;
  web_url: string;
};

export type CloneProgress = { line: string; percent: number | null };

/** Ergebnis einer Forge-Abfrage zum Remote eines lokalen Repos; null, wenn Remote unbekannt oder kein Konto passt. */
export type ForgeList<T> = { kind: ForgeKind; items: T[] } | null;

export type ReleaseAsset = { name: string; size: number; downloads: number; url: string };

export type Release = {
  /** GitHub: numerische Release-ID als String; GitLab: Tag-Name (Releases haben dort keine eigene ID). */
  id: string;
  tag: string;
  name: string;
  /** Markdown, wird nur als Text angezeigt. */
  body: string;
  /** Nur GitHub; GitLab kennt keine Drafts/Prereleases (immer false). */
  draft: boolean;
  prerelease: boolean;
  created_at: string;
  published_at: string | null;
  author: string;
  web_url: string;
  assets: ReleaseAsset[];
};

/** Anlegen/Bearbeiten. target: Branch oder SHA, falls der Tag noch nicht existiert (sonst null). */
export type ReleaseInput = {
  tag: string;
  name: string;
  body: string;
  draft: boolean;
  prerelease: boolean;
  target: string | null;
};

export type RunStatus = "queued" | "running" | "success" | "failure" | "cancelled" | "skipped";

/** GitHub-Actions-Workflow-Run bzw. GitLab-Pipeline. */
export type Run = {
  id: number;
  /** Workflow-Name (GitHub) bzw. "Pipeline #<iid>" (GitLab). */
  name: string;
  /** Commit-Titel, falls die API ihn liefert, sonst leer. */
  title: string;
  branch: string;
  sha: string;
  /** push, pull_request, schedule, workflow_dispatch, tag … wie von der API. */
  event: string;
  status: RunStatus;
  created_at: string;
  duration_s: number | null;
  web_url: string;
};

export type Job = {
  id: number;
  name: string;
  /** GitLab-Stage, bei GitHub null. */
  stage: string | null;
  status: RunStatus;
  started_at: string | null;
  duration_s: number | null;
  web_url: string;
};

export const forge = {
  /** Prueft den Token gegen die API und legt ihn ab. host z. B. "github.com", "gitlab.firma.de". */
  login: (kind: ForgeKind, host: string, token: string) =>
    call<Account>("forge_login", { kind, host, token }),
  logout: (kind: ForgeKind, host: string, user: string) => call<void>("forge_logout", { kind, host, user }),
  /** Token aus `gh auth token` (GitHub) bzw. `glab auth token` uebernehmen, falls installiert. */
  importCli: (kind: ForgeKind, host: string) => call<Account>("forge_import_cli", { kind, host }),
  /** query leer = eigene Repos (zuletzt aktiv zuerst), sonst Suche. page ab 1. */
  repos: (kind: ForgeKind, host: string, user: string, query: string, page: number) =>
    call<RemoteRepo[]>("forge_repos", { kind, host, user, query, page }),
  /** Offene PRs/MRs zum Remote-URL eines lokalen Repos; leer, wenn kein passendes Konto. */
  pulls: (repo: string, remoteUrl: string) => call<PullRequest[]>("forge_pulls", { repo, remoteUrl }),
  /** Releases, neueste zuerst (GitHub inkl. Drafts, wenn der Token Schreibrechte hat). page ab 1, 30 pro Seite. */
  releases: (repo: string, remoteUrl: string, page: number) =>
    call<ForgeList<Release>>("forge_releases", { repo, remoteUrl, page }),
  /** id null = neu anlegen, sonst bearbeiten (GitHub: ID, GitLab: Tag). Liefert den gespeicherten Stand. */
  releaseSave: (repo: string, remoteUrl: string, id: string | null, input: ReleaseInput) =>
    call<Release>("forge_release_save", { repo, remoteUrl, id, input }),
  /** Loescht nur das Release, der Git-Tag bleibt. */
  releaseDelete: (repo: string, remoteUrl: string, id: string) =>
    call<void>("forge_release_delete", { repo, remoteUrl, id }),
  /** Workflow-Runs bzw. Pipelines, neueste zuerst. branch leer = alle. page ab 1, 30 pro Seite. */
  runs: (repo: string, remoteUrl: string, branch: string, page: number) =>
    call<ForgeList<Run>>("forge_runs", { repo, remoteUrl, branch, page }),
  /** Jobs eines Runs bzw. einer Pipeline. */
  jobs: (repo: string, remoteUrl: string, runId: number) => call<Job[]>("forge_jobs", { repo, remoteUrl, runId }),
  repoAccount: (repo: string) => call<RepoAccount>("forge_repo_account", { repo }),
  /** Konto fuer das Repo festlegen und user.name/user.email lokal daraus setzen; null = zurueck auf System-Git. */
  setRepoAccount: (repo: string, account: string | null) =>
    call<RepoAccount>("forge_set_repo_account", { repo, account }),
  /** Klont nach dest; Fortschritt als Event `clone:{id}` (CloneProgress). Liefert den Zielpfad. account = Konto-ID fuer den Token. */
  clone: (id: string, url: string, dest: string, account: string | null) =>
    call<string>("forge_clone", { id, url, dest, account }),
  /** Oeffnet eine URL im Standardbrowser. */
  openUrl: (url: string) => call<void>("open_url", { url }),
  /** Browser-URL zu einem Remote (ssh/scp -> https); null, wenn nicht erkennbar. */
  webUrl: (remoteUrl: string) => call<string | null>("forge_web_url", { remoteUrl }),
};
