//! Unity-Projekte ueber die unity-CLI: Status, Einrichten, Editor auf/zu, Tests.
//! Formen und Namen wie in src/lib/unity.ts.
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UnityInfo {
    version: Option<String>,
    cli: bool,
    editor_installed: bool,
    editor_open: bool,
    pipeline: bool,
    connected: bool,
    skill: bool,
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    passed: u32,
    failed: u32,
    skipped: u32,
    inconclusive: u32,
    total: u32,
    duration_secs: f64,
    failures: Vec<Failure>,
    via: String,
    report_path: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Failure {
    name: String,
    message: String,
}

// ---------- Projekt ----------

/// Marker fuer den Scan: nur der Repo-Root zaehlt.
pub(crate) fn is_project(dir: &Path) -> bool {
    dir.join("Assets").is_dir() && dir.join("ProjectSettings").join("ProjectVersion.txt").is_file()
}

/// `m_EditorVersion: 6000.0.47f1` aus ProjectVersion.txt; nicht die WithRevision-Zeile.
fn parse_version(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("m_EditorVersion:"))
        .map(|v| v.trim().to_string())
        // Geht als Argument an die CLI, darf also nie wie eine Option aussehen.
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
}

fn read_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("ProjectSettings").join("ProjectVersion.txt")).ok()?;
    parse_version(&text)
}

fn manifest_has_pipeline(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .is_ok_and(|m| m["dependencies"].get("com.unity.pipeline").is_some())
}

fn has_pipeline(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Packages").join("manifest.json"))
        .is_ok_and(|t| manifest_has_pipeline(&t))
}

fn project(path: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err(format!("Ordner nicht gefunden: {path}"));
    }
    if !is_project(&dir) {
        return Err(format!("Kein Unity-Projekt: {path}"));
    }
    Ok(dir)
}

// ---------- CLI ----------

/// GUI-Apps haben auf macOS kaum PATH; der Installer legt die CLI nach ~/.unity/bin.
fn find_cli(
    windows: bool,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let (exe, sep, slash) = if windows { ("unity.exe", ';', '\\') } else { ("unity", ':', '/') };
    let join = |dir: &str, rest: &str| format!("{}{slash}{rest}", dir.trim_end_matches(['\\', '/']));
    let home = env(if windows { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.trim().is_empty())
        .map(|h| join(&h, &format!(".unity{slash}bin{slash}{exe}")));
    env("PATH")
        .unwrap_or_default()
        .split(sep)
        .filter(|d| !d.trim().is_empty())
        .map(|d| join(d, exe))
        .chain(home)
        .find(|p| exists(p))
}

fn cli() -> Result<String, String> {
    find_cli(cfg!(windows), |k| std::env::var(k).ok(), |p| Path::new(p).is_file()).ok_or_else(|| {
        "Unity-CLI nicht gefunden (weder im PATH noch in ~/.unity/bin). Installieren: \
         https://public-cdn.cloud.unity3d.com/hub/prod/cli/install.sh"
            .to_string()
    })
}

/// Statusabfragen (editors running/path) antworten sofort; haengen sie, soll die UI nicht ewig warten.
const SHORT: Duration = Duration::from_secs(30);
/// Oeffnen, Schliessen (CLI wartet selbst bis 60 s), Einrichten, Editor-Befehle.
const LONG: Duration = Duration::from_secs(600);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// PIDs laufender CLI-Aufrufe, damit beim App-Ende kein Batch-Unity verwaist weiterlaeuft.
static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Beim App-Ende: laufende Testlaeufe samt Unity beenden.
pub(crate) fn kill_all() {
    for pid in lock(&RUNNING).drain(..) {
        crate::kill_tree(pid);
    }
}

fn unity(cli: &str, dir: &Path, args: &[&str], limit: Duration) -> Result<Output, String> {
    let mut cmd = crate::quiet(cli);
    cmd.args(["--no-banner", "--no-pager", "--non-interactive"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Eigene Gruppe: crate::kill_tree trifft so die CLI und den von ihr gestarteten Unity-Prozess, nicht uns.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd.spawn().map_err(|e| format!("Unity-CLI konnte nicht gestartet werden: {e}"))?;
    let pid = child.id();
    lock(&RUNNING).push(pid);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || tx.send(child.wait_with_output()));
    let res = rx.recv_timeout(limit);
    if res.is_err() {
        crate::kill_tree(pid);
    }
    lock(&RUNNING).retain(|p| *p != pid);
    match res {
        Ok(o) => o.map_err(|e| format!("Unity-CLI: {e}")),
        Err(_) => Err(format!("Unity-CLI hing bei `{}` und wurde nach {} s beendet", args.join(" "), limit.as_secs())),
    }
}

/// Test, Oeffnen und Einrichten schliessen sich je Projekt aus: sonst laeuft z. B. Batchmode neben dem
/// gerade startenden Editor oder das Paket-Manifest aendert sich mitten im Testlauf.
/// Schliessen bleibt frei — es ist der Notausgang fuer einen haengenden Editor-Testlauf.
static BUSY: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[derive(Debug)]
struct Busy(PathBuf);

impl Drop for Busy {
    fn drop(&mut self) {
        lock(&BUSY).retain(|p| p != &self.0);
    }
}

fn busy(dir: &Path) -> Result<Busy, String> {
    let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut all = lock(&BUSY);
    if all.contains(&key) {
        return Err("Unity ist für dieses Projekt gerade beschäftigt (Test, Öffnen oder Einrichten läuft). Kurz warten.".into());
    }
    all.push(key.clone());
    Ok(Busy(key))
}

/// JSON-Huelle der CLI: `data` bei success, sonst die erste Fehlermeldung.
/// Nicht am Exit-Code allein festmachen — manche Befehle melden Fehler mit 0 und umgekehrt.
fn parse_envelope(ok: bool, stdout: &str, stderr: &str) -> Result<Value, String> {
    let json = stdout
        .find('{')
        .and_then(|i| serde_json::Deserializer::from_str(&stdout[i..]).into_iter::<Value>().next())
        .and_then(Result::ok)
        .filter(|v| v.get("success").is_some());
    if let Some(v) = json {
        if v["success"] == true {
            return Ok(v["data"].clone());
        }
        let msg = [&v["errors"][0]["message"], &v["error"], &v["data"]["error"]]
            .into_iter()
            .find_map(|m| m.as_str())
            .unwrap_or("unbekannter Fehler");
        return Err(msg.to_string());
    }
    if ok {
        return Ok(Value::Null);
    }
    let err = stderr.trim();
    Err(if err.is_empty() { stdout.trim() } else { err }.to_string())
}

fn envelope(o: &Output) -> Result<Value, String> {
    parse_envelope(
        o.status.success(),
        &String::from_utf8_lossy(&o.stdout),
        &String::from_utf8_lossy(&o.stderr),
    )
}

fn same_path(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    // macOS/Windows: Dateisystem ohne Gross-/Kleinschreibung.
    a == b
        || (!cfg!(target_os = "linux")
            && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase())
}

/// Aus `unity editors running --json`: None = kein Editor hat das Projekt offen,
/// Some(reachable) = offen, reachable = per Pipeline steuerbar.
fn find_instance(data: &Value, dir: &Path) -> Option<bool> {
    data["instances"]
        .as_array()?
        .iter()
        .find(|i| i["projectPath"].as_str().is_some_and(|p| same_path(Path::new(p), dir)))
        .map(|i| i["reachable"] == true)
}

/// Prozessliste statt Temp/UnityLockfile: die Datei ueberlebt einen Absturz.
fn editor_state(cli: &str, dir: &Path) -> Result<Option<bool>, String> {
    let data = envelope(&unity(cli, dir, &["editors", "running", "--json"], SHORT)?)
        .map_err(|e| format!("Laufende Unity-Editoren nicht lesbar: {e}"))?;
    Ok(find_instance(&data, dir))
}

// ---------- Info ----------

fn info_blocking(path: &str) -> Result<UnityInfo, String> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err(format!("Ordner nicht gefunden: {path}"));
    }
    let mut info = UnityInfo {
        version: read_version(&dir),
        pipeline: has_pipeline(&dir),
        skill: dir.join(".claude").join("skills").join("unity-cli").is_dir(),
        ..Default::default()
    };
    let Ok(cli) = cli() else { return Ok(info) };
    info.cli = true;
    if let Some(v) = &info.version {
        info.editor_installed = unity(&cli, &dir, &["editors", "path", v, "--json"], SHORT)
            .is_ok_and(|o| envelope(&o).is_ok());
    }
    if let Some(reachable) = editor_state(&cli, &dir)? {
        info.editor_open = true;
        info.connected = reachable;
    }
    Ok(info)
}

