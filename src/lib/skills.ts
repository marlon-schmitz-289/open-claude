// Vertrag zwischen Frontend und src-tauri/src/skills.rs.
// Namen und Felder hier sind verbindlich; Rust serialisiert exakt diese Formen (camelCase).
import { invoke } from "@tauri-apps/api/core";
import { fuzzy } from "./fuzzy.ts";

export type Skill = {
  /** "name" bzw. "plugin:name" fuer Plugin-Skills (plugin = Teil vor "@" im Plugin-Key). */
  key: string;
  name: string;
  description: string;
  source: "user" | "project" | "plugin";
  /** Plugin-Key aus installed_plugins.json, z.B. "caveman@caveman"; nur bei source "plugin". */
  plugin?: string;
};

/** Werte wie in skillOverrides von Claude Code; fehlend = "on". */
export type SkillValue = "on" | "off" | "user-invocable-only" | "name-only";

/** skillOverrides/enabledPlugins einer settings.json (User oder Team). */
export type Settings = {
  skillOverrides: Record<string, SkillValue>;
  enabledPlugins: Record<string, boolean>;
};

export type SkillsInfo = {
  skills: Skill[];
  /** Plugin-Keys aus installed_plugins.json. */
  plugins: string[];
  user: Settings;
  team: Settings;
};

/** Nur Vorlage: Projekte nutzen es per Projekttyp oder explizit, aendern es aber nie. */
export type Profile = {
  overrides: Record<string, "on" | "off">;
  plugins: Record<string, boolean>;
};

/** Pro Projekt im Store (projectSkills[pfad]). profile: undefined = nach Typ, null = kein Profil, string = explizit. */
export type ProjectSkills = Profile & { profile?: string | null };

/** Was in settings.local.json landet; null = Key entfernen. */
export type LocalPayload = {
  skillOverrides: Record<string, SkillValue> | null;
  enabledPlugins: Record<string, boolean> | null;
};

/** Eigene Mods aus ~/.claude/open-claude-mods; das Terminal laedt sie per CLAUDE_CODE_PLUGIN_DIRS. */
export type Mod = { name: string; description: string; path: string };
export type ModsInfo = { dir: string; mods: Mod[] };

// ---------- Plugin-Verwaltung (claude plugin ...) ----------

export type InstalledPlugin = {
  id: string;
  version: string;
  scope: "user" | "project" | "local";
  enabled: boolean;
  installPath: string;
  projectPath?: string;
  installedAt?: string;
  lastUpdated?: string;
};
export type AvailablePlugin = {
  pluginId: string;
  name: string;
  description?: string;
  marketplaceName: string;
  version?: string;
  installCount?: number;
};
export type Marketplace = {
  source: { source: string; repo?: string; url?: string; path?: string };
  installLocation: string;
  lastUpdated?: string;
};
export type PluginsInfo = {
  installed: InstalledPlugin[];
  available: AvailablePlugin[];
  marketplaces: Record<string, Marketplace>;
  meta: Record<string, { latest: string | null; update: boolean; description: string }>;
};
export type PluginAction = "install" | "update" | "uninstall" | "enable" | "disable" | "mp-add" | "mp-remove" | "mp-update";
/** Eine JSON-Ergebniszeile der CLI. */
export type PluginResult = {
  outcome?: string;
  message?: string;
  shownCommand?: { sha256: string; command?: unknown };
} & Record<string, unknown>;

const call = <T>(cmd: string, args: Record<string, unknown>) => invoke<T>(cmd, args);

export const skills = {
  list: (path: string) => call<SkillsInfo>("skills_list", { path }),
  mods: () => call<ModsInfo>("mods_list", {}),
  /** Schreibt nur diese beiden Keys in <repo>/.claude/settings.local.json; null entfernt den Key. */
  writeLocal: (path: string, p: LocalPayload) => call<void>("skills_write_local", { path, ...p }),
  plugins: (path: string) => call<PluginsInfo>("plugins_list", { path }),
  pluginRun: (path: string, action: PluginAction, target: string, scope?: string, accept?: string) =>
    call<PluginResult>("plugins_run", { path, action, target, scope, accept }),
};

/** Installierte nach ID gruppiert; main = user-, sonst project-Eintrag; locals = Eintraege mit Scope local. */
export function groupInstalled(list: InstalledPlugin[]) {
  const ids = [...new Set(list.map((p) => p.id))];
  return ids.map((id) => {
    const all = list.filter((p) => p.id === id);
    const main = all.find((p) => p.scope === "user") ?? all.find((p) => p.scope === "project");
    return { id, main, locals: all.filter((p) => p.scope === "local") };
  });
}

