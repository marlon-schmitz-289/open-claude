// Workflow-Vorlagen: Builder-Daten <-> Workflow-Skript fuer das Workflow-Tool von Claude Code.
// Rein, ohne Tauri, damit node --test es direkt laden kann.

/** "" = von der Session erben bzw. kein Schema. */
export type WfAgent = {
  label: string;
  prompt: string;
  model: string;
  effort: string;
  schema: string;
};
export type WfPhase = {
  title: string;
  mode: "parallel" | "sequence";
  agents: WfAgent[];
};
/** Spiegel von workflows::WfFile; file = Dateiname ohne .js */
export type WfFile = {
  scope: "user" | "project";
  file: string;
  path: string;
  text: string;
};
export type WfTemplate = {
  name: string;
  description: string;
  whenToUse: string;
  phases: WfPhase[];
};

export const MODELS = ["", "sonnet", "opus", "haiku", "fable"];
export const EFFORTS = ["", "low", "medium", "high", "xhigh", "max"];
export const MARK = "// open-claude-builder: ";
export const NAME = /^[a-z0-9][a-z0-9-]*$/;

export const newAgent = (): WfAgent => ({
  label: "",
  prompt: "",
  model: "",
  effort: "",
  schema: "",
});
export const newPhase = (n: number): WfPhase => ({
  title: `Phase ${n}`,
  mode: "sequence",
  agents: [newAgent()],
});
export const newTemplate = (): WfTemplate => ({
  name: "",
  description: "",
  whenToUse: "",
  phases: [newPhase(1)],
});

// JSON-Literal; U+2028/9 maskiert, sonst endet die Kommentarzeile vorzeitig
const lit = (v: unknown) =>
  JSON.stringify(v)
    .replace(/\u2028/g, "\\u2028")
    .replace(/\u2029/g, "\\u2029");

/** Deterministisch (kein Date/Math.random), sonst bricht Resume. Format von meta passt zu phases_of in activity.rs. */
export function toScript(t: WfTemplate): string {
  const call = (a: WfAgent, title: string) =>
    `agent(fill(${lit(a.prompt)}, prev), ${lit({
      label: a.label || undefined,
      phase: title,
      model: a.model || undefined,
      effort: a.effort || undefined,
      schema: a.schema.trim() ? JSON.parse(a.schema) : undefined,
    })})`;
  const out = [
    "export const meta = {",
    `  name: ${lit(t.name)},`,
    `  description: ${lit(t.description)},`,
  ];
  if (t.whenToUse.trim()) out.push(`  whenToUse: ${lit(t.whenToUse)},`);
  out.push(
    "  phases: [",
    ...t.phases.map((p) => `    { title: ${lit(p.title)} },`),
    "  ],",
    "}",
    "",
  );
  out.push(
    "// {{args}} = Aufruf-Argumente, {{prev}} = Ergebnis des vorigen Schritts",
    'const str = (v) => (v == null ? "" : typeof v === "string" ? v : JSON.stringify(v, null, 2))',
    // Ein Durchlauf: {{prev}} in den Args (und umgekehrt) bleibt stehen; Callback, damit $ in Werten egal ist
    'const fill = (p, prev) => p.replace(/\\{\\{(args|prev)\\}\\}/g, (_, k) => str(k === "args" ? (typeof args === "undefined" ? null : args) : prev))',
    "let prev = null",
  );
  for (const p of t.phases) {
    out.push("", `phase(${lit(p.title)})`);
    if (p.mode === "parallel")
      out.push(
        "prev = (await parallel([",
        ...p.agents.map((a) => `  () => ${call(a, p.title)},`),
        "])).filter(Boolean)",
      );
    else
      out.push(
        ...p.agents.map((a) => `prev = (await ${call(a, p.title)}) ?? prev`),
      );
  }
  out.push("return prev", "", MARK + lit({ v: 1, ...t }), "");
  return out.join("\n");
}

