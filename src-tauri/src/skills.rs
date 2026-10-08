//! Skills aus User, Projekt und Plugins lesen; skillOverrides/enabledPlugins in settings.local.json schreiben.
//! Formen und Namen wie in src/lib/skills.ts.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::git::blocking;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    key: String,
    name: String,
    description: String,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    skill_overrides: BTreeMap<String, String>,
    enabled_plugins: BTreeMap<String, bool>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SkillsInfo {
    skills: Vec<Skill>,
    plugins: Vec<String>,
    user: Settings,
    team: Settings,
}

const VALUES: [&str; 4] = ["on", "off", "user-invocable-only", "name-only"];

/// Home wie default_root in lib.rs.
pub(crate) fn home() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default())
}

/// name/description aus dem ---Block; eingerueckte Folgezeilen (auch nach > oder |) werden angehaengt.
fn frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let (mut name, mut desc) = (None::<String>, None::<String>);
    let mut current: Option<&mut Option<String>> = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(Some(v)) = current.as_mut().map(|c| c.as_mut()) {
                let t = line.trim();
                if !t.is_empty() {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(t);
                }
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            current = None;
            continue;
        };
        let v = v.trim();
        let v = if v.starts_with(['>', '|']) { "" } else { v };
        // ponytail: nur die haeufigen YAML-Escapes ('' und \" \\), kein voller Parser
        let v = if let Some(s) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            s.replace("\\\\", "\0").replace("\\\"", "\"").replace('\0', "\\")
        } else if let Some(s) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            s.replace("''", "'")
        } else {
            v.to_string()
        };
        current = match k.trim() {
            "name" => Some(&mut name),
            "description" => Some(&mut desc),
            _ => None,
        };
        if let Some(c) = current.as_mut() {
            **c = Some(v.clone());
        }
    }
    let clean = |o: Option<String>| o.filter(|s| !s.is_empty());
    (clean(name), clean(desc))
}

