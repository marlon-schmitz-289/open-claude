// Reine Entscheidungen des Editor-Stores, ohne Runen und ohne Importe, damit `node --test` sie direkt laedt.

/** Was das Polling mit einer offenen Datei tut. */
export type Verdict = "none" | "reload" | "close" | "changed" | "deleted";

/**
 * was = mtime beim letzten Lesen/Speichern, now = mtime auf der Platte (null = gibt es nicht).
 * Ohne lokale Aenderungen wird still nachgeladen bzw. der Tab geschlossen, mit ihnen nur der Konflikt gemeldet.
 */
export function verdict(dirty: boolean, was: number | null, now: number | null): Verdict {
  if (was === now) return "none";
  if (dirty) return now === null ? "deleted" : "changed";
  return now === null ? "close" : "reload";
}

/** path ist `at` selbst oder liegt in diesem Ordner. */
export const under = (path: string, at: string) => path === at || path.startsWith(at + "/");

/** Neuer Pfad, nachdem `from` (Datei oder Ordner) nach `to` umbenannt wurde; null = nicht betroffen. */
export const moved = (path: string, from: string, to: string): string | null =>
  under(path, from) ? to + path.slice(from.length) : null;

/** Kleinste Ersetzung, die `a` zu `b` macht: gemeinsamer Anfang und Schluss bleiben stehen (und mit ihnen Cursor und Scrollposition). */
export function span(a: string, b: string): { from: number; to: number; insert: string } {
  const max = Math.min(a.length, b.length);
  let from = 0;
  while (from < max && a[from] === b[from]) from++;
  let end = 0;
  while (end < max - from && a[a.length - 1 - end] === b[b.length - 1 - end]) end++;
  return { from, to: a.length - end, insert: b.slice(from, b.length - end) };
}

/**
 * Eingabe im URL-Feld der Vorschau -> URL fuer den iframe; "" = Datei zeigen, null = abgelehnt.
 * Nur http(s): "javascript:" im iframe liefe mit den Rechten der App. Ohne Schema wird http:// ergaenzt ("localhost:5173").
 */
export function devUrl(input: string): string | null {
  const v = input.trim();
  if (!v) return "";
  try {
    const u = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(v) ? v : `http://${v}`);
    return u.protocol === "http:" || u.protocol === "https:" ? u.href : null;
  } catch {
    return null;
  }
}
