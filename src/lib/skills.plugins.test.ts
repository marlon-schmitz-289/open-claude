import { test } from "node:test";
import assert from "node:assert/strict";
import { groupInstalled, pickAvailable, type InstalledPlugin, type AvailablePlugin } from "./skills.ts";

const inst = (id: string, scope: InstalledPlugin["scope"], projectPath?: string): InstalledPlugin => ({
  id,
  version: "1.0.0",
  scope,
  enabled: true,
  installPath: `/c/${id}`,
  ...(projectPath ? { projectPath } : {}),
});

const installed = [
  inst("caveman@caveman", "user"),
  inst("caveman@caveman", "local", "/d/randex"),
  inst("caveman@caveman", "local", "/d/open-claude"),
  inst("caveman@caveman", "local", "/d/x"),
  inst("ponytail@ponytail", "user"),
];

const available: AvailablePlugin[] = [
  {
    pluginId: "a@m",
    name: "a",
    marketplaceName: "m",
    description: "Rust language server",
    installCount: 5,
  },
  {
    pluginId: "b@m",
    name: "b",
    marketplaceName: "m",
    version: "1.0.0",
    installCount: 50,
  },
  { pluginId: "c@m", name: "c", marketplaceName: "m" },
  {
    pluginId: "caveman@caveman",
    name: "caveman",
    marketplaceName: "caveman",
    installCount: 999,
  },
];

test("groupInstalled: main = user, locals mit Pfaden", () => {
  const g = groupInstalled(installed);
  assert.equal(g.length, 2);
  assert.equal(g[0].main?.scope, "user");
  assert.deepEqual(
    g[0].locals.map((p) => p.projectPath),
    ["/d/randex", "/d/open-claude", "/d/x"],
  );
  assert.equal(g[1].locals.length, 0);
});

test("groupInstalled: nur lokal ohne main, Projekt wird main", () => {
  assert.equal(groupInstalled([inst("x@y", "local", "/p")])[0].main, undefined);
  assert.equal(groupInstalled([inst("x@y", "local", "/p"), inst("x@y", "project", "/q")])[0].main?.scope, "project");
});

test("pickAvailable: ohne Installierte, nach installCount, Suche, limit", () => {
  const ids = new Set(installed.map((p) => p.id));
  assert.deepEqual(
    pickAvailable(available, ids, "").items.map((p) => p.pluginId),
    ["b@m", "a@m", "c@m"],
  );
  assert.deepEqual(
    pickAvailable(available, ids, "language").items.map((p) => p.pluginId),
    ["a@m"],
  );
  assert.deepEqual(pickAvailable(available, ids, "caveman").items, []);
  const r = pickAvailable(available, ids, "", 2);
  assert.equal(r.items.length, 2);
  assert.equal(r.total, 3);
});