/// <dir>/*/SKILL.md; plugin = (Namensraum, Plugin-Key).
fn scan(dir: &Path, source: &'static str, plugin: Option<(&str, &str)>) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Skill> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.path().join("SKILL.md");
            // lossy: Claude Code laedt auch nicht-UTF-8-Dateien, der Skill darf nicht verschwinden
            let text = String::from_utf8_lossy(&std::fs::read(&file).ok()?).into_owned();
            let (name, description) = frontmatter(&text);
            // Key = Ordnername wie in Claude Code (caveman/skills/compress -> caveman:compress); name nur Anzeige
            let dir = e.file_name().to_string_lossy().into_owned();
            let name = name.unwrap_or_else(|| dir.clone());
            Some(Skill {
                key: plugin.map_or_else(|| dir.clone(), |(ns, _)| format!("{ns}:{dir}")),
                name,
                description: description.unwrap_or_default(),
                source,
                plugin: plugin.map(|(_, id)| id.to_string()),
            })
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Fehlend/kaputt = leer; unbekannte Werte werden ignoriert.
fn read_settings(path: &Path) -> Settings {
    let v = read_json(path).unwrap_or_default();
    let obj = |k: &str| v.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
    Settings {
        skill_overrides: obj("skillOverrides")
            .into_iter()
            .filter_map(|(k, v)| v.as_str().filter(|s| VALUES.contains(s)).map(|s| (k, s.to_string())))
            .collect(),
        enabled_plugins: obj("enabledPlugins").into_iter().filter_map(|(k, v)| Some((k, v.as_bool()?))).collect(),
    }
}

pub(crate) fn list(home: &Path, repo: &Path) -> SkillsInfo {
    let claude = home.join(".claude");
    let user = read_settings(&claude.join("settings.json"));
    let team = read_settings(&repo.join(".claude").join("settings.json"));
    let mut skills = scan(&claude.join("skills"), "user", None);
    skills.extend(scan(&repo.join(".claude").join("skills"), "project", None));
    let installed = read_json(&claude.join("plugins").join("installed_plugins.json"));
    let installed = installed.as_ref().and_then(|v| v.get("plugins")).and_then(Value::as_object);
    let mut plugins = Vec::new();
    for (id, installs) in installed.into_iter().flatten() {
        // ponytail: nur die erste Installation; mehrere Scopes pro Plugin erst bei Bedarf
        let path = installs.get(0).and_then(|i| i.get("installPath")).and_then(Value::as_str);
        if let Some(path) = path {
            let ns = id.split('@').next().unwrap_or(id);
            skills.extend(scan(&Path::new(path).join("skills"), "plugin", Some((ns, id))));
        }
        plugins.push(id.clone());
    }
    SkillsInfo { skills, plugins, user, team }
}

// ponytail: die App besitzt skillOverrides und enabledPlugins in settings.local.json komplett
// (Frontend schreibt den gemergten Stand); Aenderungen von Hand an diesen Keys werden ueberschrieben.
// Frontend serialisiert pro Projekt; Restfenster: schreibt Claude Code zwischen Lesen und rename, geht das verloren.
pub(crate) fn write_local(
    repo: &Path,
    skill_overrides: Option<Map<String, Value>>,
    enabled_plugins: Option<Map<String, Value>>,
) -> Result<(), String> {
    let file = repo.join(".claude").join("settings.local.json");
    if tracked(repo) {
        return Err(format!("{} ist in git eingecheckt, private Skill-Einstellungen werden dort nicht gespeichert", file.display()));
    }
    let old = match std::fs::read_to_string(&file) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Some(m),
            _ => return Err(format!("{} ist kein gueltiges JSON-Objekt", file.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let mut map = old.clone().unwrap_or_default();
    for (key, val) in [("skillOverrides", skill_overrides), ("enabledPlugins", enabled_plugins)] {
        match val {
            Some(v) => map.insert(key.into(), Value::Object(v)),
            None => map.remove(key),
        };
    }
    if old.as_ref() == Some(&map) || (old.is_none() && map.is_empty()) {
        return Ok(());
    }
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    // eindeutiger Temp-Name, parallele Aufrufe zerstoeren sich sonst gegenseitig die Datei
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = file.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&Value::Object(map)).unwrap() + "\n";
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    exclude(repo);
    Ok(())
}

/// Getrackte settings.local.json nie anfassen: sonst landen die privaten Plugins des Users im Commit.
fn tracked(repo: &Path) -> bool {
    let out = crate::quiet("git").arg("-C").arg(repo).args(["ls-files", "--error-unmatch", "--", ".claude/settings.local.json"]).output();
    out.is_ok_and(|o| o.status.success())
}

/// settings.local.json in .git/info/exclude, falls git sie nicht schon ignoriert; .gitignore bleibt unberuehrt.
fn exclude(repo: &Path) {
    let git = |args: &[&str]| crate::quiet("git").arg("-C").arg(repo).args(args).output().ok();
    // 0 = ignoriert, 1 = nicht ignoriert, sonst kein Repo/Fehler
    if git(&["check-ignore", "-q", ".claude/settings.local.json"]).and_then(|o| o.status.code()) != Some(1) {
        return;
    }
    let Some(out) = git(&["rev-parse", "--git-path", "info/exclude"]).filter(|o| o.status.success()) else { return };
    let path = repo.join(String::from_utf8_lossy(&out.stdout).trim());
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    // getrackte Dateien meldet check-ignore nie als ignoriert: Eintrag nur einmal anhaengen
    if text.lines().any(|l| l.trim() == "**/.claude/settings.local.json") {
        return;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("**/.claude/settings.local.json\n");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

#[tauri::command]
pub async fn skills_list(path: String) -> Result<SkillsInfo, String> {
    blocking(move || Ok(list(&home(), Path::new(&path)))).await
}

#[tauri::command]
pub async fn skills_write_local(
    path: String,
    skill_overrides: Option<Map<String, Value>>,
    enabled_plugins: Option<Map<String, Value>>,
) -> Result<(), String> {
    blocking(move || write_local(Path::new(&path), skill_overrides, enabled_plugins)).await
}

/// Eigene Mods (Claude-Code-Plugins mit Function-Hooks); das Terminal reicht sie per CLAUDE_CODE_PLUGIN_DIRS an claude.
pub fn mods_dir() -> PathBuf {
    home().join(".claude").join("open-claude-mods")
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Mod {
    name: String,
    description: String,
    pub path: String,
}

/// Unterordner von dir mit .claude-plugin/plugin.json; name/description von dort, sonst Ordnername.
pub fn mods(dir: &Path) -> Vec<Mod> {
    let mut out: Vec<Mod> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let text = std::fs::read_to_string(path.join(".claude-plugin").join("plugin.json")).ok()?;
            let json: Value = serde_json::from_str(&text).unwrap_or_default();
            let s = |k: &str| json.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(String::from);
            Some(Mod {
                name: s("name").unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()),
                description: s("description").unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[derive(Serialize)]
pub struct ModsInfo {
    dir: String,
    mods: Vec<Mod>,
}

/// Legt den Ordner an, damit "Ordner oeffnen" immer klappt.
#[tauri::command]
pub async fn mods_list() -> Result<ModsInfo, String> {
    blocking(|| {
        let dir = mods_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(ModsInfo { mods: mods(&dir), dir: dir.to_string_lossy().into_owned() })
    })
    .await
}

#[cfg(test)]
#[path = "skills_scan_tests.rs"]
mod scan_tests;
#[cfg(test)]
#[path = "skills_write_tests.rs"]
mod write_tests;
