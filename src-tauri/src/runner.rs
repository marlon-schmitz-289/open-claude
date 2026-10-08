//! Test-Explorer: Testlaeufe als normale Prozesse (kein PTY). Output (stdout + stderr in einer Pipe, damit die
//! Reihenfolge stimmt) geht zeilenweise und gebuendelt ueber einen Channel ans Frontend, das ihn auswertet.
//! Kein Shell-String: Programm und Argumente gehen einzeln an den Prozess, Werte aus dem Projekt nur als Argumente.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

/// Reporter fuer die JS-Tools; Protokoll: eine Zeile "@@ocui <json>" je Ereignis (siehe src/lib/testing.logic.ts).
const REPORTERS: [(&str, &str); 3] = [
    ("vitest.mjs", include_str!("../reporters/vitest.mjs")),
    ("node.mjs", include_str!("../reporters/node.mjs")),
    ("jest.cjs", include_str!("../reporters/jest.cjs")),
];

/// Fuer alle Laeufe: keine Farben, kein Watch-Modus und keine Rueckfragen (CI), dotnet englisch und ohne
/// Build-Knoten, die nach dem Lauf weiterleben und die Pipe offen hielten.
const ENV: [(&str, &str); 6] = [
    ("NO_COLOR", "1"),
    ("FORCE_COLOR", "0"),
    ("CI", "1"),
    ("DOTNET_CLI_UI_LANGUAGE", "en"),
    ("DOTNET_NOLOGO", "1"),
    ("MSBUILDDISABLENODEREUSE", "1"),
];

/// Buendelung: hoechstens alle 30 ms bzw. je 500 Zeilen eine Nachricht.
const TICK: Duration = Duration::from_millis(30);
const BATCH: usize = 500;
/// node: ab so vielen Dateien ohne Dateiargumente starten (Windows-Kommandozeile max. 32k Zeichen).
const MAX_FILES: usize = 300;

/// Laufende Testprozesse: run-id -> pid (Unix zugleich die Prozessgruppe).
#[derive(Default)]
pub struct Runs(Mutex<HashMap<String, u32>>);