/** Builder-Daten aus der letzten Marker-Zeile; null bei freien Skripten oder kaputtem JSON. */
export function fromScript(text: string): WfTemplate | null {
  const line = text.trimEnd().split("\n").at(-1) ?? "";
  if (!line.startsWith(MARK)) return null;
  try {
    const { name, description, whenToUse, phases } = JSON.parse(
      line.slice(MARK.length),
    );
    if (typeof name !== "string" || !Array.isArray(phases)) return null;
    return {
      name,
      description: String(description ?? ""),
      whenToUse: String(whenToUse ?? ""),
      phases: phases.map((p: WfPhase) => ({
        title: String(p.title ?? ""),
        mode: p.mode === "parallel" ? "parallel" : "sequence",
        agents: (p.agents ?? []).map((a: WfAgent) => ({
          label: String(a.label ?? ""),
          prompt: String(a.prompt ?? ""),
          model: String(a.model ?? ""),
          effort: String(a.effort ?? ""),
          schema: String(a.schema ?? ""),
        })),
      })),
    };
  } catch {
    return null;
  }
}

/** name/description aus meta per Regex, fuer freie Skripte. */
export function metaOf(text: string): { name?: string; description?: string } {
  const body = text.slice(Math.max(0, text.search(/\bmeta\s*=/)));
  const get = (k: string) => {
    const m = body.match(
      new RegExp(
        `\\b${k}\\s*:\\s*("(?:[^"\\\\\\n]|\\\\.)*"|'[^'\\n]*'|\`[^\`]*\`)`,
      ),
    );
    if (!m) return undefined;
    try {
      return m[1][0] === '"' ? JSON.parse(m[1]) : m[1].slice(1, -1);
    } catch {
      return undefined;
    }
  };
  return { name: get("name"), description: get("description") };
}

function schemaProblem(s: string): string | null {
  if (!s.trim()) return null;
  let v: unknown;
  try {
    v = JSON.parse(s);
  } catch {
    return "kein gültiges JSON";
  }
  const o = v as { type?: unknown; properties?: unknown; required?: unknown };
  if (!o || typeof o !== "object" || Array.isArray(o) || o.type !== "object")
    return 'braucht type: "object"';
  if (
    !o.properties ||
    typeof o.properties !== "object" ||
    Array.isArray(o.properties)
  )
    return "braucht ein properties-Objekt";
  if (
    o.required !== undefined &&
    (!Array.isArray(o.required) ||
      o.required.some(
        (k) => typeof k !== "string" || !(k in (o.properties as object)),
      ))
  )
    return "required nennt Felder, die nicht in properties stehen";
  return null;
}

/** Fehler, die das Speichern sperren. taken = Namen im selben Scope ohne die eigene Datei. */
export function problems(t: WfTemplate, taken: string[]): string[] {
  const out: string[] = [];
  if (!NAME.test(t.name))
    out.push("Name: nur a-z, 0-9 und -, nicht mit - beginnen");
  else if (taken.includes(t.name))
    out.push(`Name „${t.name}“ ist hier schon vergeben`);
  if (!t.description.trim()) out.push("Beschreibung fehlt");
  if (!t.phases.length) out.push("Mindestens eine Phase");
  const seen = new Set<string>();
  t.phases.forEach((p, i) => {
    const n = `Phase ${i + 1}`;
    // " und \ schneiden in phases_of (activity.rs) die Anzeige ab
    if (!p.title.trim()) out.push(`${n}: Titel fehlt`);
    else if (/["\\]/.test(p.title)) out.push(`${n}: Titel ohne " und \\`);
    else if (seen.has(p.title)) out.push(`${n}: Titel „${p.title}“ doppelt`);
    seen.add(p.title);
    if (!p.agents.length) out.push(`${n}: mindestens ein Agent`);
    p.agents.forEach((a, j) => {
      const m = `${n}, Agent ${j + 1}`;
      if (!a.prompt.trim()) out.push(`${m}: Prompt fehlt`);
      const s = schemaProblem(a.schema);
      if (s) out.push(`${m}: Schema ${s}`);
    });
  });
  return out;
}
