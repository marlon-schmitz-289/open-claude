//! Workflow-Vorlagen: Skripte in <config>/workflows (global) und <cwd>/.claude/workflows (Projekt).
//! Dieselben Ordner, aus denen Claude Code benannte Workflows laedt.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::git::blocking;

/// Wie Claude Code: groessere Skripte werden nicht geladen.
const MAX: u64 = 524_288;

#[derive(Serialize)]
pub struct WfFile {
    scope: &'static str,
    file: String,
    path: String,
    text: String,
}

fn dir(scope: &str, cwd: &str) -> Result<PathBuf, String> {
    match scope {
        "user" => Ok(crate::activity::config_dir().join("workflows")),
        "project" => Ok(Path::new(cwd).join(".claude/workflows")),
        _ => Err(format!("Unbekannter Ort: {scope}")),
    }
}

fn list(dirs: &[(&'static str, PathBuf)]) -> Vec<WfFile> {
    let mut out = Vec::new();
    for (scope, d) in dirs {
        let Ok(rd) = fs::read_dir(d) else { continue };
        let mut files: Vec<_> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.metadata().is_ok_and(|m| m.len() <= MAX))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "js"))
            .collect();
        files.sort();
        for p in files {
            let (Some(file), Ok(text)) = (p.file_stem().and_then(|s| s.to_str()), fs::read_to_string(&p)) else { continue };
            out.push(WfFile { scope, file: file.into(), path: p.to_string_lossy().into(), text });
        }
    }
    out
}

fn save(dir: &Path, file: &str, text: &str) -> Result<(), String> {
    if !crate::pty::valid_id(file) {
        return Err(format!("Ungueltiger Name: {file}"));
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{file}.js"));
    // tmp+rename: Claude Code liest nie eine halb geschriebene Datei
    let tmp = dir.join(format!(".{file}.js.{}.tmp", std::process::id()));
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn delete(dir: &Path, file: &str) -> Result<(), String> {
    if !crate::pty::valid_id(file) {
        return Err(format!("Ungueltiger Name: {file}"));
    }
    fs::remove_file(dir.join(format!("{file}.js"))).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workflows_list(cwd: String) -> Result<Vec<WfFile>, String> {
    blocking(move || {
        let user = dir("user", &cwd)?;
        let project = dir("project", &cwd)?;
        // cwd = Home: Projekt-Ordner ist der globale, sonst doppelt gelistet
        let same = fs::canonicalize(&user).ok().is_some_and(|u| fs::canonicalize(&project).ok() == Some(u));
        let mut dirs = vec![("user", user)];
        if !same {
            dirs.push(("project", project));
        }
        Ok(list(&dirs))
    })
    .await
}

#[tauri::command]
pub async fn workflows_save(cwd: String, scope: String, file: String, text: String) -> Result<(), String> {
    blocking(move || save(&dir(&scope, &cwd)?, &file, &text)).await
}

#[tauri::command]
pub async fn workflows_delete(cwd: String, scope: String, file: String) -> Result<(), String> {
    blocking(move || delete(&dir(&scope, &cwd)?, &file)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ocui-workflows-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_list_delete() {
        let d = tmp("roundtrip");
        save(&d, "b", "export const meta = {}\n").unwrap();
        save(&d, "a", "x").unwrap();
        save(&d, "a", "y").unwrap();
        let l = list(&[("user", d.clone())]);
        assert_eq!(l.iter().map(|f| (f.scope, f.file.as_str(), f.text.as_str())).collect::<Vec<_>>(), [("user", "a", "y"), ("user", "b", "export const meta = {}\n")]);
        assert_eq!(l[0].path, d.join("a.js").to_string_lossy());
        // keine tmp-Reste
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        delete(&d, "a").unwrap();
        assert!(!d.join("a.js").exists());
        assert!(delete(&d, "a").is_err());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn list_skips_other_files() {
        let d = tmp("skip");
        fs::create_dir_all(d.join("sub.js")).unwrap();
        fs::write(d.join("x.mjs"), "").unwrap();
        fs::write(d.join("big.js"), vec![b' '; MAX as usize + 1]).unwrap();
        fs::write(d.join("ok.js"), "").unwrap();
        let l = list(&[("project", d.clone()), ("user", d.join("fehlt"))]);
        assert_eq!(l.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), ["ok"]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn rejects_bad_names() {
        let d = tmp("bad");
        for f in ["../x", "a/b", "", "a.b"] {
            assert!(save(&d, f, "x").is_err(), "{f}");
            assert!(delete(&d, f).is_err(), "{f}");
        }
        assert!(!d.exists());
        assert!(dir("other", "/x").is_err());
        assert_eq!(dir("project", "/x").unwrap(), Path::new("/x/.claude/workflows"));
    }
}
