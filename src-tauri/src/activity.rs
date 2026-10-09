// Aktivitaets-Panel: Workflows und Bilder der laufenden Claude-Session, nur aus dem, was Claude Code
// ohnehin unter ~/.claude schreibt. Undokumentierte Formate: fehlende Felder bleiben leer, nie Fehler.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::git::blocking;

#[derive(Serialize, Clone, Debug)]
pub struct Activity {
    session: Option<String>,
    runs: Vec<Run>,
    images: Vec<Img>,
    /// Session-Daten der eingebauten Mod (siehe panel_file), unveraendert durchgereicht.
    panel: Option<Value>,
    /// Aktive Plugin-Modi (caveman, ponytail, ...), siehe modes()
    modes: Vec<Mode>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Mode {
    name: String,
    level: String,
}

/// Plugins wie caveman/ponytail legen ~/.claude/.<name>-active mit der Stufe an und loeschen sie beim
/// Abschalten (ihre Statuszeilen lesen dieselbe Datei). Gilt fuer alle Sessions, nicht nur diese.
fn modes(c: &Path) -> Vec<Mode> {
    let mut out: Vec<Mode> = fs::read_dir(c)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_str()?.strip_prefix('.')?.strip_suffix("-active")?.to_string();
            let level = fs::read_to_string(e.path()).unwrap_or_default();
            (!name.is_empty()).then(|| Mode { name, level: trunc(level.trim(), 20) })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Datei, in die die Mod im Terminal `id` ihr Panel-Atom schreibt (Env OPEN_CLAUDE_PANEL).
pub fn panel_file(id: &str) -> PathBuf {
    std::env::temp_dir().join("open-claude-panel").join(format!("{id}.json"))
}

#[derive(Serialize, Clone, Debug)]
pub struct Run {
    id: String,
    name: String,
    /// running|completed|failed|killed|aborted
    status: String,
    started: u64,
    duration_ms: u64,
    tokens: u64,
    phases: Vec<String>,
    agents: Vec<Agent>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Agent {
    id: String,
    label: String,
    phase: String,
    /// running|done|error|aborted
    state: String,
    started: u64,
    last_at: u64,
    tokens: u64,
    tool_calls: u32,
    steps: Vec<Step>,
    text: Option<String>,
    result: Option<String>,
    /// Auftrag ohne Harness-Vorspann, gekuerzt
    prompt: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Step {
    at: u64,
    tool: String,
    summary: String,
}

/// Ein Eintrag der Agent-Zeitleiste (claude_agent_log).
#[derive(Serialize, Clone, Debug, Default)]
pub struct Entry {
    at: u64,
    /// prompt|text|tool
    kind: &'static str,
    tool: String,
    summary: String,
    /// Prompt/Text bzw. Tool-Eingabe, gekuerzt
    body: String,
    /// Ergebnis des Tool-Calls; None solange er laeuft
    output: Option<String>,
    error: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Img {
    id: usize,
    at: u64,
    kind: String,
    label: String,
}

#[derive(Clone)]
struct ImgRef {
    path: PathBuf,
    offset: u64,
    nth: usize,
}

/// Zwischenstand eines Agent-Transkripts, inkrementell fortgeschrieben.
#[derive(Default, Clone)]
struct Sum {
    tokens: u64,
    tool_calls: u32,
    started: u64,
    last_at: u64,
    steps: Vec<Step>,
    text: Option<String>,
    prompt: Option<String>,
}

#[derive(Default)]
struct State {
    offsets: HashMap<PathBuf, u64>,
    /// eigene Offsets, weil Agent-Transkripte auch fuer Bilder gescannt werden
    sum_offsets: HashMap<PathBuf, u64>,
    reads: HashMap<PathBuf, HashMap<String, String>>,
    hashes: HashSet<u64>,
    imgs: Vec<(ImgRef, Img)>,
    sums: HashMap<PathBuf, Sum>,
    done: HashMap<String, Run>,
}

/// Cache pro Session-ID: Tab-Wechsel liest nichts neu, Bild-ids bleiben je Session stabil.
type Cache = HashMap<String, State>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Default::default);

const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

pub(crate) fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| crate::skills::home().join(".claude"))
}

/// Pfad, wie claude ihn sieht: realpath mit echter Schreibweise (Dev -> dev, /tmp -> /private/tmp), Windows ohne "\\?\".
pub(crate) fn real(cwd: &str) -> String {
    fs::canonicalize(cwd).map_or_else(|_| cwd.into(), |p| p.to_string_lossy().trim_start_matches(r"\\?\").into())
}

pub(crate) fn slug(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// "2026-10-08T19:03:10.208Z" -> epoch ms (nur UTC-Z, wie Claude Code schreibt); sonst 0.
fn iso_ms(s: &str) -> u64 {
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) = (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19))
    else {
        return 0;
    };
    let frac = s.get(19..).and_then(|r| r.strip_prefix('.')).map_or(0, |r| {
        let digits: String = r.chars().take_while(char::is_ascii_digit).take(3).collect();
        format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
    });
    // Tage seit 1970 (Howard Hinnant, days_from_civil)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (((days * 24 + h) * 60 + mi) * 60 + sec) as u64 * 1000 + frac as u64
}

pub(crate) fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).chain(['…']).collect()
    }
}

