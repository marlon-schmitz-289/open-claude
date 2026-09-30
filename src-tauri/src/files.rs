//! Dateizugriff fuer den eingebauten Editor: Baum, Lesen, Schreiben mit Konflikterkennung.
//! Alle Pfade sind relativ zum Repo und werden gegen Ausbruch geprueft (`..`, `.git`, Symlinks).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::git::{blocking, exec, git, inside};

/// Groessere Dateien bremsen den Editor aus und sind fast nie Quelltext.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const BOM: &str = "\u{feff}";

#[derive(Serialize)]
pub struct Entry {
    name: String,
    dir: bool,
    /// Von git ignoriert (node_modules, target ...): der Baum zeigt sie gedimmt.
    ignored: bool,
}

#[derive(Serialize, Debug)]
pub struct FileText {
    /// Immer mit LF und ohne BOM; `crlf`/`bom` stellen das Original beim Schreiben wieder her.
    content: String,
    crlf: bool,
    bom: bool,
    mtime: u64,
}

/// Pfad im Repo, der auch ueber Symlinks nicht nach draussen zeigt.
fn within(repo: &str, path: &str) -> Result<PathBuf, String> {
    let file = inside(repo, path)?;
    let root = Path::new(repo).canonicalize().map_err(|e| format!("Projekt nicht gefunden: {e}"))?;
    // Neue Dateien gibt es noch nicht: den tiefsten vorhandenen Vorfahren pruefen.
    let mut probe = file.as_path();
    let real = loop {
        match probe.canonicalize() {
            Ok(p) => break p,
            Err(_) => probe = probe.parent().ok_or_else(|| format!("Ungültiger Pfad: {path}"))?,
        }
    };
    if !real.starts_with(&root) {
        return Err(format!("Liegt außerhalb des Projekts: {path}"));
    }
    Ok(file)
}

/// Millisekunden seit 1970; 0, wenn das Dateisystem keine Aenderungszeit liefert.
fn mtime(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let at = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    Some(at.map_or(0, |d| d.as_millis() as u64))
}

fn list(repo: &str, dir: &str) -> Result<Vec<Entry>, String> {
    let base = if dir.is_empty() { PathBuf::from(repo) } else { within(repo, dir)? };
    let read = std::fs::read_dir(&base).map_err(|e| format!("Ordner nicht lesbar: {e}"))?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // metadata statt file_type: ein Symlink auf einen Ordner zaehlt als Ordner.
            let dir = std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
            (name != ".git").then_some(Entry { name, dir, ignored: false })
        })
        .collect();
    entries.sort_by_key(|e| (!e.dir, e.name.to_lowercase()));

    // Ein git-Aufruf fuer die ganze Ebene. Schlaegt er fehl, gilt nichts als ignoriert.
    let rel = |name: &str| if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };
    let input = entries.iter().map(|e| rel(&e.name)).collect::<Vec<_>>().join("\0");
    if let Ok(o) = exec(git(repo).args(["check-ignore", "-z", "--stdin"]), Some(input.as_bytes())) {
        let hit = String::from_utf8_lossy(&o.stdout).into_owned();
        let hit: std::collections::HashSet<&str> = hit.split('\0').collect();
        for e in &mut entries {
            e.ignored = hit.contains(rel(&e.name).as_str());
        }
    }
    Ok(entries)
}

fn read(repo: &str, path: &str) -> Result<FileText, String> {
    let file = within(repo, path)?;
    let meta = std::fs::metadata(&file).map_err(|e| format!("{path} nicht lesbar: {e}"))?;
    if meta.len() > MAX_BYTES {
        return Err(format!("{path} ist größer als 5 MB."));
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("{path} nicht lesbar: {e}"))?;
    let raw = match String::from_utf8(bytes) {
        Ok(s) if !s.contains('\0') => s,
        _ => return Err(format!("{path} ist binär oder kein UTF-8.")),
    };
    let bom = raw.starts_with(BOM);
    let body = raw.strip_prefix(BOM).unwrap_or(&raw);
    // ponytail: gemischte Zeilenenden werden beim Speichern zu CRLF vereinheitlicht.
    let crlf = body.contains("\r\n");
    Ok(FileText { content: body.replace("\r\n", "\n"), crlf, bom, mtime: mtime(&file).unwrap_or(0) })
}

