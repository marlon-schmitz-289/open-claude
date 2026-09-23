//! Terminal in der App: je Session ein PTY (ConPTY unter Windows), Output roh als Event ans Frontend.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, Manager, State};

struct Session {
    // Unterscheidet eine neue Session mit gleicher id von der alten, deren Waiter noch auslaeuft.
    gen: u64,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    // Das Child selbst gehoert dem Waiter-Thread (wait() blockiert).
    killer: Box<dyn ChildKiller + Send + Sync>,
}

// ponytail: ein Lock fuer alle Sessions, pro-Session-Locks falls viele Terminals parallel schreiben.
#[derive(Default)]
pub struct Ptys(Mutex<HashMap<String, Session>>);

static NEXT_GEN: AtomicU64 = AtomicU64::new(0);

impl Ptys {
    // Threads duerfen nicht paniken (panic = "abort"), also Poisoning schlucken.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Beim Beenden: sonst ueberleben Shell und claude die App.
    pub fn close_all(&self) {
        let sessions: Vec<Session> = self.lock().drain().map(|(_, s)| s).collect();
        for mut s in sessions {
            let _ = s.killer.kill();
            // Drop schliesst die Pseudo-Konsole und reisst angehaengte Prozesse mit.
        }
    }
}

/// id landet im Eventnamen, darum nur ein sicherer Zeichensatz.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Pfade als Strings, damit die Windows-Logik auch auf macOS testbar bleibt.
fn join(dir: &str, file: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), file)
}

fn parent(dir: &str) -> Option<&str> {
    dir.trim_end_matches(['\\', '/'])
        .rsplit_once(['\\', '/'])
        .map(|(p, _)| p)
}

/// Fallback ohne $SHELL: Standard-Shell des Systems.
const UNIX_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };

/// Welche Shell `claude` startet. Unix: $SHELL als Login-Shell. Windows: zuerst die eigene
/// ocui-sh (`own`, liegt neben der App), sonst Git-Bash, pwsh, cmd.
fn shell(
    windows: bool,
    own: &str,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> (String, Vec<String>) {
    if !windows {
        let sh = env("SHELL")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| UNIX_SHELL.into());
        // Login-Shell fuer PATH; nach claude bleibt dieselbe Shell offen (auch zsh/fish).
        let rest = format!("claude; exec {sh} -li");
        return (sh, vec!["-lic".into(), rest]);
    }
    // ocui-sh (fuer Git-Bash gebaut) startet claude selbst, danach ihre REPL.
    if exists(own) {
        return (own.into(), vec![]);
    }

    let path = env("PATH").unwrap_or_default();
    let on_path = |exe: &str| {
        path.split(';')
            .filter(|d| !d.trim().is_empty())
            .map(|d| join(d, exe))
            .find(|p| exists(p))
    };

    let mut bash = Vec::new();
    if let Some(pf) = env("ProgramFiles") {
        bash.push(join(&pf, "Git\\bin\\bash.exe"));
    }
    if let Some(la) = env("LOCALAPPDATA") {
        bash.push(join(&la, "Programs\\Git\\bin\\bash.exe"));
    }
    // <Git>\cmd\git.exe oder <Git>\mingw64\bin\git.exe -> <Git>\bin\bash.exe
    if let Some(git) = on_path("git.exe") {
        let dir = parent(&git);
        let up1 = dir.and_then(parent);
        let up2 = up1.and_then(parent);
        bash.extend([up1, up2].into_iter().flatten().map(|g| join(g, "bin\\bash.exe")));
    }
    if let Some(b) = bash.into_iter().find(|b| exists(b)) {
        return (b, vec!["-lic".into(), "claude; exec bash -li".into()]);
    }

    if let Some(pwsh) = on_path("pwsh.exe") {
        return (pwsh, vec!["-NoExit".into(), "-Command".into(), "claude".into()]);
    }
    ("cmd.exe".into(), vec!["/k".into(), "claude".into()])
}

