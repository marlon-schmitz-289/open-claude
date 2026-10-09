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

/** Berechtigungsmodi, die claude mitten in der Sitzung annimmt (bypassPermissions nur mit Startflag). */
export const MODES = [
  ["Manuell", "default", "Fragt vor Änderungen und Befehlen"],
  ["Edits annehmen", "acceptEdits", "Dateiänderungen ohne Rückfrage"],
  ["Plan", "plan", "Nur lesen und planen"],
  ["Auto", "auto", "Claude entscheidet, was sicher ist"],
  ["Nicht fragen", "dontAsk", "Nur Erlaubtes, alles andere wird abgelehnt"],
] as const;

/** Umschalt+Tab wie im Terminal: Manuell, Edits, Plan, Auto, dann wieder von vorn. */
export function nextMode(mode: string): string {
  const cycle = ["default", "acceptEdits", "plan", "auto"];
  return cycle[(cycle.indexOf(mode) + 1) % cycle.length];
}

export type Question = { question: string; header?: string; options: { label: string; description?: string }[]; multiSelect?: boolean };

/** AskUserQuestion-Antwort wie claude sie erwartet: { Frage: "A, B" }; Freitext ("Andere") kommt hinten dran. */
export function answers(questions: Question[], picks: string[][], other: string[]): Record<string, string> {
  return Object.fromEntries(questions.map((q, i) => [q.question, [...(picks[i] ?? []), other[i]?.trim()].filter(Boolean).join(", ")]));
}

/** tool_result-Inhalt als Text: String oder Bloecke (Text, Bilder). */
export function resultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.map((b) => (b?.type === "text" ? b.text : b?.type === "image" ? "[Bild]" : "")).filter(Boolean).join("\n");
}

type Rule = { toolName: string; ruleContent?: string };
export type Suggestion = { type: string; mode?: string; rules?: Rule[]; directories?: string[] };

/** Beschriftung fuer "nicht mehr fragen" aus den permission_suggestions von claude. */
export function suggestionLabel(list: Suggestion[]): string {
  const parts = list.map((s) =>
    s.type === "setMode"
      ? s.mode === "acceptEdits" ? "Edits in dieser Sitzung ohne Rückfrage" : `Modus ${s.mode}`
      : s.type === "addRules"
        ? (s.rules ?? []).map((r) => (r.ruleContent ? `${r.toolName}(${r.ruleContent})` : r.toolName)).join(", ") + " immer erlauben"
        : s.type === "addDirectories"
          ? `Zugriff auf ${(s.directories ?? []).join(", ")}`
          : "",
  );
  return parts.filter(Boolean).join(" · ") || "Nicht mehr fragen";
}

export type McpServer = {
  name: string;
  status: string;
  error?: string;
  scope?: string;
  config?: { type?: string };
  tools?: { name: string }[];
};

/** Server-Objekt fuer `claude mcp add-json` aus Eingabezeile: Befehl samt Argumenten oder http(s)-URL. */
// ponytail: Argumente nach Leerzeichen getrennt, keine Quotes; Pfade mit Leerzeichen dann per Hand in die Config.
export function mcpConfig(kind: "stdio" | "http", text: string): { command: string; args: string[] } | { type: "http"; url: string } | null {
  const t = text.trim();
  if (kind === "http") return /^https?:\/\/\S+$/.test(t) ? { type: "http", url: t } : null;
  const [command, ...args] = t.split(/\s+/).filter(Boolean);
  return command ? { command, args } : null;
}
