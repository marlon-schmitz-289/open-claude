// Unified-Diff aus `git diff` parsen und Teil-Patches fuer `git apply` bauen.

export type LineKind = "add" | "del" | "ctx" | "meta";

/** text ohne Praefix-Zeichen; bei "meta" die ganze Zeile ("\ No newline at end of file"). */
export type DiffLine = { kind: LineKind; text: string; old?: number; new?: number };

export type Hunk = {
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
};

export type FileStatus = "modified" | "added" | "deleted" | "renamed" | "copied";

export type DiffFile = {
  oldPath: string;
  newPath: string;
  status: FileStatus;
  binary: boolean;
  /** Rohe Kopfzeilen ab "diff --git" bis vor den ersten Hunk, fuer Patches. */
  head: string[];
  hunks: Hunk[];
};

const HUNK = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

/** Pfad aus git-Ausgabe: C-Quoting aufloesen (Umlaute kommen als \303\244) und a/ b/ abschneiden. */
function path(raw: string): string {
  let s = raw.trim();
  if (s.startsWith('"') && s.endsWith('"')) {
    const bytes: number[] = [];
    const esc: Record<string, number> = { n: 10, t: 9, r: 13, '"': 34, "\\": 92, a: 7, b: 8, f: 12, v: 11 };
    const inner = s.slice(1, -1);
    for (let i = 0; i < inner.length; i++) {
      const c = inner[i];
      if (c !== "\\") {
        bytes.push(...new TextEncoder().encode(c));
      } else if (/[0-7]/.test(inner[i + 1])) {
        bytes.push(parseInt(inner.slice(i + 1, i + 4), 8));
        i += 3;
      } else {
        bytes.push(esc[inner[++i]] ?? inner.charCodeAt(i));
      }
    }
    s = new TextDecoder().decode(new Uint8Array(bytes));
  }
  return s === "/dev/null" ? s : s.replace(/^[ab]\//, "");
}

export function parseDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  const lines = text.split("\n");
  let i = 0;
  while (i < lines.length) {
    if (!lines[i].startsWith("diff --git ")) {
      i++;
      continue;
    }
    const f: DiffFile = { oldPath: "", newPath: "", status: "modified", binary: false, head: [lines[i]], hunks: [] };
    // Fallback fuer Dateien ohne ---/+++ (Binaer, reine Modusaenderung): "diff --git a/x b/x"
    const m = lines[i].slice(11).match(/^("?a\/.*?"?) ("?b\/.*"?)$/);
    if (m) [f.oldPath, f.newPath] = [path(m[1]), path(m[2])];
    i++;
    while (i < lines.length && !lines[i].startsWith("diff --git ") && !lines[i].startsWith("@@")) {
      const l = lines[i];
      if (l.startsWith("new file mode")) f.status = "added";
      else if (l.startsWith("deleted file mode")) f.status = "deleted";
      else if (l.startsWith("rename from ")) [f.status, f.oldPath] = ["renamed", path(l.slice(12))];
      else if (l.startsWith("rename to ")) f.newPath = path(l.slice(10));
      else if (l.startsWith("copy from ")) [f.status, f.oldPath] = ["copied", path(l.slice(10))];
      else if (l.startsWith("copy to ")) f.newPath = path(l.slice(8));
      else if (l.startsWith("--- ")) f.oldPath = path(l.slice(4));
      else if (l.startsWith("+++ ")) f.newPath = path(l.slice(4));
      else if (l.startsWith("Binary files ") || l === "GIT binary patch") f.binary = true;
      if (l !== "") f.head.push(l);
      i++;
    }
    if (f.oldPath === "/dev/null") f.oldPath = f.newPath;
    if (f.newPath === "/dev/null") f.newPath = f.oldPath;
    // Hunks; Ende ueber die Zeilenzahlen, damit Inhalt wie "diff --git" nicht stoert
    let h: RegExpMatchArray | null;
    while (i < lines.length && (h = lines[i].match(HUNK))) {
      const hunk: Hunk = {
        header: lines[i],
        oldStart: +h[1],
        oldLines: h[2] === undefined ? 1 : +h[2],
        newStart: +h[3],
        newLines: h[4] === undefined ? 1 : +h[4],
        lines: [],
      };
      i++;
      let [o, n, oldLeft, newLeft] = [hunk.oldStart, hunk.newStart, hunk.oldLines, hunk.newLines];
      while (i < lines.length && (oldLeft > 0 || newLeft > 0 || lines[i].startsWith("\\"))) {
        const l = lines[i];
        const text = l.slice(1);
        if (l.startsWith("+")) (hunk.lines.push({ kind: "add", text, new: n++ }), newLeft--);
        else if (l.startsWith("-")) (hunk.lines.push({ kind: "del", text, old: o++ }), oldLeft--);
        else if (l.startsWith("\\")) hunk.lines.push({ kind: "meta", text: l });
        else (hunk.lines.push({ kind: "ctx", text, old: o++, new: n++ }), oldLeft--, newLeft--);
        i++;
      }
      f.hunks.push(hunk);
    }
    files.push(f);
  }
  return files;
}

