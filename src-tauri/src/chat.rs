//! Chat-Ansicht: claude headless (`-p`, stream-json in beide Richtungen), je Session ein Prozess mit Pipes.
//! Jede stdout-Zeile geht roh als Event ans Frontend, das auch die JSON-Zeilen fuer stdin baut.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Stdio};
use std::sync::Mutex;

use crate::activity::str_of;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

struct Chat {
    stdin: ChildStdin,
    pid: u32,
}

// ponytail: ein Lock fuer alle Chats wie bei den PTYs.
#[derive(Default)]
pub struct Chats(Mutex<HashMap<String, Chat>>);

impl Chats {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Chat>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Beim Beenden: sonst ueberlebt claude die App.
    pub fn close_all(&self) {
        for (_, c) in self.lock().drain() {
            crate::kill_tree(c.pid);
        }
    }
}

/// `--permission-prompt-tool stdio`: Rechte-Abfragen kommen als control_request, das Frontend antwortet.
const CLAUDE: [&str; 9] = [
    "claude",
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
];

/// Unix ueber die Login-Shell wie im Terminal (PATH aus .zprofile/.zshrc), Windows ueber cmd (findet claude.cmd/.exe).
/// `resume` ist per valid_id geprueft und damit shell-sicher.
fn command(resume: Option<&str>) -> std::process::Command {
    let mut args: Vec<&str> = CLAUDE.to_vec();
    if let Some(sid) = resume {
        args.extend(["--resume", sid]);
    }
    if cfg!(windows) {
        let mut cmd = crate::quiet("cmd.exe");
        cmd.args(["/d", "/c"]).args(args);
        return cmd;
    }
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::pty::UNIX_SHELL.into());
    let mut cmd = crate::quiet(&sh);
    cmd.args(["-lic", &format!("exec {}", args.join(" "))]);
    // Eigene Gruppe, damit kill_tree auch Tool-Prozesse von claude trifft.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd
}

#[tauri::command]
pub fn chat_open(app: AppHandle, chats: State<'_, Chats>, id: String, cwd: String, resume: Option<String>) -> Result<(), String> {
    if !crate::pty::valid_id(&id) {
        return Err(format!("Ungueltige Chat-id: {id}"));
    }
    if resume.as_deref().is_some_and(|r| !crate::pty::valid_id(r)) {
        return Err("Ungueltige Session-id".into());
    }
    if !Path::new(&cwd).is_dir() {
        return Err(format!("Ordner nicht gefunden: {cwd}"));
    }
    if chats.lock().contains_key(&id) {
        return Err(format!("Chat laeuft schon: {id}"));
    }
    let mut cmd = command(resume.as_deref());
    cmd.current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in crate::pty::MARKERS {
        cmd.env_remove(key);
    }
    if let Some(d) = crate::pty::mod_dirs(&app) {
        cmd.env("CLAUDE_CODE_PLUGIN_DIRS", d);
    }
    // Die Mod schreibt ihre Session-Daten dorthin, das App-Panel liest sie.
    cmd.env("OPEN_CLAUDE_PANEL", crate::activity::panel_file(&id));
    let mut child = cmd.spawn().map_err(|e| format!("claude konnte nicht gestartet werden: {e}"))?;
    let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        return Err("claude: Pipes fehlen".into());
    };
    chats.lock().insert(id.clone(), Chat { stdin, pid: child.id() });

    let pipe = |r: Box<dyn std::io::Read + Send>, event: String, app: AppHandle| {
        std::thread::spawn(move || {
            for line in BufReader::new(r).lines().map_while(Result::ok) {
                let _ = app.emit(&event, line);
            }
        })
    };
    pipe(Box::new(stdout), format!("chat:{id}"), app.clone());
    pipe(Box::new(stderr), format!("chat-err:{id}"), app.clone());
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        if let Some(chats) = app.try_state::<Chats>() {
            chats.lock().remove(&id);
        }
        let _ = app.emit(&format!("chat-exit:{id}"), code);
    });
    Ok(())
}

