// Reporter fuer den Test-Explorer (Jest). Eine Zeile "@@ocui <json>" je Ereignis, alles andere ist Output.
// Experimentell: nur nach der Doku gebaut, nicht gegen ein echtes Projekt geprueft.
const ev = (o) => process.stdout.write(`@@ocui ${JSON.stringify(o)}\n`);
const ANSI = /\x1b\[[0-9;]*m/g;
const grab = (re, s) => re.exec(s)?.[1];

module.exports = class {
  onTestCaseResult(test, r) {
    const fail = (r.failureMessages?.[0] ?? "").replace(ANSI, "");
    ev({
      e: "done",
      file: test.path,
      path: [...r.ancestorTitles, r.title],
      line: r.location?.line,
      state: r.status === "passed" || r.status === "failed" ? r.status : "skipped",
      ms: r.duration ?? undefined,
      msg: fail ? fail.split("\n").find((l) => l.trim()) : undefined,
      exp: fail ? grab(/^\s*Expected:\s*(.*)$/m, fail) : undefined,
      act: fail ? grab(/^\s*Received:\s*(.*)$/m, fail) : undefined,
      stack: fail || undefined,
    });
  }
  onTestFileResult(test, result) {
    if (result.testExecError)
      ev({ e: "done", file: test.path, path: [], state: "failed", msg: String(result.testExecError.message ?? "").replace(ANSI, "") });
  }
  getLastError() {}
};
