use super::*;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ocui-skills-plugins-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn args(action: &str, target: &str, scope: Option<&str>, accept: Option<&str>) -> Result<Vec<String>, String> {
    plugin_args(action, target, scope, accept, &["a@b".into()], &["mp".into()])
}

fn ok(action: &str, target: &str, scope: Option<&str>, accept: Option<&str>) -> String {
    args(action, target, scope, accept).unwrap().join(" ")
}

#[test]
fn plugin_args_gueltig() {
    let sha = "a".repeat(64);
    assert_eq!(ok("install", "x.y_z-1@mp", None, None), "install x.y_z-1@mp --json -s user");
    assert_eq!(ok("install", "x@mp", Some("project"), Some(&sha)), format!("install x@mp --json -s project --accept-command {sha}"));
    assert_eq!(ok("update", "a@b", Some("local"), Some(&sha)), format!("update a@b --json -s local --accept-command {sha}"));
    assert_eq!(ok("uninstall", "a@b", Some("project"), None), "uninstall a@b --json -s project");
    assert_eq!(ok("enable", "a@b", Some("user"), None), "enable a@b --json -s user");
    assert_eq!(ok("disable", "a@b", Some("project"), None), "disable a@b --json -s project");
    assert_eq!(ok("mp-add", "owner/repo", None, None), "marketplace add owner/repo --json");
    assert_eq!(ok("mp-remove", "mp", None, None), "marketplace remove mp --json");
    assert_eq!(ok("mp-update", "mp", None, None), "marketplace update mp --json");
}

#[test]
fn plugin_args_abgelehnt() {
    for t in ["-x", "a b", "a;b", "x@", "@x", "x@y@z", "", "a\n@b"] {
        assert!(args("install", t, None, None).is_err(), "{t}");
    }
    assert!(args("enable", "nope@nope", None, None).is_err());
    assert!(args("install", "x@y", Some("local"), None).is_err());
    assert!(args("enable", "a@b", Some("local"), None).is_err());
    assert!(args("uninstall", "a@b", Some("local"), None).is_err());
    assert!(args("update", "a@b", Some("foo"), None).is_err());
    assert!(args("update", "a@b", None, Some("abc")).is_err());
    assert!(args("update", "a@b", None, Some(&"g".repeat(64))).is_err());
    assert!(args("mp-remove", "unbekannt", None, None).is_err());
    assert!(args("rm", "a@b", None, None).is_err());
}

#[test]
fn plugin_args_mp_add_quellen() {
    let dir = tmp("mpadd");
    for t in ["http://a/b", "-foo", "rel/pfad/x", "owner/re po", "/gibt/es/nicht"] {
        assert!(args("mp-add", t, None, None).is_err(), "{t}");
    }
    for t in ["owner/repo", "https://example.com/r.git", "git@github.com:o/r.git", "ssh://git@h/r", dir.to_str().unwrap()] {
        assert!(args("mp-add", t, None, None).is_ok(), "{t}");
    }
}

#[test]
fn latest_quellen() {
    let dir = tmp("latest");
    let mp = |d: &Path, entry: &str| put(&d.join(".claude-plugin/marketplace.json"), &format!(r#"{{"plugins":[{entry}]}}"#));
    let a = dir.join("a");
    mp(&a, r#"{"name":"p","version":"1.0.0","description":"D","source":"./"}"#);
    assert_eq!(latest(&a, "p"), (Some("1.0.0".into()), "D".into()));
    assert_eq!(latest(&a, "fehlt"), (None, String::new()));

    let b = dir.join("b");
    mp(&b, r#"{"name":"p","source":"./sub"}"#);
    put(&b.join("sub/.claude-plugin/plugin.json"), r#"{"version":"3.2.0","description":"aus plugin"}"#);
    assert_eq!(latest(&b, "p"), (Some("3.2.0".into()), "aus plugin".into()));

    let c = dir.join("c");
    mp(&c, r#"{"name":"p","source":{"source":"git-subdir","sha":"abc"}}"#);
    assert_eq!(latest(&c, "p").0.as_deref(), Some("abc"));

    let d = dir.join("d");
    mp(&d, r#"{"name":"p","source":"./"}"#);
    let git = |args: &[&str]| {
        let o = crate::quiet("git").arg("-C").arg(&d).args(args).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"]);
    assert_eq!(latest(&d, "p").0, Some(git(&["rev-parse", "HEAD"])));

    assert_eq!(latest(&dir.join("leer"), "p"), (None, String::new()));
}

#[test]
fn outdated_echte_faelle() {
    let sha = "ad30d62cd52ad0d0af1beda1eab7f61c601045d0";
    assert!(outdated("63e797cd753b", Some("63e797cd753b301374947a5ed975c21775d962b9"), "3.2.0"));
    assert!(!outdated("4.8.4", Some("16f29800fd2681bdf24f3eb4ccffe38be3baec6b"), "4.8.4"));
    assert!(!outdated("1.0.0", Some(sha), "1.0.0"));
    assert!(!outdated("x", Some(sha), sha));
    assert!(outdated("x", Some(sha), &"b".repeat(40)));
    assert!(outdated("x", None, sha));
}

#[test]
fn last_json_zeilen() {
    assert_eq!(last_json("Hinweis\n{\"outcome\":\"ok\"}\n\n  \n"), Some(serde_json::json!({"outcome": "ok"})));
    assert_eq!(last_json("kein json\n"), None);
    assert_eq!(last_json("[1,2]\n"), None);
    assert_eq!(last_json(""), None);
}
