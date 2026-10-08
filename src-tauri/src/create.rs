//! Neues Repo lokal anlegen: Ordner, Scaffold ueber das offizielle Tool (falls installiert),
//! .gitignore, README, LICENSE, git init -b main, erster Commit. Remote: forge::forge_create_remote.

use std::path::Path;
use std::process::Stdio;

use serde::Serialize;
use tauri::AppHandle;

struct Lang {
    id: &'static str,
    label: &'static str,
    tool: Option<&'static str>,
    ignore: &'static str,
}

const LANGS: &[Lang] = &[
    Lang { id: "none", label: "Leer", tool: None, ignore: "" },
    Lang { id: "node", label: "Node", tool: Some("npm"), ignore: "node_modules/\ndist/\n.env\n*.log\n" },
    Lang { id: "rust", label: "Rust", tool: Some("cargo"), ignore: "/target\n" },
    Lang { id: "dotnet", label: ".NET", tool: Some("dotnet"), ignore: "bin/\nobj/\n.vs/\n*.user\n" },
    Lang {
        id: "python",
        label: "Python",
        tool: Some("uv"),
        ignore: "__pycache__/\n*.py[cod]\n.venv/\n.env\ndist/\n*.egg-info/\n",
    },
    Lang { id: "go", label: "Go", tool: Some("go"), ignore: "/bin/\n*.exe\n*.test\n*.out\n" },
    // Unity-Projekte entstehen im Hub, hier nur die .gitignore.
    Lang {
        id: "unity",
        label: "Unity",
        tool: None,
        ignore: "/[Ll]ibrary/\n/[Tt]emp/\n/[Oo]bj/\n/[Bb]uild/\n/[Bb]uilds/\n/[Ll]ogs/\n/[Uu]ser[Ss]ettings/\n\
/[Mm]emoryCaptures/\n.vs/\n.idea/\n*.csproj\n*.sln\n*.user\n*.pidb\n*.apk\n*.aab\n",
    },
];

#[derive(Serialize)]
pub struct RepoLang {
    id: &'static str,
    label: &'static str,
    tool: Option<&'static str>,
    found: bool,
}

const MIT: &str = "MIT License

Copyright (c) {year} {holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
";

fn lang(id: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.id == id)
}

/// Wie NAME im Frontend: Buchstaben, Ziffern, . _ -, kein fuehrendes - oder ., kein
/// abschliessender ., max. 100. Dazu keine Windows-Geraetenamen (con, nul, com1, ...).
fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    let stem = n.split('.').next().unwrap_or("").to_ascii_lowercase();
    let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit());
    c.next().is_some_and(|f| f.is_ascii_alphanumeric() || f == '_')
        && n.len() <= 100
        && !n.ends_with('.')
        && !device
        && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '.' | '_' | '-'))
}

/// cargo will Kleinbuchstaben, Ziffern, - und _, keine fuehrende Ziffer: "My.App" -> "my-app".
fn cargo_name(n: &str) -> String {
    let s: String = n
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) { format!("_{s}") } else { s }
}

/// Jahr (UTC) zu Unix-Sekunden, civil_from_days nach H. Hinnant (Gegenstueck zu forge::unix_secs).
fn year(secs: i64) -> i64 {
    let z = secs.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    // Maerz-basiertes Jahr: Januar und Februar gehoeren schon zum naechsten.
    era * 400 + yoe + i64::from(mp >= 10)
}

/// Argumente fuer das Scaffold-Tool der Sprache; None = kein Tool.
fn scaffold_args(id: &str, name: &str, mit: bool) -> Option<Vec<String>> {
    let a: Vec<&str> = match id {
        "node" if mit => vec!["init", "-y", "--init-license=MIT"],
        "node" => vec!["init", "-y"],
        "rust" => return Some(["init", "--vcs", "none", "--name", &cargo_name(name)].map(String::from).to_vec()),
        "dotnet" => vec!["new", "console", "-n", name, "-o", ".", "--no-restore"],
        "python" => vec!["init", "--vcs", "none", "--name", name],
        "go" => vec!["mod", "init", name],
        _ => return None,
    };
    Some(a.into_iter().map(String::from).collect())
}

