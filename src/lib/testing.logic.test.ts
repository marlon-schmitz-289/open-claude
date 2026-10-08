import { test } from "node:test";
import assert from "node:assert/strict";
import {
  aggregate,
  discover,
  inScope,
  jsFw,
  marks,
  modPath,
  placer,
  projects,
  replaceFiles,
  rows,
  stackLinks,
  type Result,
  type TestNode,
  type Tree,
} from "./testing.logic.ts";

const tree = (...lists: TestNode[][]): Tree => Object.fromEntries(lists.flat().map((n) => [n.id, n]));
const brief = (ns: TestNode[]) => ns.map((n) => `${n.kind} ${n.key} @${n.line} <${n.parent?.split("|")[2] ?? "-"}`);

test("JS: Verschachtelung ueber Einrueckung, .each und Modifikatoren", () => {
  const src = [
    'import { describe, it } from "vitest";',
    'describe("parse", () => {',
    '  it("leer", () => {});',
    '  describe.skip("tief", () => {',
    "    it.each([1, 2])('n %i', () => {});",
    "  });",
    '  test.only(`spaet`, () => {});',
    "});",
    'it("oben", () => {});',
  ].join("\n");
  assert.deepEqual(brief(discover("vitest", "a.test.ts", src)), [
    "file  @null <-",
    "suite parse @2 <",
    "test parse > leer @3 <parse",
    "suite parse > tief @4 <parse",
    "test parse > tief > n %i @5 <parse > tief",
    "test parse > spaet @7 <parse",
    "test oben @9 <",
  ]);
});

test("Rust: mod tests, Modulpfad aus dem Dateipfad", () => {
  assert.equal(modPath("src/lib.rs"), "");
  assert.equal(modPath("src/main.rs"), "");
  assert.equal(modPath("src/a/mod.rs"), "a");
  assert.equal(modPath("src/a/b.rs"), "a::b");
  assert.equal(modPath("tests/x.rs"), "");
  assert.equal(modPath("src/bin/x.rs"), "");
  const src = [
    "fn helper() {}",
    "#[cfg(test)]",
    "mod tests {",
    "    use super::*;",
    "    #[test]",
    "    fn eins() {",
    '        let s = r#"',
    "x = 1",
    '"#;',
    "    }",
    "    #[tokio::test]",
    "    #[ignore]",
    "    async fn zwei() {}",
    "}",
  ].join("\n");
  const ns = discover("cargo", "crate/src/pty.rs", src, "crate");
  assert.deepEqual(brief(ns).slice(1), ["suite pty::tests @3 <", "test pty::tests::eins @6 <pty::tests", "test pty::tests::zwei @13 <pty::tests"]);
  // Ohne Modul direkt in tests/: nur der Funktionsname.
  assert.deepEqual(brief(discover("cargo", "tests/api.rs", "#[test]\nfn geht() {}")).slice(1), ["test geht @2 <"]);
});

test("C#: file-scoped Namespace, Klasse als Suite, Theory", () => {
  const src = [
    "namespace T.Unit;",
    "public class Calc",
    "{",
    "    [Fact]",
    "    public void Bad() { }",
    "    [Theory]",
    "    [InlineData(1)]",
    "    public async Task Th(int x) { }",
    "    public void Helper() { }",
    "}",
  ].join("\n");
  assert.deepEqual(brief(discover("dotnet", "t/Calc.cs", src)).slice(1), [
    "suite T.Unit.Calc @2 <",
    "test T.Unit.Calc.Bad @5 <T.Unit.Calc",
    "test T.Unit.Calc.Th @8 <T.Unit.Calc",
  ]);
});

test("Python: Klassen und Funktionen, nodeid relativ zum Projekt", () => {
  const src = ["class TestX:", "    def test_a(self): pass", "", "def helper(): pass", "def test_b(): pass"].join("\n");
  assert.deepEqual(brief(discover("pytest", "py/tests/test_m.py", src, "py")).slice(1), [
    "suite tests/test_m.py::TestX @1 <",
    "test tests/test_m.py::TestX::test_a @2 <tests/test_m.py::TestX",
    "test tests/test_m.py::test_b @5 <",
  ]);
});