// ---------- Einrichten ----------

/// Pipeline-Versionen, die hier nicht kompilieren (CS0246 DialogEventInfo, alle 8 Paketversionen).
/// Installieren wuerde das Projekt in den Safe Mode schicken.
// ponytail: feste Liste aus einem Probelauf; erweitern, wenn weitere Editoren scheitern.
const BROKEN_PIPELINE: &[&str] = &["6000.7.0a3"];

/// 0.8 hat `[CliCommand]` nach `Unity.Pipeline.Attributes` verschoben; das CLI-Assembly von
/// URP-Core 17.7 referenziert nur `Unity.Pipeline` und kompiliert mit 0.8 nicht (CS0246 CliCommand).
// ponytail: fest gepinnt; anheben, sobald URP-Core `Unity.Pipeline.Attributes` referenziert.
const PIPELINE_VERSION: &str = "0.7.0-exp.1";

const MD_HEAD: &str = "## Unity-CLI";
const MD_SECTION: &str = "## Unity-CLI

Unity per `unity`-CLI steuern (Skill: `.claude/skills/unity-cli`). Zuerst `unity editors running --json` für dieses Projekt prüfen:
- Editor offen und `reachable: true`: nur über ihn arbeiten – `unity command <name> --project-path .`, Tests mit `unity command run_tests --mode editor --async_tests true`, dann `unity command test_status`.
- Editor offen, aber nicht erreichbar: Nutzer bitten, ihn zu schließen – nie selbst beenden (ungespeicherte Arbeit). `unity test`/`unity run` scheitern, solange er offen ist.
- Editor zu: `unity test . --mode EditMode --output Logs/tests.xml` (bzw. PlayMode), sonstige Batch-Aufgaben mit `unity run .`.
- Grün heißt: failed, skipped und inconclusive sind 0.
- `Temp/UnityLockfile` nie löschen.
";

/// None: Abschnitt steht schon drin.
fn with_section(md: &str) -> Option<String> {
    if md.contains(MD_HEAD) {
        return None;
    }
    let mut out = md.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(MD_SECTION);
    Some(out)
}

