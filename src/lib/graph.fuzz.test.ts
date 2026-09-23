// Property-Tests fuer layout(): Invarianten gegen zufaellige DAGs und echte `git log --all --topo-order`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { layout, type Row } from "./graph.ts";

type C = { sha: string; parents: string[] };

let seed = 1;
const ri = (n: number) => Math.floor(((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x80000000) * n);

/** Prueft Zeile fuer Zeile, dass jede Lane oben genau dort weiterlaeuft, wo sie in der Zeile davor endete. */
function check(commits: C[], rows: Row[]) {
  let bottom = new Map<number, string>(); // Lane -> erwarteter SHA
  rows.forEach((r, i) => {
    const c = commits[i];
    const at = `Zeile ${i}`;
    const top = r.edges.filter((e) => e.kind !== "out").map((e) => e.from).sort();
    assert.deepEqual(top, [...bottom.keys()].sort(), `${at}: Lanes oben != unten davor`);
    assert.equal(r.lanesIn, i ? rows[i - 1].lanesOut : 0, `${at}: lanesIn`);
    const keys = r.edges.map((e) => `${e.from}-${e.to}-${e.kind}`); // Svelte-each-Key
    assert.equal(new Set(keys).size, keys.length, `${at}: doppelte Kanten ${keys}`);
    for (const [lane, sha] of bottom)
      if (sha === c.sha) assert.ok(r.edges.some((e) => e.kind === "in" && e.from === lane), `${at}: Lane ${lane} endet nicht im Commit`);
    for (const e of r.edges) {
      if (e.kind === "in") assert.ok(e.to === r.lane && bottom.get(e.from) === c.sha, `${at}: in-Kante`);
      if (e.kind === "out") assert.equal(e.from, r.lane, `${at}: out-Kante`);
      if (e.kind === "pass") assert.notEqual(e.to === r.lane && e.from === r.lane, true, `${at}: Commit auf belegter Lane`);
    }
    const outs = r.edges.filter((e) => e.kind === "out");
    assert.equal(outs.length, c.parents.length, `${at}: out-Kanten je Elternteil`);
    const next = new Map<number, string>();
    for (const e of r.edges)
      if (e.kind === "pass") {
        const sha = bottom.get(e.from)!;
        assert.ok(!next.has(e.to) || next.get(e.to) === sha, `${at}: Lane ${e.to} doppelt belegt`);
        next.set(e.to, sha);
      }
    outs.forEach((e, n) => {
      assert.ok(!next.has(e.to) || next.get(e.to) === c.parents[n], `${at}: Elternkante in fremde Lane ${e.to}`);
      next.set(e.to, c.parents[n]);
    });
    assert.equal(new Set(next.values()).size, next.size, `${at}: SHA in mehreren Lanes`);
    assert.equal(r.lanesOut, Math.max(-1, ...next.keys()) + 1, `${at}: lanesOut (keine verwaisten Lanes)`);
    bottom = next;
  });
}

/** Zufaelliger DAG in topologischer Ordnung (Kinder vor Eltern), mit Merges, Oktopus und mehreren Wurzeln. */
function randomDag(n: number): C[] {
  const shas = Array.from({ length: n }, (_, i) => `c${i}`);
  return shas.map((sha, i) => {
    const later = n - i - 1;
    const k = later === 0 || ri(8) === 0 ? 0 : ri(10) < 7 ? 1 : ri(3) === 0 ? 3 + ri(3) : 2;
    const parents = new Set<string>();
    for (let j = 0; j < k && later; j++) parents.add(shas[i + 1 + (ri(3) ? ri(Math.min(later, 4)) : ri(later))]);
    return { sha, parents: [...parents] };
  });
}

test("layout-Invarianten auf zufaelligen DAGs (auch als Teilseite)", () => {
  for (let s = 1; s <= 3000; s++) {
    seed = s;
    const commits = randomDag(1 + ri(40));
    try {
      check(commits, layout(commits.map((c) => ({ ...c }))));
      const k = 1 + ri(commits.length); // Paging: Eltern ausserhalb der Seite
      check(commits.slice(0, k), layout(commits.slice(0, k)));
    } catch (e) {
      throw new Error(`Seed ${s}: ${JSON.stringify(commits)}\n${e}`);
    }
  }
});

test("layout-Invarianten gegen echte git log --all --topo-order", () => {
  const git = (dir: string, a: string[]) =>
    execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...a], { cwd: dir, encoding: "utf8", stdio: "pipe" });
  for (let s = 1; s <= 2; s++) {
    seed = s * 7919;
    const dir = mkdtempSync(join(tmpdir(), "ocui-graph-"));
    try {
      git(dir, ["init", "-q", "-b", "b0"]);
      const branches = ["b0"];
      let n = 0;
      const commit = () => git(dir, ["commit", "-q", "--allow-empty", "-m", `c${n++}`]);
      commit();
      for (let k = 0; k < 25; k++) {
        const op = ri(10);
        if (op < 4) (git(dir, ["checkout", "-q", branches[ri(branches.length)]]), commit());
        else if (op < 6) {
          const b = `b${branches.length}`;
          git(dir, ["checkout", "-q", "-b", b, branches[ri(branches.length)]]);
          branches.push(b);
          commit();
        } else if (op < 9 && branches.length > 1) {
          const t = branches[ri(branches.length)];
          git(dir, ["checkout", "-q", t]);
          const others = branches.filter((b) => b !== t).slice(0, 1 + ri(3)); // bis zu 3 = Oktopus
          try {
            git(dir, ["merge", "-q", "--no-ff", "--allow-unrelated-histories", "-m", "m", ...others]);
          } catch {
            git(dir, ["merge", "--abort"]);
          }
        } else {
          const b = `b${branches.length}`; // weitere Wurzel
          git(dir, ["checkout", "-q", "--orphan", b]);
          branches.push(b);
          commit();
        }
      }
      const commits = git(dir, ["log", "--all", "--topo-order", "--format=%H %P"])
        .trim()
        .split("\n")
        .map((l) => {
          const [sha, ...parents] = l.trim().split(" ");
          return { sha, parents };
        });
      assert.ok(commits.some((c) => c.parents.length > 1));
      check(commits, layout(commits.map((c) => ({ ...c }))));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});
