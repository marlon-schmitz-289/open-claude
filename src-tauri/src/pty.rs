//! Terminal in der App: je Session ein PTY (ConPTY unter Windows), Output roh als Event ans Frontend.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    // Nur Run-Leiste: pid der Shell, um beim Stoppen den ganzen Prozessbaum zu treffen (npm -> node).
    tree: Option<u32>,
}

impl Session {
    fn kill(&mut self) {
        // Vor dem Kill des Elternprozesses, sonst findet taskkill die Kinder nicht mehr.
        #[cfg(windows)]
        if let Some(pid) = self.tree {
            let _ = crate::quiet("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).status();
        }
        // Gruppe der Shell (nach setsid ist pid auch die Gruppe) und die im Vordergrund des PTY: eine interaktive
        // Shell gibt dem Kommando eine eigene. Erst SIGHUP, und wer das ignoriert, bekommt nach 1 s SIGKILL.
        // ponytail: `kill` als Prozess statt libc::kill, spart die direkte Abhaengigkeit; blockiert hoechstens 1 s.
        #[cfg(unix)]
        if let Some(pid) = self.tree {
            let mut groups = vec![pid as i32];
            groups.extend(self.master.process_group_leader());
            groups.retain(|g| *g > 1);
            groups.dedup();
            let signal = |sig: &str| {
                let sent = |g: &i32| {
                    std::process::Command::new("kill")
                        .args([sig, "--", &format!("-{g}")])
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
                };
                groups.iter().filter(|g| sent(g)).count()
            };
            signal("-HUP");
            for _ in 0..10 {
                if signal("-0") == 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            signal("-KILL");
        }
        let _ = self.killer.kill();
    }
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
            s.kill();
            // Drop schliesst die Pseudo-Konsole und reisst angehaengte Prozesse mit.
        }
    }
}

/// id landet im Eventnamen, darum nur ein sicherer Zeichensatz.
pub(crate) fn valid_id(id: &str) -> bool {
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
pub(crate) const UNIX_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };

