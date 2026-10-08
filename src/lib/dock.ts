// Layout-Baum des Editors: Splits (Achse + Kinder mit Anteilen) und Tab-Gruppen.
// Reine Funktionen ohne DOM und ohne Importe, damit `node --test src/lib/dock.test.ts` sie direkt laedt.
// Alle Funktionen liefern einen NEUEN Baum und veraendern die Eingabe nicht (Layout liegt in $state.raw).

/** "files", "preview", "tests", "run" oder "file:<repo-relativer Pfad>". */
export type PanelId = string;

export type Group = {
  kind: "group";
  /** Eindeutig im Baum: "g1", "g2", ... Neue Gruppen bekommen die kleinste freie Nummer. */
  id: string;
  tabs: PanelId[];
  /** Immer ein Eintrag aus tabs; null nur bei leerer Gruppe. */
  active: PanelId | null;
  /** Genau eine Gruppe im Baum: Editorbereich. Bleibt auch leer stehen, neue Dateien landen hier. */
  main?: true;
};

export type Split = {
  kind: "split";
  /** "x" = Kinder nebeneinander, "y" = uebereinander. */
  axis: "x" | "y";
  /** Mindestens zwei; nie ein Split mit derselben Achse direkt darunter. */
  children: Node[];
  /** Anteile der Kinder, gleiche Laenge wie children, jeder > 0, Summe 1. */
  sizes: number[];
};

export type Node = Group | Split;
export type Layout = Node;
export type Edge = "left" | "right" | "top" | "bottom";

export const FILES: PanelId = "files";
export const PREVIEW: PanelId = "preview";
export const TESTS: PanelId = "tests";
export const RUN: PanelId = "run";
export const filePanel = (path: string): PanelId => `file:${path}`;
/** Pfad eines Datei-Panels, sonst null. */
export const panelPath = (panel: PanelId): string | null => (panel.startsWith("file:") ? panel.slice(5) : null);

const axisOf = (edge: Edge) => (edge === "left" || edge === "right" ? "x" : "y");

/** Tiefe Kopie nur mit den bekannten Feldern; danach darf die Kopie veraendert werden ($state-Proxys bleiben unberuehrt). */
function copy(n: Node): Node {
  if (n.kind === "split") return { kind: "split", axis: n.axis, children: n.children.map(copy), sizes: [...n.sizes] };
  const g: Group = { kind: "group", id: n.id, tabs: [...n.tabs], active: n.active };
  if (n.main) g.main = true;
  return g;
}

/** Panel aus seiner Gruppe nehmen (veraendert den Baum!); aktiv wird der rechte, sonst der linke Nachbar. */
function take(root: Node, panel: PanelId) {
  const g = groupOf(root, panel);
  if (!g) return;
  const i = g.tabs.indexOf(panel);
  g.tabs.splice(i, 1);
  if (g.active === panel) g.active = g.tabs[i] ?? g.tabs[i - 1] ?? null;
}

/** Kleinste freie Gruppen-ID. */
function freeId(root: Node): string {
  const used = new Set(groups(root).map((g) => g.id));
  let n = 1;
  while (used.has(`g${n}`)) n++;
  return `g${n}`;
}

/** Knoten `old` im Baum durch `neu` ersetzen (veraendert den Baum!). */
function swap(root: Node, old: Node, neu: Node): Node {
  if (root === old) return neu;
  if (root.kind === "split") root.children = root.children.map((c) => swap(c, old, neu));
  return root;
}

/** normalize() fuer einen Teilbaum; null = faellt weg. */
function norm(n: Node): Node | null {
  if (n.kind === "group") {
    if (!n.tabs.length && !n.main) return null;
    const g = copy(n) as Group;
    if (g.active === null || !g.tabs.includes(g.active)) g.active = g.tabs[0] ?? null;
    return g;
  }
  const children: Node[] = [];
  const sizes: number[] = [];
  n.children.forEach((child, i) => {
    const c = norm(child);
    if (!c) return;
    if (c.kind === "split" && c.axis === n.axis) {
      children.push(...c.children);
      sizes.push(...c.sizes.map((s) => s * n.sizes[i]));
    } else {
      children.push(c);
      sizes.push(n.sizes[i]);
    }
  });
  if (children.length < 2) return children[0] ?? null;
  const sum = sizes.reduce((a, b) => a + b, 0);
  return { kind: "split", axis: n.axis, children, sizes: sizes.map((s) => s / sum) };
}

/** Split x [g1: files (0.22), g2: leer, main (0.78)]. Die Vorschau ist anfangs zu. */
export function defaultLayout(): Layout {
  return {
    kind: "split",
    axis: "x",
    children: [
      { kind: "group", id: "g1", tabs: [FILES], active: FILES },
      { kind: "group", id: "g2", tabs: [], active: null, main: true },
    ],
    sizes: [0.22, 0.78],
  };
}

