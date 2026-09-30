import { test } from "node:test";
import assert from "node:assert/strict";
import {
  activate,
  closePanel,
  defaultLayout,
  edgeAt,
  filePanel,
  groupOf,
  groups,
  mainGroup,
  moveTab,
  normalize,
  openPanel,
  panels,
  parse,
  renamePanel,
  resize,
  splitAt,
  type Edge,
  type Layout,
  type Node,
  type Split,
} from "./dock.ts";

const [a, b, c] = ["a", "b", "c"].map(filePanel);
/** Standardlayout mit a, b, c in der main-Gruppe (g2). */
const abc = () => [a, b, c].reduce((l, p) => openPanel(l, p), defaultLayout());
const tabs = (l: Layout) => groups(l).map((g) => [g.id, ...g.tabs].join(" "));
const near = (x: number, y: number) => assert.ok(Math.abs(x - y) < 1e-9, `${x} ~ ${y}`);

/** Alle Invarianten aus dem Vertrag. */
function valid(l: Layout) {
  const gs = groups(l);
  assert.equal(new Set(gs.map((g) => g.id)).size, gs.length, "Gruppen-IDs eindeutig");
  assert.equal(new Set(panels(l)).size, panels(l).length, "Panel hoechstens einmal");
  assert.equal(gs.filter((g) => g.main).length, 1, "genau eine main-Gruppe");
  for (const g of gs) {
    assert.ok(g.tabs.length || g.main, "leere Gruppe nur als main");
    assert.ok(g.tabs.length ? g.tabs.includes(g.active!) : g.active === null, "active liegt in tabs");
  }
  const walk = (n: Node) => {
    if (n.kind === "group") return;
    assert.ok(n.children.length >= 2);
    assert.equal(n.sizes.length, n.children.length);
    assert.ok(n.sizes.every((s) => s > 0));
    near(n.sizes.reduce((x, y) => x + y, 0), 1);
    for (const ch of n.children) {
      assert.ok(ch.kind === "group" || ch.axis !== n.axis, "kein Split gleicher Achse darunter");
      walk(ch);
    }
  };
  walk(l);
}

/** Eingabe tief einfrieren: jede Veraenderung wuerde im strict mode werfen. */
function frozen<T>(v: T): T {
  if (v && typeof v === "object") Object.values(v).forEach(frozen);
  return Object.freeze(v);
}

test("defaultLayout: Dateien links, leere main-Gruppe rechts", () => {
  const l = defaultLayout();
  valid(l);
  assert.deepEqual(tabs(l), ["g1 files", "g2"]);
  assert.deepEqual((l as Split).sizes, [0.22, 0.78]);
  assert.equal(mainGroup(l).id, "g2");
  assert.equal(groupOf(l, "preview"), null);
});

test("openPanel: neu in die main-Gruppe oder die genannte Gruppe, vorhandenes wird nur aktiv", () => {
  let l = abc();
  valid(l);
  assert.deepEqual(tabs(l), ["g1 files", `g2 ${a} ${b} ${c}`]);
  assert.equal(mainGroup(l).active, c);
  l = openPanel(l, a, "g1");
  assert.deepEqual(tabs(l), ["g1 files", `g2 ${a} ${b} ${c}`]);
  assert.equal(mainGroup(l).active, a);
  l = openPanel(l, "preview", "g1");
  assert.deepEqual(tabs(l)[0], "g1 files preview");
  assert.equal(groupOf(openPanel(l, "x", "gibtsnicht"), "x")?.id, "g2");
});

test("activate: unbekanntes Panel laesst das Layout unveraendert", () => {
  const l = abc();
  assert.equal(activate(l, "nix"), l);
  assert.equal(mainGroup(activate(l, b)).active, b);
});

test("moveTab: umsortieren in derselben Gruppe (index = Position vor dem Herausnehmen)", () => {
  const l = abc();
  assert.deepEqual(mainGroup(moveTab(l, a, "g2", 2)).tabs, [b, a, c]);
  assert.deepEqual(mainGroup(moveTab(l, a, "g2", 99)).tabs, [b, c, a]);
  assert.deepEqual(mainGroup(moveTab(l, c, "g2", 0)).tabs, [c, a, b]);
  assert.deepEqual(mainGroup(moveTab(l, b, "g2", 1)).tabs, [a, b, c]);
  assert.deepEqual(mainGroup(moveTab(l, b, "g2", 2)).tabs, [a, b, c]);
  assert.equal(mainGroup(moveTab(l, a, "g2", 2)).active, a);
  assert.equal(moveTab(l, a, "gibtsnicht", 0), l);
});

