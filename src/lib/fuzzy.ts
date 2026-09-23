/**
 * Subsequenz-Suche wie in Editor-Palettes: "adbe" findet "adesk-backend".
 * Gibt die Trefferpositionen zum Hervorheben zurueck, oder null wenn nichts passt.
 * Score: hoeher ist besser — zusammenhaengende Treffer und Treffer am Wortanfang zaehlen mehr.
 */
export type Match = { score: number; hits: number[] };

const BOUNDARY = /[\\/\-_. ]/;

/** Ein Zeichen fuer den Vergleich: klein und ohne Akzente ("Ü" -> "u", "İ" -> "i"). */
export const fold = (c: string) => c.normalize("NFD").replace(/\p{M}/gu, "").normalize("NFC").toLowerCase();

export function fuzzy(text: string, query: string): Match | null {
  if (!query) return { score: 0, hits: [] };

  // Zeichenweise, damit hits zum Originaltext passen (toLowerCase() macht aus "İ" zwei Zeichen)
  const hay = text.split("").map(fold);
  const needle = query.split("").map(fold);
  const hits: number[] = [];
  let score = 0;
  let at = 0;

  for (const ch of needle) {
    if (ch === " " || !ch) continue; // Leerzeichen und einzelne Akzentzeichen
    const found = hay.indexOf(ch, at);
    if (found < 0) return null;

    if (found === at && hits.length) score += 8; // direkt anschliessend
    if (found === 0 || BOUNDARY.test(hay[found - 1])) score += 6; // Wortanfang
    score += Math.max(0, 4 - (found - at)); // kleine Luecken sind besser als grosse
    hits.push(found);
    at = found + 1;
  }

  // Kurze Namen bevorzugen, damit "core" eher ae-core-hcv als ae-core-graphics-wpf trifft.
  return { score: score - text.length * 0.05, hits };
}
