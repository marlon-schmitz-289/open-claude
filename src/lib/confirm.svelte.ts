// Rueckfragen als Promise, ein Dialog (in +page.svelte) fuer alle Aufrufer.
// window.confirm taugt in der Tauri-Webview nicht: es wird asynchron umgeleitet, ohne Dialog-Recht abgelehnt
// und liefert ein Promise, das als "ja" zaehlt.

type Question = { title: string; message: string; action: string; resolve: (ok: boolean) => void };

export const confirming = $state<{ q: Question | null }>({ q: null });

/** true = bestaetigt. Eine neue Frage beantwortet eine noch offene mit nein. */
export function confirm(message: string, action = "Fortfahren", title = "Sicher?"): Promise<boolean> {
  confirming.q?.resolve(false);
  return new Promise((resolve) => (confirming.q = { title, message, action, resolve }));
}

export function answer(ok: boolean) {
  const q = confirming.q;
  confirming.q = null;
  q?.resolve(ok);
}
