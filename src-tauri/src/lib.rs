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
// GitHub/GitLab: Konten, Repos, PRs, Klonen.
mod forge;
// Git-Client ueber die git-CLI.
mod git;

#[derive(Serialize)]
struct Repo {
    path: String,
    rel: String,
    branch: String,
    last_commit: String,
    langs: Vec<String>,
}

/// Sprache an den ueblichen Projektdateien ablesen — ohne den Baum zu durchsuchen.
/// Reihenfolge = Prioritaet; `ext` trifft auf jede Datei mit dieser Endung.
const MARKERS: &[(&str, &str, bool)] = &[
    ("Cargo.toml", "Rust", false),
    ("go.mod", "Go", false),
    ("csproj", "C#", true),
    ("sln", "C#", true),
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

fn langs_in(names: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (marker, lang, by_ext) in MARKERS {
        let hit = names.iter().any(|n| {
            if *by_ext {
                n.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(marker))
            } else {
                n.eq_ignore_ascii_case(marker)
            }
        });
        if hit && !found.iter().any(|f| f == lang) {
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

fn read_langs(repo: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(repo) else {
        return Vec::new();
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    langs_in(&names)
}

/// Wie dev.bat: %DEV% wenn gesetzt, sonst %USERPROFILE%\Dev.
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

/// Startet ein Kommando ohne eigenes Konsolenfenster.
fn quiet(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
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
                        .map(|proj| Repo {
                            path: proj.to_string_lossy().into_owned(),
                            rel: proj
                                .strip_prefix(root)
                                .unwrap_or(proj)
                                .to_string_lossy()
                                .into_owned(),
                            branch: read_branch(proj),
                            last_commit: read_last_commit(proj),
                            langs: read_langs(proj),
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

    repos.sort_by(|a, b| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()));
    Ok(repos)
}

/// Oeffnet den Projektordner im Datei-Explorer.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("Ordner nicht gefunden: {path}"));
    }
    Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Explorer konnte nicht geoeffnet werden: {e}"))
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
            "beenden" => app.exit(0),
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
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Muss als erstes Plugin rein. Zweiter Start (z.B. neben Autostart) holt nur das Fenster vor.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| reopen(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(pty::Ptys::default())
        .invoke_handler(tauri::generate_handler![
            default_root,
            scan,
            reveal,
            set_tray,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            forge::forge_login,
            forge::forge_logout,
            forge::forge_import_cli,
            forge::forge_repos,
            forge::forge_pulls,
            forge::forge_clone,
            forge::open_url,
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
            git::git_resolve_side
        ])
        .setup(|app| tray(app.handle()).map_err(Into::into))
        // Mit Tray versteckt Schliessen nur — raus kommt man dann ueber das Tray-Menue.
        .on_window_event(|window, event| {
            if !TRAY.load(Ordering::Relaxed) {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<pty::Ptys>().close_all();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{branch_from_head, langs_in, scan_blocking};

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

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn erkennt_sprachen_an_projektdateien() {
        assert_eq!(langs_in(&names(&["Cargo.toml", "src"])), ["Rust"]);
        assert_eq!(langs_in(&names(&["adesk.csproj"])), ["C#"]);
        // tsconfig.json schlaegt package.json, sonst stuende beides da.
        assert_eq!(langs_in(&names(&["package.json", "tsconfig.json"])), ["TypeScript"]);
        assert_eq!(langs_in(&names(&["package.json"])), ["JavaScript"]);
        assert!(langs_in(&names(&["README.md"])).is_empty());
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