// async: on_path stattet jeden PATH-Eintrag ab, nicht auf dem UI-Thread.
#[tauri::command(async)]
pub fn repo_langs() -> Vec<RepoLang> {
    LANGS
        .iter()
        .map(|l| RepoLang { id: l.id, label: l.label, tool: l.tool, found: l.tool.is_none_or(crate::deps::on_path) })
        .collect()
}

/// Prueft alles vor dem Anlegen; true = Ordner wurde hier neu angelegt.
fn prepare(dest: &str, name: &str, lang_id: &str) -> Result<bool, String> {
    if !valid_name(name) {
        return Err(format!("Ungültiger Name: „{name}“"));
    }
    lang(lang_id).ok_or_else(|| format!("Unbekannte Sprache: {lang_id}"))?;
    let target = Path::new(dest);
    if !target.is_absolute() {
        return Err(format!("Zielordner muss ein absoluter Pfad sein: {dest}"));
    }
    let existed = target.exists();
    if existed && std::fs::read_dir(target).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("Zielordner ist nicht leer: {dest}"));
    }
    std::fs::create_dir_all(target).map_err(|e| format!("{dest}: {e}"))?;
    Ok(!existed)
}

/// Datei nur schreiben, wenn das Scaffold sie nicht schon angelegt hat.
fn put_new(dest: &str, file: &str, text: &str) -> Result<(), String> {
    let p = Path::new(dest).join(file);
    if p.exists() {
        return Ok(());
    }
    std::fs::write(&p, text).map_err(|e| format!("{file}: {e}"))
}

