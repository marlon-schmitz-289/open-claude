export const ZOOM_MIN = 50;
export const ZOOM_MAX = 200;

/** Gespeicherten Wert auf den erlaubten Bereich bringen (settings.json kann von Hand geaendert sein). */
export const clampZoom = (pct: unknown) =>
  typeof pct === "number" && Number.isFinite(pct) ? Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(pct))) : 100;

/** Naechste Stufe in Prozent (10er-Schritte); dir 0 setzt zurueck. */
export const zoomStep = (pct: number, dir: number) =>
  dir === 0 ? 100 : clampZoom(Math.round(pct / 10) * 10 + dir * 10);

type Key = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey">;

/**
 * 1 groesser, -1 kleiner, 0 zuruecksetzen, null = kein Zoom-Kuerzel. AltGr (Strg+Alt) zaehlt nicht.
 * Nur e.key: der Nummernblock liefert mit NumLock dieselben Zeichen, ohne ist Strg+Einfg Kopieren.
 */
export function zoomKey(e: Key, mac: boolean): 1 | -1 | 0 | null {
  if (!(mac ? e.metaKey : e.ctrlKey) || e.altKey) return null;
  if (e.key === "+" || e.key === "=") return 1;
  if (e.key === "-") return -1;
  if (e.key === "0") return 0;
  return null;
}