const PREFIX: Record<LineKind, string> = { add: "+", del: "-", ctx: " ", meta: "" };

function emit(head: string[], hunk: Hunk, lines: DiffLine[]): string {
  const oldN = lines.filter((l) => l.kind !== "add" && l.kind !== "meta").length;
  const newN = lines.filter((l) => l.kind !== "del" && l.kind !== "meta").length;
  // Start 0 gilt nur fuer leere Seiten
  const start = (s: number, n: number) => (n > 0 ? Math.max(s, 1) : s);
  const header = `@@ -${start(hunk.oldStart, oldN)},${oldN} +${start(hunk.newStart, newN)},${newN} @@`;
  return [...head, header, ...lines.map((l) => PREFIX[l.kind] + l.text)].join("\n") + "\n";
}

/** Patch nur fuer diesen Hunk (fuer `git apply [--cached] [--reverse]`). */
export function hunkPatch(file: DiffFile, hunk: Hunk): string {
  return emit(file.head, hunk, hunk.lines);
}

/** Kopf einer neuen/geloeschten Datei zur normalen Aenderung machen (Datei bleibt auf beiden Seiten). */
function asModify(head: string[]): string[] {
  const plus = head.find((l) => l.startsWith("+++ ") && l !== "+++ /dev/null");
  const minus = head.find((l) => l.startsWith("--- ") && l !== "--- /dev/null");
  return head
    .filter((l) => !/^(new|deleted) file mode|^index /.test(l))
    .map((l) =>
      l === "--- /dev/null"
        ? plus!.replace(/^\+\+\+ ("?)b\//, "--- $1a/")
        : l === "+++ /dev/null"
          ? minus!.replace(/^--- ("?)a\//, "+++ $1b/")
          : l,
    );
}

const flip = (l: DiffLine): DiffLine =>
  l.kind === "add" ? { ...l, kind: "del" } : l.kind === "del" ? { ...l, kind: "add" } : l;

/**
 * Patch nur fuer die gewaehlten Zeilen (Indizes in hunk.lines).
 * Vorwaerts (stagen): nicht gewaehlte + fallen weg, nicht gewaehlte - werden Kontext.
 * reverse (unstagen/verwerfen, mit `git apply --reverse`): umgekehrt, damit der Rest bleibt.
 * Liefert "" wenn keine Aenderungszeile gewaehlt ist.
 */
export function linesPatch(file: DiffFile, hunk: Hunk, selected: Set<number>, reverse = false): string {
  // Rueckwaerts = vorwaerts auf dem gespiegelten Hunk, danach zurueckspiegeln
  const src = reverse ? hunk.lines.map(flip) : hunk.lines;
  const out: DiffLine[] = [];
  let changes = 0;
  let partial = false;
  for (let i = 0; i < src.length; i++) {
    const l = src[i];
    const meta = src[i + 1]?.kind === "meta" ? src[i + 1] : null;
    const on = selected.has(i);
    if (l.kind === "meta") continue; // wird mit der Zeile davor behandelt
    if (l.kind === "ctx" || on) {
      out.push(l);
      // Ohne Newline nur, wenn auf der neuen Seite nichts mehr folgt. Rueckwaerts moeglich: gewaehltes
      // "-x" ohne Newline wird gespiegelt zu "+x", danach bleibt ein nicht gewaehltes "+y" als Kontext.
      const newAfter = src.some(
        (x, j) => j > i && (x.kind === "ctx" || (x.kind === "del" && !selected.has(j)) || (x.kind === "add" && selected.has(j))),
      );
      if (meta && !(l.kind === "add" && newAfter)) out.push(meta);
      if (l.kind !== "ctx") changes++;
      continue;
    }
    partial = true;
    if (l.kind === "add") continue; // samt Newline-Marker weglassen
    // Nicht gewaehltes - bleibt als Kontext. Ohne Newline am Ende geht das nur, wenn danach
    // nichts mehr hinzukommt; sonst Zeile ersetzen durch dieselbe Zeile mit Newline.
    const addsAfter = src.some((x, j) => j > i && x.kind === "add" && selected.has(j));
    if (meta && addsAfter) {
      out.push(l, meta, { kind: "add", text: l.text });
      changes++;
    } else {
      out.push({ ...l, kind: "ctx" });
      if (meta) out.push(meta);
    }
  }
  if (changes === 0) return "";
  const lines = reverse ? out.map(flip) : out;
  // Teilweise Loeschung (vorwaerts) bzw. teilweise zurueckgenommene Neuanlage: Datei bleibt bestehen
  const keep = partial && file.status === (reverse ? "added" : "deleted");
  return emit(keep ? asModify(file.head) : file.head, hunk, lines);
}
