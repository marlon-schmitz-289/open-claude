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

/** Geaenderter Bereich [von, bis) einer Zeile; null = Zeile unveraendert. */
export type Mark = [number, number] | null;

/**
 * Zeilen von `a` gegen `b` (LCS): pro Zeile von `a`, ob und wo sie abweicht.
 * Geaenderte Zeilen werden innerhalb einer Luecke der Reihe nach gepaart, markiert wird der Teil
 * zwischen gemeinsamem Anfang und Ende; ohne Partner die ganze Zeile.
 */
export function diffLines(a: string[], b: string[]): Mark[] {
  // ponytail: O(n*m)-Tabelle; riesige Bloecke pauschal als geaendert, Myers wenn das stoert
  if (a.length * b.length > 1_000_000) return a.map((l) => [0, l.length]);
  const w = b.length + 1;
  const dp = new Uint32Array((a.length + 1) * w);
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      dp[i * w + j] = a[i] === b[j] ? dp[(i + 1) * w + j + 1] + 1 : Math.max(dp[(i + 1) * w + j], dp[i * w + j + 1]);

  const out: Mark[] = a.map(() => null);
  let del: number[] = [];
  let add: number[] = [];
  const flush = () => {
    del.forEach((i, k) => {
      const [x, y] = [a[i], b[add[k]]];
      if (y === undefined) return (out[i] = [0, x.length]);
      const max = Math.min(x.length, y.length);
      let p = 0;
      while (p < max && x[p] === y[p]) p++;
      let s = 0;
      while (s < max - p && x[x.length - 1 - s] === y[y.length - 1 - s]) s++;
      // Surrogat-Paare nicht zerschneiden
      if (p && /[\uD800-\uDBFF]/.test(x[p - 1])) p--;
      if (s && /[\uDC00-\uDFFF]/.test(x[x.length - s])) s--;
      out[i] = [p, x.length - s];
    });
    del = [];
    add = [];
  };
  let [i, j] = [0, 0];
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) (flush(), i++, j++);
    else if (j >= b.length || (i < a.length && dp[(i + 1) * w + j] >= dp[i * w + j + 1])) del.push(i++);
    else add.push(j++);
  }
  flush();
  return out;
}
