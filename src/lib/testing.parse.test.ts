import { test } from "node:test";
import assert from "node:assert/strict";
import { flush, jsEvent, keyOf, parseLines, parser, type Ev } from "./testing.logic.ts";

// Echte Ausgaben (cargo 1.98, .NET 10 + xUnit), nur am Ende gekuerzt.
const CARGO = `     Running unittests src/lib.rs (target/debug/deps/ct-8af2605250e3036c)

running 2 tests
test tests::ok_one ... ok
test tests::bad ... FAILED

failures:

---- tests::bad stdout ----

thread 'tests::bad' (8780414) panicked at src/lib.rs:1:66:
assertion \`left == right\` failed
  left: 1
 right: 2
note: run with \`RUST_BACKTRACE=1\` environment variable to display a backtrace


failures:
    tests::bad

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`;

const DOTNET = `[xUnit.net 00:00:00.06]     T.Calc.Bad [FAIL]
  Passed T.Calc.Th(x: 1) [< 1 ms]
  Passed T.Calc.Th(x: 2) [< 1 ms]
  Failed T.Calc.Bad [< 1 ms]
  Error Message:
   Assert.Equal() Failure: Strings differ
             ↓ (pos 2)
Expected: "abc"
Actual:   "abd"
             ↑ (pos 2)
  Stack Trace:
     at T.Calc.Bad() in /abs/dt/UnitTest1.cs:line 4
   at System.Reflection.MethodBaseInvoker.InterpretedInvoke_Method(Object obj, IntPtr* args)
  Passed T.Calc.OkOne [1 ms]

Test Run Failed.
Total tests: 4`;

const short = (evs: Ev[]) => evs.map((e) => `${e.state} ${e.path.join("|")}`);

test("cargo: Ergebnisse, Fehlerblock mit Ort, left/right", () => {
  const st = parser("cargo");
  const evs = [...parseLines(st, CARGO.split("\n")), ...flush(st)];
  assert.deepEqual(short(evs), ["passed tests::ok_one", "failed tests::bad", "failed tests::bad"]);
  const bad = evs[2];
  assert.equal(bad.file, "src/lib.rs");
  assert.deepEqual(bad.at, { file: "src/lib.rs", line: 1 });
  assert.equal(bad.msg, "assertion `left == right` failed");
  assert.equal(bad.act, "1");
  assert.equal(bad.exp, "2");
  assert.deepEqual(bad.labels, ["right", "left"]);
  assert.equal(bad.ms, undefined);
});

test("cargo: Fehlerblock ueber zwei Batches, ignored, Doc-tests", () => {
  const lines = CARGO.split("\n");
  const cut = lines.indexOf("  left: 1");
  const st = parser("cargo");
  const first = parseLines(st, lines.slice(0, cut));
  assert.equal(first.length, 2);
  const rest = parseLines(st, lines.slice(cut));
  assert.equal(rest.length, 1);
  assert.equal(rest[0].act, "1");
  const more = parseLines(parser("cargo"), [
    "test a::b ... ignored, braucht Netz",
    "   Doc-tests ct",
    "test src/lib.rs - f (line 3) ... ok",
  ]);
  assert.deepEqual(short(more), ["skipped a::b"]);
});

test("dotnet: Theories, Dauer < 1 ms, Fehler mit Expected/Actual und Ort", () => {
  const st = parser("dotnet");
  const evs = [...parseLines(st, DOTNET.split("\n")), ...flush(st)];
  assert.deepEqual(short(evs), ["passed T.Calc.Th(x: 1)", "passed T.Calc.Th(x: 2)", "failed T.Calc.Bad", "passed T.Calc.OkOne"]);
  const bad = evs[2];
  assert.equal(bad.ms, 1);
  assert.equal(bad.exp, '"abc"');
  assert.equal(bad.act, '"abd"');
  assert.match(bad.msg!, /^Assert\.Equal\(\) Failure: Strings differ/);
  assert.deepEqual(bad.at, { file: "/abs/dt/UnitTest1.cs", line: 4 });
  assert.match(bad.stack!, /InterpretedInvoke_Method/);
  assert.ok(!bad.stack!.includes("OkOne"));
  assert.equal(evs[3].ms, 1);
});

test("dotnet: Block ueber Batches, NUnit/MSTest-Varianten, Sekunden", () => {
  const lines = DOTNET.split("\n");
  const st = parser("dotnet");
  const a = parseLines(st, lines.slice(0, 7));
  const b = parseLines(st, lines.slice(7));
  assert.deepEqual(short([...a, ...b]).length, 4);
  const nunit = [...parseLines(parser("dotnet"), ["  Failed N.T.X [2 s]", "  Error Message:", "   Expected: 2", "  But was:  1", "Total tests: 1"])];
  assert.equal(nunit[0].ms, 2000);
  assert.equal(nunit[0].exp, "2");
  assert.equal(nunit[0].act, "1");
  const ms = parseLines(parser("dotnet"), ["  Failed M.T.X [3 ms]", "  Error Message:", "   Assert.AreEqual failed. Expected:<2>. Actual:<1>. ", "Test Run Failed."]);
  assert.equal(ms[0].exp, "2");
  assert.equal(ms[0].act, "1");
});

// Vitest 5.0.3 und Node 24: Reporter-Zeilen wie von src-tauri/reporters erzeugt.
test("Reporter-Zeilen: vitest und node", () => {
  const v = jsEvent(
    '@@ocui {"e":"done","file":"/r/a.test.ts","path":["suite","bad"],"line":4,"state":"failed","ms":1.8,"msg":"x","exp":"{\\n  \\"a\\": 2,\\n}","act":"{\\n  \\"a\\": 1,\\n}"}',
  );
  assert.ok(v);
  assert.equal(keyOf("vitest", v.path), "suite > bad");
  assert.equal(v.exp, '{\n  "a": 2,\n}');
  const n = jsEvent('@@ocui {"e":"done","file":"/r/b.test.ts","path":["s","bad"],"line":2,"state":"failed","ms":0.59,"exp":"{ a: 2 }","act":"{ a: 1 }"}');
  assert.equal(n?.act, "{ a: 1 }");
  assert.equal(jsEvent("hallo"), null);
  assert.equal(jsEvent("@@ocui {kaputt"), null);
  assert.equal(jsEvent('@@ocui {"e":"done","file":1,"path":[]}'), null);
  assert.equal(jsEvent('@@ocui {"e":"done","file":"f","path":[],"state":"weird"}'), null);
});

// pytest: synthetisch nach Doku, lokal NICHT gegen pytest geprueft.
test("pytest (ungeprueft): -v-Zeilen und Kurzfassung", () => {
  const evs = parseLines(parser("pytest"), [
    "tests/test_a.py::test_ok PASSED                                     [ 33%]",
    "tests/test_a.py::TestX::test_p[1-2] FAILED                          [ 66%]",
    "tests/test_a.py::test_s SKIPPED (kein Netz)                         [100%]",
    "FAILED tests/test_a.py::TestX::test_p[1-2] - assert 1 == 2",
    "ERROR tests/test_b.py - ImportError: x",
  ]);
  assert.deepEqual(short(evs), [
    "passed tests/test_a.py::test_ok",
    "failed tests/test_a.py::TestX::test_p[1-2]",
    "skipped tests/test_a.py::test_s",
    "failed tests/test_a.py::TestX::test_p[1-2]",
    "failed ",
  ]);
  assert.equal(evs[3].msg, "assert 1 == 2");
  assert.equal(evs[4].file, "tests/test_b.py");
});