test("moveTab: zwischen Gruppen, Quelle aktiviert den Nachbarn, leere Quelle faellt weg", () => {
  let l = moveTab(abc(), c, "g1", 0);
  valid(l);
  assert.deepEqual(tabs(l), [`g1 ${c} files`, `g2 ${a} ${b}`]);
  assert.equal(groupOf(l, c)?.active, c);
  assert.equal(mainGroup(l).active, b);
  // files als letzter Tab aus g1 heraus: g1 verschwindet, der Split faellt auf die main-Gruppe zusammen.
  l = moveTab(moveTab(l, c, "g2", 0), "files", "g2", 99);
  valid(l);
  assert.equal(l.kind, "group");
  assert.deepEqual(tabs(l), [`g2 ${c} ${a} ${b} files`]);
  // Neues Panel einfuegen
  assert.deepEqual(mainGroup(moveTab(l, "preview", "g2", 1)).tabs, [c, "preview", a, b, "files"]);
});

test("splitAt: jede Kante einer Gruppe, neue Gruppe bekommt die Haelfte", () => {
  const want: Record<Edge, [string, number[], number]> = {
    left: ["x", [0.22, 0.39, 0.39], 1],
    right: ["x", [0.22, 0.39, 0.39], 2],
    top: ["y", [0.5, 0.5], 0],
    bottom: ["y", [0.5, 0.5], 1],
  };
  for (const edge of Object.keys(want) as Edge[]) {
    const l = splitAt(abc(), c, "g2", edge) as Split;
    valid(l);
    const [axis, sizes, at] = want[edge];
    assert.equal(groupOf(l, c)?.id, "g3", edge);
    assert.deepEqual(groupOf(l, c)?.tabs, [c]);
    assert.deepEqual(mainGroup(l).tabs, [a, b]);
    // x wird in den Wurzel-Split eingezogen, y entsteht als neuer Split an der Stelle der Gruppe.
    const split = axis === "x" ? l : (l.children[1] as Split);
    assert.equal(split.axis, axis, edge);
    split.sizes.forEach((s, i) => near(s, sizes[i]));
    assert.equal((split.children[at] as { id: string }).id, "g3", edge);
  }
});

test("splitAt: Kante des ganzen Layouts gibt 0.25, der Rest skaliert auf 0.75", () => {
  let l = splitAt(defaultLayout(), "preview", null, "right") as Split;
  valid(l);
  assert.deepEqual(tabs(l), ["g1 files", "g2", "g3 preview"]);
  [0.22 * 0.75, 0.78 * 0.75, 0.25].forEach((s, i) => near(l.sizes[i], s));
  l = splitAt(defaultLayout(), "preview", null, "top") as Split;
  valid(l);
  assert.equal(l.axis, "y");
  assert.deepEqual(l.sizes, [0.25, 0.75]);
  assert.equal(l.children[1].kind, "split");
});

test("splitAt: einziger Tab einer Gruppe auf sich selbst und unbekannte Gruppe aendern nichts; main bleibt leer stehen", () => {
  const l = defaultLayout();
  assert.equal(splitAt(l, "files", "g1", "bottom"), l);
  assert.equal(splitAt(l, "files", "gibtsnicht", "left"), l);
  const one = splitAt(openPanel(l, a), a, "g2", "right");
  valid(one);
  assert.deepEqual(tabs(one), ["g1 files", "g2", `g3 ${a}`]);
});

test("neue Gruppen nehmen die kleinste freie Nummer", () => {
  let l = splitAt(abc(), c, "g2", "right"); // g3
  l = moveTab(l, "files", "g2", 0); // g1 faellt weg
  assert.deepEqual(tabs(l), [`g2 files ${a} ${b}`, `g3 ${c}`]);
  assert.equal(groupOf(splitAt(l, b, "g3", "bottom"), b)?.id, "g1");
});

