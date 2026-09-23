// Lanes fuer den Commit-Graph aus einer Liste in --topo-order.
// Lanes behalten ihre Spalte, freie Spalten werden wiederverwendet; der erste Elternteil zieht nach links.

/**
 * Linie innerhalb einer Zeile, Spalten als Lane-Index. Die Zeile hat oben/Mitte/unten:
 * - "pass": fremde Lane laeuft durch, oben from -> unten to (to < from, wenn sie in die Commit-Lane einschwenkt)
 * - "in":   von oben aus Lane from in den Commit (to = Commit-Lane, Mitte)
 * - "out":  vom Commit (from = Commit-Lane, Mitte) nach unten in Lane to (Eltern)
 */
export type Edge = { from: number; to: number; kind: "pass" | "in" | "out"; color: number };

/** color ist ein Index, den die UI auf eine Palette abbildet (color % palette.length). */
export type Row = { lane: number; color: number; edges: Edge[]; lanesIn: number; lanesOut: number };

type Node = { sha: string; parents: string[] };

export function layout(commits: Node[]): Row[] {
  const lanes: (string | null)[] = []; // erwartete SHA je Spalte
  const colors: number[] = [];
  let next = 0;
  const trim = () => {
    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
  };
  const free = () => {
    const i = lanes.indexOf(null);
    return i < 0 ? lanes.length : i;
  };

  return commits.map((c) => {
    const edges: Edge[] = [];
    const lanesIn = lanes.length;
    let lane = lanes.indexOf(c.sha);
    const color = lane < 0 ? next++ : colors[lane];
    if (lane < 0) lane = free(); // Branch-Spitze: neue Spalte
    for (let i = 0; i < lanesIn; i++) {
      const sha = lanes[i];
      if (sha === null) continue;
      if (sha === c.sha) {
        edges.push({ from: i, to: lane, kind: "in", color: colors[i] });
        lanes[i] = null;
      } else edges.push({ from: i, to: i, kind: "pass", color: colors[i] });
    }
    c.parents.forEach((p, n) => {
      const j = lanes.indexOf(p);
      if (n === 0 && j > lane) {
        // Erster Elternteil wird weiter rechts erwartet: diese Lane nach links holen
        edges.find((e) => e.kind === "pass" && e.from === j)!.to = lane;
        lanes[j] = null;
        lanes[lane] = p;
        colors[lane] = color;
        edges.push({ from: lane, to: lane, kind: "out", color });
      } else if (j >= 0) {
        // Eltern wird schon erwartet: Linie laeuft dort hinein
        edges.push({ from: lane, to: j, kind: "out", color: n === 0 ? color : colors[j] });
      } else if (n === 0) {
        lanes[lane] = p;
        colors[lane] = color;
        edges.push({ from: lane, to: lane, kind: "out", color });
      } else {
        const k = free();
        lanes[k] = p;
        colors[k] = next++;
        edges.push({ from: lane, to: k, kind: "out", color: colors[k] });
      }
    });
    trim();
    return { lane, color, edges, lanesIn, lanesOut: lanes.length };
  });
}
