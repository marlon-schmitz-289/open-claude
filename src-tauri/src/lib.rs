use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};
use walkdir::WalkDir;

// Terminal in der App (ersetzt das externe wt.exe-Fenster).
mod pty;
mod chat;
// GitHub/GitLab: Konten, Repos, PRs, Klonen.
mod forge;
// Neue Repos anlegen (Scaffold, git init, erster Commit).
mod create;
// Git-Client ueber die git-CLI.
mod git;
// Unity-Projekte ueber die unity-CLI.
mod unity;
// Skill-Manager: SKILL.md lesen, settings.local.json schreiben.
mod skills;
mod files;
// Vorschau des Editors: preview-Schema, WPF-Helfer, Rueckfrage vor dem Beenden.
mod preview;
mod deps;
mod activity;
mod workflows;
// Test-Explorer: Testlaeufe ohne PTY, Output zeilenweise ueber einen Channel.
mod runner;

#[derive(Serialize)]
struct Repo {
    path: String,
    rel: String,
    branch: String,
    last_commit: String,
    langs: Vec<String>,
    /// Projekttyp (Unity, Tauri, ...); die Liste zeigt ihn statt der Sprachen.
    kind: Option<String>,
}

/// Sprache an den ueblichen Projektdateien ablesen — ohne den Baum zu durchsuchen.
/// Reihenfolge = Prioritaet; `ext` trifft auf jede Datei mit dieser Endung.
const MARKERS: &[(&str, &str, bool)] = &[
    ("Cargo.toml", "Rust", false),
    ("go.mod", "Go", false),
    ("csproj", "C#", true),
    ("sln", "C#", true),
    ("slnx", "C#", true),
    ("fsproj", "F#", true),
    ("pyproject.toml", "Python", false),
    ("requirements.txt", "Python", false),
    ("setup.py", "Python", false),
    ("pom.xml", "Java", false),
    ("build.gradle", "Java", false),
    ("build.gradle.kts", "Kotlin", false),
    ("composer.json", "PHP", false),
    ("Gemfile", "Ruby", false),
    ("Package.swift", "Swift", false),
    ("CMakeLists.txt", "C++", false),
    ("tsconfig.json", "TypeScript", false),
    ("package.json", "JavaScript", false),
];

/// Projekttyp an Dateien im Root; Reihenfolge = Prioritaet, `ext` wie bei MARKERS.
/// Unity steht nicht hier: braucht Assets/ UND ProjectVersion.txt (unity::is_project).
const KINDS: &[(&str, &str, bool)] = &[
    ("src-tauri", "Tauri", false),
    ("project.godot", "Godot", false),
    ("uproject", "Unreal", true),
    ("pubspec.yaml", "Flutter", false),
    ("next.config.js", "Next.js", false),
    ("next.config.mjs", "Next.js", false),
    ("next.config.ts", "Next.js", false),
    ("nuxt.config.ts", "Nuxt", false),
    ("nuxt.config.js", "Nuxt", false),
    ("svelte.config.js", "SvelteKit", false),
    ("svelte.config.ts", "SvelteKit", false),
    ("angular.json", "Angular", false),
    ("manage.py", "Django", false),
];

fn has(names: &[String], marker: &str, by_ext: bool) -> bool {
    names.iter().any(|n| {
        if by_ext {
            n.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(marker))
        } else {
            n.eq_ignore_ascii_case(marker)
        }
    })
}

fn kind_in(names: &[String], unity: bool) -> Option<String> {
    if unity {
        return Some("Unity".into());
    }
    KINDS.iter().find(|(m, _, e)| has(names, m, *e)).map(|(_, k, _)| (*k).to_string())
}

fn langs_in(names: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (marker, lang, by_ext) in MARKERS {
        if has(names, marker, *by_ext) && !found.iter().any(|f| f == lang) {
            found.push((*lang).to_string());
        }
    }
    // package.json neben tsconfig.json sagt nichts Neues.
    if found.iter().any(|l| l == "TypeScript") {
        found.retain(|l| l != "JavaScript");
    }
    found.truncate(2);
    found
}