/// Schreibt nur, wenn die Datei seit dem Lesen unveraendert ist (`expected`), sonst Fehler "KONFLIKT".
/// `expected` None: die Datei darf noch nicht existieren bzw. wird bewusst ueberschrieben (`force`).
fn write(
    repo: &str,
    path: &str,
    content: &str,
    crlf: bool,
    bom: bool,
    expected: Option<u64>,
    force: bool,
) -> Result<u64, String> {
    let file = within(repo, path)?;
    // Hinter einem Symlink die echte Datei ersetzen, nicht den Link.
    let target = file.canonicalize().unwrap_or(file);
    if !force && mtime(&target) != expected {
        return Err("KONFLIKT".into());
    }
    let mut data = String::with_capacity(content.len() + 3);
    if bom {
        data.push_str(BOM);
    }
    if crlf {
        data.push_str(&content.replace('\n', "\r\n"));
    } else {
        data.push_str(content);
    }

    // Erst daneben schreiben, dann umbenennen: ein Absturz hinterlaesst nie eine halbe Datei.
    let name = target.file_name().ok_or_else(|| format!("Ungültiger Pfad: {path}"))?.to_string_lossy();
    let tmp = target.with_file_name(format!(".{name}.ocui-tmp"));
    let err = |e: std::io::Error| format!("Konnte {path} nicht schreiben: {e}");
    let done = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.sync_all()?;
        // Rechte (ausfuehrbar) der alten Datei behalten.
        if let Ok(meta) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, meta.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(e));
    }
    Ok(mtime(&target).unwrap_or(0))
}

fn create(repo: &str, path: &str, dir: bool) -> Result<(), String> {
    let file = within(repo, path)?;
    let res = if dir {
        std::fs::create_dir(&file)
    } else {
        // create_new: eine vorhandene Datei wird nie geleert.
        std::fs::OpenOptions::new().write(true).create_new(true).open(&file).map(|_| ())
    };
    res.map_err(|e| format!("Konnte {path} nicht anlegen: {e}"))
}

fn rename(repo: &str, from: &str, to: &str) -> Result<(), String> {
    let (src, dst) = (within(repo, from)?, within(repo, to)?);
    // Nur Gross-/Kleinschreibung geaendert: auf Windows/macOS ist das dieselbe Datei.
    if dst.exists() && !from.eq_ignore_ascii_case(to) {
        return Err(format!("{to} gibt es schon."));
    }
    std::fs::rename(&src, &dst).map_err(|e| format!("Konnte {from} nicht umbenennen: {e}"))
}

#[tauri::command]
pub async fn fs_list(repo: String, dir: String) -> Result<Vec<Entry>, String> {
    blocking(move || list(&repo, &dir)).await
}

#[tauri::command]
pub async fn fs_read(repo: String, path: String) -> Result<FileText, String> {
    blocking(move || read(&repo, &path)).await
}

#[tauri::command]
pub async fn fs_write(
    repo: String,
    path: String,
    content: String,
    crlf: bool,
    bom: bool,
    expected: Option<u64>,
    force: bool,
) -> Result<u64, String> {
    blocking(move || write(&repo, &path, &content, crlf, bom, expected, force)).await
}