test("Erkennung: Framework je Import, dotnet-Testprojekt, Cargo-Wurzel, pytest", () => {
  const files = [
    "package.json",
    "src/a.test.ts",
    "web/package.json",
    "web/x.spec.js",
    "node_modules/z/a.test.js",
    "src-tauri/Cargo.toml",
    "src-tauri/src/lib.rs",
    "src-tauri/sub/Cargo.toml",
    "src-tauri/sub/src/lib.rs",
    "app/App.csproj",
    "app/Program.cs",
    "tests/Api.Tests/Api.Tests.csproj",
    "tests/Api.Tests/T.cs",
    "py/pytest.ini",
    "py/test_a.py",
    "py/util.py",
  ];
  const texts = {
    "package.json": '{"devDependencies":{"svelte":"5"}}',
    "web/package.json": '{"devDependencies":{"jest":"29","vitest":"5"}}',
    "app/App.csproj": '<Project Sdk="Microsoft.NET.Sdk"/>',
    "tests/Api.Tests/Api.Tests.csproj": '<PackageReference Include="xunit" />',
  };
  const ps = projects(files, texts);
  assert.deepEqual(
    ps.map((p) => `${p.fw} ${p.dir} ${p.files.join(",")}`),
    [
      "vitest web web/x.spec.js",
      "node  src/a.test.ts",
      "cargo src-tauri src-tauri/src/lib.rs,src-tauri/sub/src/lib.rs",
      "dotnet tests/Api.Tests tests/Api.Tests/T.cs",
      "pytest py py/test_a.py",
    ],
  );
  assert.equal(jsFw('import { test } from "node:test";', "vitest"), "node");
  assert.equal(jsFw('import { it } from "vitest"', "node"), "vitest");
  assert.equal(jsFw("it('x', () => {})", "jest"), "jest");
  assert.equal(jsFw("export const x = 1", "node"), null);
  const mtp = projects(["global.json", "t/T.csproj", "t/A.cs"], { "global.json": '{"test":{"runner":"Microsoft.Testing.Platform"}}', "t/T.csproj": "MSTest" });
  assert.equal(mtp[0].note, "MTP noch nicht unterstützt");
});

const js = () =>
  tree(
    discover(
      "vitest",
      "a.test.ts",
      ['describe("s", () => {', '  it("ok", () => {});', '  it.each([1])("n %i", () => {});', "});"].join("\n"),
    ),
  );

test("Zuordnung: exakt, Parameter-Variante, letztes Segment, neuer Knoten mit Vorlagen-Ersatz", () => {
  const nodes = js();
  const place = placer(nodes, {});
  assert.equal(place("vitest", "", "a.test.ts", "s > ok"), "vitest|a.test.ts|s > ok");
  // it.each: neue Knoten, die Vorlage auf derselben Zeile faellt weg.
  const n1 = place("vitest", "", "a.test.ts", "s > n 1", 3);
  assert.equal(nodes[n1].parent, "vitest|a.test.ts|s");
  assert.equal(nodes["vitest|a.test.ts|s > n %i"], undefined);
  assert.ok(!nodes["vitest|a.test.ts|s"].children.includes("vitest|a.test.ts|s > n %i"));
  // Unbekannte Datei: neuer Datei-Knoten.
  // Wie im echten Lauf: vorher alle als running markiert, zwei Faelle nacheinander (ueber zwei Batches).
  const loop = js();
  const running = { "vitest|a.test.ts|s > n %i": { state: "running" as const } };
  const seen = new Set<string>();
  const c1 = placer(loop, running, seen)("vitest", "", "a.test.ts", "s > n 1", 3);
  const c2 = placer(loop, { ...running, [c1]: { state: "running" as const } }, seen)("vitest", "", "a.test.ts", "s > n 2", 3);
  assert.equal(loop["vitest|a.test.ts|s > n %i"], undefined);
  assert.ok(loop[c1] && loop[c2]);
  const other = place("vitest", "", "b.test.ts", "x");
  assert.equal(nodes[other].parent, "vitest|b.test.ts|");

  // cargo: Crate-Wurzel als Datei, Test in pty.rs ueber den Key; #[path]-Modul ueber das letzte Segment.
  const rs = tree(
    discover("cargo", "c/src/pty.rs", "mod tests {\n    #[test]\n    fn a() {}\n}", "c"),
    discover("cargo", "c/src/skills_scan_tests.rs", "#[test]\nfn findet() {}", "c"),
  );
  const pr = placer(rs, {});
  assert.equal(pr("cargo", "c", "c/src/lib.rs", "pty::tests::a"), "cargo|c/src/pty.rs|pty::tests::a");
  const id = pr("cargo", "c", "c/src/lib.rs", "skills::scan_tests::findet");
  assert.equal(id, "cargo|c/src/skills_scan_tests.rs|skills_scan_tests::findet");
  assert.equal(rs[id].key, "skills::scan_tests::findet");
  // Neu einlesen (Speichern, "Neu suchen") behaelt den gelernten Key.
  const again = replaceFiles(rs, ["c/src/skills_scan_tests.rs"], discover("cargo", "c/src/skills_scan_tests.rs", "#[test]\nfn findet() {}", "c"));
  assert.equal(again[id].key, "skills::scan_tests::findet");
  // Ganz unbekannt: Pseudo-Datei "(cargo)".
  const lost = pr("cargo", "c", "", "weg::x");
  assert.equal(rs[lost].parent, "cargo|c/(cargo)|");

  // dotnet-Theory: Kinder unter dem Basistest.
  const cs = tree(discover("dotnet", "T.cs", "namespace T;\nclass Calc {\n  [Theory]\n  void Th(int x) {}\n}"));
  const pc = placer(cs, {});
  const th = pc("dotnet", "", null, "T.Calc.Th(x: 1)");
  assert.equal(cs[th].parent, "dotnet|T.cs|T.Calc.Th");
  assert.equal(cs[th].name, "Th(x: 1)");
});