/// Welche Shell `claude` startet. Unix: $SHELL als Login-Shell. Windows: zuerst die eigene
/// ocui-sh (`own`, liegt neben der App), sonst Git-Bash, pwsh, cmd.
/// `settings`: claude bekommt `--settings` aus $OPEN_CLAUDE_SETTINGS (Shell expandiert, kein Quoting).
fn shell(
    windows: bool,
    own: &str,
    settings: bool,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> (String, Vec<String>) {
    let claude = if settings { "claude --settings \"$OPEN_CLAUDE_SETTINGS\"" } else { "claude" };
    if !windows {
        let sh = env("SHELL")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| UNIX_SHELL.into());
        // Login-Shell fuer PATH; nach claude bleibt dieselbe Shell offen (auch zsh/fish).
        let rest = format!("{claude}; exec {sh} -li");
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
        return (b, vec!["-lic".into(), format!("{claude}; exec bash -li")]);
    }

    if let Some(pwsh) = on_path("pwsh.exe") {
        let claude = if settings { "claude --settings $env:OPEN_CLAUDE_SETTINGS" } else { "claude" };
        return (pwsh, vec!["-NoExit".into(), "-Command".into(), claude.into()]);
    }
    // ponytail: cmd ohne --settings (Pfad mit Leerzeichen nicht sauber quotebar), Theme bleibt dann wie gewaehlt.
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
    theme: String,
) -> Result<(), String> {
    // ocui-sh liegt neben der Exe: dev target/debug, installiert packt tauri build alle Bins
    // des Pakets mit ein. Fehlt es, greift die Fallback-Shell.
    let own = std::env::current_exe()
        .map(|e| e.with_file_name(format!("ocui-sh{}", std::env::consts::EXE_SUFFIX)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let settings = theme_settings(&app, &theme).map(|p| p.to_string_lossy().into_owned());
    let (program, args) =
        shell(cfg!(windows), &own, settings.is_some(), |k| std::env::var(k).ok(), |p| Path::new(p).is_file());
    let dirs = mod_dirs(&app);
    let panel = crate::activity::panel_file(&id).to_string_lossy().into_owned();
    let mut env: Vec<_> = dirs.iter().map(|d| ("CLAUDE_CODE_PLUGIN_DIRS", d.as_str())).collect();
    // Die Mod schreibt ihre Session-Daten dorthin, das App-Panel liest sie.
    env.push(("OPEN_CLAUDE_PANEL", &panel));
    if let Some(s) = &settings {
        env.push(("OPEN_CLAUDE_SETTINGS", s));
    }
    // Fullscreen-Layout: nur dort dockt das /panel-Pane.
    env.push(("CLAUDE_CODE_NO_FLICKER", "1"));
    // ConPTY laesst im Alt-Screen sonst Zeichenreste stehen.
    if cfg!(windows) {
        env.push(("CLAUDE_CODE_ALT_SCREEN_FULL_REPAINT", "1"));
    }
    spawn(app, &ptys, id, &cwd, &program, args, cols, rows, &env, false)
}

/// Hat der User ein Open-Claude-Theme gewaehlt, das nicht zum App-Modus passt: Settings-Datei, die es
/// fuer diese Session umstellt. Per `--settings`, weil /config (`$.config.set`) keine Custom-Themes annimmt.
fn theme_settings(app: &AppHandle, mode: &str) -> Option<PathBuf> {
    let user = std::fs::read_to_string(crate::activity::config_dir().join("settings.json")).ok()?;
    let user: serde_json::Value = serde_json::from_str(&user).ok()?;
    let want = theme_for(user.get("theme")?.as_str()?, mode)?;
    let p = app.path().app_data_dir().ok()?.join(format!("theme-{mode}.json"));
    let text = serde_json::json!({ "theme": want }).to_string();
    if std::fs::read_to_string(&p).ok().as_deref() != Some(&text) {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::write(&p, text).ok()?;
    }
    Some(p)
}

/// Nur zwischen den eigenen Themes wechseln; wer etwas anderes gewaehlt hat (auch auto), bleibt dabei.
fn theme_for(cur: &str, mode: &str) -> Option<&'static str> {
    let want = match mode {
        "light" => "custom:open-claude:open-claude-light",
        "dark" => "custom:open-claude:open-claude",
        _ => return None,
    };
    (cur.starts_with("custom:open-claude:open-claude") && cur != want).then_some(want)
}

/// Eingebaute Mod. Debug: Quellordner direkt (Hot-Reload). Release: eingebettet und nach
/// app_data_dir geschrieben, weil claude in jeden Mod-Ordner .claude-plugin/types/ schreibt
/// und das signierte .app-Bundle dabei kaputt ginge.
pub(crate) fn builtin_mod(app: &AppHandle) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("mods/open-claude"));
    }
    let dir = app.path().app_data_dir().ok()?.join("mods/open-claude");
    const FILES: [(&str, &str); 8] = [
        (".claude-plugin/plugin.json", include_str!("../mods/open-claude/.claude-plugin/plugin.json")),
        ("hooks/hooks.json", include_str!("../mods/open-claude/hooks/hooks.json")),
        ("skills/smart-tests/SKILL.md", include_str!("../mods/open-claude/skills/smart-tests/SKILL.md")),
        ("skills/workflow-templates/SKILL.md", include_str!("../mods/open-claude/skills/workflow-templates/SKILL.md")),
        ("hooks/register.tsx", include_str!("../mods/open-claude/hooks/register.tsx")),
        ("types/index.d.ts", include_str!("../mods/open-claude/types/index.d.ts")),
        ("themes/open-claude.json", include_str!("../mods/open-claude/themes/open-claude.json")),
        ("themes/open-claude-light.json", include_str!("../mods/open-claude/themes/open-claude-light.json")),
    ];
    for (rel, text) in FILES {
        let p = dir.join(rel);
        // Nur bei Aenderung schreiben, sonst laedt claude die Mod bei jedem Terminal neu.
        if std::fs::read_to_string(&p).ok().as_deref() != Some(text) {
            std::fs::create_dir_all(p.parent()?).ok()?;
            std::fs::write(&p, text).ok()?;
        }
    }
    Some(dir)
}

/// CLAUDE_CODE_PLUGIN_DIRS fuer claude: eingebaute und installierte Mods.
pub(crate) fn mod_dirs(app: &AppHandle) -> Option<String> {
    let mods = crate::skills::mods(&crate::skills::mods_dir()).into_iter().map(|m| m.path.into());
    plugin_dirs(std::env::var("CLAUDE_CODE_PLUGIN_DIRS").ok(), builtin_mod(app).into_iter().chain(mods))
}

