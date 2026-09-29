import { test } from "node:test";
import assert from "node:assert/strict";
import {
  profileFor, profileOf, resolve, localPayload, adjust, renameProfile, touched, fromProject, layers,
  type SkillStore, type Settings,
} from "./skills.ts";

const none: Settings = { skillOverrides: {}, enabledPlugins: {} };
const info = { user: none, team: none };
const web = { overrides: { a: "off" as const }, plugins: { "caveman@caveman": false } };

const store = (): SkillStore => ({
  profiles: { web, game: { overrides: {}, plugins: {} } },
  kindProfiles: { Tauri: "web" },
  projects: {
    "/typ": { overrides: {}, plugins: {} },
    "/explizit": { overrides: {}, plugins: {}, profile: "web" },
    "/keins": { overrides: {}, plugins: {}, profile: null },
    "/fremd": { overrides: {}, plugins: {}, profile: "game" },
  },
});
const repos = [
  { path: "/typ", kind: "Tauri" },
  { path: "/explizit", kind: "Unity" },
  { path: "/keins", kind: "Tauri" },
  { path: "/fremd", kind: "Tauri" },
  { path: "/neu", kind: "Tauri" }, // nie angefasst, kein Store-Eintrag
  { path: "/ohneTyp", kind: null },
];

const users = (s: SkillStore, name: string) => repos.filter((r) => profileOf(s, r.path, r.kind) === s.profiles[name]).map((r) => r.path);

test("Profilnutzer: explizit schlaegt Typ, null schliesst aus, kind null/undefined nie", () => {
  const s = store();
  assert.deepEqual(users(s, "web"), ["/typ", "/explizit", "/neu"]);
  assert.deepEqual(users(s, "game"), ["/fremd"]);
});

test("Typ ohne Zuordnung und leerer kind ergeben kein Profil", () => {
  const kp = { Tauri: "web" };
  assert.equal(profileFor(undefined, "Unity", kp), null);
  assert.equal(profileFor(undefined, "", kp), null);
  assert.equal(profileFor(undefined, undefined, kp), null);
  assert.equal(profileOf(store(), "/ohneTyp", null), null);
});

test("geloeschtes Profil, auf das Projekt oder Typ noch verweist, gilt als keins", () => {
  const s = store();
  delete s.profiles.web; // Verweise bleiben stehen (z.B. alter Store)
  assert.equal(profileOf(s, "/explizit", "Unity"), null);
  assert.equal(profileOf(s, "/typ", "Tauri"), null);
  assert.deepEqual(localPayload(layers(info, s, "/typ", "Tauri")), { skillOverrides: null, enabledPlugins: null });
  assert.deepEqual(fromProject(s, "/explizit", "Unity"), { overrides: {}, plugins: {} });
});

test("renameProfile loeschen: explizit -> nach Typ, Typ-Zuordnung weg, andere unberuehrt", () => {
  const s = renameProfile(store(), "web", null);
  assert.deepEqual(Object.keys(s.profiles), ["game"]);
  assert.deepEqual(s.kindProfiles, {});
  assert.equal("profile" in s.projects["/explizit"], false);
  assert.equal(s.projects["/keins"].profile, null);
  assert.equal(s.projects["/fremd"].profile, "game");
  for (const r of repos) assert.notEqual(profileFor(s.projects[r.path], r.kind, s.kindProfiles), "web");
});

test("renameProfile umbenennen: alle Nutzer folgen, per Typ nicht touched", () => {
  const a = store();
  const b = renameProfile(a, "web", "frontend");
  assert.deepEqual(users(b, "frontend"), ["/typ", "/explizit", "/neu"]);
  assert.equal("web" in b.profiles, false);
  assert.equal(b.profiles.frontend, web);
  // explizite Projekte aendern ihren Store-Eintrag -> einmal neu geschrieben (idempotent), Typ-Nutzer nicht
  assert.deepEqual(touched(a, b, repos), ["/explizit"]);
  // Umbenennen auf sich selbst ist No-op
  assert.deepEqual(renameProfile(a, "web", "web"), a);
});

test("touched nach Loeschen: nur Nutzer des Profils", () => {
  const a = store();
  assert.deepEqual(touched(a, renameProfile(a, "web", null), repos), ["/typ", "/explizit", "/neu"]);
});

test("touched: Profil-Aenderung trifft Nutzer, Projekt-Anpassung bleibt erhalten", () => {
  const a = store();
  a.projects["/typ"] = adjust(a.projects["/typ"], "skill", "a", "on");
  const b: SkillStore = { ...a, profiles: { ...a.profiles, web: { overrides: { a: "off", b: "off" }, plugins: {} } } };
  assert.deepEqual(touched(a, b, repos), ["/typ", "/explizit", "/neu"]);
  assert.equal(resolve("skill", "a", layers(info, b, "/typ", "Tauri")).source, "project");
  assert.equal(resolve("skill", "b", layers(info, b, "/typ", "Tauri")).source, "profile");
});

test("adjust behaelt profile-Modus (undefined bleibt nach Typ, null bleibt keins)", () => {
  const s = store();
  const t = adjust(s.projects["/typ"], "skill", "x", "off");
  assert.equal("profile" in t, false);
  assert.equal(profileFor(t, "Tauri", s.kindProfiles), "web");
  assert.equal(adjust(s.projects["/keins"], "plugin", "p@m", true).profile, null);
  assert.equal(adjust(undefined, "skill", "x").profile, undefined);
});

test("verschwundener Skill mit Override: resolve/payload/zuruecksetzen funktionieren weiter", () => {
  const project = adjust(undefined, "skill", "weg", "off");
  const l = { user: none, team: none, profile: null, project };
  assert.deepEqual(resolve("skill", "weg", l), { value: "off", source: "project" });
  assert.deepEqual(localPayload(l).skillOverrides, { weg: "off" });
  assert.deepEqual(adjust(project, "skill", "weg").overrides, {});
});

test("Plugin-Skill-Keys mit ':' und Plugin-IDs mit '@' werden unveraendert durchgereicht", () => {
  const user: Settings = { skillOverrides: { "caveman:caveman-commit": "name-only" }, enabledPlugins: { "caveman@caveman": true } };
  let project = adjust(undefined, "skill", "caveman:caveman", "off");
  project = adjust(project, "plugin", "caveman@caveman", false);
  const l = { user, team: none, profile: null, project };
  assert.deepEqual(resolve("skill", "caveman:caveman-commit", l), { value: "name-only", source: "user" });
  assert.deepEqual(resolve("skill", "caveman", l), { value: "on", source: "default" });
  assert.deepEqual(localPayload(l), {
    skillOverrides: { "caveman:caveman-commit": "name-only", "caveman:caveman": "off" },
    enabledPlugins: { "caveman@caveman": false },
  });
});

test("Skill-Keys wie Object.prototype-Namen fallen nicht auf geerbte Props rein", () => {
  // Skill-Ordner heisst z.B. "constructor" oder "toString"
  const l = { user: none, team: none, profile: null };
  assert.deepEqual(resolve("skill", "constructor", l), { value: "on", source: "default" });
  assert.deepEqual(resolve("plugin", "toString", l), { value: false, source: "default" });
});