/** Alle Gruppen, von links/oben nach rechts/unten (Tiefensuche). */
export function groups(layout: Layout): Group[] {
  return layout.kind === "group" ? [layout] : layout.children.flatMap(groups);
}

/** Alle Panels in der Reihenfolge von groups(). */
export function panels(layout: Layout): PanelId[] {
  return groups(layout).flatMap((g) => g.tabs);
}

/** Gruppe, in der das Panel liegt. */
export function groupOf(layout: Layout, panel: PanelId): Group | null {
  return groups(layout).find((g) => g.tabs.includes(panel)) ?? null;
}

/** Die Gruppe mit main: true (gibt es in jedem gueltigen Layout genau einmal). */
export function mainGroup(layout: Layout): Group {
  const all = groups(layout);
  return all.find((g) => g.main) ?? all[0];
}

/**
 * Panel zeigen: liegt es schon im Baum, wird es dort aktiv. Sonst kommt es ans Ende von `group`
 * (falls es die Gruppe gibt), sonst in die main-Gruppe, und wird aktiv.
 */
export function openPanel(layout: Layout, panel: PanelId, group?: string | null): Layout {
  if (groupOf(layout, panel)) return activate(layout, panel);
  const root = copy(layout);
  const g = groups(root).find((g) => g.id === group) ?? mainGroup(root);
  g.tabs.push(panel);
  g.active = panel;
  return normalize(root);
}

/** Panel in seiner Gruppe aktiv machen; unbekanntes Panel: Layout unveraendert. */
export function activate(layout: Layout, panel: PanelId): Layout {
  const root = copy(layout);
  const g = groupOf(root, panel);
  if (!g) return layout;
  g.active = panel;
  return normalize(root);
}

/**
 * Tab in die Tab-Leiste von `group` legen (verschieben, umsortieren oder neu einfuegen) und aktivieren.
 * index = Position in den Tabs der Zielgruppe VOR dem Herausnehmen ("vor dem Tab, der jetzt dort steht");
 * index >= tabs.length haengt an. Die Quellgruppe aktiviert den Nachbarn; wird sie leer, faellt sie weg (ausser main).
 * Unbekannte Gruppe: Layout unveraendert.
 */
export function moveTab(layout: Layout, panel: PanelId, group: string, index: number): Layout {
  const root = copy(layout);
  const target = groups(root).find((g) => g.id === group);
  if (!target) return layout;
  const at = target.tabs.indexOf(panel);
  let i = Math.max(0, Math.min(Number.isNaN(index) ? Infinity : index, target.tabs.length));
  // Liegt der Tab in derselben Gruppe davor, rutscht das Ziel durch das Herausnehmen um eins nach links.
  if (at >= 0 && at < i) i--;
  take(root, panel);
  target.tabs.splice(i, 0, panel);
  target.active = panel;
  return normalize(root);
}

/**
 * Panel in eine neue Gruppe an der Kante von `group` legen (verschieben oder neu einfuegen) und aktivieren.
 * Die neue Gruppe bekommt die Haelfte des Platzes der Zielgruppe. group null = Kante des ganzen Layouts;
 * die neue Gruppe bekommt dann 0.25, der Rest teilt sich 0.75 im alten Verhaeltnis.
 * Ist das Panel der einzige Tab der Zielgruppe (und diese nicht main), bleibt das Layout unveraendert.
 */
export function splitAt(layout: Layout, panel: PanelId, group: string | null, edge: Edge): Layout {
  let root = copy(layout);
  const target = group === null ? root : groups(root).find((g) => g.id === group);
  if (!target) return layout;
  if (target.kind === "group" && group !== null && !target.main && target.tabs.length === 1 && target.tabs[0] === panel)
    return layout;
  const neu: Group = { kind: "group", id: freeId(root), tabs: [panel], active: panel };
  take(root, panel);
  const first = edge === "left" || edge === "top";
  const share = group === null ? 0.25 : 0.5;
  root = swap(root, target, {
    kind: "split",
    axis: axisOf(edge),
    children: first ? [neu, target] : [target, neu],
    sizes: first ? [share, 1 - share] : [1 - share, share],
  });
  return normalize(root);
}

/** Panel entfernen; die Gruppe aktiviert den Nachbarn (rechts, sonst links). Danach wie normalize(). */
export function closePanel(layout: Layout, panel: PanelId): Layout {
  const root = copy(layout);
  take(root, panel);
  return normalize(root);
}

