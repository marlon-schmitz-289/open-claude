// Property-Tests fuer die Schichten-Logik in skills.ts: Projekt > Profil > Team > User, Payload passt zu resolve.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  adjust,
  localPayload,
  profileFor,
  renameProfile,
  resolve,
  type Layers,
  type Profile,
  type ProjectSkills,
  type Settings,
  type SkillValue,
} from "./skills.ts";

let seed = 1;
const ri = (n: number) => Math.floor(((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x80000000) * n);
const pick = <T>(a: readonly T[]) => a[ri(a.length)];

const KEYS = ["a", "b", "c", "cav:commit", "cav:review", "x-y"];
const PLUGINS = ["cav@cav", "pony@pony", "foo@bar"];
const VALUES: SkillValue[] = ["on", "off", "user-invocable-only", "name-only"];

function rec<V>(keys: string[], val: () => V): Record<string, V> {
  return Object.fromEntries(keys.filter(() => ri(3) === 0).map((k) => [k, val()]));
}
const settings = (): Settings => ({ skillOverrides: rec(KEYS, () => pick(VALUES)), enabledPlugins: rec(PLUGINS, () => ri(2) === 0) });
const profile = (): Profile => ({ overrides: rec(KEYS, () => pick(["on", "off"] as const)), plugins: rec(PLUGINS, () => ri(2) === 0) });
function project(): ProjectSkills | undefined {
  if (ri(5) === 0) return undefined;
  const ps: ProjectSkills = profile();
  const r = ri(3);
  if (r === 1) ps.profile = null;
  if (r === 2) ps.profile = pick(["web", "unity", "gone"]);
  return ps;
}
const layers = (): Layers => ({ user: settings(), team: settings(), profile: ri(4) === 0 ? pick([null, undefined]) : profile(), project: project() });

function deepFreeze<T>(o: T): T {
  if (o && typeof o === "object") for (const v of Object.values(Object.freeze(o))) deepFreeze(v);
  return o;
}

/** Was Claude Code sieht: pro Key das Objekt der hoechsten Datei mit Eintraegen gewinnt ganz (RECHERCHE: mergePerKey=false). */
function claudeSees(l: Layers, kind: "skill" | "plugin", key: string) {
  const p = localPayload(l);
  const f = kind === "skill" ? "skillOverrides" : "enabledPlugins";
  const def = kind === "skill" ? "on" : false;
  const local = p[f];
  if (local !== null) return (local as Record<string, unknown>)[key] ?? def;
  const obj = [l.team[f], l.user[f]].find((o) => Object.keys(o).length) ?? {};
  return (obj as Record<string, unknown>)[key] ?? def;
}

const N = 5000;

test("Projekt-Anpassung gewinnt immer, sonst Profil > Team > User > Default", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    for (const key of KEYS) {
      const r = resolve("skill", key, l);
      const order = [
        ["project", l.project?.overrides],
        ["profile", l.profile?.overrides],
        ["team", l.team.skillOverrides],
        ["user", l.user.skillOverrides],
      ] as const;
      const hit = order.find(([, m]) => m && key in m);
      assert.deepEqual(r, hit ? { value: hit[1]![key], source: hit[0] } : { value: "on", source: "default" }, `seed ${s} ${key}`);
    }
    for (const key of PLUGINS) {
      const r = resolve("plugin", key, l);
      if (l.project && key in l.project.plugins) assert.deepEqual(r, { value: l.project.plugins[key], source: "project" }, `seed ${s}`);
    }
  }
});

test("adjust setzt: Projekt gewinnt danach unabhaengig von allen anderen Schichten", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    const key = pick(KEYS);
    const v = pick(["on", "off"] as const);
    const project = adjust(l.project, "skill", key, v);
    assert.deepEqual(resolve("skill", key, { ...l, project }), { value: v, source: "project" }, `seed ${s}`);
    const pk = pick(PLUGINS);
    const b = ri(2) === 0;
    const p2 = adjust(project, "plugin", pk, b);
    assert.deepEqual(resolve("plugin", pk, { ...l, project: p2 }), { value: b, source: "project" }, `seed ${s}`);
    assert.deepEqual(resolve("skill", key, { ...l, project: p2 }), { value: v, source: "project" }, `seed ${s}`);
  }
});

