use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};
use walkdir::WalkDir;

// Terminal in der App (ersetzt das externe wt.exe-Fenster).
mod pty;

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
    let mut walker = WalkDir::new(&root).max_depth(4).into_iter();
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

/// Tray-Icon mit Menue. Einziger Weg, die App wirklich zu beenden.
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
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close
        ])
        .setup(|app| tray(app.handle()).map_err(Into::into))
        // Schliessen versteckt nur — raus kommt man ueber das Tray-Menue.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{branch_from_head, langs_in};

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