fn read_langs(repo: &Path) -> (Vec<String>, Option<String>) {
    let Ok(entries) = std::fs::read_dir(repo) else {
        return (Vec::new(), None);
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    (langs_in(&names), kind_in(&names, unity::is_project(repo)))
}

/// Wie dev.bat: %DEV% wenn gesetzt, sonst %USERPROFILE%\Dev bzw. ~/Dev.
#[tauri::command]
fn default_root() -> String {
    if let Ok(dev) = std::env::var("DEV") {
        if !dev.trim().is_empty() {
            return dev.trim_end_matches(['\\', '/']).to_string();
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join("Dev").to_string_lossy().into_owned()
}

fn branch_from_head(head: &str) -> String {
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(name) => name.to_string(),
        // Detached HEAD: roher SHA, auf 7 Zeichen kuerzen.
        None => head.chars().take(7).collect(),
    }
}

fn read_branch(repo: &Path) -> String {
    std::fs::read_to_string(repo.join(".git").join("HEAD"))
        .map(|h| branch_from_head(&h))
        .unwrap_or_default()
}

/// macOS: laeuft die App unter Rosetta (x86_64-Build auf Apple Silicon), erben Kinder die
/// Architektur — /usr/bin/git scheitert dann an den arm64-only Command Line Tools (libxcrun).
/// `arch -arm64 -x86_64` nimmt arm64 wo moeglich, auf Intel-Macs x86_64.
pub(crate) fn native(program: &str) -> (&str, Vec<&str>) {
    if cfg!(target_os = "macos") {
        ("/usr/bin/arch", vec!["-arm64", "-x86_64", program])
    } else {
        (program, Vec::new())
    }
}

/// AppImage: `AppRun` setzt LD_LIBRARY_PATH und die GTK-Variablen auf das entpackte
/// AppDir. Jeder Kindprozess erbt das und laedt dann unsere gebuendelten Bibliotheken
/// statt seiner eigenen — /usr/bin/git stirbt beim Klonen an
/// `undefined symbol: nghttp2_...` aus der mitgelieferten libnghttp2.
/// Also streichen, was ins AppDir zeigt; None heisst: Variable loeschen.
/// Setzt der AppRun-Hook hart, ohne dass ein Pfad drinsteht: GDK_BACKEND, weil die
/// Webview unter Wayland abstuerzt, GTK_THEME, weil gebuendelte Themes kaputt sind.
/// Eine per xdg-open gestartete GUI-App waere damit auf XWayland und Adwaita
/// festgenagelt. Den Originalwert hat der Hook ueberschrieben, er ist nicht mehr zu
/// holen — geloescht nimmt das Kind wenigstens die Vorgabe des Systems.
const HOOK_VARS: [&str; 3] = ["APPDIR", "GDK_BACKEND", "GTK_THEME"];

/// Gemeinsamer Anfang aller Mounts dieses AppImage. Die Runtime mountet nach
/// `$TMPDIR/.mount_<Name>XXXXXX`, die letzten sechs Zeichen sind Zufall. Startet sich
/// die App neu (Update, relaunch), erbt die neue Instanz LD_LIBRARY_PATH der alten
/// und AppRun setzt nur seine eigenen Pfade davor — die des alten Mounts muessen
/// genauso weg, sonst laedt git weiter die gebuendelte libnghttp2.
fn mount_prefix(appdir: &str) -> &str {
    let dir = appdir.trim_end_matches('/');
    let name = dir.rsplit('/').next().unwrap_or("");
    if name.starts_with(".mount_") && name.len() > ".mount_".len() + 6 {
        &dir[..dir.len() - 6]
    } else {
        dir
    }
}

fn strip_appdir(
    appdir: &str,
    vars: impl Iterator<Item = (String, String)>,
) -> Vec<(String, Option<String>)> {
    if appdir.is_empty() {
        return Vec::new();
    }
    let prefix = mount_prefix(appdir);
    let mut out: Vec<(String, Option<String>)> =
        HOOK_VARS.iter().map(|k| (k.to_string(), None)).collect();
    for (key, val) in vars {
        if HOOK_VARS.contains(&key.as_str()) || !val.contains(prefix) {
            continue;
        }
        // Pfadlisten nur kuerzen statt loeschen: was nicht im AppDir liegt, ist die
        // echte Systemumgebung und wird gebraucht (XDG_DATA_DIRS, PATH).
        // Leere Segmente fallen mit weg — AppRun haengt gern ein ":" an, und ein
        // leerer Eintrag heisst fuer den Loader "aktuelles Verzeichnis".
        let kept: Vec<&str> = val
            .split(':')
            .filter(|p| !p.is_empty() && !p.starts_with(prefix))
            .collect();
        out.push((key, (!kept.is_empty()).then(|| kept.join(":"))));
    }
    out
}

/// Nimmt dem Kommando die AppImage-Umgebung. Ausserhalb eines AppImage (deb, dev,
/// Windows, macOS) ist APPDIR nicht gesetzt und nichts aendert sich.
pub(crate) fn unbundle_env(mut set: impl FnMut(&str, Option<&str>)) {
    let Ok(appdir) = std::env::var("APPDIR") else { return };
    for (key, val) in strip_appdir(&appdir, std::env::vars()) {
        set(&key, val.as_deref());
    }
}

fn unbundle(cmd: &mut Command) {
    unbundle_env(|key, val| match val {
        Some(v) => {
            cmd.env(key, v);
        }
        None => {
            cmd.env_remove(key);
        }
    });
}

/// Startet ein Kommando ohne eigenes Konsolenfenster.
fn quiet(program: &str) -> Command {
    let (exe, pre) = native(program);
    let mut cmd = Command::new(exe);
    cmd.args(pre);
    unbundle(&mut cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Prozess samt Kindern beenden. Unix: die Prozessgruppe (Kind mit process_group(0) gestartet), Windows: der Baum.
/// Die Unity-CLI startet Unity in ihrer Gruppe, Testlaeufer ihre Worker: nur das Kind zu beenden liesse sie weiterlaufen.
// ponytail: nur SIGTERM, Eskalation auf KILL erst, wenn haengende Prozesse auftreten.
pub(crate) fn kill_tree(pid: u32) {
    #[cfg(unix)]
    let _ = Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
    #[cfg(windows)]
    let _ = quiet("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).status();
}

fn read_last_commit(repo: &Path) -> String {
    quiet("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "-1", "--format=%cI"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Laeuft auf einem Worker-Thread: ein synchrones Command wuerde den UI-Thread blockieren.
#[tauri::command]
async fn scan(root: String) -> Result<Vec<Repo>, String> {
    tauri::async_runtime::spawn_blocking(move || scan_blocking(root))
        .await
        .map_err(|e| format!("Scan abgebrochen: {e}"))?
}

fn scan_blocking(root: String) -> Result<Vec<Repo>, String> {
    let root = PathBuf::from(root.trim_end_matches(['\\', '/']));
    if !root.is_dir() {
        return Err(format!("Ordner nicht gefunden: {}", root.display()));
    }

    let mut dirs = Vec::new();
    let mut walker = WalkDir::new(&root).max_depth(6).into_iter();
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), "node_modules" | "target" | ".venv") {
            walker.skip_current_dir();
            continue;
        }
        if name != ".git" {
            continue;
        }
        // .git gefunden -> Projektordner ist der Parent, Teilbaum nicht weiter verfolgen.
        walker.skip_current_dir();
        if let Some(proj) = entry.path().parent() {
            dirs.push(proj.to_path_buf());
        }
    }

    // Jedes Repo kostet einen git-Prozess; nacheinander dauert das bei vielen Repos spuerbar.
    let lanes = dirs.len().div_ceil(8).max(1);
    let mut repos: Vec<Repo> = std::thread::scope(|s| {
        let workers: Vec<_> = dirs
            .chunks(lanes)
            .map(|chunk| {
                let root = &root;
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|proj| {
                            let (langs, kind) = read_langs(proj);
                            Repo {
                                path: proj.to_string_lossy().into_owned(),
                                rel: proj
                                    .strip_prefix(root)
                                    .unwrap_or(proj)
                                    .to_string_lossy()
                                    .into_owned(),
                                branch: read_branch(proj),
                                last_commit: read_last_commit(proj),
                                langs,
                                kind,
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    });

    repos.sort_by_key(|a| a.rel.to_lowercase());
    Ok(repos)
}

/// Oeffnet den Projektordner im Datei-Explorer.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("Ordner nicht gefunden: {path}"));
    }
    open_system(&dir)
        .map_err(|e| format!("Explorer konnte nicht geoeffnet werden: {e}"))
}

/// Nur Git-Repos echt unterhalb des Dev-Ordners, nie den Dev-Ordner selbst.
fn trashable(path: &str, root: &str) -> Result<PathBuf, String> {
    let dir = Path::new(path).canonicalize().map_err(|e| format!("Ordner nicht gefunden: {e}"))?;
    let root = Path::new(root).canonicalize().map_err(|e| format!("Dev-Ordner nicht gefunden: {e}"))?;
    if dir == root || !dir.starts_with(&root) {
        return Err(format!("Liegt nicht im Dev-Ordner: {path}"));
    }
    if !dir.join(".git").exists() {
        return Err(format!("Kein Git-Repo: {path}"));
    }
    Ok(dir)
}

/// Verschiebt das Repo in den Papierkorb, damit ein Fehlklick wiederherstellbar bleibt.
#[tauri::command]
async fn trash_repo(path: String, root: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = trashable(&path, &root)?;
        trash::delete(&dir).map_err(|e| format!("Löschen fehlgeschlagen: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Oeffnet Ordner oder URL mit dem Standardprogramm (Explorer, Finder, xdg-open).
/// Navigation der Webview (auch iframes) pruefen. Links, die JS nicht abfaengt (natives Kontextmenue
/// "Link oeffnen" u. ae.), landeten sonst in der App: extern -> System-Browser, App-Unterseiten -> 404.
/// localhost bleibt erlaubt, die Vorschau zeigt lokale Dev-Server.
// ponytail: externe Seiten in der Vorschau oeffnen damit im Browser; Frame-Unterscheidung erst wenn noetig
/// Some(true) = erlauben, Some(false) = verwerfen, None = im System-Browser oeffnen.
fn nav(url: &tauri::Url, dev: Option<&tauri::Url>) -> Option<bool> {
    if url.scheme() == "ipc" || url.host_str() == Some("ipc.localhost") {
        return Some(true);
    }
    if url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost") || dev.is_some_and(|d| d.origin() == url.origin()) {
        // Start-URL: tauri://localhost (leerer Pfad, macOS) bzw. .../index.html je nach Plattform
        return Some(matches!(url.path(), "" | "/" | "/index.html"));
    }
    match url.scheme() {
        "http" | "https" if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) => Some(true),
        "http" | "https" => None,
        "about" | "data" | "blob" => Some(true),
        _ => Some(false),
    }
}

pub(crate) fn open_system(target: impl AsRef<std::ffi::OsStr>) -> std::io::Result<()> {
    let prog = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // wait im Thread, sonst bleibt unter Unix ein Zombie-Prozess stehen
    let mut cmd = Command::new(prog);
    cmd.arg(target);
    unbundle(&mut cmd);
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Holt das Fenster aus dem Tray zurueck. Versteckt reicht nicht als Annahme:
/// es kann genauso gut nur minimiert sein.
fn reopen(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Standardmenue, aber "Beenden" (Cmd+Q) laeuft ueber preview::quit und fragt bei ungespeicherten Aenderungen.
/// Das vordefinierte Beenden ruft terminate: und kaeme ohne Rueckfrage daran vorbei.
// ponytail: Dock > Beenden und Abmelden rufen ebenfalls terminate:. Abfangen hiesse applicationShouldTerminate
// im App-Delegate, das tao nicht anbietet.
#[cfg(target_os = "macos")]
fn menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    // Erstes Untermenue = App-Menue, sein letzter Eintrag = Beenden.
    if let Some(sub) = menu.items()?.first().and_then(|i| i.as_submenu()) {
        sub.remove_at(sub.items()?.len().saturating_sub(1))?;
        sub.append(&tauri::menu::MenuItem::with_id(app, "quit", "Open Claude beenden", true, Some("Cmd+Q"))?)?;
    }
    Ok(menu)
}

/// Schliessen versteckt ins Tray statt zu beenden. Das Frontend setzt es aus den Einstellungen.
static TRAY: AtomicBool = AtomicBool::new(true);

#[tauri::command]
fn set_tray(on: bool) {
    TRAY.store(on, Ordering::Relaxed);
}

/// Tray-Icon mit Menue. Mit Tray an der einzige Weg, die App wirklich zu beenden.
fn tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let oeffnen = MenuItem::with_id(app, "oeffnen", "Öffnen", true, None::<&str>)?;
    let beenden = MenuItem::with_id(app, "beenden", "Beenden", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&oeffnen, &beenden])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Open Claude")
        .menu(&menu)
        // Linksklick oeffnet, das Menue haengt auf rechts.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "oeffnen" => reopen(app),
            "beenden" => preview::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reopen(tray.app_handle());
            }
        });
    // macOS: einfarbiges Template, das System faerbt es passend zur Menueleiste.
    if cfg!(target_os = "macos") {
        builder = builder
            .icon(tauri::include_image!("icons/tray-template.png"))
            .icon_as_template(true);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Aus Finder/Dock gestartet ist PATH nur /usr/bin:/bin:… — git findet dann weder git-lfs
/// noch Credential-Helper aus Homebrew & Co. PATH daher einmal aus der Login-Shell holen.
fn login_path() -> Option<String> {
    let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = quiet(&sh)
        .args(["-lic", "printf '\\n__PATH__%s__PATH__' \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let path = s.split("__PATH__").nth(1)?;
    (!path.is_empty()).then(|| path.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Vor allen Threads, set_var ist nicht threadsicher.
    if cfg!(unix) {
        if let Some(path) = login_path() {
            std::env::set_var("PATH", path);
        }
    }
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu).on_menu_event(|app, event| {
        if event.id().as_ref() == "quit" {
            preview::quit(app);
        }
    });
    builder
        // Muss als erstes Plugin rein. Zweiter Start (z.B. neben Autostart) holt nur das Fenster vor.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| reopen(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(pty::Ptys::default())
        .manage(chat::Chats::default())
        .manage(preview::PreviewRoots::default())
        .manage(runner::Runs::default())
        .register_asynchronous_uri_scheme_protocol("preview", |ctx, req, responder| {
            preview::serve(ctx.app_handle(), req, responder)
        })
        .invoke_handler(tauri::generate_handler![
            default_root,
            scan,
            reveal,
            trash_repo,
            set_tray,
            deps::deps_missing,
            deps::deps_install,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            pty::run_start,
            chat::chat_open,
            chat::chat_send,
            chat::chat_close,
            chat::chat_sessions,
            chat::chat_history,
            chat::chat_image,
            forge::forge_login,
            forge::forge_logout,
            forge::forge_import_cli,
            forge::forge_repos,
            forge::forge_pulls,
            forge::forge_releases,
            forge::forge_release_save,
            forge::forge_release_delete,
            forge::forge_runs,
            forge::forge_jobs,
            forge::forge_run_action,
            forge::forge_clone,
            forge::forge_create_remote,
            create::repo_langs,
            create::repo_create,
            forge::forge_repo_account,
            forge::forge_set_repo_account,
            forge::open_url,
            forge::forge_web_url,
            git::git_status,
            git::git_log,
            git::git_show,
            git::git_diff,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_apply,
            git::git_commit,
            git::git_branches,
            git::git_checkout,
            git::git_branch_delete,
            git::git_branch_rename,
            git::git_merge,
            git::git_rebase,
            git::git_abort,
            git::git_continue,
            git::git_cherry_pick,
            git::git_revert,
            git::git_reset,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_remotes,
            git::git_stashes,
            git::git_stash_push,
            git::git_stash_apply,
            git::git_stash_drop,
            git::git_stash_show,
            git::git_tags,
            git::git_tag_create,
            git::git_tag_delete,
            git::git_get_base,
            git::git_nesting,
            git::git_set_base,
            git::git_conflict,
            git::git_resolve,
            git::git_resolve_side,
            git::git_conflict_sides,
            unity::unity_info,
            unity::unity_setup,
            unity::unity_open,
            unity::unity_close,
            unity::unity_test,
            files::fs_list,
            files::fs_read,
            files::fs_write,
            files::fs_stat,
            files::fs_create,
            files::fs_rename,
            files::fs_delete,
            files::fs_files,
            files::fs_diff,
            preview::preview_allow,
            preview::xaml_render,
            preview::editor_dirty,
            skills::skills_list,
            skills::skills_write_local,
            skills::mods_list,
            skills::plugins_list,
            skills::plugins_run,
            activity::claude_activity,
            activity::claude_image,
            activity::claude_agent_log,
            workflows::workflows_list,
            workflows::workflows_save,
            workflows::workflows_delete,
            runner::test_run,
            runner::test_cancel
        ])
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("nav-guard")
                .on_navigation(|w, url| {
                    nav(url, w.app_handle().config().build.dev_url.as_ref()).unwrap_or_else(|| {
                        let _ = open_system(url.as_str());
                        false
                    })
                })
                .build(),
        )
        .setup(|app| tray(app.handle()).map_err(Into::into))
        // Mit Tray (macOS: immer) versteckt Schliessen nur — raus kommt man dann ueber das Tray-Menue.
        // Ohne Tray beendet Schliessen die App: bei ungespeicherten Aenderungen im Editor erst fragen.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if TRAY.load(Ordering::Relaxed) {
                    let _ = window.hide();
                } else {
                    preview::quit(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => {
                app.state::<pty::Ptys>().close_all();
                app.state::<chat::Chats>().close_all();
                unity::kill_all();
                app.state::<runner::Runs>().kill_all();
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => reopen(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn navigation_nur_app_und_lokal() {
        let u = |s: &str| tauri::Url::parse(s).unwrap();
        let dev = u("http://localhost:1420");
        let nav = |s: &str| super::nav(&u(s), Some(&dev));
        assert_eq!(nav("http://localhost:1420/"), Some(true));
        assert_eq!(nav("http://localhost:1420/releases"), Some(false));
        assert_eq!(nav("tauri://localhost/"), Some(true));
        assert_eq!(nav("tauri://localhost/releases"), Some(false));
        // Release auf macOS laedt tauri://localhost (leerer Pfad) bzw. /index.html
        assert_eq!(nav("tauri://localhost"), Some(true));
        assert_eq!(nav("tauri://localhost/index.html"), Some(true));
        assert_eq!(nav("http://tauri.localhost/index.html"), Some(true));
        assert_eq!(nav("http://tauri.localhost/x"), Some(false));
        assert_eq!(nav("http://ipc.localhost/plugin%3Afoo"), Some(true));
        assert_eq!(nav("http://localhost:5173/app"), Some(true));
        assert_eq!(nav("about:blank"), Some(true));
        assert_eq!(nav("https://github.com/x"), None);
        assert_eq!(nav("file:///etc/passwd"), Some(false));
        assert_eq!(nav("javascript:alert(1)"), Some(false));
    }

    use super::{branch_from_head, kind_in, langs_in, scan_blocking, strip_appdir, trashable, HOOK_VARS};

    const APPDIR: &str = "/tmp/.mount_OpenCl42";

    fn stripped(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let owned = vars.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        strip_appdir(APPDIR, owned)
    }

    /// None: bleibt unangetastet. Some(None): wird geloescht.
    /// Some(Some(v)): wird auf v gekuerzt.
    fn wert(vars: &[(&str, &str)], key: &str) -> Option<Option<String>> {
        stripped(vars).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    #[test]
    fn loescht_variablen_die_nur_ins_appdir_zeigen() {
        // Der Fall aus dem Bug: git laedt sonst unsere libnghttp2 statt der vom System.
        let env = &[
            ("LD_LIBRARY_PATH", "/tmp/.mount_OpenCl42/usr/lib:/tmp/.mount_OpenCl42/lib"),
            ("GDK_PIXBUF_MODULE_FILE", "/tmp/.mount_OpenCl42/usr/lib/loaders.cache"),
        ];
        assert_eq!(wert(env, "APPDIR"), Some(None));
        assert_eq!(wert(env, "LD_LIBRARY_PATH"), Some(None));
        assert_eq!(wert(env, "GDK_PIXBUF_MODULE_FILE"), Some(None));
        // AppRun haengt ein abschliessendes ":" an; das leere Segment darf nicht
        // als Wert uebrigbleiben, sonst sucht der Loader im Arbeitsverzeichnis.
        let trailing = &[("LD_LIBRARY_PATH", "/tmp/.mount_OpenCl42/usr/lib/:")];
        assert_eq!(wert(trailing, "LD_LIBRARY_PATH"), Some(None));
    }

    #[test]
    fn streicht_auch_pfade_eines_frueheren_mounts() {
        // Nach relaunch: AppRun der neuen Instanz setzt sich vor die Pfade der alten.
        let env = &[(
            "LD_LIBRARY_PATH",
            "/tmp/.mount_OpenCl42/usr/lib/:/tmp/.mount_OpenClxyzw/usr/lib/:/opt/lib:",
        )];
        assert_eq!(wert(env, "LD_LIBRARY_PATH"), Some(Some("/opt/lib".into())));
        // Ein anderes AppImage bleibt unangetastet.
        let fremd = &[("LD_LIBRARY_PATH", "/tmp/.mount_Krita0abcdef/usr/lib")];
        assert_eq!(wert(fremd, "LD_LIBRARY_PATH"), None);
    }

    #[test]
    fn kuerzt_pfadlisten_statt_sie_zu_leeren() {
        // XDG_DATA_DIRS haengt das AppDir vorne an; der Systemteil muss bleiben.
        let env = &[("XDG_DATA_DIRS", "/tmp/.mount_OpenCl42/usr/share:/usr/share:/usr/local/share")];
        assert_eq!(wert(env, "XDG_DATA_DIRS"), Some(Some("/usr/share:/usr/local/share".into())));
        // PATH beginnt mit $APPDIR/usr/bin — sonst findet ein Kind dort ocui-sh.
        let path = &[("PATH", "/tmp/.mount_OpenCl42/usr/bin:/usr/bin:/bin")];
        assert_eq!(wert(path, "PATH"), Some(Some("/usr/bin:/bin".into())));
    }

    #[test]
    fn raeumt_auch_die_hart_gesetzten_hook_variablen_weg() {
        let env = &[("GDK_BACKEND", "x11"), ("GTK_THEME", "Adwaita:dark")];
        assert_eq!(wert(env, "GDK_BACKEND"), Some(None));
        assert_eq!(wert(env, "GTK_THEME"), Some(None));
        // Kein Doppeleintrag, obwohl sie schon in HOOK_VARS stehen.
        assert_eq!(stripped(env).len(), HOOK_VARS.len());
    }

    #[test]
    fn laesst_umgebung_ohne_appdir_in_ruhe() {
        assert_eq!(wert(&[("HOME", "/home/m"), ("PATH", "/usr/bin:/bin")], "PATH"), None);
        // Ausserhalb eines AppImage faellt die Bereinigung ganz weg.
        assert!(strip_appdir("", std::iter::empty()).is_empty());
    }

    #[test]
    fn findet_tief_verschachtelte_repos() {
        let root = std::env::temp_dir().join(format!("ocui-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for p in ["a/b/c/d/tief/.git", "flach/.git", "node_modules/x/.git"] {
            std::fs::create_dir_all(root.join(p)).unwrap();
        }
        let repos = scan_blocking(root.display().to_string()).unwrap();
        let mut rels: Vec<String> = repos.iter().map(|r| r.rel.replace('\\', "/")).collect();
        rels.sort();
        assert_eq!(rels, ["a/b/c/d/tief", "flach"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loescht_nur_repos_im_dev_ordner() {
        let root = std::env::temp_dir().join(format!("ocui-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();
        std::fs::create_dir_all(root.join("kein-repo")).unwrap();
        let r = root.display().to_string();
        let p = |s: &str| root.join(s).display().to_string();
        assert!(trashable(&p("repo"), &r).is_ok());
        assert!(trashable(&p("kein-repo"), &r).is_err());
        assert!(trashable(&r, &r).is_err());
        // Ausbruch per ".." aus dem Dev-Ordner.
        assert!(trashable(&p("repo/../.."), &p("repo")).is_err());
        assert!(trashable(&p("fehlt"), &r).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn erkennt_sprachen_an_projektdateien() {
        assert_eq!(langs_in(&names(&["Cargo.toml", "src"])), ["Rust"]);
        assert_eq!(langs_in(&names(&["adesk.csproj"])), ["C#"]);
        // Neues Solution-Format, csproj nur in Unterordnern.
        assert_eq!(langs_in(&names(&["MEFactory.slnx", "MEFactory"])), ["C#"]);
        // tsconfig.json schlaegt package.json, sonst stuende beides da.
        assert_eq!(langs_in(&names(&["package.json", "tsconfig.json"])), ["TypeScript"]);
        assert_eq!(langs_in(&names(&["package.json"])), ["JavaScript"]);
        assert!(langs_in(&names(&["README.md"])).is_empty());
    }

    #[test]
    fn erkennt_projekttyp() {
        let k = |list: &[&str], unity| kind_in(&names(list), unity);
        assert_eq!(k(&["Assets", "Game.sln", "Game.csproj"], true).as_deref(), Some("Unity"));
        // Unity schlaegt alles andere, auch wenn zufaellig ein Marker daneben liegt.
        assert_eq!(k(&["Assets", "package.json", "src-tauri"], true).as_deref(), Some("Unity"));
        assert_eq!(k(&["src-tauri", "svelte.config.js", "package.json"], false).as_deref(), Some("Tauri"));
        assert_eq!(k(&["project.godot", "Game.csproj"], false).as_deref(), Some("Godot"));
        assert_eq!(k(&["Shooter.UPROJECT", "Source"], false).as_deref(), Some("Unreal"));
        assert_eq!(k(&["pubspec.yaml"], false).as_deref(), Some("Flutter"));
        assert_eq!(k(&["next.config.mjs", "package.json"], false).as_deref(), Some("Next.js"));
        assert_eq!(k(&["svelte.config.js"], false).as_deref(), Some("SvelteKit"));
        assert_eq!(k(&["manage.py", "requirements.txt"], false).as_deref(), Some("Django"));
        // Nur Sprache, kein Typ; Ordner namens "Assets" allein ist kein Unity.
        assert_eq!(k(&["Cargo.toml", "src"], false), None);
        assert_eq!(k(&["Assets"], false), None);
    }

    #[test]
    fn parses_head() {
        assert_eq!(branch_from_head("ref: refs/heads/feature/x\n"), "feature/x");
        assert_eq!(branch_from_head("ref: refs/heads/main\n"), "main");
        assert_eq!(
            branch_from_head("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0\n"),
            "a1b2c3d"
        );
    }
}
