import { forge } from "$lib/git";

/** xterm-linkHandler fuer OSC-8-Links (claude gibt Links so aus): http(s) im Browser, Rest verwerfen.
 *  xterms Standard fragt per window.confirm und oeffnet window.open, also ein App-Fenster. */
export const LINKS = {
  activate: (_e: MouseEvent, uri: string) => {
    if (/^https?:\/\//i.test(uri)) forge.openUrl(uri).catch(() => {});
  },
};

// Terminal-Farben (xterm ITheme) fuer Terminal und RunPane.
// Aus app.css abgeleitet (oklch -> Hex), bei Token-Aenderung neu umrechnen.
export const DARK = {
  // --card mit Alpha 0: WebGL malt Zellen mit Attribut (dim, kursiv) deckend in dieser Farbe,
  // bei #00000000 also schwarz.
  background: "#181b2100",
  foreground: "#e5e8ed", // --foreground 0.93 0.008 265
  cursor: "#a793f4", // --primary 0.72 0.14 292
  cursorAccent: "#181b21", // --card
  selectionBackground: "#a793f440",
  black: "#21242b", // --secondary 0.26 0.014 265
  red: "#eb8182", // 0.72 0.13 20
  green: "#6fc884", // 0.76 0.13 150
  yellow: "#e8be62", // 0.82 0.12 85
  blue: "#6fa7ee", // 0.72 0.12 255
  magenta: "#a793f4", // --primary
  cyan: "#5fc9db", // 0.78 0.10 210
  white: "#c1c4c9", // 0.82 0.008 265
  brightBlack: "#8a8f99", // --muted-foreground 0.65 0.016 265
  brightRed: "#fda1a0", // 0.80 0.11 20
  brightGreen: "#95dfa4", // 0.84 0.11 150
  brightYellow: "#f6d389", // 0.88 0.10 85
  brightBlue: "#92c1fd", // 0.80 0.10 255
  brightMagenta: "#bfb1ff", // 0.80 0.11 292
  brightCyan: "#91e0ee", // 0.86 0.08 210
  brightWhite: "#e5e8ed", // --foreground
};
export const LIGHT = {
  background: "#fdfdff00", // --card, Alpha 0 (gleicher WebGL-Trick)
  foreground: "#1c1f27",
  cursor: "#7046cf",
  cursorAccent: "#fdfdff",
  selectionBackground: "#7046cf40",
  black: "#1c1f27",
  red: "#c12535",
  green: "#137738",
  yellow: "#8d5e00",
  blue: "#2460b7",
  magenta: "#7046cf",
  cyan: "#0e7381",
  white: "#676c75",
  // Hell: Bright dunkler als normal, sonst zu wenig Kontrast auf weiss.
  brightBlack: "#555b66",
  brightRed: "#a51d2b",
  brightGreen: "#0f6a31",
  brightYellow: "#7e4f04",
  brightBlue: "#1351a6",
  brightMagenta: "#6335be",
  brightCyan: "#006471",
  brightWhite: "#5e636f", // --muted-foreground
};

