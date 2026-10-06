// Mehrfachauswahl in einer Liste (Pfade als Schluessel). `order` ist die sichtbare Reihenfolge.

/** Auswahl auf das beschraenken, was noch in der Liste steht; Ergebnis in Listenreihenfolge. */
export function pruneSel(sel: string[], order: string[]): string[] {
  const set = new Set(sel);
  return order.filter((p) => set.has(p));
}

/** Strg+Klick: Eintrag zur Auswahl hinzu oder wieder heraus. */
export const toggleSel = (sel: string[], path: string): string[] =>
  sel.includes(path) ? sel.filter((p) => p !== path) : [...sel, path];

/** Umschalt+Klick: alles vom Anker bis zum Ziel (beide inklusive); ohne gueltigen Anker nur das Ziel. */
export function rangeSel(order: string[], anchor: string, path: string): string[] {
  const b = order.indexOf(path);
  if (b < 0) return [];
  const a = order.indexOf(anchor);
  return a < 0 ? [path] : order.slice(Math.min(a, b), Math.max(a, b) + 1);
}
