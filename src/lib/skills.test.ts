import { test } from "node:test";
import assert from "node:assert/strict";
import { profileFor, resolve, localPayload, adjust, emptyProject, pkey, managed, renameProfile, touched, fromProject, profileOf, type Layers } from "./skills.ts";

const none = { skillOverrides: {}, enabledPlugins: {} };

test("resolve: Projekt > Profil > Team > User > Default", () => {
  const l: Layers = {
    user: { skillOverrides: { a: "off", b: "off", c: "off", d: "name-only" }, enabledPlugins: { p: true } },
    team: { skillOverrides: { b: "on", c: "on" }, enabledPlugins: {} },
    profile: { overrides: { c: "off", e: "off" }, plugins: { p: false } },
    project: { overrides: { e: "on" }, plugins: {} },
  };
  assert.deepEqual(resolve("skill", "a", l), { value: "off", source: "user" });
  assert.deepEqual(resolve("skill", "b", l), { value: "on", source: "team" });
  assert.deepEqual(resolve("skill", "c", l), { value: "off", source: "profile" });
  assert.deepEqual(resolve("skill", "e", l), { value: "on", source: "project" });
  assert.deepEqual(resolve("skill", "x", l), { value: "on", source: "default" });
  assert.deepEqual(resolve("plugin", "p", l), { value: false, source: "profile" });
  assert.deepEqual(resolve("plugin", "q", l), { value: false, source: "default" });
});

test("profileFor: undefined = nach Typ, null = keins, string = explizit", () => {
  const kp = { Tauri: "web", Unity: "game" };
  assert.equal(profileFor(undefined, "Tauri", kp), "web");
  assert.equal(profileFor(emptyProject(), "Unity", kp), "game");
  assert.equal(profileFor(emptyProject(), "Rust", kp), null);
  assert.equal(profileFor(emptyProject(), null, kp), null);
  assert.equal(profileFor({ ...emptyProject(), profile: null }, "Tauri", kp), null);
  assert.equal(profileFor({ ...emptyProject(), profile: "game" }, "Tauri", kp), "game");
});

test("adjust: setzen und zuruecksetzen aendern nur die Projekt-Anpassung", () => {
  const profile = { overrides: { a: "off" as const }, plugins: {} };
  let ps = adjust(undefined, "skill", "a", "on");
  ps = adjust(ps, "plugin", "p", false);
  assert.deepEqual(ps, { overrides: { a: "on" }, plugins: { p: false } });
  assert.equal(resolve("skill", "a", { user: none, team: none, profile, project: ps }).source, "project");
  ps = adjust(ps, "skill", "a");
  assert.deepEqual(ps.overrides, {});
  assert.deepEqual(resolve("skill", "a", { user: none, team: none, profile, project: ps }), { value: "off", source: "profile" });
  assert.deepEqual(profile.overrides, { a: "off" });
});

test("Profilwechsel behaelt Projekt-Anpassungen", () => {
  const ps = { ...adjust(undefined, "skill", "a", "on"), profile: "x" };
  const switched = { ...ps, profile: "y" };
  const y = { overrides: { a: "off" as const, b: "off" as const }, plugins: {} };
  assert.deepEqual(resolve("skill", "a", { user: none, team: none, profile: y, project: switched }).value, "on");
  assert.deepEqual(resolve("skill", "b", { user: none, team: none, profile: y, project: switched }).value, "off");
});

test("localPayload: nichts zu schreiben ohne Profil/Projekt und ohne User+Team-Konflikt", () => {
  assert.deepEqual(localPayload({ user: none, team: none }), { skillOverrides: null, enabledPlugins: null });
  const user = { skillOverrides: { a: "off" as const }, enabledPlugins: { p: true } };
  assert.deepEqual(localPayload({ user, team: none, profile: null, project: emptyProject() }), {
    skillOverrides: null,
    enabledPlugins: null,
  });
});