/// Eine JSON-Zeile an claude (Nachricht, Rechte-Antwort, Abbruch).
#[tauri::command]
pub fn chat_send(chats: State<'_, Chats>, id: String, line: String) -> Result<(), String> {
    let mut map = chats.lock();
    let c = map.get_mut(&id).ok_or_else(|| format!("Kein Chat: {id}"))?;
    writeln!(c.stdin, "{line}")
        .and_then(|_| c.stdin.flush())
        .map_err(|e| format!("Senden fehlgeschlagen: {e}"))
}

#[tauri::command]
pub async fn chat_close(chats: State<'_, Chats>, id: String) -> Result<(), String> {
    if crate::pty::valid_id(&id) {
        let _ = std::fs::remove_file(crate::activity::panel_file(&id));
    }
    let Some(c) = chats.lock().remove(&id) else { return Ok(()) };
    crate::git::blocking(move || {
        crate::kill_tree(c.pid);
        Ok(())
    })
    .await
}

/// Projektordner von claude.
// ponytail: Slugs > 200 Zeichen (Hash-Suffix) nicht nachgebaut, dann leere Liste
fn project_dir(cwd: &str) -> PathBuf {
    crate::activity::config_dir().join("projects").join(crate::activity::slug(&crate::activity::real(cwd)))
}

/// Text eines User-Prompts (String oder erster Text-Block).
fn prompt(v: &Value) -> Option<&str> {
    let c = v.get("message")?.get("content")?;
    c.as_str().or_else(|| c.as_array()?.iter().find(|b| str_of(b, "type") == "text").map(|b| str_of(b, "text")))
}

/// Sichtbarer Verlauf: user/assistant ohne Sub-Agents, Meta, Compact-Zusammenfassung und Command-Echos.
fn keep(v: &Value) -> bool {
    let flag = |k| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    matches!(str_of(v, "type"), "user" | "assistant")
        && !flag("isSidechain")
        && !flag("isMeta")
        && !flag("isCompactSummary")
        && !v.pointer("/message/content").and_then(Value::as_str).is_some_and(|s| s.starts_with("<command-name>") || s.starts_with("<local-command"))
}

/// Titel wie in claudes /resume: custom-title > ai-title > last-prompt > erster Prompt.
fn title(text: &str) -> String {
    let mut t: [String; 4] = Default::default();
    for v in text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        let (i, s) = match str_of(&v, "type") {
            "custom-title" => (0, str_of(&v, "customTitle")),
            "ai-title" => (1, str_of(&v, "aiTitle")),
            "last-prompt" => (2, str_of(&v, "lastPrompt")),
            "user" if t[3].is_empty() && keep(&v) => (3, prompt(&v).unwrap_or("")),
            _ => continue,
        };
        if !s.trim().is_empty() {
            t[i] = s.trim().to_string();
        }
    }
    let s = t.into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    crate::activity::trunc(s.lines().next().unwrap_or(""), 80)
}

// ponytail: Dateireihenfolge statt parentUuid-Kette; verworfene Rewind-Zweige erscheinen mit, Kette erst wenn das stoert
fn history(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| serde_json::from_str::<Value>(l).is_ok_and(|v| keep(&v)))
        .map(String::from)
        .collect()
}

#[derive(Serialize)]
pub struct Past {
    id: String,
    title: String,
    modified: u64,
}

/// Die 20 neuesten Sessions mit Titel, neueste zuerst.
fn past_in(dir: &Path) -> Vec<Past> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
        .into_iter()
        .filter_map(|(t, p)| {
            let title = title(&std::fs::read_to_string(&p).ok()?);
            let id = p.file_stem()?.to_string_lossy().into_owned();
            (!title.is_empty() && crate::pty::valid_id(&id)).then(|| Past { id, title, modified: crate::activity::ms(t) })
        })
        .take(20)
        .collect()
}

#[tauri::command]
pub async fn chat_sessions(cwd: String) -> Result<Vec<Past>, String> {
    crate::git::blocking(move || Ok(past_in(&project_dir(&cwd)))).await
}

