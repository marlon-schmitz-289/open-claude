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

/// Fallback ohne $SHELL: Standard-Shell des Systems.
pub(crate) const UNIX_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };

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

/// Build-Profil der Run-Leiste: release haengt --release (cargo) bzw. -c Release (dotnet) an, args kommen hinten dran.
#[derive(serde::Deserialize, Default)]
pub struct Profile {
    #[serde(default)]
    release: bool,
    #[serde(default)]
    args: Vec<String>,
}

/// Zusatzargument eines Profils: Flags und Werte ohne Leerzeichen, Quotes oder Shell-Zeichen.
fn arg_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.:=/@+,-".contains(c))
}

/// Script- bzw. Binary-Name, der ohne Quoting in die Shell darf.
fn name_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || ":_.-".contains(c))
}

/// Kommando der Run-Leiste in cwd. Nur feste Woerter und gepruefte Namen erreichen die Shell, darum bleibt es
/// ein Shell-String (Login-Shell wie das Terminal, auch fish). Prueft, dass das Ziel im Ordner existiert.
fn run_command(
    p: &Profile,
    windows: bool,
    t: &Target,
    cwd: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(String, Vec<String>), String> {
    let mut words: Vec<&str> = match t {
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
    if p.release {
        match t {
            Target::Cargo { .. } => words.push("--release"),
            Target::Dotnet { .. } => words.extend(["-c", "Release"]),
            // npm kennt kein Release, das Script ist das Profil.
            Target::Npm { .. } => {}
        }
    }
    if let Some(a) = p.args.iter().find(|a| !arg_ok(a)) {
        return Err(format!("Nicht erlaubtes Argument im Profil: {a}"));
    }
    words.extend(p.args.iter().map(String::as_str));
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
    profile: Option<Profile>,
) -> Result<(), String> {
    let cwd = if dir.is_empty() { PathBuf::from(&repo) } else { crate::files::within(&repo, &dir)? };
    let (program, args) = run_command(&profile.unwrap_or_default(), cfg!(windows), &target, &cwd, |k| std::env::var(k).ok())?;
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
    use super::{plugin_dirs, run_command, valid_id, Profile, Target, UNIX_SHELL};

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
        let (p, a) = run_command(&Profile::default(), false, &npm("pnpm", "dev"), &d, env).unwrap();
        assert_eq!(p, "/bin/fish");
        assert_eq!(a, ["-lic", "pnpm run dev"]);
        assert_eq!(run_command(&Profile::default(), false, &npm("npm", "tauri:dev"), &d, |_| None).unwrap().0, UNIX_SHELL);
        let (p, a) = run_command(&Profile::default(), true, &npm("yarn", "dev"), &d, env).unwrap();
        assert_eq!(p, "cmd.exe");
        assert_eq!(a, ["/d", "/c", "yarn", "run", "dev"]);
        assert!(run_command(&Profile::default(), false, &npm("npm; rm -rf ~", "dev"), &d, env).is_err());
        // Fehlt in package.json, Sonderzeichen, Option.
        for s in ["build", "dev & calc", "$(x)", "-x", ""] {
            assert!(run_command(&Profile::default(), true, &npm("npm", s), &d, env).is_err(), "{s}");
        }
        assert!(run_command(&Profile::default(), false, &npm("npm", "dev"), &d.join("fehlt"), env).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn profil_release_und_args() {
        let d = std::env::temp_dir().join(format!("ocui-profil-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
        std::fs::write(d.join("a.csproj"), "").unwrap();
        let env = |_: &str| None;
        let rel = |args: &[&str]| Profile { release: true, args: args.iter().map(|s| s.to_string()).collect() };
        let cargo = Target::Cargo { bin: None };
        assert_eq!(run_command(&rel(&["--features", "x,y", "--", "--port=3000"]), false, &cargo, &d, env).unwrap().1, ["-lic", "cargo run --release --features x,y -- --port=3000"]);
        assert_eq!(run_command(&rel(&[]), true, &Target::Dotnet { watch: false }, &d, env).unwrap().1, ["/d", "/c", "dotnet", "run", "-c", "Release"]);
        for bad in ["a b", "x;rm", "$(id)", "a&b", "%PATH%", "\"q\"", ""] {
            assert!(run_command(&rel(&[bad]), false, &cargo, &d, env).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn run_dotnet_und_cargo() {
        let env = |_: &str| None;
        let one = tmp("dn1", &[("App.csproj", "")]);
        let (_, a) = run_command(&Profile::default(), false, &Target::Dotnet { watch: true }, &one, env).unwrap();
        assert_eq!(a, ["-lic", "dotnet watch"]);
        let (_, a) = run_command(&Profile::default(), true, &Target::Dotnet { watch: false }, &one, env).unwrap();
        assert_eq!(a, ["/d", "/c", "dotnet", "run"]);
        let two = tmp("dn2", &[("A.csproj", ""), ("B.fsproj", "")]);
        assert!(run_command(&Profile::default(), false, &Target::Dotnet { watch: false }, &two, env).is_err());
        let none = tmp("dn0", &[]);
        assert!(run_command(&Profile::default(), false, &Target::Dotnet { watch: false }, &none, env).is_err());

        let c = tmp("cargo", &[("Cargo.toml", "[package]")]);
        let cargo = |b: Option<&str>| Target::Cargo { bin: b.map(String::from) };
        assert_eq!(run_command(&Profile::default(), false, &cargo(None), &c, env).unwrap().1, ["-lic", "cargo run"]);
        assert_eq!(run_command(&Profile::default(), true, &cargo(Some("ocui-sh")), &c, env).unwrap().1, ["/d", "/c", "cargo", "run", "--bin", "ocui-sh"]);
        assert!(run_command(&Profile::default(), false, &cargo(Some("a b")), &c, env).is_err());
        assert!(run_command(&Profile::default(), false, &cargo(None), &none, env).is_err());
        for d in [one, two, none, c] {
            let _ = std::fs::remove_dir_all(d);
        }
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