fn fill(dest: &str, name: &str, lang_id: &str, mit: bool) -> Result<(), String> {
    let l = lang(lang_id).ok_or_else(|| format!("Unbekannte Sprache: {lang_id}"))?;
    if let (Some(tool), Some(args)) = (l.tool.filter(|t| crate::deps::on_path(t)), scaffold_args(l.id, name, mit)) {
        let exe = if cfg!(windows) && tool == "npm" { "npm.cmd" } else { tool };
        let o = crate::quiet(exe)
            .args(&args)
            .current_dir(dest)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{tool} nicht startbar: {e}"))?;
        if !o.status.success() {
            let err = String::from_utf8_lossy(if o.stderr.is_empty() { &o.stdout } else { &o.stderr }).into_owned();
            let lines: Vec<&str> = err.trim().lines().collect();
            return Err(format!("{tool}: {}", lines[lines.len().saturating_sub(12)..].join("\n")));
        }
    }
    if !l.ignore.is_empty() {
        put_new(dest, ".gitignore", l.ignore)?;
    }
    put_new(dest, "README.md", &format!("# {name}\n"))?;
    if mit {
        let holder = crate::git::config(dest, "user.name").unwrap_or_default();
        let text = MIT.replace("{year}", &year(crate::forge::now_secs()).to_string()).replace("{holder}", &holder);
        put_new(dest, "LICENSE", &text)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn repo_create(
    app: AppHandle,
    dest: String,
    name: String,
    lang: String,
    mit: bool,
    account: Option<String>,
) -> Result<String, String> {
    let created = {
        let (d, n, l) = (dest.clone(), name.clone(), lang.clone());
        crate::git::blocking(move || prepare(&d, &n, &l)).await?
    };
    let res = async {
        let d = dest.clone();
        crate::git::blocking(move || crate::git::init(&d)).await?;
        // Vor dem Commit binden: Identitaet fuer den Commit, Token fuer den spaeteren Push.
        if let Some(id) = account {
            crate::forge::forge_set_repo_account(app, dest.clone(), Some(id)).await?;
        }
        let d = dest.clone();
        crate::git::blocking(move || {
            fill(&d, &name, &lang, mit)?;
            crate::git::commit_all(&d, "Initial commit")
        })
        .await
    }
    .await;
    match res {
        Ok(_) => Ok(dest),
        Err(e) if created => match std::fs::remove_dir_all(&dest) {
            Ok(()) => Err(e),
            Err(r) => Err(format!("{e}\n(Ordner {dest} konnte nicht entfernt werden: {r})")),
        },
        // Vorher schon vorhandene Ordner nie loeschen.
        Err(e) => Err(format!("{e}\n(Ordner {dest} ist teilweise befüllt)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("ocui-create-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_str().unwrap().to_string()
    }

    fn git(r: &str, args: &[&str]) -> String {
        let o = crate::git::git(r).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    #[test]
    fn prueft_namen() {
        for n in ["my-app", "My.App", "_1", "console", "com", "a.b"] {
            assert!(valid_name(n), "{n}");
        }
        let long = "a".repeat(101);
        for n in ["", "-x", ".x", "a b", "a/b", "..", "foo.", "con", "NUL", "con.txt", "com1", "LPT9.x", long.as_str()] {
            assert!(!valid_name(n), "{n}");
        }
        assert!(valid_name(&"a".repeat(100)));
    }

    #[test]
    fn cargo_namen() {
        assert_eq!(cargo_name("My.App"), "my-app");
        assert_eq!(cargo_name("1app"), "_1app");
        assert_eq!(cargo_name("ok_name"), "ok_name");
    }

    #[test]
    fn jahr() {
        assert_eq!(year(0), 1970);
        assert_eq!(year(1767225600), 2026);
        assert_eq!(year(1767222000), 2025);
        assert_eq!(year(1709208000), 2024); // 29.2.2024
        assert_eq!(year(1735689599), 2024); // 31.12.2024 23:59:59
    }

    #[test]
    fn tabelle() {
        let ids: std::collections::HashSet<_> = LANGS.iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), LANGS.len());
        for l in LANGS {
            assert_eq!(l.id == "none", l.ignore.is_empty(), "{}", l.id);
            let args = scaffold_args(l.id, "x-name", true);
            assert_eq!(args.is_some(), l.tool.is_some(), "{}", l.id);
            // Name steht nie als Option da (valid_name verbietet fuehrendes -), nur als Wert.
            for a in args.unwrap_or_default() {
                assert!(!a.starts_with("-x") && !a.starts_with("--x"), "{}: {a}", l.id);
            }
        }
    }

    #[test]
    fn legt_lokal_an() {
        let d = tmp("unity");
        assert!(prepare(&d, "name", "unity").unwrap());
        crate::git::init(&d).unwrap();
        for (k, v) in [("user.name", "Test Person"), ("user.email", "t@x.de"), ("commit.gpgsign", "false")] {
            git(&d, &["config", k, v]);
        }
        fill(&d, "name", "unity", true).unwrap();
        crate::git::commit_all(&d, "Initial commit").unwrap();

        assert_eq!(git(&d, &["branch", "--show-current"]), "main");
        assert_eq!(git(&d, &["rev-list", "--count", "HEAD"]), "1");
        let read = |f: &str| std::fs::read_to_string(Path::new(&d).join(f)).unwrap();
        assert!(read("README.md").contains("# name"));
        let lic = read("LICENSE");
        assert!(lic.contains("Test Person") && lic.contains(&year(crate::forge::now_secs()).to_string()), "{lic}");
        assert!(read(".gitignore").contains("[Ll]ibrary"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn lehnt_ziel_ab() {
        // nicht leer: Datei bleibt
        let d = tmp("voll");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(Path::new(&d).join("a.txt"), "x").unwrap();
        assert!(prepare(&d, "x", "none").unwrap_err().contains("nicht leer"));
        assert!(Path::new(&d).join("a.txt").exists());
        // Ziel ist Datei
        let f = Path::new(&d).join("a.txt");
        assert!(prepare(f.to_str().unwrap(), "x", "none").is_err());
        std::fs::remove_dir_all(&d).unwrap();

        assert!(prepare("relativ/x", "x", "none").is_err());
        assert!(!Path::new("relativ").exists());
        let d = tmp("falsch");
        assert!(prepare(&d, "x", "cobol").is_err());
        assert!(prepare(&d, "-x", "none").is_err());
        assert!(!Path::new(&d).exists());
    }
}