pub(crate) fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn u64_of(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

/// Neue vollstaendige Zeilen ab dem gemerkten Offset; eine halbe letzte Zeile bleibt fuer den naechsten Scan.
fn new_lines(path: &Path, offsets: &mut HashMap<PathBuf, u64>) -> Vec<(u64, String)> {
    let Ok(mut f) = File::open(path) else { return vec![] };
    let len = f.metadata().map_or(0, |m| m.len());
    let off = offsets.entry(path.to_path_buf()).or_insert(0);
    if len < *off {
        *off = 0; // Datei neu geschrieben
    }
    if len == *off || f.seek(SeekFrom::Start(*off)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return vec![] };
    let mut out = Vec::new();
    let mut pos = 0;
    for line in buf[..end].split(|&b| b == b'\n') {
        out.push((*off + pos as u64, String::from_utf8_lossy(line).into_owned()));
        pos += line.len() + 1;
    }
    *off += end as u64 + 1;
    out
}

/// Bild-Quellen {type:image, source:{type:base64,...}} in Dokumentreihenfolge, mit umgebender tool_use_id.
fn images<'a>(v: &'a Value, tid: Option<&'a str>, out: &mut Vec<(&'a Value, Option<&'a str>)>) {
    match v {
        Value::Object(m) => {
            if str_of(v, "type") == "image" {
                if let Some(src) = m.get("source").filter(|s| str_of(s, "type") == "base64" && s.get("data").is_some_and(Value::is_string)) {
                    out.push((src, tid));
                    return;
                }
            }
            let tid = m.get("tool_use_id").and_then(Value::as_str).or(tid);
            for x in m.values() {
                images(x, tid, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| images(x, tid, out)),
        _ => {}
    }
}

fn content(v: &Value) -> &[Value] {
    v.pointer("/message/content").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn scan_images(st: &mut State, path: &Path) {
    for (offset, line) in new_lines(path, &mut st.offsets) {
        if line.contains("\"name\":\"Read\"") {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let map = st.reads.entry(path.to_path_buf()).or_default();
                for c in content(&v).iter().filter(|c| str_of(c, "type") == "tool_use" && str_of(c, "name") == "Read") {
                    if let Some(p) = c.pointer("/input/file_path").and_then(Value::as_str) {
                        map.insert(str_of(c, "id").to_string(), p.to_string());
                    }
                }
            }
        }
        if !line.contains("\"type\":\"image\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let mut found = Vec::new();
        images(&v, None, &mut found);
        let paste = v.get("imagePasteIds").or(v.pointer("/attachment/imagePasteIds")).and_then(Value::as_array);
        let at = iso_ms(str_of(&v, "timestamp"));
        for (nth, (src, tid)) in found.into_iter().enumerate() {
            let mut h = DefaultHasher::new();
            str_of(src, "data").hash(&mut h);
            if !st.hashes.insert(h.finish()) {
                continue;
            }
            let read = tid.and_then(|t| st.reads.get(path)?.get(t));
            let (kind, label) = match (paste, read) {
                (Some(ids), _) => ("paste", format!("Eingefügt #{}", ids.get(nth).and_then(Value::as_u64).unwrap_or(nth as u64 + 1))),
                (None, Some(p)) => ("read", p.clone()),
                _ => ("other", "Bild".to_string()),
            };
            let id = st.imgs.len();
            st.imgs.push((
                ImgRef { path: path.to_path_buf(), offset, nth },
                Img { id, at, kind: kind.into(), label },
            ));
        }
    }
}

fn step(c: &Value, at: u64) -> Step {
    let input = c.get("input").unwrap_or(&Value::Null);
    let summary = ["command", "file_path", "pattern", "url", "query", "description"]
        .iter()
        .find_map(|k| input.get(k).and_then(Value::as_str))
        .unwrap_or("");
    Step { at, tool: str_of(c, "name").to_string(), summary: trunc(summary.lines().next().unwrap_or(""), 120) }
}

const LOG_CHARS: usize = 4000;
const PROMPT_CHARS: usize = 20000;

/// Workflow-Agenten bekommen den Auftrag hinter einer Harness-Kopfzeile, jede Zeile um zwei Leerzeichen eingerueckt.
fn task(s: &str) -> String {
    let Some((_, body)) = s.strip_prefix("[Workflow harness").and_then(|r| r.split_once('\n')) else { return s.trim().into() };
    body.lines().map(|l| l.strip_prefix("  ").unwrap_or(l)).collect::<Vec<_>>().join("\n").trim().into()
}

/// Auftrag aus der ersten Transkript-Zeile {type:user, message:{content:"..."}}.
fn prompt_of(v: &Value) -> Option<String> {
    let t = v.pointer("/message/content").and_then(Value::as_str).filter(|_| str_of(v, "type") == "user")?;
    Some(trunc(&task(t), PROMPT_CHARS))
}

/// Nur die erste Zeile lesen: fertige Runs brauchen vom Transkript nur den Auftrag.
fn first_prompt(path: &Path) -> Option<String> {
    let mut line = String::new();
    BufReader::new(File::open(path).ok()?).read_line(&mut line).ok()?;
    prompt_of(&serde_json::from_str(&line).ok()?)
}

/// Ganzes Agent-Transkript als Zeitleiste: Auftrag, Texte, Tool-Calls samt Ergebnis.
// ponytail: liest bei jedem Aufruf die ganze Datei; inkrementell wie sum_agent erst wenn es bremst
fn agent_log(path: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut calls: HashMap<String, usize> = HashMap::new();
    for line in fs::read_to_string(path).unwrap_or_default().lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let at = iso_ms(str_of(&v, "timestamp"));
        match str_of(&v, "type") {
            "user" => {
                // Erste Nutzer-Nachricht als Text ist der Auftrag; spaetere Strings sind Harness-Rauschen.
                if let Some(t) = v.pointer("/message/content").and_then(Value::as_str) {
                    if out.is_empty() {
                        out.push(Entry { at, kind: "prompt", body: trunc(&task(t), LOG_CHARS), ..Default::default() });
                    }
                    continue;
                }
                for c in content(&v).iter().filter(|c| str_of(c, "type") == "tool_result") {
                    let Some(&i) = calls.get(str_of(c, "tool_use_id")) else { continue };
                    let text = match c.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(a)) => a.iter().map(|x| str_of(x, "text")).collect::<Vec<_>>().join("\n"),
                        _ => String::new(),
                    };
                    out[i].output = Some(trunc(text.trim_end(), LOG_CHARS));
                    out[i].error = c.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                }
            }
            "assistant" => {
                for c in content(&v) {
                    match str_of(c, "type") {
                        "text" if !str_of(c, "text").trim().is_empty() => {
                            out.push(Entry { at, kind: "text", body: trunc(str_of(c, "text"), LOG_CHARS), ..Default::default() })
                        }
                        "tool_use" => {
                            let s = step(c, at);
                            let input = c.get("input").map(|i| serde_json::to_string_pretty(i).unwrap_or_default()).unwrap_or_default();
                            calls.insert(str_of(c, "id").into(), out.len());
                            out.push(Entry { at, kind: "tool", tool: s.tool, summary: s.summary, body: trunc(&input, LOG_CHARS), ..Default::default() });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Pfadteile kommen vom Frontend: nur erwartete Zeichen, damit kein ../ durchrutscht.
#[tauri::command]
pub async fn claude_agent_log(cwd: String, sid: String, run: String, agent: String) -> Result<Vec<Entry>, String> {
    let ok = |s: &str, f: fn(&u8) -> bool| !s.is_empty() && s.bytes().all(|b| f(&b));
    let valid = ok(&sid, |b| b.is_ascii_hexdigit() || *b == b'-')
        && run.starts_with("wf_")
        && ok(&run, |b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        && ok(&agent, u8::is_ascii_alphanumeric);
    if !valid {
        return Err("Ungueltige Kennung".into());
    }
    blocking(move || {
        let dir = config_dir().join("projects").join(slug(&real(&cwd))).join(&sid).join("subagents/workflows").join(&run);
        Ok(agent_log(&dir.join(format!("agent-{agent}.jsonl"))))
    })
    .await
}

fn sum_agent(st: &mut State, path: &Path) -> Sum {
    let lines = new_lines(path, &mut st.sum_offsets);
    let s = st.sums.entry(path.to_path_buf()).or_default();
    for (off, line) in lines {
        if off == 0 {
            s.prompt = serde_json::from_str(&line).ok().as_ref().and_then(prompt_of);
        }
        if !line.contains("\"type\":\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if str_of(&v, "type") != "assistant" {
            continue;
        }
        let at = iso_ms(str_of(&v, "timestamp"));
        if s.started == 0 {
            s.started = at;
        }
        s.last_at = s.last_at.max(at);
        // Zeilen derselben message.id tragen dieselbe usage: letzte nehmen, nicht summieren.
        if let Some(u) = v.pointer("/message/usage") {
            s.tokens = u64_of(u, "input_tokens") + u64_of(u, "cache_read_input_tokens") + u64_of(u, "cache_creation_input_tokens");
        }
        for c in content(&v) {
            match str_of(c, "type") {
                "tool_use" => {
                    s.tool_calls += 1;
                    s.steps.push(step(c, at));
                    if s.steps.len() > 5 {
                        s.steps.remove(0);
                    }
                }
                "text" if !str_of(c, "text").trim().is_empty() => s.text = Some(trunc(str_of(c, "text"), 500)),
                _ => {}
            }
        }
    }
    s.clone()
}

/// Name aus <name>-<runId>.js, Phasen aus den title:-Zeilen des meta-Blocks.
fn script_meta(dir: &Path, run: &str) -> (Option<String>, Vec<String>) {
    let suffix = format!("-{run}.js");
    let Some(file) = fs::read_dir(dir).ok().and_then(|rd| {
        rd.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&suffix)))
    }) else {
        return (None, vec![]);
    };
    let name = file.file_name().and_then(|n| n.to_str()).map(|n| n.trim_end_matches(&suffix).to_string());
    (name, phases_of(&fs::read_to_string(&file).unwrap_or_default()))
}

fn phases_of(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in script.lines() {
        if line.contains("export const meta") {
            inside = true;
        } else if inside && line == "}" {
            break;
        }
        if !inside {
            continue;
        }
        let Some(rest) = line.split_once("title:").map(|x| x.1.trim_start()) else { continue };
        let Some(q) = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { continue };
        if let Some(t) = rest[1..].split(q).next() {
            out.push(t.to_string());
        }
    }
    out
}

/// dir = subagents/workflows/<id> mit den Agent-Transkripten (fuer den vollen Auftrag).
fn finished(v: &Value, id: &str, dir: &Path) -> Run {
    let arr = |k: &str| v.get(k).and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let agents = arr("workflowProgress")
        .iter()
        .filter(|a| str_of(a, "type") == "workflow_agent")
        .map(|a| {
            let started = u64_of(a, "startedAt");
            let last = u64_of(a, "lastProgressAt").max(started + u64_of(a, "durationMs"));
            let id = str_of(a, "agentId");
            let full = (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
                .then(|| first_prompt(&dir.join(format!("agent-{id}.jsonl"))))
                .flatten();
            Agent {
                id: id.into(),
                label: str_of(a, "label").into(),
                phase: str_of(a, "phaseTitle").into(),
                state: match str_of(a, "state") {
                    "" => "done",
                    s => s,
                }
                .into(),
                started,
                last_at: last,
                tokens: u64_of(a, "tokens"),
                tool_calls: u64_of(a, "toolCalls") as u32,
                steps: a
                    .get("lastToolName")
                    .and_then(Value::as_str)
                    .map(|t| Step { at: last, tool: t.into(), summary: str_of(a, "lastToolSummary").into() })
                    .into_iter()
                    .collect(),
                text: None,
                result: a.get("resultPreview").and_then(Value::as_str).map(Into::into),
                prompt: full.or_else(|| a.get("promptPreview").and_then(Value::as_str).map(Into::into)),
            }
        })
        .collect();
    Run {
        id: id.into(),
        name: match str_of(v, "workflowName") {
            "" => id,
            n => n,
        }
        .into(),
        status: str_of(v, "status").into(),
        started: u64_of(v, "startTime"),
        duration_ms: u64_of(v, "durationMs"),
        tokens: u64_of(v, "totalTokens"),
        phases: arr("phases").iter().map(|p| str_of(p, "title").to_string()).collect(),
        agents,
    }
}

/// Run aus journal.jsonl + Agent-Transkripten; alive = startedAt der lebenden Session mit dieser SID.
fn live(st: &mut State, dir: &Path, scripts: &Path, id: &str, alive: Option<u64>, now: u64) -> Run {
    let journal = dir.join("journal.jsonl");
    let created = fs::metadata(&journal).and_then(|m| m.created()).map(ms).ok();
    let running = alive.is_some_and(|s| created.is_none_or(|c| s <= c));
    let mut agents: Vec<(String, Agent)> = Vec::new();
    for line in fs::read_to_string(&journal).unwrap_or_default().lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let key = str_of(&v, "key");
        let state = match str_of(&v, "type") {
            "started" => "running",
            "result" => "done",
            "failed" => "error",
            _ => continue,
        };
        let pos = agents.iter().position(|(k, _)| k == key);
        let a = match pos {
            Some(i) => &mut agents[i].1,
            None => {
                agents.push((key.into(), Agent::default()));
                &mut agents.last_mut().unwrap().1
            }
        };
        a.state = state.into();
        for (f, k) in [(&mut a.id, "agentId"), (&mut a.label, "label"), (&mut a.phase, "phase")] {
            if let Some(s) = v.get(k).and_then(Value::as_str) {
                *f = s.into();
            }
        }
        if state != "running" {
            let r = v.get("result").or(v.get("error"));
            a.result = r.map(|r| trunc(&r.as_str().map_or_else(|| r.to_string(), Into::into), 2000));
        }
    }
    let (name, mut phases) = script_meta(scripts, id);
    let mut agents: Vec<Agent> = agents.into_iter().map(|x| x.1).collect();
    for a in &mut agents {
        if !a.id.is_empty() && a.id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            let s = sum_agent(st, &dir.join(format!("agent-{}.jsonl", a.id)));
            (a.started, a.last_at, a.tokens, a.tool_calls, a.steps, a.text, a.prompt) = (s.started, s.last_at, s.tokens, s.tool_calls, s.steps, s.text, s.prompt);
        }
        if !running && a.state == "running" {
            a.state = "aborted".into();
        }
        if !a.phase.is_empty() && !phases.contains(&a.phase) {
            phases.push(a.phase.clone());
        }
    }
    let started = created.or(agents.iter().map(|a| a.started).filter(|&s| s > 0).min()).unwrap_or(0);
    let end = if running { now } else { agents.iter().map(|a| a.last_at).max().unwrap_or(started) };
    Run {
        id: id.into(),
        name: name.unwrap_or_else(|| id.into()),
        status: if running { "running" } else { "aborted" }.into(),
        started,
        duration_ms: end.saturating_sub(started),
        tokens: agents.iter().map(|a| a.tokens).sum(),
        phases,
        agents,
    }
}

/// (sid, startedAt der lebenden Session) — sessions/*.json mit passendem cwd, sonst neueste Transkript-Datei.
// ponytail: zwei claude im selben cwd -> neueste gewinnt; exakt waere Kindprozess der PTY-PID
fn session(c: &Path, cwd: &str, p: &Path) -> Option<(String, Option<u64>)> {
    let live = fs::read_dir(c.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str::<Value>(&fs::read_to_string(e.path()).ok()?).ok())
        .filter(|v| str_of(v, "cwd") == cwd)
        .max_by_key(|v| u64_of(v, "updatedAt"));
    let (sid, alive) = match live {
        Some(v) => (str_of(&v, "sessionId").to_string(), Some(u64_of(&v, "startedAt"))),
        None => {
            let newest = fs::read_dir(p)
                .ok()?
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
                .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
            (newest.path().file_stem()?.to_str()?.to_string(), None)
        }
    };
    (!sid.is_empty() && sid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')).then_some((sid, alive))
}

fn activity_in(cache: &mut Cache, c: &Path, cwd: &str, now: u64) -> Activity {
    let p = c.join("projects").join(slug(cwd));
    let Some((sid, alive)) = session(c, cwd, &p) else {
        return Activity { session: None, runs: vec![], images: vec![], panel: None, modes: vec![] };
    };
    // ponytail: max. 4 Sessions im Cache, dann alles verwerfen; LRU erst wenn das stoert
    if cache.len() >= 4 && !cache.contains_key(&sid) {
        cache.clear();
    }
    let st = cache.entry(sid.clone()).or_default();
    let base = p.join(&sid);

    // Bilder
    scan_images(st, &p.join(format!("{sid}.jsonl")));
    let subs: Vec<PathBuf> = walkdir::WalkDir::new(base.join("subagents"))
        .into_iter()
        .flatten()
        .map(|e| e.into_path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("agent-") && n.ends_with(".jsonl")))
        .collect();
    for f in &subs {
        scan_images(st, f);
    }

    // Workflows
    let mut runs = Vec::new();
    for e in fs::read_dir(base.join("subagents/workflows")).into_iter().flatten().flatten() {
        let Some(id) = e.file_name().to_str().filter(|n| n.starts_with("wf_")).map(String::from) else { continue };
        if let Some(r) = st.done.get(&id) {
            runs.push(r.clone());
            continue;
        }
        let json = base.join("workflows").join(format!("{id}.json"));
        if let Some(v) = fs::read_to_string(&json).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
            let r = finished(&v, &id, &e.path());
            st.done.insert(id, r.clone());
            runs.push(r);
        } else {
            runs.push(live(st, &e.path(), &base.join("workflows/scripts"), &id, alive, now));
        }
    }
    let rank = |s: &str| match s {
        "running" => 0,
        "error" | "failed" => 1,
        "aborted" | "killed" => 2,
        _ => 3,
    };
    runs.sort_by_key(|r| (r.status != "running", std::cmp::Reverse(r.started)));
    runs.truncate(10);
    for r in &mut runs {
        r.agents.sort_by_key(|a| rank(&a.state));
    }
    Activity { session: Some(sid), runs, images: st.imgs.iter().map(|x| x.1.clone()).collect(), panel: None, modes: vec![] }
}

#[tauri::command]
pub async fn claude_activity(cwd: String, id: String) -> Result<Activity, String> {
    blocking(move || {
        let mut a = activity_in(&mut *CACHE.lock().map_err(|e| e.to_string())?, &config_dir(), &real(&cwd), now_ms());
        // Halb geschriebene Datei: None, das Frontend behaelt den letzten Wert.
        a.modes = modes(&config_dir());
        if crate::pty::valid_id(&id) {
            a.panel = fs::read_to_string(panel_file(&id)).ok().and_then(|t| serde_json::from_str(&t).ok());
        }
        Ok(a)
    })
    .await
}

fn image_at(r: &ImgRef) -> Result<String, String> {
    let mut f = File::open(&r.path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(r.offset)).map_err(|e| e.to_string())?;
    let mut line = Vec::new();
    BufReader::new(f.take(64 << 20)).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(line.trim_ascii_end()).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    images(&v, None, &mut found);
    let src = found.get(r.nth).ok_or("Bild nicht gefunden")?.0;
    let mt = str_of(src, "media_type");
    if !IMAGE_TYPES.contains(&mt) {
        return Err(format!("Bildtyp nicht erlaubt: {mt}"));
    }
    Ok(format!("data:{mt};base64,{}", str_of(src, "data")))
}

#[tauri::command]
pub async fn claude_image(sid: String, id: usize) -> Result<String, String> {
    blocking(move || {
        let cache = CACHE.lock().map_err(|e| e.to_string())?;
        let r = cache.get(&sid).and_then(|st| st.imgs.get(id)).map(|x| x.0.clone()).ok_or("Unbekanntes Bild")?;
        drop(cache);
        image_at(&r)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SID: &str = "0000aaaa-1111-2222-3333-444455556666";

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ocui-act-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new().append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    /// Config-Ordner mit Projekt fuer /w/p; alive = sessions-Datei mit startedAt 0.
    fn setup(name: &str, alive: bool) -> (PathBuf, PathBuf) {
        let c = tmp(name);
        if alive {
            put(&c.join("sessions/1.json"), &format!(r#"{{"sessionId":"{SID}","cwd":"/w/p","startedAt":0,"updatedAt":1}}"#));
        }
        let base = c.join("projects/-w-p").join(SID);
        put(&c.join(format!("projects/-w-p/{SID}.jsonl")), "");
        (c, base)
    }

    #[test]
    fn slug_ersetzt() {
        assert_eq!(slug("/Users/x/dev/a.b"), "-Users-x-dev-a-b");
        assert_eq!(iso_ms("1970-01-01T00:00:01.5Z"), 1500);
        assert_eq!(iso_ms("2026-10-08T19:03:10.208Z"), 1791486190208);
    }

    const JOURNAL: &str = concat!(
        r#"{"type":"launched"}"#,
        "\n",
        r#"{"type":"started","key":"k1","agentId":"a1","label":"eins","phase":"Research"}"#,
        "\n",
        r#"{"type":"started","key":"k2","agentId":"a2","label":"zwei","phase":"Research"}"#,
        "\n",
        r#"{"type":"started","key":"k3","agentId":"a3","label":"drei","phase":"Extra"}"#,
        "\n",
        r#"{"type":"result","key":"k1","agentId":"a1","result":"fertig"}"#,
        "\n",
        r#"{"type":"failed","key":"k2","agentId":"a2","error":"kaputt"}"#,
        "\n"
    );

    #[test]
    fn journal_zustaende() {
        for alive in [true, false] {
            let (c, base) = setup(&format!("j{alive}"), alive);
            put(&base.join("subagents/workflows/wf_1/journal.jsonl"), JOURNAL);
            put(&base.join("workflows/scripts/mein-flow-wf_1.js"), "export const meta = {\n  phases: [{ title: 'Research' }],\n}\n");
            let a = activity_in(&mut Cache::new(), &c, "/w/p", now_ms());
            let r = &a.runs[0];
            assert_eq!(r.name, "mein-flow");
            assert_eq!(r.phases, ["Research", "Extra"]);
            let st = |l: &str| r.agents.iter().find(|a| a.label == l).unwrap().state.clone();
            assert_eq!(st("eins"), "done");
            assert_eq!(st("zwei"), "error");
            assert_eq!(r.agents.iter().find(|a| a.label == "zwei").unwrap().result.as_deref(), Some("kaputt"));
            if alive {
                assert_eq!((st("drei").as_str(), r.status.as_str()), ("running", "running"));
                assert_eq!(r.agents[0].label, "drei");
            } else {
                assert_eq!((st("drei").as_str(), r.status.as_str()), ("aborted", "aborted"));
            }
        }
    }

    #[test]
    fn fertige_json_gewinnt() {
        let (c, base) = setup("json", true);
        put(&base.join("subagents/workflows/wf_2/journal.jsonl"), JOURNAL);
        put(
            &base.join("workflows/wf_2.json"),
            r#"{"workflowName":"x","status":"completed","durationMs":5,"totalTokens":575163,"startTime":9,"phases":[{"title":"Research"}],
               "workflowProgress":[{"type":"workflow_phase"},{"type":"workflow_agent","label":"eins","state":"done","tokens":3,"lastToolName":"Bash","lastToolSummary":"ls","agentId":"a1","promptPreview":"kurz"},
                 {"type":"workflow_agent","label":"zwei","agentId":"a2","promptPreview":"nur Vorschau"}]}"#,
        );
        put(&base.join("subagents/workflows/wf_2/agent-a1.jsonl"), "{\"type\":\"user\",\"message\":{\"content\":\"voller Auftrag\"}}\n{kaputt");
        let a = activity_in(&mut Cache::new(), &c, "/w/p", 0);
        let r = &a.runs[0];
        assert_eq!((r.status.as_str(), r.tokens, r.name.as_str()), ("completed", 575163, "x"));
        assert_eq!(r.agents.len(), 2);
        assert_eq!(r.agents[0].steps[0].summary, "ls");
        let p = |i: usize| r.agents[i].prompt.as_deref();
        assert_eq!((p(0), p(1)), (Some("voller Auftrag"), Some("nur Vorschau")));
    }

    #[test]
    fn agent_transkript() {
        let d = tmp("agent");
        let f = d.join("agent-a1.jsonl");
        let line = |content: &str, input: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"1970-01-01T00:00:0{input}.000Z","message":{{"id":"m1","usage":{{"input_tokens":{input},"cache_read_input_tokens":10,"cache_creation_input_tokens":100}},"content":[{content}]}}}}"#
            ) + "\n"
        };
        put(
            &f,
            &[
                r#"{"type":"user","message":{"content":"\"type\":\"assistant\""}}"#.to_string() + "\n",
                line(r#"{"type":"text","text":"hallo"}"#, 1),
                line(r#"{"type":"tool_use","name":"Bash","input":{"command":"ls -la\nzwei"}}"#, 2),
                line(r#"{"type":"tool_use","name":"Read","input":{"file_path":"/a.png"}}"#, 3),
            ]
            .concat(),
        );
        let s = sum_agent(&mut State::default(), &f);
        assert_eq!(s.tokens, 113);
        assert_eq!(s.tool_calls, 2);
        assert_eq!((s.started, s.last_at), (1000, 3000));
        assert_eq!(s.steps[0].summary, "ls -la");
        assert_eq!(s.steps[1].summary, "/a.png");
        assert_eq!(s.text.as_deref(), Some("hallo"));
        assert_eq!(s.prompt.as_deref(), Some("\"type\":\"assistant\""));
    }

    #[test]
    fn auftrag_ohne_vorspann() {
        let t = "[Workflow harness — computed task] Kein User. The computed task text follows:\n  \n  Projekt: x\n    - eingerueckt\n  \n  Ende\n";
        assert_eq!(task(t), "Projekt: x\n  - eingerueckt\n\nEnde");
        assert_eq!(task("  normaler Prompt\n  zwei\n"), "normaler Prompt\n  zwei");
        let v = serde_json::json!({"type":"user","message":{"content":t}});
        assert_eq!(prompt_of(&v).unwrap(), "Projekt: x\n  - eingerueckt\n\nEnde");
    }

    #[test]
    fn plugin_modi() {
        let c = tmp("modi");
        put(&c.join(".ponytail-active"), "ultra\n");
        put(&c.join(".caveman-active"), "");
        put(&c.join(".foo"), "x");
        fs::create_dir_all(c.join(".dir-active")).unwrap();
        let m = |n: &str, l: &str| Mode { name: n.into(), level: l.into() };
        assert_eq!(modes(&c), [m("caveman", ""), m("ponytail", "ultra")]);
    }

    #[test]
    fn agent_zeitleiste() {
        let f = tmp("log").join("agent-a1.jsonl");
        put(
            &f,
            concat!(
                r#"{"type":"user","message":{"content":"Auftrag"}}"#, "\n",
                r#"{"type":"assistant","timestamp":"1970-01-01T00:00:01.000Z","message":{"content":[{"type":"text","text":"los"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"a\nb\n"}],"is_error":true}]}}"#, "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/x"}}]}}"#, "\n",
                r#"{"type":"user","message":{"content":"spaeter"}}"#, "\n",
            ),
        );
        let log = agent_log(&f);
        let kinds: Vec<_> = log.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, ["prompt", "text", "tool", "tool"]);
        assert_eq!((log[0].body.as_str(), log[1].at), ("Auftrag", 1000));
        assert_eq!((log[2].summary.as_str(), log[2].output.as_deref(), log[2].error), ("ls", Some("a\nb"), true));
        assert!(log[2].body.contains("\"command\": \"ls\""));
        // t2 laeuft noch
        assert_eq!(log[3].output, None);
    }

    #[test]
    fn agent_log_kennungen() {
        for (sid, run, agent) in [("../x", "wf_1", "a1"), (SID, "../wf", "a1"), (SID, "wf_1", "a/1"), (SID, "x", "a1")] {
            let r = tauri::async_runtime::block_on(claude_agent_log("/w".into(), sid.into(), run.into(), agent.into()));
            assert!(r.is_err());
        }
    }

    #[test]
    fn phasen_aus_meta() {
        let script = "export const meta = {\n  name: 'x',\n  phases: [\n    { title: 'Research', detail: 'a' },\n    { title: \"Design\", detail: 'b' },\n    { title: `Implement` },\n    { title: 'Review' },\n    { title: 'Fix' },\n  ],\n}\n\nphase({ title: 'Nope' })\n";
        assert_eq!(phases_of(script), ["Research", "Design", "Implement", "Review", "Fix"]);
    }

    fn img(data: &str) -> String {
        format!(r#"{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}}"#)
    }

    #[test]
    fn bilder_scan() {
        let (c, base) = setup("img", false);
        let main = c.join(format!("projects/-w-p/{SID}.jsonl"));
        put(
            &main,
            &[
                format!(r#"{{"type":"user","timestamp":"1970-01-01T00:00:01Z","imagePasteIds":[2],"message":{{"content":[{{"type":"text","text":"[Image #2]"}},{}]}}}}"#, img("QUJD")),
                // Dublette als queued_command
                format!(r#"{{"type":"attachment","attachment":{{"type":"queued_command","imagePasteIds":[2],"prompt":[{}]}}}}"#, img("QUJD")),
                r#"{"type":"user","message":{"content":[{"type":"text","text":"nur \"type\":\"image\" als Text"}]}}"#.into(),
                String::new(),
            ]
            .join("\n"),
        );
        let agent = base.join("subagents/agent-a9.jsonl");
        put(
            &agent,
            &(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/x/shot.png"}}]}}"#.to_string()
                + "\n"
                + &format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":[{}]}}]}}}}"#, img("REVG"))),
        );
        let mut cache = Cache::new();
        let a = activity_in(&mut cache, &c, "/w/p", 0);
        assert_eq!(a.images.len(), 1, "halbe letzte Zeile noch nicht");
        assert_eq!((a.images[0].kind.as_str(), a.images[0].label.as_str(), a.images[0].at), ("paste", "Eingefügt #2", 1000));
        append(&agent, "\n");
        let a = activity_in(&mut cache, &c, "/w/p", 0);
        assert_eq!(a.images.len(), 2);
        assert_eq!((a.images[1].kind.as_str(), a.images[1].label.as_str()), ("read", "/x/shot.png"));
        let st = &cache[SID];
        assert_eq!(image_at(&st.imgs[1].0).unwrap(), "data:image/png;base64,REVG");
        assert_eq!(image_at(&st.imgs[0].0).unwrap(), "data:image/png;base64,QUJD");
    }

    #[test]
    fn bild_id_unbekannt() {
        let r = tauri::async_runtime::block_on(claude_image(SID.into(), 999));
        assert!(r.is_err());
    }
}