/// Mod-Ordner hinter einen schon gesetzten Wert haengen; ohne Mods None (Env bleibt wie geerbt).
fn plugin_dirs(prev: Option<String>, mods: impl Iterator<Item = std::path::PathBuf>) -> Option<String> {
    let mut mods = mods.peekable();
    mods.peek()?;
    let prev = prev.unwrap_or_default();
    let all = std::env::split_paths(&prev).filter(|p| !p.as_os_str().is_empty()).chain(mods);
    std::env::join_paths(all).ok().map(|s| s.to_string_lossy().into_owned())
}

/// Was die Run-Leiste startet. Spiegel von `Target` in run.logic.ts.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Npm { pm: String, script: String },
    Dotnet { watch: bool },
    Cargo { bin: Option<String> },
}

/// Script- bzw. Binary-Name, der ohne Quoting in die Shell darf.
fn name_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || ":_.-".contains(c))
}

/// Kommando der Run-Leiste in cwd. Nur feste Woerter und gepruefte Namen erreichen die Shell, darum bleibt es
/// ein Shell-String (Login-Shell wie das Terminal, auch fish). Prueft, dass das Ziel im Ordner existiert.
fn run_command(
    windows: bool,
    t: &Target,
    cwd: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(String, Vec<String>), String> {
    let words: Vec<&str> = match t {
        Target::Npm { pm, script } => {
            if !["npm", "pnpm", "yarn", "bun"].contains(&pm.as_str()) || !name_ok(script) {
                return Err(format!("Nicht erlaubt: {pm} run {script}"));
            }
            let pkg = std::fs::read_to_string(cwd.join("package.json"))
                .map_err(|_| format!("Keine package.json in {}", cwd.display()))?;
            let has = serde_json::from_str::<serde_json::Value>(&pkg)
                .ok()
                .is_some_and(|v| v.get("scripts").and_then(|s| s.get(script)).is_some());
            if !has {
                return Err(format!("Script „{script}“ fehlt in package.json"));
            }
            vec![pm, "run", script]
        }
        Target::Dotnet { watch } => {
            // Ohne --project: dotnet nimmt die eine Projektdatei im Ordner, kein Pfad in der Shell.
            let n = std::fs::read_dir(cwd).map_or(0, |d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "csproj" || x == "fsproj"))
                    .count()
            });
            if n != 1 {
                return Err(format!("Genau eine .csproj/.fsproj erwartet in {}", cwd.display()));
            }
            vec!["dotnet", if *watch { "watch" } else { "run" }]
        }
        Target::Cargo { bin } => {
            if !cwd.join("Cargo.toml").is_file() {
                return Err(format!("Keine Cargo.toml in {}", cwd.display()));
            }
            if !bin.as_deref().is_none_or(name_ok) {
                return Err(format!("Nicht erlaubt: --bin {}", bin.as_deref().unwrap_or_default()));
            }
            let mut w = vec!["cargo", "run"];
            if let Some(b) = bin {
                w.extend(["--bin", b]);
            }
            w
        }
    };
    if windows {
        // cmd loest die .cmd-Shims (npm.cmd, pnpm.cmd) auf.
        let args = ["/d", "/c"].into_iter().chain(words).map(String::from).collect();
        return Ok(("cmd.exe".into(), args));
    }
    // Login-Shell wie das Terminal: PATH samt nvm/fnm. Ohne exec: npm darf auch eine Shell-Funktion sein (lazy nvm).
    let sh = env("SHELL")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| UNIX_SHELL.into());
    Ok((sh, vec!["-lic".into(), words.join(" ")]))
}