test("Zuruecksetzen entfernt nur diese Projekt-Anpassung, Rest und Profil bleiben", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    const key = pick(KEYS);
    const reset = adjust(l.project, "skill", key);
    assert.ok(!(key in reset.overrides), `seed ${s}`);
    const { [key]: _, ...rest } = l.project?.overrides ?? {};
    assert.deepEqual(reset.overrides, rest, `seed ${s}`);
    assert.deepEqual(reset.plugins, l.project?.plugins ?? {}, `seed ${s}`);
    assert.equal(reset.profile, l.project?.profile, `seed ${s}`);
    // Wert faellt auf die naechste Schicht zurueck, als gaebe es die Anpassung nicht
    const without = { ...l, project: { ...(l.project ?? {}), overrides: rest, plugins: l.project?.plugins ?? {} } };
    assert.deepEqual(resolve("skill", key, { ...l, project: reset }), resolve("skill", key, without), `seed ${s}`);
    assert.notEqual(resolve("skill", key, { ...l, project: reset }).source, "project", `seed ${s}`);

    const pk = pick(PLUGINS);
    const pr = adjust(l.project, "plugin", pk);
    assert.ok(!(pk in pr.plugins), `seed ${s}`);
    assert.deepEqual(pr.overrides, l.project?.overrides ?? {}, `seed ${s}`);
  }
});

test("Profilwechsel laesst Projekt-Anpassungen unveraendert und sie gewinnen weiter", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    const ps = l.project ?? { overrides: {}, plugins: {} };
    const switched: ProjectSkills = { ...ps, profile: pick([undefined, null, "web", "unity"]) };
    const other = profile();
    for (const key of Object.keys(ps.overrides)) {
      assert.deepEqual(resolve("skill", key, { ...l, profile: other, project: switched }), { value: ps.overrides[key], source: "project" }, `seed ${s}`);
    }
    // Umbenennen/Loeschen eines Profils fasst Anpassungen nicht an
    const store = { profiles: { web: profile(), unity: profile() }, kindProfiles: { Tauri: "web" }, projects: { "/p": switched } };
    for (const to of ["neu", null]) {
      const out = renameProfile(deepFreeze(structuredClone(store)), pick(["web", "unity"]), to);
      assert.deepEqual(out.projects["/p"].overrides, ps.overrides, `seed ${s}`);
      assert.deepEqual(out.projects["/p"].plugins, ps.plugins, `seed ${s}`);
    }
  }
});

test("profileFor: explizit (auch null) schlaegt Typ, undefined faellt auf Typ zurueck", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const ps = project();
    const kind = pick([null, undefined, "", "Tauri", "Unity", "Rust"]);
    const kp = rec(["Tauri", "Unity", "Rust"], () => pick(["web", "unity", ""]));
    const got = profileFor(ps, kind, kp);
    if (ps && ps.profile !== undefined) assert.equal(got, ps.profile, `seed ${s}`);
    else assert.equal(got, (kind && kp[kind]) || null, `seed ${s}`);
  }
});

test("localPayload: Claude sieht fuer jeden Key denselben Wert wie resolve", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    for (const key of KEYS) assert.equal(claudeSees(l, "skill", key), resolve("skill", key, l).value, `seed ${s} ${key} ${JSON.stringify(l)}`);
    for (const key of PLUGINS) assert.equal(claudeSees(l, "plugin", key), resolve("plugin", key, l).value, `seed ${s} ${key} ${JSON.stringify(l)}`);
    const p = localPayload(l);
    if (p.skillOverrides) assert.ok(!Object.values(p.skillOverrides).includes("on"), `seed ${s}: "on" ist implizit`);
  }
});

test("localPayload: nichts zu schreiben ohne Profil und Projekt, solange nur eine Settings-Datei Werte hat", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l: Layers = { user: settings(), team: { skillOverrides: {}, enabledPlugins: {} } };
    if (ri(2)) [l.user, l.team] = [l.team, l.user];
    assert.deepEqual(localPayload(l), { skillOverrides: null, enabledPlugins: null }, `seed ${s}`);
  }
});

test("Keine Mutation der Eingaben", () => {
  for (let s = 1; s <= N; s++) {
    seed = s;
    const l = layers();
    const before = JSON.stringify(l);
    deepFreeze(l);
    localPayload(l);
    for (const key of KEYS) resolve("skill", key, l);
    for (const key of PLUGINS) resolve("plugin", key, l);
    adjust(l.project, "skill", pick(KEYS), pick(["on", "off"] as const));
    adjust(l.project, "skill", pick(KEYS));
    adjust(l.project, "plugin", pick(PLUGINS), true);
    adjust(l.project, "plugin", pick(PLUGINS));
    profileFor(l.project, "Tauri", deepFreeze({ Tauri: "web" }));
    assert.equal(JSON.stringify(l), before, `seed ${s}`);
  }
});

// Skill-Namen sind frei waehlbar (Ordnername); "in" sieht auch geerbte Object.prototype-Keys.
test("Skill namens constructor/toString: kein Wert aus dem Prototyp", () => {
  const l: Layers = { user: { skillOverrides: {}, enabledPlugins: {} }, team: { skillOverrides: {}, enabledPlugins: {} }, project: { overrides: {}, plugins: {} } };
  for (const key of ["constructor", "toString", "hasOwnProperty"]) {
    assert.deepEqual(resolve("skill", key, l), { value: "on", source: "default" }, key);
    assert.deepEqual(resolve("plugin", key, l), { value: false, source: "default" }, key);
  }
});