test("Scope-Regel: Ergebnisse ausserhalb des Lauf-Knotens zaehlen nicht", () => {
  const nodes = js();
  assert.ok(inScope(nodes, "vitest|a.test.ts|s > ok", "vitest|a.test.ts|s"));
  assert.ok(inScope(nodes, "vitest|a.test.ts|s > ok", null));
  // vitest -t meldet Nachbarn als skipped: nicht unter dem gestarteten Test.
  assert.ok(!inScope(nodes, "vitest|a.test.ts|s > n %i", "vitest|a.test.ts|s > ok"));
});

test("Aggregation und Zeilen mit Filter", () => {
  const nodes = js();
  const results: Record<string, Result> = { "vitest|a.test.ts|s > ok": { state: "passed" } };
  let st = aggregate(nodes, results);
  assert.equal(st["vitest|a.test.ts|"], "passed");
  results["vitest|a.test.ts|s > n %i"] = { state: "failed" };
  st = aggregate(nodes, results);
  assert.equal(st["vitest|a.test.ts|s"], "failed");
  results["vitest|a.test.ts|s > ok"] = { state: "running" };
  assert.equal(aggregate(nodes, results)["vitest|a.test.ts|"], "failed");

  assert.deepEqual(rows(nodes, st, [], "", false).map((r) => r.id), ["vitest|a.test.ts|"]);
  const open = ["vitest|a.test.ts|", "vitest|a.test.ts|s"];
  assert.equal(rows(nodes, st, open, "", false).length, 4);
  // Filter klappt auf und zeigt Treffer samt Vorfahren.
  assert.deepEqual(
    rows(nodes, st, [], "ok", false).map((r) => `${r.depth}${r.id.split("|")[2]}`),
    ["0", "1s", "2s > ok"],
  );
  assert.deepEqual(rows(nodes, st, [], "", true).map((r) => r.id.split("|")[2]), ["", "s", "s > n %i"]);
});

test("Neu scannen ersetzt nur die Datei, Marken je Datei", () => {
  const nodes = js();
  const b = discover("node", "b.test.ts", 'test("x", () => {})');
  const both = replaceFiles(nodes, ["b.test.ts"], b);
  assert.equal(Object.keys(both).length, 6);
  const again = replaceFiles(both, ["a.test.ts"], discover("vitest", "a.test.ts", 'it("neu", () => {})'));
  assert.deepEqual(Object.keys(again).sort(), ["node|b.test.ts|", "node|b.test.ts|x", "vitest|a.test.ts|", "vitest|a.test.ts|neu"]);
  assert.deepEqual(marks(nodes, {}, "a.test.ts").map((m) => m.line), [1, 2, 3]);
});

test("Stack-Links: Pfade mit Zeile, file:// entfernt", () => {
  const parts = stackLinks("at f (file:///r/src/a.test.ts:5:26)\n at /abs/T.cs:line 4");
  assert.deepEqual(parts.filter((p) => p.loc).map((p) => p.loc), [
    { file: "/r/src/a.test.ts", line: 5 },
    { file: "/abs/T.cs", line: 4 },
  ]);
  assert.equal(parts.map((p) => p.text).join(""), "at f (file:///r/src/a.test.ts:5:26)\n at /abs/T.cs:line 4");
});