#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    ptys: State<'_, Ptys>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    if !valid_id(&id) {
        return Err(format!("Ungueltige Terminal-id: {id}"));
    }
    if !Path::new(&cwd).is_dir() {
        return Err(format!("Ordner nicht gefunden: {cwd}"));
    }
    if ptys.lock().contains_key(&id) {
        return Err(format!("Terminal laeuft schon: {id}"));
    }

    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("PTY konnte nicht geoeffnet werden: {e}"))?;

    // ocui-sh liegt neben der Exe: dev target/debug, installiert packt tauri build alle Bins
    // des Pakets mit ein. Fehlt es, greift die Fallback-Shell.
    let own = std::env::current_exe()
        .map(|e| e.with_file_name(format!("ocui-sh{}", std::env::consts::EXE_SUFFIX)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (program, args) =
        shell(cfg!(windows), &own, |k| std::env::var(k).ok(), |p| Path::new(p).is_file());
    let (exe, pre) = crate::native(&program);
    let mut cmd = CommandBuilder::new(exe);
    cmd.args(pre);
    cmd.args(args);
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    // Aus einer Claude-Session gestartet (z. B. tauri dev) erben wir deren Marker;
    // das claude im Terminal haelt sich dann fuer einen Sub-Agent: keine Farben, kein Transcript.
    // NO_COLOR setzt Claude Code fuer seine Tool-Shells; ein Terminal soll trotzdem Farben zeigen.
    for key in [
        "NO_COLOR",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDECODE",
        "AI_AGENT",
        "CLAUDE_PID",
        "CLAUDE_EFFORT",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
    ] {
        cmd.env_remove(key);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Shell konnte nicht gestartet werden: {e}"))?;
    // Unix: sonst haelt die App selbst den Slave offen und der Reader sieht nie EOF.
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| format!("PTY-Fehler: {e}"))?;
    let writer = pair.master.take_writer().map_err(|e| format!("PTY-Fehler: {e}"))?;

    let gen = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
    ptys.lock().insert(
        id.clone(),
        Session { gen, writer, master: pair.master, killer: child.clone_killer() },
    );

    let out = app.clone();
    let data = format!("pty:{id}");
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                // Roh als Bytes: read() kann mitten in einer UTF-8-Sequenz enden.
                Ok(n) => {
                    let _ = out.emit(&data, buf[..n].to_vec());
                }
            }
        }
    });

    // Exit am Prozess erkennen, nicht am Reader: ConPTY schliesst die Pipe erst bei
    // ClosePseudoConsole (Drop des Masters). wait() raeumt unter Unix auch den Zombie ab.
    std::thread::spawn(move || {
        let _ = child.wait();
        let Some(ptys) = app.try_state::<Ptys>() else { return };
        let mut map = ptys.lock();
        // Nach pty_close (oder bei neuer Session gleicher id) kein Exit melden.
        if map.get(&id).is_none_or(|s| s.gen != gen) {
            return;
        }
        let s = map.remove(&id);
        drop(map);
        // Master ausserhalb des Locks droppen, ClosePseudoConsole kann blockieren.
        drop(s);
        let _ = app.emit(&format!("pty-exit:{id}"), ());
    });
    Ok(())
}

#[tauri::command]
pub fn pty_write(ptys: State<'_, Ptys>, id: String, data: String) -> Result<(), String> {
    let mut map = ptys.lock();
    let s = map.get_mut(&id).ok_or_else(|| format!("Kein Terminal: {id}"))?;
    s.writer
        .write_all(data.as_bytes())
        .and_then(|_| s.writer.flush())
        .map_err(|e| format!("Schreiben fehlgeschlagen: {e}"))
}

#[tauri::command]
pub fn pty_resize(ptys: State<'_, Ptys>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let map = ptys.lock();
    let s = map.get(&id).ok_or_else(|| format!("Kein Terminal: {id}"))?;
    s.master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("Resize fehlgeschlagen: {e}"))
}