impl Runs {
    // Threads duerfen nicht paniken (panic = "abort"), also Poisoning schlucken.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, u32>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Beim App-Ende: sonst laufen Testprozesse weiter.
    pub(crate) fn kill_all(&self) {
        for (_, pid) in self.lock().drain() {
            crate::kill_tree(pid);
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Fw {
    Vitest,
    Jest,
    Node,
    Cargo,
    Dotnet,
    Pytest,
}

/// Was laufen soll. Dateien relativ zum Projektordner (dir), mit "/".
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Scope {
    /// files: nur node (erkannte Testdateien, wie in CI).
    All { files: Vec<String> },
    /// keys: Test-Schluessel der Datei; nur cargo (exakte Liste) und dotnet (Filter) brauchen sie.
    File { file: String, keys: Vec<String> },
    /// key: JS "a > b", cargo "mod::fn", dotnet "Ns.Class.Method", pytest "datei::Class::test".
    Test { file: String, key: String, suite: bool },
}

#[derive(Serialize, Clone)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum Msg {
    Lines { lines: Vec<String> },
    Exit { code: Option<i32> },
}

/// Regex-Sonderzeichen escapen: Namen landen in -t / --test-name-pattern.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Datei relativ zum Projekt, nicht nach draussen und nicht als Option lesbar.
fn rel(file: &str) -> Result<&str, String> {
    if file.starts_with('-') || crate::git::inside(".", file).is_err() {
        return Err(format!("Ungültiger Pfad: {file}"));
    }
    Ok(file)
}

/// cargo/dotnet-Namen: nur Bezeichner-Zeichen, damit kein Filter- oder Optionstext entsteht.
fn name(key: &str) -> Result<&str, String> {
    let ok = !key.is_empty() && !key.starts_with('-') && key.chars().all(|c| c.is_alphanumeric() || "_:.".contains(c));
    ok.then_some(key).ok_or_else(|| format!("Ungültiger Testname: {key}"))
}

/// cargo-Ziel einer Datei: Integrationstest bzw. eigenes Binary, sonst alle.
fn cargo_target(file: &str) -> Vec<String> {
    let stem = |f: &str| f.strip_suffix(".rs").filter(|s| !s.contains('/')).map(String::from);
    if let Some(t) = file.strip_prefix("tests/").and_then(stem) {
        return vec!["--test".into(), t];
    }
    if let Some(b) = file.strip_prefix("src/bin/").and_then(stem) {
        return vec!["--bin".into(), b];
    }
    Vec::new()
}

/// Programm und Argumente fuer einen Lauf. prog = aufgeloestes Programm (JS: lokales Binary aus node_modules/.bin),
/// reporters = Ordner mit den Reporter-Dateien. Pure, kein Shell-Text.
fn test_command(fw: Fw, scope: &Scope, prog: &str, reporters: &Path, windows: bool) -> Result<(String, Vec<String>), String> {
    let rep = |f: &str| reporters.join(f).to_string_lossy().into_owned();
    let mut a: Vec<String> = Vec::new();
    match fw {
        Fw::Vitest => {
            a.extend(["run".into(), format!("--reporter={}", rep("vitest.mjs")), "--includeTaskLocation".into()]);
            match scope {
                Scope::All { .. } => {}
                Scope::File { file, .. } => a.push(rel(file)?.into()),
                Scope::Test { file, key, suite } => {
                    let end = if *suite { "( > |$)" } else { "$" };
                    a.extend([rel(file)?.into(), "-t".into(), format!("^{}{end}", esc(key))]);
                }
            }
        }
        Fw::Jest => {
            a.extend([
                format!("--reporters={}", rep("jest.cjs")),
                "--testLocationInResults".into(),
                "--watchAll=false".into(),
                "--colors=false".into(),
            ]);
            match scope {
                Scope::All { .. } => {}
                Scope::File { file, .. } => a.extend(["--runTestsByPath".into(), rel(file)?.into()]),
                Scope::Test { file, key, .. } => a.extend([
                    "--runTestsByPath".into(),
                    rel(file)?.into(),
                    "-t".into(),
                    format!("^{}( |$)", esc(&key.replace(" > ", " "))),
                ]),
            }
        }
        Fw::Node => {
            // Windows: ESM-Specifier statt Pfad, sonst liest node "C:" als Schema.
            let r = rep("node.mjs");
            let r = if windows { format!("file:///{}", r.replace('\\', "/")) } else { r };
            a.extend(["--test".into(), format!("--test-reporter={r}")]);
            match scope {
                // Wie in CI: die erkannten Dateien ausdruecklich, node findet .ts nicht von selbst.
                Scope::All { files } if files.len() <= MAX_FILES => {
                    for f in files {
                        a.push(rel(f)?.into());
                    }
                }
                // ponytail: darueber node-Standard-Globs, die .ts-Dateien ausserhalb von test/ uebersehen koennen.
                Scope::All { .. } => {}
                Scope::File { file, .. } => a.push(rel(file)?.into()),
                Scope::Test { file, key, .. } => {
                    a.push(format!("--test-name-pattern=^{}( |$)", esc(&key.replace(" > ", " "))));
                    a.push(rel(file)?.into());
                }
            }
            return Ok(("node".into(), a));
        }
        Fw::Cargo => {
            a.extend(["test", "--no-fail-fast", "--color", "never"].map(String::from));
            match scope {
                Scope::All { .. } => {}
                Scope::File { file, keys } => {
                    if keys.is_empty() {
                        return Err(format!("Keine Tests in {file}"));
                    }
                    a.extend(cargo_target(rel(file)?));
                    a.extend(["--".into(), "--exact".into()]);
                    for k in keys {
                        a.push(name(k)?.into());
                    }
                }
                // ponytail: Suite als Teilstring ("tests::" trifft auch andere Module); was ausserhalb liegt,
                // verwirft das Frontend (Scope-Regel). Genauer ginge es mit der Liste aller Tests der Suite.
                Scope::Test { file, key, suite } => {
                    a.extend(cargo_target(rel(file)?));
                    a.push("--".into());
                    if *suite {
                        a.push(format!("{}::", name(key)?));
                    } else {
                        a.extend([name(key)?.into(), "--exact".into()]);
                    }
                }
            }
        }
        Fw::Dotnet => {
            a.extend(["test", "--logger", "console;verbosity=normal"].map(String::from));
            let filter = match scope {
                Scope::All { .. } => None,
                // "~": Theories heissen "Methode(x: 1)"; zu viel Getroffenes verwirft die Scope-Regel.
                Scope::File { keys, .. } => {
                    Some(keys.iter().map(|k| name(k).map(|k| format!("FullyQualifiedName~{k}"))).collect::<Result<Vec<_>, _>>()?.join("|"))
                }
                Scope::Test { key, suite, .. } => Some(format!("FullyQualifiedName{}{}", if *suite { "~" } else { "=" }, name(key)?)),
            };
            if let Some(f) = filter.filter(|f| !f.is_empty()) {
                a.extend(["--filter".into(), f]);
            }
        }
        Fw::Pytest => {
            a.extend(["-m", "pytest", "-v", "-rfE", "--color=no", "-p", "no:cacheprovider"].map(String::from));
            match scope {
                Scope::All { .. } => {}
                Scope::File { file, .. } => a.push(rel(file)?.into()),
                Scope::Test { key, .. } => {
                    // nodeid "datei::Class::test[param]": die Datei pruefen, der Rest ist nur Argument.
                    rel(key.split("::").next().unwrap_or_default())?;
                    a.push(key.clone());
                }
            }
            return Ok(((if windows { "python" } else { "python3" }).into(), a));
        }
    }
    Ok((prog.into(), a))
}

/// Lokales JS-Tool: node_modules/.bin/<tool> ab cwd aufwaerts bis zum Projektordner. Kein npx: kein Netz, kein Overhead.
fn local_bin(repo: &Path, cwd: &Path, tool: &str, windows: bool, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let exe = if windows { format!("{tool}.cmd") } else { tool.into() };
    cwd.ancestors()
        .take_while(|d| d.starts_with(repo))
        .map(|d| d.join("node_modules").join(".bin").join(&exe))
        .find(|p| exists(p))
}

/// Reporter ins Cache-Verzeichnis schreiben (nur wenn anders) und den Ordner liefern.
fn reporters(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("reporters");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cache-Ordner: {e}"))?;
    for (name, text) in REPORTERS {
        let p = dir.join(name);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(text) {
            std::fs::write(&p, text).map_err(|e| format!("Reporter schreiben: {e}"))?;
        }
    }
    Ok(dir)
}

/// Zeilen aus rx gebuendelt weitergeben: nach TICK ab der ersten wartenden Zeile, bei BATCH Zeilen und am Ende.
fn forward(rx: Receiver<String>, mut send: impl FnMut(Vec<String>)) {
    let mut batch = Vec::new();
    let mut first = Instant::now();
    loop {
        let next = if batch.is_empty() {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            rx.recv_timeout(TICK.saturating_sub(first.elapsed()))
        };
        match next {
            Ok(line) => {
                if batch.is_empty() {
                    first = Instant::now();
                }
                batch.push(line);
                if batch.len() >= BATCH || first.elapsed() >= TICK {
                    send(std::mem::take(&mut batch));
                }
            }
            Err(RecvTimeoutError::Timeout) => send(std::mem::take(&mut batch)),
            Err(RecvTimeoutError::Disconnected) => {
                if !batch.is_empty() {
                    send(batch);
                }
                return;
            }
        }
    }
}

fn start(app: AppHandle, id: String, repo: &str, dir: &str, fw: Fw, scope: &Scope, on: Channel<Msg>) -> Result<(), String> {
    let cwd = if dir.is_empty() { PathBuf::from(repo) } else { crate::files::within(repo, dir)? };
    // Auch ueber Symlinks nicht aus dem Projekt heraus.
    let file = match scope {
        Scope::All { .. } => None,
        Scope::File { file, .. } | Scope::Test { file, .. } => Some(file),
    };
    if let Some(f) = file {
        crate::files::within(repo, &if dir.is_empty() { f.clone() } else { format!("{dir}/{f}") })?;
    }
    let prog = match fw {
        Fw::Vitest | Fw::Jest => {
            let tool = if fw == Fw::Vitest { "vitest" } else { "jest" };
            let root = Path::new(repo).canonicalize().map_err(|e| e.to_string())?;
            let at = cwd.canonicalize().map_err(|e| e.to_string())?;
            local_bin(&root, &at, tool, cfg!(windows), Path::is_file)
                .ok_or_else(|| format!("{tool} nicht installiert (npm install)"))?
                .to_string_lossy()
                .into_owned()
        }
        Fw::Node => "node".into(),
        Fw::Cargo => "cargo".into(),
        Fw::Dotnet => "dotnet".into(),
        Fw::Pytest => String::new(),
    };
    let (prog, args) = test_command(fw, scope, &prog, &reporters(&app)?, cfg!(windows))?;

    let mut cmd = crate::quiet(&prog);
    cmd.args(&args).current_dir(&cwd).envs(ENV);
    // Lock haelt den Waiter auf, bis die pid eingetragen ist: ein sofort endender Prozess hinterliesse sonst einen Eintrag.
    let state = app.state::<Runs>();
    let mut runs = state.inner().lock();
    let done = {
        let (app, id) = (app.clone(), id.clone());
        move || {
            app.state::<Runs>().lock().remove(&id);
        }
    };
    let pid = launch(cmd, move |m| {
        let _ = on.send(m);
    }, done).map_err(|e| format!("{prog} konnte nicht gestartet werden: {e}"))?;
    runs.insert(id, pid);
    Ok(())
}

/// Prozess starten (stdin leer, stdout + stderr in einer Pipe, Unix: eigene Prozessgruppe) und seine Zeilen
/// gebuendelt als Msg::Lines melden, zuletzt Msg::Exit. done laeuft vor Exit. Liefert die pid.
fn launch(mut cmd: Command, send: impl Fn(Msg) + Send + Sync + 'static, done: impl FnOnce() + Send + 'static) -> std::io::Result<u32> {
    let (reader, writer) = std::io::pipe()?;
    cmd.stdin(Stdio::null()).stdout(writer.try_clone()?).stderr(writer);
    // Eigene Gruppe: kill_tree trifft so auch Worker (vitest-Pool, Testbinaries), nicht uns.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn()?;
    // Unser Schreibende schliessen, sonst kommt nie EOF.
    drop(cmd);
    let pid = child.id();
    let send = Arc::new(send);

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(line).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let (fin_tx, fin_rx) = mpsc::channel::<()>();
    let out = send.clone();
    std::thread::spawn(move || {
        forward(rx, |lines| out(Msg::Lines { lines }));
        let _ = fin_tx.send(());
    });
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        // Exit nach dem letzten Output; haelt ein uebrig gebliebener Enkel die Pipe offen, nicht ewig warten.
        let _ = fin_rx.recv_timeout(Duration::from_secs(2));
        done();
        send(Msg::Exit { code });
    });
    Ok(pid)
}

