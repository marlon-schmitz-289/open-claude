/**
 * Subsequenz-Suche wie in Editor-Palettes: "adbe" findet "adesk-backend".
 * Gibt die Trefferpositionen zum Hervorheben zurueck, oder null wenn nichts passt.
 * Score: hoeher ist besser — zusammenhaengende Treffer und Treffer am Wortanfang zaehlen mehr.
 */
export type Match = { score: number; hits: number[] };

const BOUNDARY = /[\\/\-_. ]/;

export function fuzzy(text: string, query: string): Match | null {
  if (!query) return { score: 0, hits: [] };

  const hay = text.toLowerCase();
  const needle = query.toLowerCase();
  const hits: number[] = [];
  let score = 0;
  let at = 0;

  for (const ch of needle) {
    if (ch === " ") continue;
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