fn setup_blocking(path: &str) -> Result<String, String> {
    let dir = project(path)?;
    let cli = cli()?;
    let _busy = busy(&dir)?;
    let p = dir.to_string_lossy().into_owned();
    let mut log = Vec::new();

    if has_pipeline(&dir) {
        log.push("Pipeline-Paket: schon installiert".to_string());
    } else if let Some(v) = read_version(&dir).filter(|v| BROKEN_PIPELINE.contains(&v.as_str())) {
        log.push(format!(
            "Pipeline-Paket: übersprungen – kompiliert unter Unity {v} nicht. Nach einem Editor-Update erneut einrichten."
        ));
    } else {
        envelope(&unity(&cli, &dir, &["pipeline", "install", "--project-path", &p, "--package-version", PIPELINE_VERSION, "--json"], LONG)?)
            .map_err(|e| format!("Pipeline-Paket nicht installiert: {e}"))?;
        log.push("Pipeline-Paket: installiert".to_string());
    }

    // Immer: spiegelt auch den unity-pipeline-Skill, sobald das Paket im PackageCache liegt.
    envelope(&unity(&cli, &dir, &["skill", "install", "claude-code", "--local", "--yes", "--json"], LONG)?)
        .map_err(|e| format!("Unity-Skill nicht installiert: {e}"))?;
    log.push("Skill: .claude/skills/unity-cli aktuell".to_string());

    let md_path = dir.join("CLAUDE.md");
    let md = match std::fs::read_to_string(&md_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("CLAUDE.md nicht lesbar: {e}")),
    };
    match with_section(&md) {
        Some(new) => {
            std::fs::write(&md_path, new).map_err(|e| format!("CLAUDE.md nicht schreibbar: {e}"))?;
            log.push("CLAUDE.md: Unity-CLI-Abschnitt ergänzt".to_string());
        }
        None => log.push("CLAUDE.md: Unity-CLI-Abschnitt vorhanden".to_string()),
    }
    Ok(log.join("\n"))
}

// ---------- Editor ----------

const OPEN_TIMEOUT: Duration = Duration::from_secs(180);

fn open_blocking(path: &str) -> Result<(), String> {
    let dir = project(path)?;
    let cli = cli()?;
    let _busy = busy(&dir)?;
    if editor_state(&cli, &dir)?.is_some() {
        return Ok(());
    }
    // Die CLI startet den Editor im Hintergrund und kehrt sofort zurueck.
    envelope(&unity(&cli, &dir, &["open", &dir.to_string_lossy()], LONG)?)
        .map_err(|e| format!("Unity-Editor ließ sich nicht öffnen: {e}"))?;
    // Erst fertig, wenn der Editor gelistet ist: sonst sieht ein Test gleich danach "zu" und startet
    // Batchmode daneben, und ein zweites Oeffnen startet einen zweiten Editor.
    let start = Instant::now();
    while start.elapsed() < OPEN_TIMEOUT {
        std::thread::sleep(Duration::from_secs(2));
        if editor_state(&cli, &dir)?.is_some() {
            return Ok(());
        }
    }
    Err(format!("Unity-Editor nach {} s noch nicht gestartet (Unity Hub/Lizenz prüfen).", OPEN_TIMEOUT.as_secs()))
}

/// force beendet den Prozess, falls der Editor nicht per Pipeline zu schliessen ist.
/// Gespeichert wird so oder so nicht.
fn close_blocking(path: &str, force: bool) -> Result<(), String> {
    let dir = project(path)?;
    let cli = cli()?;
    if editor_state(&cli, &dir)?.is_none() {
        return Ok(());
    }
    let p = dir.to_string_lossy();
    let mut args = vec!["close", &p, "--timeout", "60", "--json"];
    if force {
        args.push("--force");
    }
    envelope(&unity(&cli, &dir, &args, LONG)?)
        .map(drop)
        .map_err(|e| format!("Unity-Editor ließ sich nicht schließen: {e}"))
}

// ---------- Tests ----------

#[derive(Debug, PartialEq)]
enum Via {
    Editor,
    Batch,
}

/// `unity test` weigert sich bei offenem Editor; dann nur ueber dessen Pipeline.
fn route(open: Option<bool>, pipeline: bool) -> Result<Via, String> {
    match open {
        None => Ok(Via::Batch),
        Some(true) => Ok(Via::Editor),
        Some(false) if pipeline => Err("Der Unity-Editor hat das Projekt offen, antwortet aber nicht \
            über die Pipeline (startet noch, Kompilierfehler oder Safe Mode?). Kurz warten oder den Editor schließen."
            .into()),
        Some(false) => Err("Der Unity-Editor hat das Projekt offen, ist aber nicht steuerbar. \
            Unity einrichten (Pipeline-Paket) oder den Editor schließen."
            .into()),
    }
}

fn unescape(s: &str) -> String {
    // &amp; zuletzt, sonst wird aus "&amp;lt;" ein "<".
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Attributwert aus einem Start-Tag; `name` trifft nicht auf `fullname`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&pat) {
        let at = from + i;
        if tag[..at].ends_with(char::is_whitespace) {
            let val = &tag[at + pat.len()..];
            return val.find('"').map(|end| unescape(&val[..end]));
        }
        from = at + pat.len();
    }
    None
}

fn element_text(body: &str, name: &str) -> Option<String> {
    let start = body.find(&format!("<{name}>"))? + name.len() + 2;
    let len = body[start..].find(&format!("</{name}>"))?;
    let raw = body[start..start + len].trim();
    Some(match raw.strip_prefix("<![CDATA[").and_then(|r| r.strip_suffix("]]>")) {
        Some(cdata) => cdata.trim().to_string(),
        None => unescape(raw),
    })
}

