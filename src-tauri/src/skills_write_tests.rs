use super::*;
use serde_json::json;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ocui-skills-write-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn obj(v: Value) -> Option<Map<String, Value>> {
    v.as_object().cloned()
}

fn local(repo: &Path) -> PathBuf {
    repo.join(".claude/settings.local.json")
}

fn read(repo: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(local(repo)).unwrap()).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git").current_dir(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

#[test]
fn user_settings_schreiben_nur_wenn_unveraendert() {
    let home = tmp("user");
    assert_eq!(user_read(&home).unwrap(), "");
    assert!(user_write(&home, "[1]", "").is_err());
    user_write(&home, r#"{"model":"opus"}"#, "").unwrap();
    assert_eq!(user_read(&home).unwrap(), r#"{"model":"opus"}"#);
    // veralteter Stand wird abgelehnt, Datei bleibt
    assert!(user_write(&home, "{}", "").is_err());
    assert_eq!(user_read(&home).unwrap(), r#"{"model":"opus"}"#);
}

#[test]
fn nichts_zu_schreiben_legt_nichts_an() {
    let repo = tmp("none");
    write_local(&repo, None, None).unwrap();
    assert!(!repo.join(".claude").exists());
}

#[test]
fn schreibt_und_erhaelt_andere_keys() {
    let repo = tmp("keep");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(local(&repo), r#"{"permissions":{"allow":["Bash"]},"skillOverrides":{"alt":"off"}}"#).unwrap();
    write_local(&repo, obj(json!({"a": "off"})), obj(json!({"p@m": false}))).unwrap();
    assert_eq!(
        read(&repo),
        json!({"permissions": {"allow": ["Bash"]}, "skillOverrides": {"a": "off"}, "enabledPlugins": {"p@m": false}})
    );
    // null entfernt die Keys, Rest bleibt; kein .tmp liegen gelassen.
    write_local(&repo, None, None).unwrap();
    assert_eq!(read(&repo), json!({"permissions": {"allow": ["Bash"]}}));
    assert!(!repo.join(".claude/settings.local.json.tmp").exists());
    // Leeres Objekt ist ein Wert (ueberdeckt User/Team), kein Entfernen.
    write_local(&repo, obj(json!({})), None).unwrap();
    assert_eq!(read(&repo)["skillOverrides"], json!({}));
}

#[test]
fn kaputtes_json_wird_nicht_ueberschrieben() {
    let repo = tmp("broken");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    for text in ["{kaputt", "[1]"] {
        std::fs::write(local(&repo), text).unwrap();
        assert!(write_local(&repo, obj(json!({"a": "off"})), None).is_err());
        assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), text);
    }
}

#[test]
fn traegt_in_git_exclude_ein() {
    let repo = tmp("exclude");
    git(&repo, &["init", "-q"]);
    // Globale Ignore-Datei des Rechners ausblenden (kann settings.local.json schon ignorieren).
    git(&repo, &["config", "core.excludesFile", "keine-datei"]);
    let exclude = repo.join(".git/info/exclude");
    write_local(&repo, obj(json!({"a": "off"})), None).unwrap();
    let text = std::fs::read_to_string(&exclude).unwrap();
    assert!(text.ends_with("**/.claude/settings.local.json\n"), "{text}");
    // Schon ignoriert: kein zweiter Eintrag.
    write_local(&repo, obj(json!({"b": "off"})), None).unwrap();
    assert_eq!(std::fs::read_to_string(&exclude).unwrap(), text);
    assert!(!repo.join(".gitignore").exists());
}

fn git_repo(name: &str) -> PathBuf {
    let repo = tmp(name);
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "core.excludesFile", "keine-datei"]);
    repo
}

fn entries(file: &Path) -> usize {
    std::fs::read_to_string(file).unwrap_or_default().matches("**/.claude/settings.local.json").count()
}

#[test]
fn verschachtelte_fremde_keys_bleiben_exakt() {
    let repo = tmp("nested");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    let fremd = json!({
        "permissions": {"allow": ["Bash(npm:*)", "Read"], "deny": [], "additionalDirectories": ["../x"]},
        "env": {"A": "1", "LEER": ""},
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "echo \"hi\""}]}]},
        "zahl": 1.5, "nix": null, "wahr": true, "umlaut": "ä ✓"
    });
    std::fs::write(local(&repo), fremd.to_string()).unwrap();
    write_local(&repo, obj(json!({"a": "off", "p:b": "name-only"})), obj(json!({"p@m": true}))).unwrap();
    let mut soll = fremd.clone();
    soll["skillOverrides"] = json!({"a": "off", "p:b": "name-only"});
    soll["enabledPlugins"] = json!({"p@m": true});
    assert_eq!(read(&repo), soll);
    // Nur einer der beiden Keys entfernt, der andere bleibt.
    write_local(&repo, None, obj(json!({"p@m": false}))).unwrap();
    let mut soll = fremd.clone();
    soll["enabledPlugins"] = json!({"p@m": false});
    assert_eq!(read(&repo), soll);
}

#[test]
fn claude_ordner_fehlt_wird_angelegt() {
    let repo = tmp("nodir");
    write_local(&repo, None, obj(json!({"p@m": false}))).unwrap();
    assert_eq!(read(&repo), json!({"enabledPlugins": {"p@m": false}}));
}

