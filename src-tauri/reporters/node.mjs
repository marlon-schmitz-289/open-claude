// Reporter fuer den Test-Explorer (node --test). Eine Zeile "@@ocui <json>" je Ereignis, alles andere ist Output.
// Wird von runner.rs ins Cache-Verzeichnis geschrieben und mit --test-reporter=<abs> geladen.
import { inspect } from "node:util";

// Output ohne Zeilenende (process.stdout.write("x")) wuerde das naechste Ereignis an seine Zeile haengen.
let eol = true;
const out = (m) => ((eol = m.endsWith("\n")), m);
const ev = (o) => `${eol ? "" : ((eol = true), "\n")}@@ocui ${JSON.stringify(o)}\n`;
const text = (v) => (v === undefined ? undefined : (typeof v === "string" ? v : inspect(v, { depth: 6 })).slice(0, 100_000));

export default async function* (source) {
  // Namen der offenen Tests je Datei und Tiefe: daraus entsteht der Pfad.
  const stacks = new Map();
  for await (const { type, data: d } of source) {
    if (type === "test:stdout" || type === "test:stderr") {
      yield out(d.message);
      continue;
    }
    if (type === "test:diagnostic") {
      yield out(`# ${d.message}\n`);
      continue;
    }
    if (type !== "test:start" && type !== "test:pass" && type !== "test:fail") continue;
    const s = stacks.get(d.file) ?? [];
    stacks.set(d.file, s);
    if (type === "test:start") {
      s.length = d.nesting;
      s.push(d.name);
      yield ev({ e: "start", file: d.file, path: [...s], line: d.line });
      continue;
    }
    // Datei laesst sich nicht laden: node meldet sie als Test mit ihrem Pfad als Namen.
    const whole =
      d.nesting === 0 &&
      d.details?.type !== "suite" &&
      !!d.file &&
      (d.name === d.file || d.file.endsWith(`/${d.name}`) || d.file.endsWith(`\\${d.name}`));
    if (d.details?.type !== "test" && !whole) continue;
    const err = d.details?.error;
    const cause = err?.cause ?? err;
    yield ev({
      e: "done",
      file: d.file,
      path: whole ? [] : [...s.slice(0, d.nesting), d.name],
      line: d.line,
      state: d.skip || d.todo ? "skipped" : type === "test:pass" ? "passed" : "failed",
      ms: d.details?.duration_ms,
      msg: cause?.message ?? (err ? String(err) : undefined),
      exp: text(cause?.expected),
      act: text(cause?.actual),
      stack: cause?.stack,
    });
  }
}