/// Start-Tag `<name ...>` bzw. `<name .../>`; `<test-run` trifft nicht auf `<test-runs`.
fn start_tag<'a>(xml: &'a str, name: &str) -> Option<(usize, &'a str)> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let at = from + i;
        let rest = &xml[at + open.len()..];
        if rest.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            let end = rest.find('>')? + at + open.len() + 1;
            return Some((at, &xml[at..end]));
        }
        from = at + open.len();
    }
    None
}

/// NUnit3-Bericht von `unity test`: Zahlen vom <test-run>, Fehler aus den <test-case>.
fn parse_nunit(xml: &str) -> Result<TestReport, String> {
    let (_, run) = start_tag(xml, "test-run").ok_or("Testbericht unlesbar: kein <test-run>")?;
    let num = |k: &str| attr(run, k).and_then(|v| v.trim().parse::<u32>().ok());
    let (passed, failed) = (num("passed").unwrap_or(0), num("failed").unwrap_or(0));
    let (skipped, inconclusive) = (num("skipped").unwrap_or(0), num("inconclusive").unwrap_or(0));
    let total = num("total").unwrap_or(passed + failed + skipped + inconclusive);

    let mut failures = Vec::new();
    let mut rest = xml;
    while let Some((at, tag)) = start_tag(rest, "test-case") {
        let after = &rest[at + tag.len()..];
        let body = if tag.ends_with("/>") {
            ""
        } else {
            &after[..after.find("</test-case>").unwrap_or(after.len())]
        };
        if attr(tag, "result").as_deref() == Some("Failed") {
            failures.push(Failure {
                name: attr(tag, "fullname").or_else(|| attr(tag, "name")).unwrap_or_default(),
                message: element_text(body, "message").unwrap_or_default(),
            });
        }
        rest = after;
    }
    Ok(TestReport { passed, failed, skipped, inconclusive, total, failures, ..Default::default() })
}

/// Statusdatei des Pipeline-Testrunners. Ok(None): laeuft noch (oder halb geschrieben).
fn parse_editor_status(text: &str) -> Result<Option<TestReport>, String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Ok(None) };
    // Summary kommt klein, die Einzelergebnisse als C#-Properties gross geschrieben.
    fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
        v.as_object()
            .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)))
            .map_or(&Value::Null, |(_, v)| v)
    }
    let s = |v: &Value, k: &str| field(v, k).as_str().unwrap_or_default().to_string();
    match s(&v, "status").as_str() {
        "completed" => {}
        "error" | "cancelled" => {
            let msg = s(&v, "message");
            return Err(format!("Testlauf im Editor abgebrochen: {}", if msg.is_empty() { s(&v, "status") } else { msg }));
        }
        _ => return Ok(None),
    }
    let sum = field(&v, "summary");
    let n = |k: &str| u32::try_from(field(sum, k).as_u64().unwrap_or(0)).unwrap_or(u32::MAX);
    let failures = field(&v, "results")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| s(r, "status") == "Failed")
        .map(|r| Failure { name: s(r, "fullName"), message: s(r, "message") })
        .collect();
    Ok(Some(TestReport {
        passed: n("passed"),
        failed: n("failed"),
        skipped: n("skipped"),
        inconclusive: n("inconclusive"),
        total: n("total"),
        failures,
        ..Default::default()
    }))
}

fn report_file(dir: &Path, mode: &str, ext: &str) -> Result<PathBuf, String> {
    let logs = dir.join("Logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("Logs-Ordner nicht anlegbar: {e}"))?;
    Ok(logs.join(format!("open-claude-{mode}.{ext}")))
}

// Harte Obergrenze, damit ein haengender Editor den Worker nicht ewig belegt.
const TEST_TIMEOUT: Duration = Duration::from_secs(3600);

fn batch_test(cli: &str, dir: &Path, mode: &str) -> Result<TestReport, String> {
    let report = report_file(dir, mode, "xml")?;
    // Ein alter Bericht darf nie als Ergebnis dieses Laufs durchgehen.
    let _ = std::fs::remove_file(&report);
    let (p, r) = (dir.to_string_lossy(), report.to_string_lossy());
    let secs = TEST_TIMEOUT.as_secs().to_string();
    let o = unity(cli, dir, &["test", &p, "--mode", mode, "--output", &r, "--timeout", &secs, "--json"], TEST_TIMEOUT + LONG)?;
    // Exit 8 = Tests liefen, einige rot. Alles andere ohne success hat kein Ergebnis.
    if let Err(e) = envelope(&o) {
        if o.status.code() != Some(8) {
            return Err(format!("Testlauf abgebrochen: {e}"));
        }
    }
    let xml = std::fs::read_to_string(&report)
        .map_err(|e| format!("Testbericht fehlt ({}): {e}", report.display()))?;
    Ok(TestReport { via: "batchmode".into(), report_path: r.into_owned(), ..parse_nunit(&xml)? })
}

fn editor_test(cli: &str, dir: &Path, mode: &str) -> Result<TestReport, String> {
    let p = dir.to_string_lossy();
    let pmode = if mode == "PlayMode" { "playmode" } else { "editor" };
    // PlayMode laedt die Domain neu und kappt dabei jede Verbindung — die Statusdatei ueberlebt das.
    // Selbst vorher loeschen: ein "completed" eines frueheren Laufs darf nie als dieses Ergebnis gelten.
    let status = dir.join("Temp").join("pipeline_test_status.json");
    match std::fs::remove_file(&status) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("Alte Test-Statusdatei nicht löschbar: {e}"));
        }
        _ => {}
    }
    // Ohne Autotick haengen Tests, solange der Editor nicht im Vordergrund ist.
    let _ = unity(cli, dir, &["command", "set_autotick", "--enable", "true", "--project-path", &p, "--json"], LONG);
    let args = ["command", "run_tests", "--mode", pmode, "--async_tests", "true", "--project-path", &p, "--json"];
    envelope(&unity(cli, dir, &args, LONG)?).map_err(|e| format!("Tests im Editor nicht gestartet: {e}"))?;

    let start = Instant::now();
    let mut alive = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let text = std::fs::read_to_string(&status).unwrap_or_default();
        if let Some(report) = parse_editor_status(&text)? {
            let out = report_file(dir, mode, "json")?;
            std::fs::write(&out, &text).map_err(|e| format!("Testbericht nicht schreibbar: {e}"))?;
            let report_path = out.to_string_lossy().into_owned();
            return Ok(TestReport { via: "editor".into(), report_path, ..report });
        }
        if start.elapsed() > TEST_TIMEOUT {
            return Err("Tests im Editor: Zeitlimit überschritten".into());
        }
        if alive.elapsed() > Duration::from_secs(10) {
            alive = Instant::now();
            if editor_state(cli, dir)?.is_none() {
                return Err("Unity-Editor wurde während der Tests beendet".into());
            }
        }
    }
}

