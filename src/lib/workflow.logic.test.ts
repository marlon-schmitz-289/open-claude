import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  fromScript,
  metaOf,
  newTemplate,
  problems,
  toScript,
  type WfTemplate,
} from "./workflow.logic.ts";

const evil = 'q"`${x}*/ \\';
const T: WfTemplate = {
  name: "review-x",
  description: `Prüft ${evil}`,
  whenToUse: "",
  phases: [
    {
      title: "Finden",
      mode: "parallel",
      agents: [
        {
          label: "A",
          prompt: `Args: {{args}} ${evil}`,
          model: "haiku",
          effort: "",
          schema: "",
        },
        {
          label: "",
          prompt: "B {{prev}}",
          model: "",
          effort: "low",
          schema:
            '{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]}',
        },
      ],
    },
    {
      title: "Bericht",
      mode: "sequence",
      agents: [
        {
          label: "R1",
          prompt: "Fasse zusammen: {{prev}}",
          model: "",
          effort: "",
          schema: "",
        },
        {
          label: "R2",
          prompt: "Kürzer: {{prev}}",
          model: "opus",
          effort: "max",
          schema: "",
        },
      ],
    },
  ],
};

const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
async function run(
  script: string,
  args: unknown,
  answer: (p: string, i: number) => unknown,
) {
  const calls: { prompt: string; opts: Record<string, unknown> }[] = [];
  const phases: string[] = [];
  const agent = async (prompt: string, opts: Record<string, unknown>) => (
    calls.push({ prompt, opts }),
    answer(prompt, calls.length - 1)
  );
  const parallel = async (fs: (() => Promise<unknown>)[]) =>
    Promise.all(fs.map((f) => f()));
  const body = script.replace("export const meta", "const meta");
  const ret = await new AsyncFunction(
    "agent",
    "parallel",
    "phase",
    "args",
    body,
  )(agent, parallel, (t: string) => phases.push(t), args);
  return { calls, phases, ret };
}

test("toScript: Ablauf, Platzhalter, Optionen und Escaping zur Laufzeit", async () => {
  const { calls, phases, ret } = await run(toScript(T), ["a", 1], (p, i) =>
    i === 0 ? null : i === 1 ? { n: 1 } : `out${i}`,
  );
  assert.deepEqual(phases, ["Finden", "Bericht"]);
  assert.equal(calls.length, 4);
  assert.equal(
    calls[0].prompt,
    `Args: ${JSON.stringify(["a", 1], null, 2)} ${evil}`,
  );
  assert.deepEqual(calls[0].opts, {
    label: "A",
    phase: "Finden",
    model: "haiku",
  });
  // vor der ersten Phase gibt es kein prev
  assert.equal(calls[1].prompt, "B ");
  assert.deepEqual(calls[1].opts, {
    phase: "Finden",
    effort: "low",
    schema: {
      type: "object",
      properties: { n: { type: "number" } },
      required: ["n"],
    },
  });
  // parallele Phase liefert Array ohne null
  assert.equal(
    calls[2].prompt,
    `Fasse zusammen: ${JSON.stringify([{ n: 1 }], null, 2)}`,
  );
  assert.equal(calls[3].prompt, "Kürzer: out2");
  assert.deepEqual(calls[3].opts, {
    label: "R2",
    phase: "Bericht",
    model: "opus",
    effort: "max",
  });
  assert.equal(ret, "out3");
  // String-Args roh, null-Ergebnis im Nacheinander-Modus behaelt prev
  const r2 = await run(toScript(T), "roh {{prev}} $&", (_, i) =>
    i === 3 ? null : `o${i}`,
  );
  assert.ok(r2.calls[0].prompt.startsWith("Args: roh {{prev}} $& "));
  assert.equal(r2.ret, "o2");
});

test("toScript: meta im Format, das phases_of erwartet", () => {
  const s = toScript(T);
  const meta = s.slice(0, s.indexOf("\n}\n") + 2).split("\n");
  assert.equal(meta[0], "export const meta = {");
  assert.equal(meta.at(-1), "}");
  const titles = meta
    .filter((l) => l.includes("title:"))
    .map((l) => l.match(/^\s*\{ title: "(.*)" \},$/)?.[1]);
  assert.deepEqual(titles, ["Finden", "Bericht"]);
  assert.ok(!s.includes("Date") && !s.includes("Math.random"));
});

test("fromScript/metaOf: Roundtrip, freie Skripte", () => {
  assert.deepEqual(fromScript(toScript(T)), T);
  const free =
    "// x\nexport const meta = {\n  name: 'nightly-audit',\n  description: \"Nachts \\\"alles\\\"\",\n}\nawait agent('x')\n";
  assert.equal(fromScript(free), null);
  assert.deepEqual(metaOf(free), {
    name: "nightly-audit",
    description: 'Nachts "alles"',
  });
  assert.deepEqual(metaOf("nix"), { name: undefined, description: undefined });
  assert.equal(fromScript("// open-claude-builder: {kaputt"), null);
});

test("problems: jede Regel", () => {
  assert.deepEqual(problems(T, []), []);
  const p = (f: (t: WfTemplate) => void, taken: string[] = []) => {
    const t = structuredClone(T);
    f(t);
    return problems(t, taken);
  };
  assert.equal(p((t) => (t.name = "Bad Name")).length, 1);
  assert.equal(p((t) => (t.name = "-x")).length, 1);
  assert.match(p(() => {}, ["review-x"])[0], /vergeben/);
  assert.match(p((t) => (t.description = " "))[0], /Beschreibung/);
  assert.match(p((t) => (t.phases = []))[0], /Phase/);
  assert.match(p((t) => (t.phases[0].title = ""))[0], /Titel fehlt/);
  assert.match(p((t) => (t.phases[0].title = 'a"b'))[0], /Titel ohne/);
  assert.match(p((t) => (t.phases[1].title = "a\\b"))[0], /Titel ohne/);
  assert.match(p((t) => (t.phases[1].title = "Finden"))[0], /doppelt/);
  assert.match(p((t) => (t.phases[1].agents = []))[0], /mindestens ein Agent/);
  assert.match(
    p((t) => (t.phases[1].agents[0].prompt = " "))[0],
    /Prompt fehlt/,
  );
  assert.match(p((t) => (t.phases[0].agents[0].schema = "{"))[0], /JSON/);
  assert.match(p((t) => (t.phases[0].agents[0].schema = "[]"))[0], /type/);
  assert.match(
    p((t) => (t.phases[0].agents[0].schema = '{"type":"object"}'))[0],
    /properties/,
  );
  assert.match(
    p(
      (t) =>
        (t.phases[0].agents[0].schema =
          '{"type":"object","properties":{},"required":["x"]}'),
    )[0],
    /required/,
  );
  assert.ok(problems(newTemplate(), []).length > 0);
});

test("Skill-Beispiel ist exakt Generator-Ausgabe", () => {
  const md = readFileSync(
    new URL(
      "../../src-tauri/mods/open-claude/skills/workflow-templates/SKILL.md",
      import.meta.url,
    ),
    "utf8",
    // Windows-Checkout (autocrlf) liefert CRLF
  ).replace(/\r\n/g, "\n");
  const js = md.match(/```js\n([\s\S]*?)```/)![1];
  const t = fromScript(js);
  assert.ok(t);
  assert.deepEqual(problems(t, []), []);
  assert.equal(toScript(t), js);
});