/// Roh-Zeilen im stream-json-Format, das Frontend rendert sie wie Live-Ausgabe.
#[tauri::command]
pub async fn chat_history(cwd: String, sid: String) -> Result<Vec<String>, String> {
    if !crate::pty::valid_id(&sid) {
        return Err("Ungueltige Session-id".into());
    }
    crate::git::blocking(move || {
        std::fs::read_to_string(project_dir(&cwd).join(format!("{sid}.jsonl")))
            .map(|t| history(&t))
            .map_err(|e| format!("Verlauf nicht lesbar: {e}"))
    })
    .await
}

/// Bildtyp nach Endung; nur was die API annimmt.
fn image_type(path: &str) -> Option<&'static str> {
    match Path::new(path).extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Reingezogenes Bild als data:-URL; verkleinert wird im Frontend.
#[tauri::command]
pub async fn chat_image(path: String) -> Result<String, String> {
    use base64::Engine;
    let mt = image_type(&path).ok_or("Kein Bild (png, jpg, gif, webp)")?;
    crate::git::blocking(move || {
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 50 << 20 {
            return Err("Bild groesser als 50 MB".into());
        }
        let b = std::fs::read(&path).map_err(|e| e.to_string())?;
        Ok(format!("data:{mt};base64,{}", base64::engine::general_purpose::STANDARD.encode(b)))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bildtyp_nach_endung() {
        assert_eq!(image_type("/a/B.JPG"), Some("image/jpeg"));
        assert_eq!(image_type("x.webp"), Some("image/webp"));
        assert_eq!(image_type("x.svg"), None);
        assert_eq!(image_type("png"), None);
    }

    #[test]
    fn titel_vorrang() {
        let lp = r#"{"type":"last-prompt","lastPrompt":"letzter"}"#;
        let ai = |t: &str| format!(r#"{{"type":"ai-title","aiTitle":"{t}"}}"#);
        let ct = r#"{"type":"custom-title","customTitle":"eigen"}"#;
        let first = r#"{"type":"user","message":{"role":"user","content":"erster"}}"#;
        assert_eq!(title(&[lp, &ai("a"), ct, &ai("b")].join("\n")), "eigen");
        assert_eq!(title(&[lp, &ai("a"), &ai("b")].join("\n")), "b");
        assert_eq!(title(&[first, lp].join("\n")), "letzter");
        assert_eq!(title(first), "erster");
        assert_eq!(title(r#"{"type":"last-prompt"}"#), "");
    }

    #[test]
    fn verlauf_ohne_meta_und_commands() {
        let lines = [
            r#"{"type":"queue-operation"}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"meta"}}"#,
            r#"{"type":"user","message":{"content":"<command-name>/context</command-name>"}}"#,
            r#"{"type":"user","message":{"content":"<local-command-stdout>x</local-command-stdout>"}}"#,
            r#"{"type":"user","message":{"content":"hallo"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t"}]}}"#,
        ];
        let h = history(&lines.join("\n"));
        assert_eq!(h, lines[4..].iter().filter(|l| !l.contains("isSidechain")).map(|s| s.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn sessions_neueste_zuerst() {
        let d = std::env::temp_dir().join(format!("ocui-chat-past-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let put = |n: &str, t: &str| std::fs::write(d.join(n), t).unwrap();
        put("alt.jsonl", r#"{"type":"ai-title","aiTitle":"Alt"}"#);
        std::thread::sleep(std::time::Duration::from_millis(20));
        put("neu.jsonl", r#"{"type":"ai-title","aiTitle":"Neu"}"#);
        put("leer.txt", "");
        put("ohne.jsonl", r#"{"type":"queue-operation"}"#);
        let p = past_in(&d);
        assert_eq!(p.iter().map(|p| (p.id.as_str(), p.title.as_str())).collect::<Vec<_>>(), [("neu", "Neu"), ("alt", "Alt")]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
