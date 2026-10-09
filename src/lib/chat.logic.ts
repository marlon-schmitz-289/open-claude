// Slash-Commands der Chat-Ansicht: was claude headless kann, geht durch; der Rest wird hier nachgebaut oder abgefangen.

export type Cmd = { name: string; description: string; argumentHint?: string; aliases?: string[] };

export const LOCAL: Cmd[] = [
  { name: "resume", description: "Frühere Sitzung fortsetzen", argumentHint: "[id|titel]" },
  { name: "continue", description: "Letzte Sitzung fortsetzen" },
  { name: "status", description: "Version, Konto, Modell, Einstellungen" },
  { name: "permissions", description: "Berechtigungsregeln anzeigen" },
  { name: "hooks", description: "Hooks anzeigen" },
  { name: "memory", description: "CLAUDE.md-Dateien und Auto-Memory" },
  { name: "skills", description: "Skills anzeigen" },
  { name: "sandbox", description: "Sandbox-Status" },
  { name: "export", description: "Verlauf in die Zwischenablage" },
  { name: "plan", description: "Plan-Modus einschalten", argumentHint: "[prompt]" },
  { name: "copy", description: "Letzte Antwort in die Zwischenablage" },
  { name: "help", description: "Alle Commands" },
  { name: "exit", description: "Sitzung beenden", aliases: ["quit"] },
  { name: "login", description: "Anmelden (im Terminal)" },
  { name: "logout", description: "Abmelden (im Terminal)" },
];

/** Lokale Commands, die claude per control_request beantwortet. */
export const CONTROL: Record<string, string> = {
  status: "get_status",
  permissions: "list_permission_rules",
  hooks: "get_hooks_listing",
  memory: "get_memory_dialog",
  skills: "get_skills_dialog",
  sandbox: "get_sandbox_dialog",
};

const find = (list: Cmd[], n: string) => list.find((c) => c.name === n || c.aliases?.includes(n));

/** claude zuerst (wird ein Command headless verfuegbar, gewinnt das Original), dann lokal, sonst unbekannt.
 *  Leere claude-Liste (initialize noch offen): alles ausser lokalen durchreichen. Pfade wie "/Users/x" sind kein Command. */
export function route(text: string, claude: Cmd[]): { kind: "claude" | "local" | "unknown"; name: string; args: string } | null {
  const m = /^\/([\w:.-]+)(?:\s+([\s\S]*))?$/.exec(text);
  if (!m) return null;
  const [, n, args = ""] = m;
  const c = find(claude, n);
  const l = find(LOCAL, n);
  if (c || (!claude.length && !l)) return { kind: "claude", name: c?.name ?? n, args: args.trim() };
  return { kind: l ? "local" : "unknown", name: l?.name ?? n, args: args.trim() };
}

/** Antwort eines control_request als Text: get_status-Sections als Liste, sonst JSON. */
export const fmt = (r: any): string =>
  Array.isArray(r?.sections)
    ? r.sections.map((s: any) => [s.title, ...(s.rows ?? []).map((x: any) => `  ${x.label}: ${x.value}`)].join("\n")).join("\n\n")
    : JSON.stringify(r, null, 2);