test("closePanel: rechter Nachbar wird aktiv, sonst der linke; letzter Tab entfernt Gruppe und Split", () => {
  let l = activate(abc(), b);
  assert.equal(mainGroup(closePanel(l, b)).active, c);
  assert.equal(mainGroup(closePanel(activate(l, c), c)).active, b);
  assert.equal(mainGroup(closePanel(l, a)).active, b);
  // verschachtelt: g3 unter g2; letzter Tab zu -> y-Split faellt weg, danach files zu -> nur noch main
  l = splitAt(l, c, "g2", "bottom");
  assert.equal(((l as Split).children[1] as Split).axis, "y");
  l = closePanel(l, c);
  valid(l);
  assert.deepEqual(tabs(l), ["g1 files", `g2 ${a} ${b}`]);
  assert.deepEqual((l as Split).sizes, [0.22, 0.78]);
  l = closePanel(l, "files");
  assert.equal(l.kind, "group");
  l = closePanel(closePanel(l, a), b);
  assert.deepEqual(l, { kind: "group", id: "g2", tabs: [], active: null, main: true });
});

test("renamePanel: an Ort und Stelle, active zieht mit", () => {
  const l = renamePanel(abc(), c, "file:neu");
  assert.deepEqual(mainGroup(l).tabs, [a, b, "file:neu"]);
  assert.equal(mainGroup(l).active, "file:neu");
  assert.deepEqual(mainGroup(renamePanel(abc(), a, b)).tabs, [b, c]);
});

test("normalize: leere Gruppen raus, gleiche Achse einziehen, Anteile auf 1", () => {
  const l = normalize({
    kind: "split",
    axis: "x",
    sizes: [2, 2, 4],
    children: [
      { kind: "group", id: "g1", tabs: [], active: null },
      { kind: "group", id: "g2", tabs: ["files"], active: "weg" },
      {
        kind: "split",
        axis: "x",
        sizes: [0.5, 0.5],
        children: [
          { kind: "group", id: "g3", tabs: [], active: null, main: true },
          { kind: "split", axis: "y", sizes: [1, 1], children: [{ kind: "group", id: "g4", tabs: [a], active: a }, { kind: "group", id: "g5", tabs: [], active: null }] },
        ],
      },
    ],
  });
  valid(l);
  assert.deepEqual(tabs(l), ["g2 files", "g3", `g4 ${a}`]);
  (l as Split).sizes.forEach((s, i) => near(s, [1 / 3, 1 / 3, 1 / 3][i]));
});

test("resize: Anteil des Paars, begrenzt, Summe bleibt; ungueltig = unveraendert", () => {
  const l = splitAt(defaultLayout(), "preview", null, "right") as Split; // 0.165, 0.585, 0.25
  const r = resize(l, [], 1, 0.5) as Split;
  valid(r);
  [0.165, 0.4175, 0.4175].forEach((s, i) => near(r.sizes[i], s));
  near((resize(l, [], 0, 0) as Split).sizes[0], 0.75 * 0.05);
  near((resize(l, [], 0, 7) as Split).sizes[0], 0.75 * 0.95);
  for (const bad of [resize(l, [], 2, 0.5), resize(l, [], -1, 0.5), resize(l, [0], 0, 0.5), resize(l, [5], 0, 0.5), resize(l, [], 0, NaN)])
    assert.equal(bad, l);
  // verschachtelt ueber path
  const n = splitAt(abc(), c, "g2", "bottom");
  near((((resize(n, [1], 0, 0.8) as Split).children[1]) as Split).sizes[0], 0.8);
});

test("edgeAt: aeusseres Viertel ist Kante, Mitte null, naechste Kante gewinnt", () => {
  assert.equal(edgeAt(50, 50, 100, 100), null);
  assert.equal(edgeAt(10, 50, 100, 100), "left");
  assert.equal(edgeAt(90, 50, 100, 100), "right");
  assert.equal(edgeAt(50, 10, 100, 100), "top");
  assert.equal(edgeAt(50, 90, 100, 100), "bottom");
  assert.equal(edgeAt(5, 10, 100, 100), "left");
  assert.equal(edgeAt(10, 5, 100, 100), "top");
  assert.equal(edgeAt(30, 100, 400, 200), "left");
  assert.equal(edgeAt(110, 100, 400, 200), null);
});

