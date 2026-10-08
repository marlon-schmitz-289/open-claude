import { test } from "node:test";
import assert from "node:assert/strict";
import { cargoConfigs, configs, dotnetRunnable, npmConfigs, pickDefault, scan } from "./run.logic.ts";
import { serverUrl } from "./editor.logic.ts";

const labels = (c: { label: string }[]) => c.map((x) => x.label);

test("npmConfigs: dev, start, serve, watch zuerst, Hooks und unsichere Namen fallen weg", () => {
  const pkg = JSON.stringify({
    scripts: { build: "x", prebuild: "x", watch: "x", dev: "x", postinstall: "x", prepare: "x", preview: "x", "a b": "x", lint: 1 },
  });
  assert.deepEqual(labels(npmConfigs(pkg, "", "npm")), ["npm run dev", "npm run watch", "npm run build", "npm run preview"]);
  assert.deepEqual(npmConfigs(pkg, "apps/web", "pnpm")[0], {
    label: "pnpm run dev (apps/web)",
    dir: "apps/web",
    target: { kind: "npm", pm: "pnpm", script: "dev" },
  });
  assert.deepEqual(npmConfigs("kaputt", "", "npm"), []);
  assert.deepEqual(npmConfigs('{"scripts":null}', "", "npm"), []);
});

test("dotnetRunnable: Exe, Web-SDK ja; Testprojekt, Bibliothek nein", () => {
  assert.ok(dotnetRunnable("<Project><PropertyGroup><OutputType>Exe</OutputType></PropertyGroup></Project>"));
  assert.ok(dotnetRunnable("<OutputType>WinExe</OutputType>"));
  assert.ok(dotnetRunnable('<Project Sdk="Microsoft.NET.Sdk.Web">'));
  assert.ok(!dotnetRunnable('<Project Sdk="Microsoft.NET.Sdk"><OutputType>Exe</OutputType><PackageReference Include="Microsoft.NET.Test.Sdk" />'));
  assert.ok(!dotnetRunnable("<OutputType>Exe</OutputType><IsTestProject>true</IsTestProject>"));
  assert.ok(!dotnetRunnable('<Project Sdk="Microsoft.NET.Sdk"><TargetFramework>net8.0</TargetFramework></Project>'));
});

test("cargoConfigs: main.rs ohne --bin, src/bin als --bin, Workspace ohne package nichts", () => {
  const bins = ["src/main.rs", "src/bin/ocui-sh.rs", "src/bin/tool/main.rs", "src/lib.rs", "src/bin/tool/util.rs"];
  assert.deepEqual(labels(cargoConfigs('[package]\nname = "x"', "src-tauri", bins)), [
    "cargo run (src-tauri)",
    "cargo run --bin ocui-sh (src-tauri)",
    "cargo run --bin tool (src-tauri)",
  ]);
  assert.deepEqual(cargoConfigs("[workspace]\nmembers = []", "", bins), []);
});

test("configs: Paketmanager nach Lockfile aufwaerts, dotnet nur bei einer Projektdatei im Ordner", () => {
  const files = ["pnpm-lock.yaml", "apps/web/package.json", "a/A.csproj", "b/B.csproj", "b/C.csproj", "rs/Cargo.toml", "rs/src/main.rs"];
  const texts = {
    "apps/web/package.json": '{"scripts":{"dev":"vite"}}',
    "a/A.csproj": "<OutputType>Exe</OutputType>",
    "b/B.csproj": "<OutputType>Exe</OutputType>",
    "b/C.csproj": "<OutputType>Exe</OutputType>",
    "rs/Cargo.toml": "[package]",
  };
  assert.deepEqual(labels(configs(files, texts)), ["pnpm run dev (apps/web)", "dotnet run (a)", "dotnet watch (a)", "cargo run (rs)"]);
});

test("pickDefault: zuletzt gewaehlt, Naehe zur Datei, Fallbacks", () => {
  const root = npmConfigs('{"scripts":{"dev":"x","build":"x"}}', "", "npm");
  const web = npmConfigs('{"scripts":{"start":"x"}}', "apps/web", "npm");
  const dn = { label: "dotnet run (a)", dir: "a", target: { kind: "dotnet", watch: false } } as const;
  const rs = { label: "cargo run", dir: "", target: { kind: "cargo", bin: null } } as const;
  const all = [...root, ...web, dn, rs];
  assert.equal(pickDefault(all, "npm run build", null)?.label, "npm run build");
  assert.equal(pickDefault(all, "weg", "apps/web/src/a.ts")?.label, "npm run start (apps/web)");
  assert.equal(pickDefault(all, null, "lib/x.ts")?.label, "npm run dev");
  assert.equal(pickDefault([...web, rs, dn], null, "lib/x.ts")?.label, "dotnet run (a)");
  assert.equal(pickDefault([rs], null, null)?.label, "cargo run");
  assert.equal(pickDefault(npmConfigs('{"scripts":{"build":"x"}}', "", "npm"), null, null)?.label, "npm run build");
  assert.equal(pickDefault([], null, null), null);
});

test("scan: URL und Escape-Sequenz ueber Chunk-Grenzen", () => {
  let tail = "";
  for (const c of ["  \x1b[32m➜\x1b[39m  Local:   http://local", "host:5173/\x1b", "[0m\r", "\n"]) tail = scan(tail, c);
  assert.equal(serverUrl(tail, true), "http://localhost:5173/");
  // Begrenzt auf 4 KB.
  assert.equal(scan("x".repeat(5000), "y").length, 4096);
});