/// Testlauf starten; kehrt nach dem Start zurueck, Output und Ende kommen ueber `on`.
/// Fuehrt Code aus dem Projekt aus: das Frontend ruft das nur auf Klick.
#[tauri::command]
pub async fn test_run(
    app: AppHandle,
    id: String,
    repo: String,
    dir: String,
    fw: Fw,
    scope: Scope,
    on: Channel<Msg>,
) -> Result<(), String> {
    if !crate::pty::valid_id(&id) {
        return Err(format!("Ungültige Lauf-id: {id}"));
    }
    crate::git::blocking(move || start(app, id, &repo, &dir, fw, &scope, on)).await
}

/// Lauf abbrechen (samt Kindprozessen); unbekannte id: nichts. Exit kommt wie gewohnt ueber den Channel.
#[tauri::command]
pub async fn test_cancel(app: AppHandle, id: String) -> Result<(), String> {
    let pid = app.state::<Runs>().lock().remove(&id);
    if let Some(pid) = pid {
        crate::git::blocking(move || {
            crate::kill_tree(pid);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(fw: Fw, scope: Scope) -> Result<(String, Vec<String>), String> {
        test_command(fw, &scope, "/p/node_modules/.bin/tool", Path::new("/cache/rep"), false)
    }
    fn file(f: &str) -> Scope {
        Scope::File { file: f.into(), keys: vec![] }
    }
    fn test(f: &str, key: &str, suite: bool) -> Scope {
        Scope::Test { file: f.into(), key: key.into(), suite }
    }
    fn all() -> Scope {
        Scope::All { files: vec![] }
    }

    #[test]
    fn vitest_kommandos() {
        let base = ["run", "--reporter=/cache/rep/vitest.mjs", "--includeTaskLocation"];
        let (p, a) = cmd(Fw::Vitest, all()).unwrap();
        assert_eq!(p, "/p/node_modules/.bin/tool");
        assert_eq!(a, base);
        assert_eq!(cmd(Fw::Vitest, file("src/a.test.ts")).unwrap().1[3..], ["src/a.test.ts"]);
        let (_, a) = cmd(Fw::Vitest, test("a.test.ts", "suite > bad (x)", false)).unwrap();
        assert_eq!(a[3..], ["a.test.ts", "-t", "^suite > bad \\(x\\)$"]);
        let (_, a) = cmd(Fw::Vitest, test("a.test.ts", "suite", true)).unwrap();
        assert_eq!(a[5], "^suite( > |$)");
    }

    #[test]
    fn jest_kommandos() {
        let (_, a) = cmd(Fw::Jest, test("a.test.js", "s > t.x", false)).unwrap();
        assert_eq!(a[0], "--reporters=/cache/rep/jest.cjs");
        assert_eq!(a[4..], ["--runTestsByPath", "a.test.js", "-t", "^s t\\.x( |$)"]);
        assert_eq!(cmd(Fw::Jest, file("a.test.js")).unwrap().1[4..], ["--runTestsByPath", "a.test.js"]);
    }

    #[test]
    fn node_kommandos() {
        let (p, a) = cmd(Fw::Node, Scope::All { files: vec!["a.test.ts".into(), "b.test.ts".into()] }).unwrap();
        assert_eq!(p, "node");
        assert_eq!(a, ["--test", "--test-reporter=/cache/rep/node.mjs", "a.test.ts", "b.test.ts"]);
        let many = Scope::All { files: (0..400).map(|i| format!("t{i}.test.ts")).collect() };
        assert_eq!(cmd(Fw::Node, many).unwrap().1.len(), 2);
        let (_, a) = cmd(Fw::Node, test("a.test.ts", "suite > bad", true)).unwrap();
        assert_eq!(a[2..], ["--test-name-pattern=^suite bad( |$)", "a.test.ts"]);
    }

    #[test]
    fn cargo_kommandos() {
        let base = ["test", "--no-fail-fast", "--color", "never"];
        assert_eq!(cmd(Fw::Cargo, all()).unwrap(), ("/p/node_modules/.bin/tool".into(), base.map(String::from).to_vec()));
        let (_, a) = cmd(Fw::Cargo, Scope::File { file: "src/pty.rs".into(), keys: vec!["pty::tests::a".into(), "pty::tests::b".into()] }).unwrap();
        assert_eq!(a[4..], ["--", "--exact", "pty::tests::a", "pty::tests::b"]);
        let (_, a) = cmd(Fw::Cargo, test("tests/api.rs", "geht", false)).unwrap();
        assert_eq!(a[4..], ["--test", "api", "--", "geht", "--exact"]);
        let (_, a) = cmd(Fw::Cargo, test("src/bin/x.rs", "tests", true)).unwrap();
        assert_eq!(a[4..], ["--bin", "x", "--", "tests::"]);
        assert!(cmd(Fw::Cargo, Scope::File { file: "src/a.rs".into(), keys: vec![] }).is_err());
    }

    #[test]
    fn dotnet_kommandos() {
        let base = ["test", "--logger", "console;verbosity=normal"];
        assert_eq!(cmd(Fw::Dotnet, all()).unwrap().1, base);
        let (_, a) = cmd(Fw::Dotnet, test("T.cs", "T.Calc.Bad", false)).unwrap();
        assert_eq!(a[3..], ["--filter", "FullyQualifiedName=T.Calc.Bad"]);
        let (_, a) = cmd(Fw::Dotnet, test("T.cs", "T.Calc.Th", true)).unwrap();
        assert_eq!(a[4], "FullyQualifiedName~T.Calc.Th");
        let (_, a) = cmd(Fw::Dotnet, Scope::File { file: "T.cs".into(), keys: vec!["T.A.x".into(), "T.A.y".into()] }).unwrap();
        assert_eq!(a[4], "FullyQualifiedName~T.A.x|FullyQualifiedName~T.A.y");
    }

    #[test]
    fn pytest_kommandos() {
        let (p, a) = cmd(Fw::Pytest, test("tests/test_a.py", "tests/test_a.py::TestX::test_y[1]", false)).unwrap();
        assert_eq!(p, "python3");
        assert_eq!(a.last().unwrap(), "tests/test_a.py::TestX::test_y[1]");
        let (p, _) = test_command(Fw::Pytest, &all(), "", Path::new("/c"), true).unwrap();
        assert_eq!(p, "python");
        assert!(cmd(Fw::Pytest, test("a.py", "../x.py::t", false)).is_err());
    }

    #[test]
    fn werte_bleiben_ein_argument() {
        // Shell-Zeichen und Leerzeichen: ein einzelnes Argument, nie Shell-Text.
        let evil = "a; rm -rf ~ $(x) `y` | z";
        let (_, a) = cmd(Fw::Vitest, test("a.test.ts", evil, false)).unwrap();
        assert_eq!(a.len(), 6);
        assert_eq!(a[5], "^a; rm -rf ~ \\$\\(x\\) `y` \\| z$");
        let (_, a) = cmd(Fw::Node, test("a b.test.ts", evil, false)).unwrap();
        assert_eq!(a.last().unwrap(), "a b.test.ts");
        // cargo/dotnet: nur Bezeichner.
        assert!(cmd(Fw::Cargo, test("src/a.rs", evil, false)).is_err());
        assert!(cmd(Fw::Dotnet, test("a.cs", "A|FullyQualifiedName~B", false)).is_err());
        assert!(cmd(Fw::Cargo, test("src/a.rs", "--nocapture", false)).is_err());
    }

    #[test]
    fn dateien_ausserhalb_abgelehnt() {
        for f in ["../x.test.ts", "/etc/passwd", "--reporter=x.mjs", "", "a/../../b.ts"] {
            assert!(cmd(Fw::Vitest, file(f)).is_err(), "{f}");
            assert!(cmd(Fw::Node, Scope::All { files: vec![f.into()] }).is_err(), "{f}");
        }
    }

    #[test]
    fn windows_varianten() {
        let (_, a) = test_command(Fw::Node, &all(), "", Path::new("C:\\Users\\m\\cache\\rep"), true).unwrap();
        assert_eq!(a[1], "--test-reporter=file:///C:/Users/m/cache/rep/node.mjs");
        let repo = Path::new("/r");
        let found = local_bin(repo, Path::new("/r/pkg"), "vitest", true, |p| p == Path::new("/r/node_modules/.bin/vitest.cmd"));
        assert_eq!(found.as_deref(), Some(Path::new("/r/node_modules/.bin/vitest.cmd")));
    }

    #[test]
    fn lokales_binary_nur_im_projekt() {
        let repo = Path::new("/r");
        let any = |_: &Path| true;
        assert_eq!(local_bin(repo, Path::new("/r/a/b"), "jest", false, any).unwrap(), Path::new("/r/a/b/node_modules/.bin/jest"));
        let only_home = |p: &Path| p == Path::new("/node_modules/.bin/jest");
        assert_eq!(local_bin(repo, Path::new("/r/a"), "jest", false, only_home), None);
    }

    #[test]
    fn buendelt_zeilen() {
        let (tx, rx) = mpsc::channel();
        for i in 0..1200 {
            tx.send(i.to_string()).unwrap();
        }
        drop(tx);
        let mut sizes = Vec::new();
        forward(rx, |b| sizes.push(b.len()));
        assert_eq!(sizes.iter().sum::<usize>(), 1200);
        assert!(sizes.iter().all(|n| *n <= BATCH));
    }

    #[cfg(unix)]
    #[test]
    fn launch_meldet_zeilen_und_exit_zuletzt() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo a; echo b >&2; printf 'c'; exit 3"]);
        let (tx, rx) = mpsc::channel();
        let (dtx, drx) = mpsc::channel();
        launch(cmd, move |m| drop(tx.send(m)), move || {
            let _ = dtx.send(());
        })
        .unwrap();
        let mut lines = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                Msg::Lines { lines: l } => lines.extend(l),
                Msg::Exit { code } => {
                    assert_eq!(code, Some(3));
                    break;
                }
            }
        }
        assert_eq!(lines, ["a", "b", "c"]);
        assert!(drx.try_recv().is_ok());
    }

    #[test]
    fn msg_format() {
        let j = serde_json::to_string(&Msg::Lines { lines: vec!["x".into()] }).unwrap();
        assert_eq!(j, r#"{"event":"lines","data":{"lines":["x"]}}"#);
        let j = serde_json::to_string(&Msg::Exit { code: Some(1) }).unwrap();
        assert_eq!(j, r#"{"event":"exit","data":{"code":1}}"#);
        let s: Scope = serde_json::from_str(r#"{"kind":"test","file":"a","key":"b","suite":false}"#).unwrap();
        assert!(matches!(s, Scope::Test { suite: false, .. }));
    }
}