/// Projekt starten (dir = Ordner des Ziels, "" = Projektordner) in einem PTY. Output und Ende kommen wie beim
/// Terminal als pty:<id> / pty-exit:<id> (Exit-Code oder null), gestoppt wird mit pty_close.
/// Fuehrt Code aus dem Projekt aus: das Frontend ruft das nur auf Klick oder Taste.
#[tauri::command]
pub fn run_start(
    app: AppHandle,
    ptys: State<'_, Ptys>,
    id: String,
    repo: String,
    dir: String,
    target: Target,
) -> Result<(), String> {
    let cwd = if dir.is_empty() { PathBuf::from(&repo) } else { crate::files::within(&repo, &dir)? };
    let (program, args) = run_command(cfg!(windows), &target, &cwd, |k| std::env::var(k).ok())?;
    let env: &[(&str, &str)] = match target {
        // Vite, CRA & Co. oeffnen sonst zusaetzlich den Browser.
        Target::Npm { .. } => &[("BROWSER", "none")],
        Target::Dotnet { .. } => &[("DOTNET_CLI_UI_LANGUAGE", "en")],
        Target::Cargo { .. } => &[],
    };
    spawn(app, &ptys, id, &cwd.to_string_lossy(), &program, args, 120, 30, env, true)
}

/// Env-Marker einer umgebenden Claude-Session, die ein gestartetes claude nicht erben darf (siehe spawn).
pub(crate) const MARKERS: [&str; 12] = [
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
];

/// Programm in einem neuen PTY starten, Output und Ende als Events melden. tree: siehe Session.
#[allow(clippy::too_many_arguments)]
fn spawn(
    app: AppHandle,
    ptys: &Ptys,
    id: String,
    cwd: &str,
    program: &str,
    args: Vec<String>,
    cols: u16,
    rows: u16,
    env: &[(&str, &str)],
    tree: bool,
) -> Result<(), String> {
    if !valid_id(&id) {
        return Err(format!("Ungueltige Terminal-id: {id}"));
    }
    if !Path::new(cwd).is_dir() {
        return Err(format!("Ordner nicht gefunden: {cwd}"));
    }
    if ptys.lock().contains_key(&id) {
        return Err(format!("Terminal laeuft schon: {id}"));
    }

    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("PTY konnte nicht geoeffnet werden: {e}"))?;

    let (exe, pre) = crate::native(program);
    let mut cmd = CommandBuilder::new(exe);
    cmd.args(pre);
    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (k, v) in env {
        cmd.env(k, v);
    }

    // Aus einer Claude-Session gestartet (z. B. tauri dev) erben wir deren Marker;
    // das claude im Terminal haelt sich dann fuer einen Sub-Agent: keine Farben, kein Transcript.
    // NO_COLOR setzt Claude Code fuer seine Tool-Shells; ein Terminal soll trotzdem Farben zeigen.
    for key in MARKERS {
        cmd.env_remove(key);
    }

    // Sonst laufen claude und git im Terminal gegen unsere gebuendelten Bibliotheken.
    crate::unbundle_env(|key, val| match val {
        Some(v) => cmd.env(key, v),
        None => cmd.env_remove(key),
    });

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
        Session {
            gen,
            writer,
            master: pair.master,
            killer: child.clone_killer(),
            tree: child.process_id().filter(|_| tree),
        },
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
        let code = child.wait().ok().map(|s| s.exit_code());
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
        let _ = app.emit(&format!("pty-exit:{id}"), code);
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

