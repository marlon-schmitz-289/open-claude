// Hell/Dunkel/System. Wahl in localStorage, weil app.html sie vor dem ersten Paint braucht.
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Mode = "system" | "light" | "dark";

const mq = matchMedia("(prefers-color-scheme: dark)");

function read(): Mode {
  try {
    const v = localStorage.getItem("theme");
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export const theme = $state({ mode: read(), dark: false });

function apply() {
  theme.dark = theme.mode === "dark" || (theme.mode === "system" && mq.matches);
  document.documentElement.classList.toggle("dark", theme.dark);
}

export function setMode(m: Mode) {
  theme.mode = m;
  try {
    localStorage.setItem("theme", m);
  } catch {
    /* gilt nur fuer diese Sitzung */
  }
  apply();
  getCurrentWindow()
    .setTheme(m === "system" ? null : m)
    .then(apply, () => {}); // erzwungenes Fenster-Theme verfaelscht matchMedia bis hierher
}

mq.addEventListener("change", apply);
setMode(theme.mode); // auch beim Start: Fenster-Theme an die gespeicherte Wahl angleichen