test("parse: Muell faellt auf das Standardlayout zurueck", () => {
  const g = (o: object = {}) => ({ kind: "group", id: "g1", tabs: [], active: null, main: true, ...o });
  const s = (o: object = {}) => ({ kind: "split", axis: "x", sizes: [0.5, 0.5], children: [g({ tabs: ["files"], active: "files", main: undefined }), g({ id: "g2" })], ...o });
  const garbage: unknown[] = [
    null,
    "",
    "{",
    "42",
    "[]",
    '"x"',
    "{}",
    JSON.stringify({ kind: "nix" }),
    JSON.stringify(g({ main: undefined })), // keine main-Gruppe
    JSON.stringify(g({ id: 1 })),
    JSON.stringify(g({ tabs: "files" })),
    JSON.stringify(g({ tabs: [1], active: 1 })),
    JSON.stringify(g({ tabs: ["a"], active: "b" })),
    JSON.stringify(g({ tabs: ["a"], active: null })),
    JSON.stringify(g({ tabs: ["a", "a"], active: "a" })),
    JSON.stringify(g({ main: "ja" })),
    JSON.stringify(s({ axis: "z" })),
    JSON.stringify(s({ sizes: [1] })),
    JSON.stringify(s({ sizes: [1, 0] })),
    JSON.stringify(s({ sizes: [1, -1] })),
    JSON.stringify(s({ sizes: [1, "1"] })),
    JSON.stringify(s({ sizes: [1, null] })), // NaN/Infinity werden in JSON zu null
    JSON.stringify(s({ children: [g()], sizes: [1] })),
    JSON.stringify(s({ children: [g(), g()] })), // doppelte ID, zwei main
    JSON.stringify(s({ children: [g({ tabs: ["a"], active: "a" }), g({ id: "g2", main: undefined, tabs: ["a"], active: "a" })] })),
    JSON.stringify(s({ children: [g(), null] })),
    // Unbekannte Panels wuerden als namenlose Datei-Tabs gezeichnet.
    JSON.stringify(g({ tabs: ["foo"], active: "foo" })),
    JSON.stringify(g({ tabs: ["file:"], active: "file:" })),
    JSON.stringify(g({ tabs: [""], active: "" })),
  ];
  for (const text of garbage) assert.deepEqual(parse(text as string | null), defaultLayout(), String(text));
});

test("parse: gueltiges kommt normalisiert und ohne fremde Felder zurueck", () => {
  const l = splitAt(splitAt(abc(), c, "g2", "bottom"), "preview", null, "right");
  assert.deepEqual(parse(JSON.stringify(l)), l);
  const extra = JSON.parse(JSON.stringify(defaultLayout()));
  extra.fremd = 1;
  extra.children[0].fremd = 1;
  extra.sizes = [22, 78];
  const p = parse(JSON.stringify(extra)) as Split;
  assert.deepEqual(tabs(p), ["g1 files", "g2"]);
  assert.deepEqual(p.sizes, [0.22, 0.78]);
  assert.ok(!("fremd" in p) && !("fremd" in p.children[0]));
});

test("keine Funktion veraendert ihre Eingabe", () => {
  const l = frozen(splitAt(abc(), c, "g2", "bottom"));
  const before = JSON.stringify(l);
  openPanel(l, "x");
  openPanel(l, a);
  activate(l, a);
  moveTab(l, a, "g1", 0);
  moveTab(l, a, "g2", 9);
  splitAt(l, a, "g1", "top");
  splitAt(l, "preview", null, "left");
  closePanel(l, c);
  renamePanel(l, a, "file:z");
  normalize(l);
  resize(l, [], 0, 0.3);
  assert.equal(JSON.stringify(l), before);
});

test("Zufallsfolgen halten die Invarianten", () => {
  let seed = 7;
  const rnd = (n: number) => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) % n;
  const ps = ["files", "preview", a, b, c, filePanel("d")];
  const edges: Edge[] = ["left", "right", "top", "bottom"];
  let l = defaultLayout();
  for (let i = 0; i < 3000; i++) {
    const p = ps[rnd(ps.length)];
    const gs = groups(l);
    const g = gs[rnd(gs.length)].id;
    l = [
      () => openPanel(l, p, g),
      () => moveTab(l, p, g, rnd(4)),
      () => splitAt(l, p, rnd(4) ? g : null, edges[rnd(4)]),
      () => closePanel(l, p),
      () => resize(l, [], rnd(3), rnd(100) / 100),
      () => parse(JSON.stringify(l)),
    ][rnd(6)]();
    valid(l);
  }
});