/** Panel an Ort und Stelle umbenennen (Datei umbenannt); `active` zieht mit. */
export function renamePanel(layout: Layout, from: PanelId, to: PanelId): Layout {
  // Ziel schon offen (Datei ueber eine offene umbenannt): der alte Tab faellt weg, sonst laege das Panel doppelt im Baum.
  if (from !== to && groupOf(layout, to)) return closePanel(layout, from);
  const root = copy(layout);
  for (const g of groups(root)) {
    g.tabs = g.tabs.map((t) => (t === from ? to : t));
    if (g.active === from) g.active = to;
  }
  return normalize(root);
}

/**
 * Invarianten herstellen: leere Gruppen ohne main entfernen, Splits mit einem Kind durch das Kind ersetzen,
 * Splits gleicher Achse in den Eltern-Split einziehen (Anteile multipliziert), sizes auf Summe 1 bringen.
 * Alle anderen Funktionen liefern bereits normalisierte Baeume.
 */
export function normalize(layout: Layout): Layout {
  return norm(layout) ?? defaultLayout();
}

/**
 * Trennleiste zwischen Kind index und index + 1 des Splits an `path` (Kind-Indizes ab der Wurzel, [] = Wurzel).
 * share (0..1) = Anteil von Kind index an der Summe beider Kinder; wird auf 0.05..0.95 begrenzt.
 * Die Summe der beiden bleibt gleich. Ungueltiger Pfad/Index: Layout unveraendert.
 */
export function resize(layout: Layout, path: number[], index: number, share: number): Layout {
  const root = copy(layout);
  let n = root;
  for (const i of path) {
    if (n.kind !== "split" || !n.children[i]) return layout;
    n = n.children[i];
  }
  if (n.kind !== "split" || !Number.isInteger(index) || index < 0 || index > n.sizes.length - 2 || Number.isNaN(share))
    return layout;
  const sum = n.sizes[index] + n.sizes[index + 1];
  n.sizes[index] = sum * Math.min(0.95, Math.max(0.05, share));
  n.sizes[index + 1] = sum - n.sizes[index];
  return normalize(root);
}

/**
 * Wo ein gezogener Tab ueber dem Inhalt einer Gruppe landet: aeusseres Viertel = Kante (naechste Kante gewinnt),
 * Mitte = null (als Tab in die Gruppe). x/y relativ zur linken oberen Ecke, w/h Groesse des Bereichs.
 */
export function edgeAt(x: number, y: number, w: number, h: number): Edge | null {
  const near: [number, Edge][] = [
    [x / w, "left"],
    [1 - x / w, "right"],
    [y / h, "top"],
    [1 - y / h, "bottom"],
  ];
  const [d, edge] = near.reduce((a, b) => (b[0] < a[0] ? b : a));
  return d < 0.25 ? edge : null;
}

/**
 * Layout aus localStorage (JSON) lesen; wirft nie. Bei null, kaputtem JSON oder verletzten Invarianten (falsche Typen,
 * sizes nicht endlich/<= 0/falsche Laenge, doppelte Gruppen-IDs oder Panels, unbekannte Panels, active nicht in tabs,
 * nicht genau eine main-Gruppe) kommt defaultLayout(). Gueltiges wird normalisiert, unbekannte Felder fallen weg.
 */
export function parse(text: string | null): Layout {
  try {
    const ids = new Set<string>();
    const seen = new Set<PanelId>();
    let mains = 0;
    const bad = (): never => {
      throw new Error("layout");
    };
    const check = (n: any): Node => {
      if (n?.kind === "group") {
        if (typeof n.id !== "string" || ids.has(n.id) || !Array.isArray(n.tabs)) bad();
        ids.add(n.id);
        for (const t of n.tabs) {
          if (typeof t !== "string" || seen.has(t) || (![FILES, PREVIEW, TESTS, RUN].includes(t) && !panelPath(t))) bad();
          seen.add(t);
        }
        if (n.tabs.length ? !n.tabs.includes(n.active) : n.active !== null) bad();
        if (n.main !== undefined && n.main !== true) bad();
        if (n.main) mains++;
        return copy(n);
      }
      if (n?.kind !== "split" || (n.axis !== "x" && n.axis !== "y")) bad();
      if (!Array.isArray(n.children) || !Array.isArray(n.sizes) || n.children.length < 2) bad();
      if (n.sizes.length !== n.children.length || !n.sizes.every((s: unknown) => typeof s === "number" && Number.isFinite(s) && s > 0))
        bad();
      return { kind: "split", axis: n.axis, children: n.children.map(check), sizes: [...n.sizes] };
    };
    const root = check(JSON.parse(text ?? "null"));
    return mains === 1 ? normalize(root) : defaultLayout();
  } catch {
    return defaultLayout();
  }
}