fn test_blocking(path: &str, mode: &str) -> Result<TestReport, String> {
    if !matches!(mode, "EditMode" | "PlayMode") {
        return Err(format!("Unbekannter Testmodus: {mode}"));
    }
    let dir = project(path)?;
    let cli = cli()?;
    let _busy = busy(&dir)?;
    let start = Instant::now();
    let mut report = match route(editor_state(&cli, &dir)?, has_pipeline(&dir))? {
        Via::Batch => batch_test(&cli, &dir, mode)?,
        Via::Editor => editor_test(&cli, &dir, mode)?,
    };
    // Wanduhr statt Berichtszeit: die Tests selbst brauchen ms, der Editor-Start dominiert.
    report.duration_secs = start.elapsed().as_secs_f64();
    Ok(report)
}

// ---------- Commands ----------

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("Unity abgebrochen: {e}"))?
}

#[tauri::command]
pub async fn unity_info(path: String) -> Result<UnityInfo, String> {
    blocking(move || info_blocking(&path)).await
}

#[tauri::command]
pub async fn unity_setup(path: String) -> Result<String, String> {
    blocking(move || setup_blocking(&path)).await
}

#[tauri::command]
pub async fn unity_open(path: String) -> Result<(), String> {
    blocking(move || open_blocking(&path)).await
}

#[tauri::command]
pub async fn unity_close(path: String, force: bool) -> Result<(), String> {
    blocking(move || close_blocking(&path, force)).await
}

