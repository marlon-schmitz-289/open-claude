// Rueckfrage vor Git-Aktionen mit "Nicht erneut anzeigen". Ein Dialog (AskDialog) fuer alle Aufrufer.

/** Aktionen mit abschaltbarer Rueckfrage; Label fuer die Einstellungen. */
export const ASKS = {
  checkout: "Branch wechseln",
  merge: "Mergen",
  rebase: "Rebasen",
  pick: "Cherry-Pick / Revert",
  stash: "Stash anwenden",
} as const;
export type AskKey = keyof typeof ASKS;

const KEY = "git.noAsk";
function read(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {};
  } catch {
    return {};
  }
}

/** true = Rueckfrage fuer diese Aktion abgeschaltet. */
export const noAsk = $state<Record<string, boolean>>(read());
export function setNoAsk(key: AskKey, off: boolean) {
  noAsk[key] = off;
  try {
    localStorage.setItem(KEY, JSON.stringify(noAsk));
  } catch {
    // Ohne localStorage gilt die Wahl nur fuer diese Sitzung.
  }
}

export const pending = $state<{ ask: { key: AskKey; title: string; message: string; run: () => void } | null }>({
  ask: null,
});

/** Fuehrt run sofort aus, wenn die Rueckfrage abgeschaltet ist, sonst nach Bestaetigung im Dialog. */
export function ask(key: AskKey, title: string, message: string, run: () => void) {
  if (noAsk[key]) run();
  else pending.ask = { key, title, message, run };
}
