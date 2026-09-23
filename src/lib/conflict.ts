// Konfliktmarker einer Arbeitsdatei zerlegen und mit Entscheidungen wieder zusammensetzen.

export type ConflictBlock = {
  kind: "conflict";
  ours: string;
  /** Nur mit diff3/zdiff3-Markern (|||||||). */
  base: string | null;
  theirs: string;
  oursLabel: string;
  theirsLabel: string;
  /** Originaltext samt Markern (fuer ungeloeste Bloecke). */
  raw: string;
};

export type Segment = { kind: "plain"; text: string } | ConflictBlock;

/** "ours-theirs" = beide, ours zuerst; "theirs-ours" umgekehrt. */
export type Choice = "ours" | "theirs" | "ours-theirs" | "theirs-ours" | "base";

const marker = (line: string, ch: string) => line.startsWith(ch.repeat(7)) && /^.{7}(\s|$)/.test(line);

const label = (line: string) => line.slice(7).trim();

/** Zeilen behalten ihre Zeilenenden, render(parse(x), []) === x. */
export function parseConflicts(text: string): Segment[] {
  const lines = text.split(/(?<=\n)/);
  const out: Segment[] = [];
  const plain = (t: string) => {
    const last = out[out.length - 1];
    if (last?.kind === "plain") last.text += t;
    else if (t) out.push({ kind: "plain", text: t });
  };
  let i = 0;
  while (i < lines.length) {
    if (!marker(lines[i], "<")) {
      plain(lines[i++]);
      continue;
    }
    // Block bis >>>>>>> einlesen; unvollstaendige Bloecke bleiben Text
    const start = i;
    const parts: string[][] = [[]];
    let seps = "";
    let end = -1;
    for (let j = i + 1; j < lines.length; j++) {
      const l = lines[j];
      if (marker(l, "|") && seps === "") (seps = "|", parts.push([]));
      else if (marker(l, "=") && !seps.includes("=")) (seps += "=", parts.push([]));
      else if (marker(l, ">") && seps.includes("=")) {
        end = j;
        break;
      } else parts[parts.length - 1].push(l);
    }
    if (end < 0) {
      plain(lines[i++]);
      continue;
    }
    const diff3 = seps === "|=";
    out.push({
      kind: "conflict",
      ours: parts[0].join(""),
      base: diff3 ? parts[1].join("") : null,
      theirs: parts[diff3 ? 2 : 1].join(""),
      oursLabel: label(lines[start]),
      theirsLabel: label(lines[end]),
      raw: lines.slice(start, end + 1).join(""),
    });
    i = end + 1;
  }
  return out;
}

/** Zusammensetzen; choices[n] gilt fuer den n-ten Konflikt, fehlend/null = Marker bleiben. */
export function render(segments: Segment[], choices: (Choice | null | undefined)[]): string {
  let n = 0;
  return segments
    .map((s) => {
      if (s.kind === "plain") return s.text;
      const c = choices[n++];
      if (c === "ours") return s.ours;
      if (c === "theirs") return s.theirs;
      if (c === "ours-theirs") return s.ours + s.theirs;
      if (c === "theirs-ours") return s.theirs + s.ours;
      if (c === "base") return s.base ?? "";
      return s.raw;
    })
    .join("");
}