/** Verfuegbare ohne installierte IDs, gefiltert per fuzzy(id + " " + description), sonst nach installCount; max. limit. */
export function pickAvailable(av: AvailablePlugin[], installed: Set<string>, query: string, limit = 50) {
  const q = query.trim();
  const scored = av
    .filter((p) => !installed.has(p.pluginId))
    .map((p) => ({ p, m: fuzzy(`${p.pluginId} ${p.description ?? ""}`, q) }))
    .filter((x) => x.m)
    .sort((a, b) => (q ? b.m!.score - a.m!.score : (b.p.installCount ?? 0) - (a.p.installCount ?? 0)));
  // ponytail: max. 50 Zeilen statt Virtualisierung; reicht, weil die Suche eingrenzt
  return { items: scored.slice(0, limit).map((x) => x.p), total: scored.length };
}

// ---------- Reine Logik: Schichten, Payload, Projekt-Anpassungen ----------

export const emptyProject = (): ProjectSkills => ({ overrides: {}, plugins: {} });

export type Layers = {
  user: Settings;
  team: Settings;
  profile?: Profile | null;
  project?: ProjectSkills;
};

export type Source = "default" | "user" | "team" | "profile" | "project";

/** Profilname fuer ein Projekt oder null. */
export function profileFor(
  project: ProjectSkills | undefined,
  kind: string | null | undefined,
  kindProfiles: Record<string, string>,
): string | null {
  if (project?.profile !== undefined) return project.profile;
  return (kind && Object.hasOwn(kindProfiles, kind) && kindProfiles[kind]) || null;
}

/** Wirksamer Wert eines Skills/Plugins; Projekt > Profil > Team > User > Default. */
export function resolve(kind: "skill", key: string, l: Layers): { value: SkillValue; source: Source };
export function resolve(kind: "plugin", key: string, l: Layers): { value: boolean; source: Source };
export function resolve(kind: "skill" | "plugin", key: string, l: Layers) {
  const [s, p] = kind === "skill" ? (["skillOverrides", "overrides"] as const) : (["enabledPlugins", "plugins"] as const);
  const layers: [Source, Record<string, unknown> | undefined][] = [
    ["project", l.project?.[p]],
    ["profile", l.profile?.[p]],
    ["team", l.team[s]],
    ["user", l.user[s]],
  ];
  for (const [source, map] of layers) if (map && Object.hasOwn(map, key)) return { value: map[key], source };
  // ponytail: nicht gelistete Plugins gelten als aus (wie Claude Code); Skills als an
  return { value: kind === "skill" ? "on" : false, source: "default" };
}

// Claude Code merged Objekte nicht pro Key: das Objekt der hoechsten Datei gewinnt ganz.
// Darum schreibt settings.local.json den vollstaendig gemergten Stand aller Schichten.
// ponytail: Schnappschuss; spaetere Aenderungen an ~/.claude/settings.json greifen erst beim naechsten
// Sync (Start ueber die App, Umschalten). Start im Terminal synct nie; Nachsync bei Fokus erst bei Bedarf.
// null (Key weg) nur, wenn Claude ohne local.json ohnehin dasselbe sieht.
function merge<V>(user: Record<string, V>, team: Record<string, V>, profile: Record<string, V> = {}, project: Record<string, V> = {}) {
  const n = (o: object) => Object.keys(o).length;
  if (!n(profile) && !n(project) && !(n(user) && n(team))) return null;
  return { ...user, ...team, ...profile, ...project };
}

export function localPayload(l: Layers): LocalPayload {
  const skills = merge(l.user.skillOverrides, l.team.skillOverrides, l.profile?.overrides, l.project?.overrides);
  // "on" ist implizit (nicht gelistet); {} bleibt bewusst stehen und ueberdeckt User/Team
  const skillOverrides = skills && Object.fromEntries(Object.entries(skills).filter(([, v]) => v !== "on"));
  return { skillOverrides, enabledPlugins: merge(l.user.enabledPlugins, l.team.enabledPlugins, l.profile?.plugins, l.project?.plugins) };
}