/// Async: kill() wartet bis zu 1 s, ein Sync-Command liefe auf dem Main-Thread und froere die UI ein.
#[tauri::command]
pub async fn pty_close(ptys: State<'_, Ptys>, id: String) -> Result<(), String> {
    // Auch wenn der Prozess schon selbst endete (dann fehlt der Eintrag).
    if valid_id(&id) {
        let _ = std::fs::remove_file(crate::activity::panel_file(&id));
    }
    let Some(mut s) = ptys.lock().remove(&id) else { return Ok(()) };
    crate::git::blocking(move || {
        s.kill();
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{plugin_dirs, run_command, shell, theme_for, valid_id, Target, UNIX_SHELL};

    #[test]
    fn plugin_dirs_haengt_an() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let m = |v: &[&str]| v.iter().map(std::path::PathBuf::from).collect::<Vec<_>>().into_iter();
        assert_eq!(plugin_dirs(Some("/x".into()), m(&[])), None);
        assert_eq!(plugin_dirs(None, m(&["/a", "/b"])), Some(format!("/a{sep}/b")));
        assert_eq!(plugin_dirs(Some(String::new()), m(&["/a"])), Some("/a".into()));
        assert_eq!(plugin_dirs(Some(format!("/x{sep}/y")), m(&["/a"])), Some(format!("/x{sep}/y{sep}/a")));
    }

    fn tmp(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ocui-run-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (f, text) in files {
            std::fs::write(d.join(f), text).unwrap();
        }
        d
    }

    #[test]
    fn run_kommando_nur_feste_werte() {
        let env = |k: &str| (k == "SHELL").then(|| "/bin/fish".to_string());
        let npm = |pm: &str, s: &str| Target::Npm { pm: pm.into(), script: s.into() };
        let d = tmp("npm", &[("package.json", r#"{"scripts":{"dev":"vite","tauri:dev":"x","-x":"y"}}"#)]);
        let (p, a) = run_command(false, &npm("pnpm", "dev"), &d, env).unwrap();
        assert_eq!(p, "/bin/fish");
        assert_eq!(a, ["-lic", "pnpm run dev"]);
        assert_eq!(run_command(false, &npm("npm", "tauri:dev"), &d, |_| None).unwrap().0, UNIX_SHELL);
        let (p, a) = run_command(true, &npm("yarn", "dev"), &d, env).unwrap();
        assert_eq!(p, "cmd.exe");
        assert_eq!(a, ["/d", "/c", "yarn", "run", "dev"]);
        assert!(run_command(false, &npm("npm; rm -rf ~", "dev"), &d, env).is_err());
        // Fehlt in package.json, Sonderzeichen, Option.
        for s in ["build", "dev & calc", "$(x)", "-x", ""] {
            assert!(run_command(true, &npm("npm", s), &d, env).is_err(), "{s}");
        }
        assert!(run_command(false, &npm("npm", "dev"), &d.join("fehlt"), env).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn run_dotnet_und_cargo() {
        let env = |_: &str| None;
        let one = tmp("dn1", &[("App.csproj", "")]);
        let (_, a) = run_command(false, &Target::Dotnet { watch: true }, &one, env).unwrap();
        assert_eq!(a, ["-lic", "dotnet watch"]);
        let (_, a) = run_command(true, &Target::Dotnet { watch: false }, &one, env).unwrap();
        assert_eq!(a, ["/d", "/c", "dotnet", "run"]);
        let two = tmp("dn2", &[("A.csproj", ""), ("B.fsproj", "")]);
        assert!(run_command(false, &Target::Dotnet { watch: false }, &two, env).is_err());
        let none = tmp("dn0", &[]);
        assert!(run_command(false, &Target::Dotnet { watch: false }, &none, env).is_err());

        let c = tmp("cargo", &[("Cargo.toml", "[package]")]);
        let cargo = |b: Option<&str>| Target::Cargo { bin: b.map(String::from) };
        assert_eq!(run_command(false, &cargo(None), &c, env).unwrap().1, ["-lic", "cargo run"]);
        assert_eq!(run_command(true, &cargo(Some("ocui-sh")), &c, env).unwrap().1, ["/d", "/c", "cargo", "run", "--bin", "ocui-sh"]);
        assert!(run_command(false, &cargo(Some("a b")), &c, env).is_err());
        assert!(run_command(false, &cargo(None), &none, env).is_err());
        for d in [one, two, none, c] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    fn run(windows: bool, env: &[(&str, &str)], files: &[&str]) -> (String, Vec<String>) {
        shell(
            windows,
            OWN,
            false,
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
    fn settings_nur_mit_flag() {
        let s = |w| shell(w, OWN, true, |k| (k == "SHELL").then(|| "/bin/fish".into()), |p| p == "C:\\ps\\pwsh.exe" || p == OWN);
        assert_eq!(s(false).1, ["-lic", "claude --settings \"$OPEN_CLAUDE_SETTINGS\"; exec /bin/fish -li"]);
        // ocui-sh liest die Env selbst.
        assert_eq!(s(true).0, OWN);
    }

    #[test]
    fn theme_nur_zwischen_eigenen() {
        assert_eq!(theme_for("custom:open-claude:open-claude", "light"), Some("custom:open-claude:open-claude-light"));
        assert_eq!(theme_for("custom:open-claude:open-claude-light", "dark"), Some("custom:open-claude:open-claude"));
        assert_eq!(theme_for("custom:open-claude:open-claude", "dark"), None);
        assert_eq!(theme_for("auto", "light"), None);
        assert_eq!(theme_for("dark", "light"), None);
        assert_eq!(theme_for("custom:open-claude:open-claude", "system"), None);
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