#[test]
fn leeres_ergebnis_ohne_eigene_keys_fasst_datei_nicht_an() {
    let repo = tmp("untouched");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    // Kompakt formatiert: ein Neuschreiben wuerde pretty-printen.
    let text = r#"{"permissions":{"allow":["Read"]}}"#;
    std::fs::write(local(&repo), text).unwrap();
    write_local(&repo, None, None).unwrap();
    assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), text);
    // Leere .claude ohne Datei: nichts anlegen.
    let repo = tmp("emptydir");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    write_local(&repo, None, None).unwrap();
    assert!(!local(&repo).exists());
}

#[test]
fn idempotent_und_ohne_neuschreiben() {
    let repo = tmp("idem");
    let so = json!({"b": "off", "a": "user-invocable-only"});
    let ep = json!({"x@y": true});
    write_local(&repo, obj(so.clone()), obj(ep.clone())).unwrap();
    let erst = std::fs::read_to_string(local(&repo)).unwrap();
    write_local(&repo, obj(so.clone()), obj(ep.clone())).unwrap();
    assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), erst);
    // Gleicher Inhalt, andere Formatierung: kein Neuschreiben noetig.
    let kompakt = read(&repo).to_string();
    std::fs::write(local(&repo), &kompakt).unwrap();
    write_local(&repo, obj(so), obj(ep)).unwrap();
    assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), kompakt);
}

#[test]
fn keine_temp_dateien_bleiben_liegen() {
    let repo = tmp("notmp");
    for i in 0..3 {
        write_local(&repo, obj(json!({ format!("s{i}"): "off" })), None).unwrap();
    }
    write_local(&repo, None, None).unwrap();
    let names: Vec<_> = std::fs::read_dir(repo.join(".claude"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["settings.local.json"]);
    assert_eq!(read(&repo), json!({}));
}

#[test]
fn kaputtes_json_bei_leerem_ergebnis_und_leere_datei() {
    let repo = tmp("broken2");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    // Auch "Keys entfernen" darf kaputte Dateien nicht anfassen.
    for text in ["{\"a\":", "", "null", "\"s\""] {
        std::fs::write(local(&repo), text).unwrap();
        assert!(write_local(&repo, None, None).is_err(), "{text:?}");
        assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), text);
    }
}

#[test]
fn repo_ohne_git_schreibt_ohne_git_ordner() {
    let repo = tmp("nogit");
    write_local(&repo, obj(json!({"a": "off"})), None).unwrap();
    assert_eq!(read(&repo), json!({"skillOverrides": {"a": "off"}}));
    assert!(!repo.join(".git").exists());
}

#[test]
fn gitignore_ignoriert_schon_kein_exclude_eintrag() {
    let repo = git_repo("ignored");
    std::fs::write(repo.join(".gitignore"), ".claude/settings.local.json\n").unwrap();
    write_local(&repo, obj(json!({"a": "off"})), None).unwrap();
    assert_eq!(entries(&repo.join(".git/info/exclude")), 0);
    assert_eq!(std::fs::read_to_string(repo.join(".gitignore")).unwrap(), ".claude/settings.local.json\n");
}

#[test]
fn exclude_ohne_schlusszeilenumbruch_und_genau_einmal() {
    let repo = git_repo("nonl");
    let exclude = repo.join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "*.log").unwrap();
    for i in 0..3 {
        write_local(&repo, obj(json!({ format!("s{i}"): "off" })), None).unwrap();
    }
    assert_eq!(std::fs::read_to_string(&exclude).unwrap(), "*.log\n**/.claude/settings.local.json\n");
}

#[test]
fn unterordner_eines_repos_nutzt_dessen_exclude() {
    let root = git_repo("mono");
    let repo = root.join("apps/web");
    std::fs::create_dir_all(&repo).unwrap();
    write_local(&repo, obj(json!({"a": "off"})), None).unwrap();
    assert_eq!(entries(&root.join(".git/info/exclude")), 1);
    assert!(!repo.join(".git").exists());
    let out = std::process::Command::new("git")
        .current_dir(&repo)
        .args(["check-ignore", "-q", ".claude/settings.local.json"])
        .status()
        .unwrap();
    assert!(out.success(), "Datei sollte jetzt ignoriert sein");
}

#[test]
fn getrackte_datei_wird_nicht_angefasst() {
    let repo = git_repo("tracked");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(local(&repo), "{}").unwrap();
    git(&repo, &["add", ".claude/settings.local.json"]);
    let err = write_local(&repo, obj(json!({"s": "off"})), None).unwrap_err();
    assert!(err.contains("eingecheckt"), "{err}");
    assert_eq!(std::fs::read_to_string(local(&repo)).unwrap(), "{}");
    assert_eq!(entries(&repo.join(".git/info/exclude")), 0);
}

#[test]
fn parallele_schreiber_zerstoeren_sich_keine_temp_datei() {
    let repo = tmp("parallel");
    let handles: Vec<_> = (0..16)
        .map(|i| {
            let repo = repo.clone();
            std::thread::spawn(move || write_local(&repo, obj(json!({ format!("s{i}"): "off" })), None))
        })
        .collect();
    for h in handles {
        h.join().unwrap().unwrap();
    }
    let names: Vec<_> = std::fs::read_dir(repo.join(".claude")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["settings.local.json"]);
    assert_eq!(read(&repo)["skillOverrides"].as_object().unwrap().len(), 1);
}
