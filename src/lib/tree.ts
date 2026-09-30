// Reine Helfer fuer den Dateibaum des Editors. Pfade relativ zum Repo mit "/", "" = Projektordner.
import type { Entry } from "./files.ts";

export type Row = Entry & { path: string; depth: number; open: boolean };

export const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
export const dirname = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));

/** Sichtbare Zeilen: aufgeklappte Ordner mit ihren (schon gelisteten) Kindern, in Anzeigereihenfolge. */
export function rows(lists: Record<string, Entry[]>, expanded: string[], dir = "", depth = 0, ignored = false): Row[] {
  return (lists[dir] ?? []).flatMap((e) => {
    const path = join(dir, e.name);
    const open = e.dir && expanded.includes(path);
    // In einem ignorierten Ordner ist alles ignoriert, auch wenn git die Kinder nicht einzeln meldet.
    const row = { ...e, ignored: ignored || e.ignored, path, depth, open };
    return open ? [row, ...rows(lists, expanded, path, depth + 1, row.ignored)] : [row];
  });
}
