// Reporter fuer den Test-Explorer (Vitest 5). Eine Zeile "@@ocui <json>" je Ereignis, alles andere ist Output.
// Wird von runner.rs ins Cache-Verzeichnis geschrieben und mit --reporter=<abs> geladen.
const ev = (o) => process.stdout.write(`@@ocui ${JSON.stringify(o)}\n`);
const text = (v) => (v == null ? undefined : (typeof v === "string" ? v : JSON.stringify(v, null, 2)).slice(0, 100_000));

export default class {
  onTestCaseReady(t) {
    ev({ e: "start", file: t.module.moduleId, path: t.fullName.split(" > "), line: t.location?.line });
  }
  onTestCaseResult(t) {
    const r = t.result();
    const err = r.errors?.[0];
    ev({
      e: "done",
      file: t.module.moduleId,
      path: t.fullName.split(" > "),
      line: t.location?.line,
      state: r.state === "passed" || r.state === "failed" ? r.state : "skipped",
      ms: t.diagnostic()?.duration,
      msg: err?.message,
      exp: text(err?.expected),
      act: text(err?.actual),
      diff: text(err?.diff),
      stack: err?.stack,
    });
  }
  onUserConsoleLog(l) {
    process.stdout.write(l.content.endsWith("\n") ? l.content : `${l.content}\n`);
  }
  // Datei laesst sich nicht laden (Syntaxfehler, Import fehlt): ohne Tests ein Fehler auf der Datei.
  onTestModuleEnd(m) {
    const err = m.errors()[0];
    if (err && !m.children.size) ev({ e: "done", file: m.moduleId, path: [], state: "failed", msg: err.message, stack: err.stack });
  }
}
