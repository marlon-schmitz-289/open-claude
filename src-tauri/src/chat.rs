//! Chat-Ansicht: claude headless (`-p`, stream-json in beide Richtungen), je Session ein Prozess mit Pipes.
//! Jede stdout-Zeile geht roh als Event ans Frontend, das auch die JSON-Zeilen fuer stdin baut.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, Stdio};
use std::sync::Mutex;

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
fn command() -> std::process::Command {
    if cfg!(windows) {
        let mut cmd = crate::quiet("cmd.exe");
        cmd.args(["/d", "/c"]).args(CLAUDE);
        return cmd;
    }
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::pty::UNIX_SHELL.into());
    let mut cmd = crate::quiet(&sh);
    cmd.args(["-lic", &format!("exec {}", CLAUDE.join(" "))]);
    // Eigene Gruppe, damit kill_tree auch Tool-Prozesse von claude trifft.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd
}

#[tauri::command]
pub fn chat_open(app: AppHandle, chats: State<'_, Chats>, id: String, cwd: String) -> Result<(), String> {
    if !crate::pty::valid_id(&id) {
        return Err(format!("Ungueltige Chat-id: {id}"));
    }
    if !Path::new(&cwd).is_dir() {
        return Err(format!("Ordner nicht gefunden: {cwd}"));
    }
    if chats.lock().contains_key(&id) {
        return Err(format!("Chat laeuft schon: {id}"));
    }
    let mut cmd = command();
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