#[tauri::command]
pub async fn unity_test(path: String, mode: String) -> Result<TestReport, String> {
    blocking(move || test_blocking(&path, &mode)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocui-unity-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn liest_editor_version() {
        let text = "m_EditorVersion: 6000.7.0a3\nm_EditorVersionWithRevision: 6000.7.0a3 (4e1564fc2cca)\n";
        assert_eq!(parse_version(text).as_deref(), Some("6000.7.0a3"));
        // Windows-Checkout mit CRLF: kein \r im Versionsstring.
        let crlf = "m_EditorVersion: 2022.3.10f1\r\nm_EditorVersionWithRevision: x\r\n";
        assert_eq!(parse_version(crlf).as_deref(), Some("2022.3.10f1"));
        // Reihenfolge egal, WithRevision zaehlt nicht.
        assert_eq!(parse_version("m_EditorVersionWithRevision: 1 (x)\nm_EditorVersion: 6000.0.1f1").as_deref(), Some("6000.0.1f1"));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("m_EditorVersion:   \n"), None);
        assert_eq!(parse_version("m_EditorVersionWithRevision: 6000.0.1f1 (x)"), None);
        assert_eq!(parse_version("m_EditorVersion: --evil"), None);
    }

    #[test]
    fn erkennt_projekt_und_pipeline() {
        let dir = tmp("proj");
        assert!(!is_project(&dir));
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        assert!(!is_project(&dir));
        std::fs::create_dir_all(dir.join("ProjectSettings")).unwrap();
        std::fs::write(dir.join("ProjectSettings/ProjectVersion.txt"), "m_EditorVersion: 6000.0.1f1\r\n").unwrap();
        assert!(is_project(&dir));
        assert_eq!(read_version(&dir).as_deref(), Some("6000.0.1f1"));
        let _ = std::fs::remove_dir_all(&dir);

        assert!(manifest_has_pipeline(r#"{"dependencies":{"com.unity.pipeline":"0.8.0-exp.1"}}"#));
        // Nur als echte Dependency, nicht als Teilstring anderer Pakete.
        assert!(!manifest_has_pipeline(r#"{"dependencies":{"com.unity.render-pipelines.universal":"17"}}"#));
        assert!(!manifest_has_pipeline(r#"{"scopedRegistries":[{"scopes":["com.unity.pipeline"]}]}"#));
        assert!(!manifest_has_pipeline("{kaputt"));
        assert!(!manifest_has_pipeline(""));
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn findet_cli_im_path_sonst_im_home() {
        let vars = [("PATH", "/usr/bin:/opt/my tools/bin/:"), ("HOME", "/Users/m")];
        let on_path = |p: &str| p == "/opt/my tools/bin/unity" || p == "/Users/m/.unity/bin/unity";
        assert_eq!(find_cli(false, env(&vars), on_path).as_deref(), Some("/opt/my tools/bin/unity"));
        // Minimaler PATH einer GUI-App: Fallback ~/.unity/bin.
        let home_only = |p: &str| p == "/Users/m/.unity/bin/unity";
        assert_eq!(find_cli(false, env(&vars), home_only).as_deref(), Some("/Users/m/.unity/bin/unity"));
        assert_eq!(find_cli(false, env(&[]), |_| true), None);
        assert_eq!(find_cli(false, env(&vars), |_| false), None);

        let win = [("PATH", r"C:\Windows;;C:\Program Files\Unity CLI\"), ("USERPROFILE", r"C:\Users\M B")];
        let found = |p: &str| p == r"C:\Program Files\Unity CLI\unity.exe";
        assert_eq!(find_cli(true, env(&win), found).as_deref(), Some(r"C:\Program Files\Unity CLI\unity.exe"));
        let home = |p: &str| p == r"C:\Users\M B\.unity\bin\unity.exe";
        assert_eq!(find_cli(true, env(&win), home).as_deref(), Some(r"C:\Users\M B\.unity\bin\unity.exe"));
    }

    #[test]
    fn liest_cli_huelle() {
        let ok = r#"{"success":true,"command":"x","data":{"count":1},"errors":[],"warnings":[]}"#;
        assert_eq!(parse_envelope(true, ok, "").unwrap()["count"], 1);
        // success zaehlt, nicht der Exit-Code (unity command meldet Fehler mit 0).
        let bad = r#"{"success":false,"data":null,"errors":[{"code":"X","message":"kaputt"}]}"#;
        assert_eq!(parse_envelope(true, bad, "").unwrap_err(), "kaputt");
        assert_eq!(parse_envelope(false, &format!("Warning: nope\n{bad}"), "").unwrap_err(), "kaputt");
        assert_eq!(parse_envelope(false, r#"{"success":false,"error":"alt"}"#, "").unwrap_err(), "alt");
        assert_eq!(parse_envelope(false, r#"{"success":false}"#, "").unwrap_err(), "unbekannter Fehler");
        // Befehle ohne JSON (open): Exit-Code und Text.
        assert_eq!(parse_envelope(true, "", "Warning: not in registry").unwrap(), Value::Null);
        assert_eq!(parse_envelope(false, "out", "  err \n").unwrap_err(), "err");
        assert_eq!(parse_envelope(false, "out\n", "").unwrap_err(), "out");
        assert_eq!(parse_envelope(false, "{kein json", "").unwrap_err(), "{kein json");
    }

    #[test]
    fn findet_editor_zum_projekt() {
        let root = tmp("inst");
        let dir = root.join("Mein Spiel");
        let other = root.join("Mein Spiel 2");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let data = |p: &Path, reachable: bool| {
            serde_json::json!({"count":1,"instances":[{"pid":1,"projectPath":p.to_string_lossy(),"reachable":reachable}]})
        };
        assert_eq!(find_instance(&data(&dir, true), &dir), Some(true));
        assert_eq!(find_instance(&data(&dir, false), &dir), Some(false));
        // Aehnlicher Praefix ist ein anderes Projekt.
        assert_eq!(find_instance(&data(&other, true), &dir), None);
        // Gleicher Ordner ueber einen anderen Weg (.., Symlink wie /tmp -> /private/tmp).
        assert_eq!(find_instance(&data(&root.join("Mein Spiel 2/../Mein Spiel"), true), &dir), Some(true));
        assert_eq!(find_instance(&serde_json::json!({"count":0,"instances":[]}), &dir), None);
        assert_eq!(find_instance(&Value::Null, &dir), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn waehlt_testweg() {
        assert_eq!(route(None, false), Ok(Via::Batch));
        assert_eq!(route(None, true), Ok(Via::Batch));
        assert_eq!(route(Some(true), true), Ok(Via::Editor));
        assert!(route(Some(false), false).unwrap_err().contains("einrichten"));
        assert!(route(Some(false), true).unwrap_err().contains("schließen"));
    }

    const NUNIT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<test-run id="2" testcasecount="3" result="Failed" total="3" passed="1" failed="1" inconclusive="0" skipped="1" duration="0.08">
  <test-suite type="Assembly" name="Tests.dll" fullname="Tests.dll" result="Failed" total="3" failed="1">
    <test-case id="1" name="Geht" fullname="Drift.Geht" result="Passed" duration="0.01" />
    <test-case id="2" name="Kaputt" fullname="Drift.Kaputt(&quot;a&amp;b&quot;)" result="Failed" label="Error">
      <properties><property name="x" value="y" /></properties>
      <failure>
        <message><![CDATA[  Expected: 3 <int>
  But was:  2  ]]></message>
        <stack-trace><![CDATA[at Drift.Kaputt ()]]></stack-trace>
      </failure>
    </test-case>
    <test-case id="3" name="Spaeter" fullname="Drift.Spaeter" result="Skipped"><reason><message>ignoriert</message></reason></test-case>
  </test-suite>
</test-run>"#;

    #[test]
    fn liest_nunit_bericht() {
        let r = parse_nunit(NUNIT).unwrap();
        assert_eq!((r.total, r.passed, r.failed, r.skipped, r.inconclusive), (3, 1, 1, 1, 0));
        assert_eq!(
            r.failures,
            [Failure { name: r#"Drift.Kaputt("a&b")"#.into(), message: "Expected: 3 <int>\n  But was:  2".into() }]
        );
    }

    #[test]
    fn nunit_randfaelle() {
        assert!(parse_nunit("").is_err());
        assert!(parse_nunit("<html>nope</html>").is_err());
        assert!(parse_nunit("<test-runs total=\"1\">").is_err());
        // Abgeschnittene Datei (Unity abgestuerzt): kein vollstaendiges Start-Tag.
        assert!(parse_nunit("<test-run total=\"3\" passed=").is_err());
        // Fehlende Attribute: 0, total aus der Summe.
        let r = parse_nunit(r#"<test-run passed="2" failed="1"></test-run>"#).unwrap();
        assert_eq!((r.total, r.passed, r.failed, r.skipped), (3, 2, 1, 0));
        let r = parse_nunit(r#"<test-run total="x" passed="-1"/>"#).unwrap();
        assert_eq!((r.total, r.passed), (0, 0));
        // Fehlschlag ohne <message>, als leeres Element und ohne fullname.
        let r = parse_nunit(
            r#"<test-run failed="2"><test-case name="A" result="Failed"/><test-case fullname="B" result="Failed"><failure></failure></test-case></test-run>"#,
        )
        .unwrap();
        assert_eq!(r.failures.iter().map(|f| (f.name.as_str(), f.message.as_str())).collect::<Vec<_>>(), [("A", ""), ("B", "")]);
        // Attribut-Namen muessen ganz passen: "name" nicht aus "fullname".
        assert_eq!(attr(r#"<test-case fullname="F.X" result="Failed">"#, "name"), None);
        assert_eq!(attr("<test-case\n\tname=\"Z\">", "name").as_deref(), Some("Z"));
    }

    #[test]
    fn liest_editor_status() {
        assert_eq!(parse_editor_status(""), Ok(None));
        assert_eq!(parse_editor_status(r#"{"status":"running"}"#), Ok(None));
        assert_eq!(parse_editor_status(r#"{"status":"compl"#), Ok(None));
        assert!(parse_editor_status(r#"{"status":"error","message":"Tests did not complete"}"#)
            .unwrap_err()
            .contains("Tests did not complete"));
        assert!(parse_editor_status(r#"{"status":"cancelled"}"#).unwrap_err().contains("cancelled"));

        // Form aus PipelineTestRunner.WriteCompletedResults.
        let done = r#"{"status":"completed","duration":0.4,
            "summary":{"total":3,"passed":1,"failed":1,"skipped":0,"inconclusive":1},
            "results":[{"FullName":"A.Ok","Status":"Passed","Duration":0.1,"Message":null,"StackTrace":null},
                       {"FullName":"A.Rot","Status":"Failed","Duration":0.1,"Message":"Expected 1","StackTrace":"at A"}]}"#;
        let r = parse_editor_status(done).unwrap().unwrap();
        assert_eq!((r.total, r.passed, r.failed, r.skipped, r.inconclusive), (3, 1, 1, 0, 1));
        assert_eq!(r.failures, [Failure { name: "A.Rot".into(), message: "Expected 1".into() }]);
        // Leerer Lauf (Filter trifft nichts).
        let r = parse_editor_status(r#"{"status":"completed","summary":{"total":0},"results":[]}"#).unwrap().unwrap();
        assert_eq!((r.total, r.failures.len()), (0, 0));
    }

    #[test]
    fn claude_md_abschnitt_nur_einmal() {
        let neu = with_section("").unwrap();
        assert!(neu.starts_with(MD_HEAD));
        assert!(MD_SECTION.lines().count() <= 10);
        let mit = with_section("# Projekt\n\nText\n\n\n").unwrap();
        assert!(mit.starts_with("# Projekt\n\nText\n\n## Unity-CLI"));
        assert_eq!(with_section(&mit), None);
        assert_eq!(with_section("# X\r\n\r\n## Unity-CLI\r\nalt\r\n"), None);
    }

    #[test]
    fn serialisiert_camel_case() {
        let r = TestReport { duration_secs: 1.5, report_path: "p".into(), ..Default::default() };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["durationSecs"], 1.5);
        assert_eq!(v["reportPath"], "p");
        let v = serde_json::to_value(UnityInfo::default()).unwrap();
        for k in ["version", "cli", "editorInstalled", "editorOpen", "pipeline", "connected", "skill"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn projekt_sperre_schliesst_aus() {
        let dir = tmp("busy");
        let a = busy(&dir).unwrap();
        assert!(busy(&dir).unwrap_err().contains("beschäftigt"));
        // Anderer Weg zum selben Ordner zaehlt auch.
        assert!(busy(&dir.join(".")).is_err());
        drop(a);
        assert!(busy(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn haengende_cli_wird_samt_kindern_beendet() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("haengt");
        let script = |name: &str, body: &str| {
            let f = dir.join(name);
            std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            f.to_string_lossy().into_owned()
        };
        let ok = script("ok", r#"echo '{"success":true,"data":7}'"#);
        assert_eq!(envelope(&unity(&ok, &dir, &[], SHORT).unwrap()).unwrap(), 7);

        // Enkel wie Unity unter `unity test`: muss mit weg, sonst haelt er die Pipe und laeuft verwaist weiter.
        let pidfile = dir.join("enkel.pid");
        let hang = script("hang", &format!("sleep 60 & echo $! > '{}'; wait", pidfile.display()));
        let t = Instant::now();
        let e = unity(&hang, &dir, &["x"], Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("beendet"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(10));
        let enkel = std::fs::read_to_string(&pidfile).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let alive = std::process::Command::new("kill").args(["-0", enkel.trim()]).status().unwrap().success();
        assert!(!alive, "Enkelprozess laeuft noch");
        assert!(lock(&RUNNING).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn negativfaelle_kein_panic() {
        // Kein Unity-Projekt: kein Absturz, informativ statt Fehler (version: None traegt das Signal).
        let dir = tmp("kein-unity");
        let info = info_blocking(&dir.to_string_lossy()).unwrap();
        assert_eq!(info.version, None);
        assert!(!info.editor_open && !info.pipeline && !info.skill);
        assert!(test_blocking(&dir.to_string_lossy(), "Foo").unwrap_err().contains("Testmodus"));
        assert!(info_blocking("/pfad/existiert/nicht/xyz").unwrap_err().contains("nicht gefunden"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Gegen eine echte Projektkopie: `UNITY_E2E_PROJECT=<kopie> cargo test e2e -- --ignored --nocapture`.
/// Oeffnet und schliesst (notfalls hart) den Editor dieser Kopie — nie auf das Original zeigen.
#[cfg(test)]
mod e2e {
    use super::*;

    fn kopie() -> Option<String> {
        std::env::var("UNITY_E2E_PROJECT").ok().filter(|p| !p.trim().is_empty())
    }

    /// Auch bei einem fehlgeschlagenen assert bleibt kein Editor offen.
    struct Zu(String);
    impl Drop for Zu {
        fn drop(&mut self) {
            let _ = close_blocking(&self.0, true);
        }
    }

    fn warte(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        false
    }

    fn gesamt(r: &TestReport) {
        assert!(r.total > 0, "{r:?}");
        assert_eq!(r.total, r.passed + r.failed + r.skipped + r.inconclusive, "{r:?}");
        assert_eq!(r.failures.len() as u32, r.failed, "{r:?}");
        assert!(Path::new(&r.report_path).is_file(), "{r:?}");
        assert!(r.duration_secs > 0.0);
    }

    #[test]
    #[ignore]
    fn ablauf_zu_offen_zu() {
        let Some(p) = kopie() else { return };
        let _zu = Zu(p.clone());
        close_blocking(&p, true).unwrap();

        let info = info_blocking(&p).unwrap();
        eprintln!("zu: {info:?}");
        assert!(info.cli && info.editor_installed && info.version.is_some(), "{info:?}");
        assert!(!info.editor_open && !info.connected, "{info:?}");

        for mode in ["EditMode", "PlayMode"] {
            let t = Instant::now();
            let r = test_blocking(&p, mode).unwrap();
            eprintln!("batch {mode} {:.1}s: {r:?}", t.elapsed().as_secs_f64());
            assert_eq!(r.via, "batchmode");
            assert!(r.report_path.ends_with(&format!("open-claude-{mode}.xml")));
            if mode == "EditMode" {
                gesamt(&r);
            }
        }
        assert!(test_blocking(&p, "Quatsch").is_err());

        // Zweimal: idempotent, Abschnitt nur einmal.
        eprintln!("setup:\n{}", setup_blocking(&p).unwrap());
        eprintln!("setup:\n{}", setup_blocking(&p).unwrap());
        let md = std::fs::read_to_string(Path::new(&p).join("CLAUDE.md")).unwrap();
        assert_eq!(md.matches(MD_HEAD).count(), 1);
        let info = info_blocking(&p).unwrap();
        assert!(info.skill, "{info:?}");

        // Kehrt erst zurueck, wenn der Editor gelistet ist; waehrenddessen ist das Projekt gesperrt.
        let q = p.clone();
        let t = Instant::now();
        let opening = std::thread::spawn(move || open_blocking(&q));
        std::thread::sleep(Duration::from_millis(500));
        assert!(test_blocking(&p, "EditMode").unwrap_err().contains("beschäftigt"));
        opening.join().unwrap().unwrap();
        eprintln!("open bis gelistet: {:.1}s", t.elapsed().as_secs_f64());
        assert!(info_blocking(&p).unwrap().editor_open, "Editor nach open nicht offen");
        // Zweites Oeffnen startet keinen zweiten Editor.
        open_blocking(&p).unwrap();
        let connected = info.pipeline && warte(600, || info_blocking(&p).is_ok_and(|i| i.connected));
        let info = info_blocking(&p).unwrap();
        eprintln!("offen: {info:?}");
        assert!(info.editor_open);
        assert_eq!(info.connected, connected);

        if connected {
            for mode in ["EditMode", "PlayMode"] {
                let r = test_blocking(&p, mode).unwrap();
                eprintln!("editor {mode}: {r:?}");
                assert_eq!(r.via, "editor");
                gesamt(&r);
            }
        } else {
            // Ohne erreichbare Pipeline: klarer Fehler statt Batch-Lauf gegen den offenen Editor.
            let e = test_blocking(&p, "EditMode").unwrap_err();
            eprintln!("offen, Pipeline nicht erreichbar (Editor-Weg ungeprueft): {e}");
            assert!(e.contains("Editor"), "{e}");
            // Ohne Pipeline gibt es keinen sanften Weg zu.
            assert!(close_blocking(&p, false).is_err());
        }

        close_blocking(&p, !connected).unwrap();
        assert!(warte(90, || info_blocking(&p).is_ok_and(|i| !i.editor_open)), "Editor noch offen");
        // Geschlossen ist idempotent.
        close_blocking(&p, false).unwrap();
    }
}