/** Setzt (value) oder entfernt (undefined = zuruecksetzen) eine Projekt-Anpassung; Profil bleibt unberuehrt. */
export function adjust(ps: ProjectSkills | undefined, kind: "skill", key: string, value?: "on" | "off"): ProjectSkills;
export function adjust(ps: ProjectSkills | undefined, kind: "plugin", key: string, value?: boolean): ProjectSkills;
export function adjust(ps: ProjectSkills | undefined, kind: "skill" | "plugin", key: string, value?: string | boolean) {
  const base = ps ?? emptyProject();
  const field = kind === "skill" ? "overrides" : "plugins";
  const map: Record<string, unknown> = { ...base[field] };
  if (value === undefined) delete map[key];
  else map[key] = value;
  return { ...base, [field]: map };
}

// ---------- Store-Zustand (plugin-store: skillProfiles, kindProfiles, projectSkills) ----------

export type SkillStore = {
  profiles: Record<string, Profile>;
  kindProfiles: Record<string, string>;
  projects: Record<string, ProjectSkills>;
};

/** Store-Key eines Projekts: unabhaengig von Trenner, Schlusstrenner und (Windows) Gross/Klein des Dev-Ordners. */
export function pkey(path: string) {
  const p = path.replace(/\\/g, "/").replace(/(.)\/+$/, "$1");
  return /^[a-z]:/i.test(p) ? p.toLowerCase() : p;
}

/** Wirksames Profil des Projekts; ein geloeschtes/unbekanntes gilt als keins. */
export function profileOf(s: SkillStore, path: string, kind?: string | null): Profile | null {
  const name = profileFor(s.projects[pkey(path)], kind, s.kindProfiles);
  return (name && Object.hasOwn(s.profiles, name) && s.profiles[name]) || null;
}

export function layers(info: Pick<SkillsInfo, "user" | "team">, s: SkillStore, path: string, kind?: string | null): Layers {
  return { user: info.user, team: info.team, profile: profileOf(s, path, kind), project: s.projects[pkey(path)] };
}

/** Verwaltet die App das Projekt? Sonst bleiben Keys, die Claude Code selbst (/plugin, Scope local) schrieb, unberuehrt. */
export const managed = (s: SkillStore, path: string, kind?: string | null) => !!s.projects[pkey(path)] || !!profileOf(s, path, kind);

/** Liest User/Team frisch und schreibt das Ergebnis in <repo>/.claude/settings.local.json. */
export async function sync(s: SkillStore, path: string, kind?: string | null) {
  const info = await skills.list(path);
  await skills.writeLocal(path, localPayload(layers(info, s, path, kind)));
}

/** Projekte, deren Profil oder Anpassung sich zwischen zwei Staenden unterscheidet (muessen neu geschrieben werden). */
export function touched(a: SkillStore, b: SkillStore, repos: { path: string; kind?: string | null }[]): string[] {
  const key = (s: SkillStore, r: (typeof repos)[number]) => JSON.stringify([profileOf(s, r.path, r.kind), s.projects[pkey(r.path)]]);
  return repos.filter((r) => key(a, r) !== key(b, r)).map((r) => r.path);
}

/** Profil umbenennen (to = null: loeschen) samt Verweisen; explizit zugeordnete Projekte fallen beim Loeschen auf "nach Typ" zurueck. */
export function renameProfile(s: SkillStore, from: string, to: string | null): SkillStore {
  const profiles = Object.fromEntries(
    Object.entries(s.profiles).flatMap(([n, p]) => (n !== from ? [[n, p]] : to ? [[to, p]] : [])),
  );
  const kindProfiles = Object.fromEntries(
    Object.entries(s.kindProfiles).flatMap(([k, n]) => (n !== from ? [[k, n]] : to ? [[k, to]] : [])),
  );
  const projects = Object.fromEntries(
    Object.entries(s.projects).map(([path, ps]) => {
      if (ps.profile !== from) return [path, ps];
      const { profile: _, ...rest } = ps;
      return [path, to ? { ...rest, profile: to } : rest];
    }),
  );
  return { profiles, kindProfiles, projects };
}

/** Neues Profil aus dem aktuellen Stand eines Projekts (sein Profil plus Anpassungen). */
export function fromProject(s: SkillStore, path: string, kind?: string | null): Profile {
  const p = profileOf(s, path, kind);
  const ps = s.projects[pkey(path)];
  return { overrides: { ...p?.overrides, ...ps?.overrides }, plugins: { ...p?.plugins, ...ps?.plugins } };
}