/// Aenderungszeiten fuer das Polling; None = geloescht. Leerer Pfad = Projektordner.
#[tauri::command]
pub async fn fs_stat(repo: String, paths: Vec<String>) -> Result<Vec<Option<u64>>, String> {
    blocking(move || {
        Ok(paths
            .iter()
            .map(|p| if p.is_empty() { mtime(Path::new(&repo)) } else { within(&repo, p).ok().and_then(|f| mtime(&f)) })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn fs_create(repo: String, path: String, dir: bool) -> Result<(), String> {
    blocking(move || create(&repo, &path, dir)).await
}

#[tauri::command]
pub async fn fs_rename(repo: String, from: String, to: String) -> Result<(), String> {
    blocking(move || rename(&repo, &from, &to)).await
}

/// In den Papierkorb, damit ein Fehlklick wiederherstellbar bleibt.
#[tauri::command]
pub async fn fs_delete(repo: String, path: String) -> Result<(), String> {
    blocking(move || {
        let file = within(&repo, &path)?;
        trash::delete(&file).map_err(|e| format!("Löschen fehlgeschlagen: {e}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("ocui-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn kein_ausbruch() {
        let repo = tmp("ausbruch");
        for bad in ["../x", "src/../../x", "/etc/passwd", ".git/config", "src/.git/x", ""] {
            assert!(within(&repo, bad).is_err(), "{bad}");
            assert!(write(&repo, bad, "x", false, false, None, true).is_err(), "{bad}");
        }
        assert!(within(&repo, "src/neu/tief.rs").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn kein_ausbruch_per_symlink() {
        let repo = tmp("symlink");
        let outside = tmp("symlink-ziel");
        std::os::unix::fs::symlink(&outside, Path::new(&repo).join("raus")).unwrap();
        assert!(within(&repo, "raus/x.txt").is_err());
        assert!(write(&repo, "raus/x.txt", "x", false, false, None, true).is_err());
        assert!(!Path::new(&outside).join("x.txt").exists());
    }

    #[test]
    fn crlf_und_bom_bleiben() {
        let repo = tmp("crlf");
        let file = Path::new(&repo).join("a.xaml");
        std::fs::write(&file, "\u{feff}<a>\r\n</a>\r\n").unwrap();
        let t = read(&repo, "a.xaml").unwrap();
        assert_eq!((t.content.as_str(), t.crlf, t.bom), ("<a>\n</a>\n", true, true));
        write(&repo, "a.xaml", "<b>\n</b>\n", t.crlf, t.bom, Some(t.mtime), false).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "\u{feff}<b>\r\n</b>\r\n");
    }

    #[test]
    fn konflikt_statt_ueberschreiben() {
        let repo = tmp("konflikt");
        let file = Path::new(&repo).join("a.txt");
        std::fs::write(&file, "alt").unwrap();
        let t = read(&repo, "a.txt").unwrap();
        // Jemand anderes (Claude im Terminal) schreibt dazwischen.
        let stale = Some(t.mtime.wrapping_sub(1000));
        assert_eq!(write(&repo, "a.txt", "meins", false, false, stale, false).unwrap_err(), "KONFLIKT");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "alt");
        // Datei inzwischen geloescht: ebenfalls Konflikt.
        std::fs::remove_file(&file).unwrap();
        assert_eq!(write(&repo, "a.txt", "meins", false, false, Some(t.mtime), false).unwrap_err(), "KONFLIKT");
        // Bewusst ueberschreiben geht.
        write(&repo, "a.txt", "meins", false, false, None, true).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "meins");
        assert!(!Path::new(&repo).join(".a.txt.ocui-tmp").exists());
    }

    #[test]
    fn binaer_abgelehnt_und_liste_ohne_git() {
        let repo = tmp("liste");
        std::fs::write(Path::new(&repo).join("b.bin"), [0u8, 159, 146, 150]).unwrap();
        std::fs::create_dir_all(Path::new(&repo).join(".git")).unwrap();
        assert!(read(&repo, "b.bin").is_err());
        let names: Vec<_> = list(&repo, "").unwrap().into_iter().map(|e| (e.name, e.dir)).collect();
        assert_eq!(names, [("src".to_string(), true), ("b.bin".to_string(), false)]);
        assert!(create(&repo, "b.bin", false).is_err());
        create(&repo, "src/neu.rs", false).unwrap();
        assert!(rename(&repo, "src/neu.rs", "b.bin").is_err());
        rename(&repo, "src/neu.rs", "src/alt.rs").unwrap();
        assert!(Path::new(&repo).join("src/alt.rs").exists());
    }
}
