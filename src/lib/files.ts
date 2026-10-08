// Vertrag zwischen Frontend und src-tauri/src/files.rs + preview.rs.
// Alle Pfade sind relativ zum Repo, mit "/" getrennt; "" = Projektordner.
import { Channel, invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { Fw } from "./testing.logic.ts";
import type { Target } from "./run.logic.ts";

export type Entry = {
  name: string;
  dir: boolean;
  /** Von git ignoriert: der Baum zeigt es gedimmt. */
  ignored: boolean;
};

export type FileText = {
  /** Immer mit LF und ohne BOM; crlf/bom stellen das Original beim Schreiben wieder her. */
  content: string;
  crlf: boolean;
  bom: boolean;
  /** Millisekunden seit 1970. */
  mtime: number;
};

/** Fehlertext von fs.write, wenn die Datei seit `expected` geaendert oder geloescht wurde. */
export const KONFLIKT = "KONFLIKT";
/** Fehlertexte von xamlRender: falsche Plattform bzw. .NET 8 Desktop Runtime fehlt. Alles andere ist eine lesbare Meldung. */
export const NUR_WINDOWS = "NUR_WINDOWS";
export const DOTNET_FEHLT = "DOTNET_FEHLT";
export const DOTNET_URL = "https://dotnet.microsoft.com/download/dotnet/8.0";

const call = <T>(cmd: string, args: Record<string, unknown>) => invoke<T>(cmd, args);

export const fs = {
  /** Eine Ebene, Ordner zuerst, ohne .git. */
  list: (repo: string, dir: string) => call<Entry[]>("fs_list", { repo, dir }),
  /** Fehler bei > 5 MB, Binaerdateien und Nicht-UTF-8. */
  read: (repo: string, path: string) => call<FileText>("fs_read", { repo, path }),
  /**
   * Liefert die neue mtime. expected = mtime vom letzten Lesen/Schreiben, null = Datei darf noch nicht existieren.
   * Weicht die Platte ab, kommt der Fehler KONFLIKT; force ueberschreibt trotzdem.
   */
  write: (repo: string, path: string, content: string, crlf: boolean, bom: boolean, expected: number | null, force: boolean) =>
    call<number>("fs_write", { repo, path, content, crlf, bom, expected, force }),
  /** mtime je Pfad, null = gibt es nicht. Ordner-mtime aendert sich, wenn Eintraege dazukommen oder verschwinden. */
  stat: (repo: string, paths: string[]) => call<(number | null)[]>("fs_stat", { repo, paths }),
  /** Leere Datei bzw. Ordner; Fehler, wenn es den Pfad schon gibt. */
  create: (repo: string, path: string, dir: boolean) => call<void>("fs_create", { repo, path, dir }),
  rename: (repo: string, from: string, to: string) => call<void>("fs_rename", { repo, from, to }),
  /** In den Papierkorb. */
  delete: (repo: string, path: string) => call<void>("fs_delete", { repo, path }),
  /** Alle Dateien des Projekts fuer Strg+P (getrackt + untracked, ohne ignorierte), sortiert. */
  files: (repo: string) => call<string[]>("fs_files", { repo }),
  /** Unified Diff Platte -> content (LF) fuer DiffView; "" wenn gleich. Kopf: "diff --git a/<path> b/<path>". */
  diff: (repo: string, path: string, content: string) => call<string>("fs_diff", { repo, path, content }),
};

/** Projekt fuer das preview-Schema freigeben (on false: wieder sperren); ohne das liefert previewUrl 403. Beliebig oft aufrufbar. */
export const previewAllow = (repo: string, on = true) => call<void>("preview_allow", { repo, on });

/**
 * XAML (ungespeicherter Stand) als PNG rendern; liefert Base64 ohne Praefix.
 * path = Datei im Repo, von dort aus wird App.xaml gesucht. Fehler: NUR_WINDOWS, DOTNET_FEHLT oder lesbarer Text.
 */
export const xamlRender = (repo: string, path: string, content: string) =>
  call<string>("xaml_render", { repo, path, content });

/**
 * Startziel der Run-Leiste in dir ("" = Projektordner) in einem PTY starten. Rust laesst nur feste Kommandos und
 * gepruefte Namen zu. Output kommt als Event "pty:<id>" (Bytes), das Ende als "pty-exit:<id>" (Exit-Code oder null);
 * ptyClose stoppt. Fuehrt Code aus dem Projekt aus: NUR auf Klick oder Taste.
 */
export const runStart = (id: string, repo: string, dir: string, target: Target) =>
  call<void>("run_start", { id, repo, dir, target });
export const ptyClose = (id: string) => call<void>("pty_close", { id });
export const ptyWrite = (id: string, data: string) => call<void>("pty_write", { id, data });
export const ptyResize = (id: string, cols: number, rows: number) => call<void>("pty_resize", { id, cols, rows });

/** Was ein Testlauf ausfuehrt; Dateien relativ zum Projektordner (dir). Spiegel von runner::Scope. */
export type TestScope =
  | { kind: "all"; files: string[] }
  | { kind: "file"; file: string; keys: string[] }
  | { kind: "test"; file: string; key: string; suite: boolean };
/** Spiegel von runner::Msg: Output-Zeilen (stdout + stderr, gebuendelt) und zuletzt das Ende. */
export type TestMsg = { event: "lines"; data: { lines: string[] } } | { event: "exit"; data: { code: number | null } };

/**
 * Testlauf in dir ("" = Projektordner) starten; kehrt nach dem Start zurueck, Output und Ende kommen ueber on.
 * Fuehrt Code aus dem Projekt aus: NUR auf Klick. Fehler beim Start (Tool fehlt) als abgelehntes Promise.
 */
export function testRun(id: string, repo: string, dir: string, fw: Fw, scope: TestScope, on: (m: TestMsg) => void) {
  const ch = new Channel<TestMsg>();
  ch.onmessage = on;
  return call<void>("test_run", { id, repo, dir, fw, scope, on: ch });
}
/** Lauf samt Kindprozessen abbrechen; Exit kommt danach noch ueber den Channel. */
export const testCancel = (id: string) => call<void>("test_cancel", { id });

/** Ungespeicherte Aenderungen melden: Rust fragt dann vor dem Beenden (Fenster schliessen, Tray "Beenden"). */
export const editorDirty = (dirty: boolean) => call<void>("editor_dirty", { dirty });

/**
 * URL einer Repo-Datei im preview-Schema, fuer <iframe src> und <img src>:
 *   macOS/Linux: preview://localhost/<encodeURIComponent(repo)>/<pfad, je Segment kodiert>
 *   Windows:     http://preview.localhost/<encodeURIComponent(repo)>/<pfad, je Segment kodiert>
 * Das Repo ist EIN Segment, der Pfad behaelt seine "/": relative Verweise (css/x.css, ../img/a.png) loesen
 * im Projekt auf. Verweise ab Wurzel ("/x.css") gehen nicht, ebenso wenig fetch und ES-Module (der iframe laeuft
 * in einer Sandbox ohne eigenen Origin, wie eine Seite von file://); dafuer ist das URL-Feld fuer Dev-Server da.
 * v haengt "?v=<v>" an (mtime oder Zaehler), damit die WebView nach dem Speichern neu laedt; Rust ignoriert die Query.
 */
export function previewUrl(repo: string, path: string, v?: number | string): string {
  // convertFileSrc kennt die Plattform: "preview://localhost/" bzw. "http://preview.localhost/".
  const origin = convertFileSrc("", "preview");
  const rel = path.split("/").map(encodeURIComponent).join("/");
  return `${origin}${encodeURIComponent(repo)}/${rel}${v === undefined ? "" : `?v=${v}`}`;
}

export type PreviewKind = "web" | "markdown" | "image" | "xaml";

/** Renderer der Vorschau nach Dateiendung (klein geschrieben). */
export const PREVIEW_KINDS: Record<string, PreviewKind> = {
  html: "web",
  htm: "web",
  md: "markdown",
  markdown: "markdown",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  avif: "image",
  bmp: "image",
  ico: "image",
  svg: "image",
  xaml: "xaml",
};

/** null = keine Vorschau fuer diese Datei. */
export const previewKind = (path: string): PreviewKind | null =>
  PREVIEW_KINDS[path.slice(path.lastIndexOf(".") + 1).toLowerCase()] ?? null;

/** Bilder, die fs.read ablehnt: sie oeffnen nur in der Vorschau, nie als Editor-Tab. svg ist Text und beides. */
export const isBinaryImage = (path: string) => previewKind(path) === "image" && !/\.svg$/i.test(path);