#[tauri::command]
pub fn pty_close(ptys: State<'_, Ptys>, id: String) -> Result<(), String> {
    let Some(mut s) = ptys.lock().remove(&id) else { return Ok(()) };
    // ponytail: unter Unix nur SIGHUP ohne SIGKILL-Nachschlag, reicht fuer bash/zsh.
    let _ = s.killer.kill();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{shell, valid_id, UNIX_SHELL};

    fn run(windows: bool, env: &[(&str, &str)], files: &[&str]) -> (String, Vec<String>) {
        shell(
            windows,
            OWN,
            |k| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()),
            |p| files.contains(&p),
        )
    }

    const WIN_ENV: &[(&str, &str)] = &[
        ("ProgramFiles", "C:\\Program Files"),
        ("LOCALAPPDATA", "C:\\Users\\m\\AppData\\Local"),
        ("PATH", "C:\\Windows;D:\\Tools\\Git\\cmd;C:\\ps"),
    ];

    const OWN: &str = "C:\\app\\ocui-sh.exe";

    #[test]
    fn eigene_shell_hat_vorrang() {
        let bash = "C:\\Program Files\\Git\\bin\\bash.exe";
        assert_eq!(run(true, WIN_ENV, &[OWN, bash]), (OWN.to_string(), vec![]));
        // Unix: ocui-sh ist fuer Git-Bash gebaut, direkt die Login-Shell.
        assert_eq!(run(false, &[("SHELL", "/bin/zsh")], &[OWN]).0, "/bin/zsh");
    }

    #[test]
    fn windows_bevorzugt_git_bash_in_reihenfolge() {
        let pf = "C:\\Program Files\\Git\\bin\\bash.exe";
        let la = "C:\\Users\\m\\AppData\\Local\\Programs\\Git\\bin\\bash.exe";
        let git = "D:\\Tools\\Git\\cmd\\git.exe";
        let via_path = "D:\\Tools\\Git\\bin\\bash.exe";

        let (p, a) = run(true, WIN_ENV, &[pf, la, git, via_path]);
        assert_eq!(p, pf);
        assert_eq!(a, ["-lic", "claude; exec bash -li"]);
        assert_eq!(run(true, WIN_ENV, &[la, git, via_path]).0, la);
        assert_eq!(run(true, WIN_ENV, &[git, via_path]).0, via_path);
        // git.exe unter mingw64\bin -> zwei Ebenen hoch.
        let env = [("PATH", "E:\\Git\\mingw64\\bin")];
        let files = ["E:\\Git\\mingw64\\bin\\git.exe", "E:\\Git\\bin\\bash.exe"];
        assert_eq!(run(true, &env, &files).0, "E:\\Git\\bin\\bash.exe");
    }

    #[test]
    fn windows_faellt_auf_pwsh_dann_cmd_zurueck() {
        let (p, a) = run(true, WIN_ENV, &["C:\\ps\\pwsh.exe"]);
        assert_eq!(p, "C:\\ps\\pwsh.exe");
        assert_eq!(a, ["-NoExit", "-Command", "claude"]);
        // git ohne bash daneben zaehlt nicht.
        let (p, a) = run(true, WIN_ENV, &["D:\\Tools\\Git\\cmd\\git.exe"]);
        assert_eq!(p, "cmd.exe");
        assert_eq!(a, ["/k", "claude"]);
        assert_eq!(run(true, &[], &[]).0, "cmd.exe");
    }

    #[test]
    fn unix_nimmt_shell_sonst_bash() {
        let (p, a) = run(false, &[("SHELL", "/bin/zsh")], &[]);
        assert_eq!(p, "/bin/zsh");
        assert_eq!(a, ["-lic", "claude; exec /bin/zsh -li"]);
        assert_eq!(run(false, &[], &[]).0, UNIX_SHELL);
        assert_eq!(run(false, &[("SHELL", " ")], &[]).0, UNIX_SHELL);
    }

    #[test]
    fn id_nur_sichere_zeichen() {
        assert!(valid_id("repo_1-a"));
        assert!(!valid_id(""));
        assert!(!valid_id("a:b"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id("ä"));
    }
}
