// Vertrag zwischen Frontend und src-tauri/src/unity.rs.
// Namen und Felder hier sind verbindlich; Rust serialisiert exakt diese Formen (camelCase).
import { invoke } from "@tauri-apps/api/core";

export type UnityInfo = {
  /** Aus ProjectSettings/ProjectVersion.txt; null solange nicht bekannt/kein gueltiges Projekt. */
  version: string | null;
  /** Unity-CLI gefunden. */
  cli: boolean;
  editorInstalled: boolean;
  editorOpen: boolean;
  /** com.unity.pipeline in Packages/manifest.json. */
  pipeline: boolean;
  /** Laufender Editor per Pipeline erreichbar. */
  connected: boolean;
  /** .claude/skills/unity-cli im Projekt. */
  skill: boolean;
};

export type UnityTestMode = "EditMode" | "PlayMode";

export type TestFailure = { name: string; message: string };

export type TestReport = {
  passed: number;
  failed: number;
  skipped: number;
  inconclusive: number;
  total: number;
  durationSecs: number;
  failures: TestFailure[];
  /** "editor" = ueber laufenden Editor, "batchmode" = frisch gestartete Headless-Instanz. */
  via: "editor" | "batchmode";
  reportPath: string;
};

const call = <T>(cmd: string, args: Record<string, unknown>) => invoke<T>(cmd, args);

export const unity = {
  info: (path: string) => call<UnityInfo>("unity_info", { path }),
  /** Log-Text; installiert Skill, Pipeline-Paket, CLAUDE.md-Abschnitt. Idempotent. */
  setup: (path: string) => call<string>("unity_setup", { path }),
  open: (path: string) => call<void>("unity_open", { path }),
  close: (path: string, force: boolean) => call<void>("unity_close", { path, force }),
  test: (path: string, mode: UnityTestMode) => call<TestReport>("unity_test", { path, mode }),
};

// ---------- Reine Logik: Ampel, Badge-Text, Dauer ----------

export type Verdict = "green" | "yellow" | "red";

/** Gruen nur wenn failed=skipped=inconclusive=0; skipped/inconclusive allein sind eine Warnung (gelb), nicht rot. */
export function verdict(r: Pick<TestReport, "failed" | "skipped" | "inconclusive">): Verdict {
  if (r.failed > 0) return "red";
  if (r.skipped > 0 || r.inconclusive > 0) return "yellow";
  return "green";
}

/** "Unity" bzw. "Unity <version>", solange unity_info noch nicht geladen ist (undefined) ohne Version. */
export function unityBadgeText(info?: UnityInfo | null): string {
  return info?.version ? `Unity ${info.version}` : "Unity";
}

// ponytail: keine Stunden-Formatierung wie PipelinesPanel.duration() — Testlaeufe dauern Sekunden bis
// wenige Minuten, nicht Stunden; bei Bedarf gleich erweitern.
/** "0.08 s", "49 s" oder "1:05" ab einer Minute. */
export function formatDuration(s: number): string {
  if (s >= 60) {
    const total = Math.round(s);
    return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
  }
  return `${Number(s.toFixed(2))} s`;
}