test("localPayload: voll gemergt, 'on' faellt raus, {} ueberdeckt User", () => {
  const user = { skillOverrides: { a: "off" as const, b: "name-only" as const }, enabledPlugins: { p: true, q: true } };
  const team = { skillOverrides: { c: "off" as const }, enabledPlugins: {} };
  // User+Team ohne Profil: Claude nimmt sonst nur Teams Objekt
  assert.deepEqual(localPayload({ user, team }).skillOverrides, { a: "off", b: "name-only", c: "off" });
  const project = { overrides: { a: "on" as const }, plugins: { q: false } };
  assert.deepEqual(localPayload({ user, team, project }), {
    skillOverrides: { b: "name-only", c: "off" },
    enabledPlugins: { p: true, q: false },
  });
  const only = { skillOverrides: { a: "off" as const }, enabledPlugins: {} };
  assert.deepEqual(localPayload({ user: only, team: none, profile: { overrides: { a: "on" }, plugins: {} } }).skillOverrides, {});
});

test("pkey: Trenner, Schlusstrenner, Windows-Gross/Klein", () => {
  assert.equal(pkey("C:\\dev\\proj\\"), "c:/dev/proj");
  assert.equal(pkey("c:/Dev/Proj"), "c:/dev/proj");
  assert.equal(pkey("/Users/X/proj/"), "/Users/X/proj");
  assert.equal(pkey("/"), "/");
});

test("managed: nur mit Store-Eintrag oder wirksamem Profil", () => {
  const s = { profiles: { web: emptyProject() }, kindProfiles: { Tauri: "web" }, projects: { "c:/dev/a": emptyProject() } };
  assert.equal(managed(s, "C:\\dev\\a", null), true);
  assert.equal(managed(s, "/b", "Tauri"), true);
  assert.equal(managed(s, "/b", "Unity"), false);
  assert.equal(managed({ ...s, profiles: {} }, "/b", "Tauri"), false);
});

test("renameProfile: Verweise ziehen mit, loeschen faellt auf Typ zurueck", () => {
  const p = { overrides: { a: "off" as const }, plugins: {} };
  const s = {
    profiles: { web: p, game: p },
    kindProfiles: { Tauri: "web", Unity: "game" },
    projects: { "/a": { ...emptyProject(), profile: "web" }, "/b": { ...emptyProject(), profile: null } },
  };
  const r = renameProfile(s, "web", "app");
  assert.deepEqual(Object.keys(r.profiles).sort(), ["app", "game"]);
  assert.deepEqual(r.kindProfiles, { Tauri: "app", Unity: "game" });
  assert.equal(r.projects["/a"].profile, "app");
  assert.equal(r.projects["/b"].profile, null);
  const d = renameProfile(s, "web", null);
  assert.deepEqual(Object.keys(d.profiles), ["game"]);
  assert.deepEqual(d.kindProfiles, { Unity: "game" });
  assert.ok(!("profile" in d.projects["/a"]));
  assert.deepEqual(s.kindProfiles, { Tauri: "web", Unity: "game" });
});

test("touched: Profilinhalt, Zuordnung und Anpassung", () => {
  const repos = [{ path: "/a", kind: "Tauri" }, { path: "/b", kind: "Unity" }, { path: "/c" }];
  const a = { profiles: { web: { overrides: {}, plugins: {} } }, kindProfiles: { Tauri: "web" }, projects: {} };
  assert.deepEqual(touched(a, a, repos), []);
  const b = { ...a, profiles: { web: { overrides: { x: "off" as const }, plugins: {} } } };
  assert.deepEqual(touched(a, b, repos), ["/a"]);
  assert.deepEqual(touched(a, { ...a, kindProfiles: { Tauri: "web", Unity: "web" } }, repos), ["/b"]);
  assert.deepEqual(touched(a, { ...a, projects: { "/c": adjust(undefined, "skill", "x", "off") } }, repos), ["/c"]);
});

test("fromProject: Profil plus Projekt-Anpassung, geloeschtes Profil = keins", () => {
  const s = {
    profiles: { web: { overrides: { a: "off" as const, b: "off" as const }, plugins: { p: true } } },
    kindProfiles: { Tauri: "web" },
    projects: { "/a": adjust(undefined, "skill", "b", "on"), "/x": { ...emptyProject(), profile: "weg" } },
  };
  assert.deepEqual(fromProject(s, "/a", "Tauri"), { overrides: { a: "off", b: "on" }, plugins: { p: true } });
  assert.equal(profileOf(s, "/x", "Tauri"), null);
});
