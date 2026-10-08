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
pub async fn skills_list(app: tauri::AppHandle, path: String) -> Result<SkillsInfo, String> {
    blocking(move || {
        let mut info = list(&home(), Path::new(&path));
        // Skills der eingebauten Mod (kein installiertes Plugin): wie eigene Skills pro Projekt schaltbar,
        // Key mit Namensraum wie in Claude Code (open-claude:smart-tests).
        if let Some(dir) = crate::pty::builtin_mod(&app) {
            info.skills.extend(scan(&dir.join("skills"), "plugin", None).into_iter().map(|s| Skill { key: format!("open-claude:{}", s.key), ..s }));
        }
        Ok(info)
    })
    .await
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

// ---------- Plugin-Verwaltung ueber `claude plugin` ----------
// Schreiben nur ueber die CLI (feste Argument-Arrays); nie Scope local, settings.local.json gehoert der App.

#[derive(Serialize, Debug, PartialEq)]
pub struct Meta {
    latest: Option<String>,
    update: bool,
    description: String,
}

#[derive(Serialize)]
pub struct PluginsInfo {
    installed: Value,
    available: Value,
    marketplaces: Value,
    meta: BTreeMap<String, Meta>,
}

/// Name wie Plugin/Marketplace: [A-Za-z0-9][A-Za-z0-9._-]*
fn name_ok(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphanumeric()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn id_ok(s: &str) -> bool {
    s.split_once('@').is_some_and(|(a, b)| name_ok(a) && name_ok(b))
}

fn source_ok(s: &str) -> bool {
    let repo = s.split_once('/').is_some_and(|(a, b)| name_ok(a) && name_ok(b));
    let path = Path::new(s);
    repo || s.starts_with("https://") || s.starts_with("git@") || s.starts_with("ssh://") || (path.is_absolute() && path.is_dir())
}

/// Argumente fuer `claude plugin ...`; installed/marketplaces = bekannte IDs bzw. Namen.
pub(crate) fn plugin_args(
    action: &str,
    target: &str,
    scope: Option<&str>,
    accept: Option<&str>,
    installed: &[String],
    marketplaces: &[String],
) -> Result<Vec<String>, String> {
    if target.is_empty() || target.starts_with('-') || target.chars().any(char::is_control) {
        return Err(format!("Ungueltiges Ziel: {target:?}"));
    }
    if accept.is_some_and(|a| a.len() != 64 || !a.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err("Ungueltige Kommando-Bestaetigung".into());
    }
    let known = || installed.iter().any(|i| i == target);
    let (ok, scopes): (bool, &[&str]) = match action {
        "install" => (id_ok(target), &["user", "project"]),
        // uninstall local wuerde settings.local.json aendern, die gehoert der App
        "update" => (known(), &["user", "project", "local"]),
        "uninstall" => (known(), &["user", "project"]),
        "enable" | "disable" => (known(), &["user", "project"]),
        "mp-add" => (source_ok(target), &[]),
        "mp-remove" | "mp-update" => (marketplaces.iter().any(|m| m == target), &[]),
        _ => return Err(format!("Unbekannte Aktion: {action}")),
    };
    if !ok {
        return Err(format!("{action}: {target} ist kein gueltiges/bekanntes Ziel"));
    }
    let mut args: Vec<String> = match action.strip_prefix("mp-") {
        Some(sub) => vec!["marketplace".into(), sub.into(), target.into(), "--json".into()],
        None => vec![action.into(), target.into(), "--json".into()],
    };
    if !scopes.is_empty() {
        let s = scope.unwrap_or("user");
        if !scopes.contains(&s) {
            return Err(format!("{action}: Scope {s} nicht erlaubt"));
        }
        args.extend(["-s".into(), s.into()]);
    }
    if let Some(a) = accept.filter(|_| matches!(action, "install" | "update")) {
        args.extend(["--accept-command".into(), a.into()]);
    }
    Ok(args)
}

/// Neueste Version eines Plugins laut Marketplace-Klon; dazu die Beschreibung.
pub(crate) fn latest(mp_dir: &Path, name: &str) -> (Option<String>, String) {
    let mp = read_json(&mp_dir.join(".claude-plugin").join("marketplace.json")).unwrap_or_default();
    let entry = mp.get("plugins").and_then(Value::as_array).and_then(|a| a.iter().find(|p| p.get("name").and_then(Value::as_str) == Some(name)));
    let Some(entry) = entry else { return (None, String::new()) };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    let source = entry.get("source");
    let plugin = source.and_then(Value::as_str).and_then(|src| read_json(&mp_dir.join(src).join(".claude-plugin").join("plugin.json")));
    let description = s(entry, "description").or_else(|| plugin.as_ref().and_then(|p| s(p, "description"))).unwrap_or_default();
    // .gcs-sha bewusst nicht: passt nicht zum installierten Stand
    let version = s(entry, "version")
        .or_else(|| plugin.as_ref().and_then(|p| s(p, "version")))
        .or_else(|| source.and_then(|v| s(v, "sha")))
        .or_else(|| {
            source.filter(|v| v.is_string() && mp_dir.join(".git").exists())?;
            let o = crate::quiet("git").arg("-C").arg(mp_dir).args(["rev-parse", "HEAD"]).output().ok()?;
            o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty())
        });
    (version, description)
}

/// Ist latest neuer als das Installierte? Commit-SHAs per Praefix, sonst Textvergleich.
pub(crate) fn outdated(version: &str, sha: Option<&str>, latest: &str) -> bool {
    if latest.len() == 40 && latest.chars().all(|c| c.is_ascii_hexdigit()) {
        return !sha.is_some_and(|s| s.starts_with(&latest[..12]));
    }
    latest != version
}

/// Letzte nicht-leere Zeile, die ein JSON-Objekt ist.
pub(crate) fn last_json(stdout: &str) -> Option<Value> {
    let line = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    serde_json::from_str::<Value>(line).ok().filter(Value::is_object)
}

fn plugins_dir() -> PathBuf {
    home().join(".claude").join("plugins")
}

fn keys(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

// ponytail: Windows nur claude.exe (nativer Installer); npm-Shim claude.cmd braeuchte cmd /c wie runner.rs, erst bei Bedarf
fn claude(path: &str, args: &[String]) -> Result<std::process::Output, String> {
    if !Path::new(path).is_dir() {
        return Err(format!("{path} ist kein Ordner"));
    }
    crate::quiet("claude")
        .arg("plugin")
        .args(args)
        .current_dir(path)
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0") // kein haengender Credential-Prompt
        .output()
        .map_err(|e| format!("claude konnte nicht gestartet werden: {e}"))
}

fn plugins_info(path: &str) -> Result<PluginsInfo, String> {
    let o = claude(path, &["list".into(), "--json".into(), "--available".into()])?;
    let text = String::from_utf8_lossy(&o.stdout);
    let mut list: Value = serde_json::from_str(text.trim()).map_err(|e| {
        let err = String::from_utf8_lossy(&o.stderr);
        format!("claude plugin list: {}", if err.trim().is_empty() { e.to_string() } else { err.trim().to_string() })
    })?;
    let dir = plugins_dir();
    let marketplaces = read_json(&dir.join("known_marketplaces.json")).filter(Value::is_object).unwrap_or_else(|| Value::Object(Map::new()));
    let shas = read_json(&dir.join("installed_plugins.json")).unwrap_or_default();
    let installed = list.get_mut("installed").map(Value::take).unwrap_or_else(|| Value::Array(vec![]));
    let mut meta: BTreeMap<String, Meta> = BTreeMap::new();
    let get = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    // update = irgendein Eintrag (user/project/local) veraltet; SHA aus dem passenden Eintrag
    for p in installed.as_array().into_iter().flatten() {
        let Some(id) = p.get("id").and_then(Value::as_str) else { continue };
        let m = meta.entry(id.to_string()).or_insert_with(|| {
            let (name, mp) = id.split_once('@').unwrap_or((id, ""));
            let loc = marketplaces.get(mp).and_then(|m| m.get("installLocation")).and_then(Value::as_str);
            let (latest, description) = loc.map(|l| latest(Path::new(l), name)).unwrap_or_default();
            Meta { latest, update: false, description }
        });
        let Some(l) = m.latest.as_deref() else { continue };
        let entries = shas.get("plugins").and_then(|m| m.get(id)).and_then(Value::as_array);
        let same = |e: &&Value| get(e, "scope") == get(p, "scope") && get(e, "projectPath") == get(p, "projectPath");
        let sha = entries.and_then(|e| e.iter().find(same)).and_then(|e| e.get("gitCommitSha")).and_then(Value::as_str);
        let version = p.get("version").and_then(Value::as_str).unwrap_or_default();
        m.update |= outdated(version, sha, l);
    }
    let available = list.get_mut("available").map(Value::take).unwrap_or_else(|| Value::Array(vec![]));
    Ok(PluginsInfo { installed, available, marketplaces, meta })
}

#[tauri::command]
pub async fn plugins_list(path: String) -> Result<PluginsInfo, String> {
    blocking(move || plugins_info(&path)).await
}

#[tauri::command]
pub async fn plugins_run(
    path: String,
    action: String,
    target: String,
    scope: Option<String>,
    accept: Option<String>,
) -> Result<Value, String> {
    blocking(move || {
        let dir = plugins_dir();
        let installed = keys(read_json(&dir.join("installed_plugins.json")).as_ref().and_then(|v| v.get("plugins")));
        let marketplaces = keys(read_json(&dir.join("known_marketplaces.json")).as_ref());
        let args = plugin_args(&action, &target, scope.as_deref(), accept.as_deref(), &installed, &marketplaces)?;
        let o = claude(&path, &args)?;
        let out = String::from_utf8_lossy(&o.stdout);
        last_json(&out).ok_or_else(|| {
            let err = String::from_utf8_lossy(&o.stderr);
            if err.trim().is_empty() { out.trim().to_string() } else { err.trim().to_string() }
        })
    })
    .await
}

#[cfg(test)]
#[path = "skills_scan_tests.rs"]
mod scan_tests;
#[cfg(test)]
#[path = "skills_write_tests.rs"]
mod write_tests;
#[cfg(test)]
#[path = "skills_plugins_tests.rs"]
mod plugins_tests;
