//! Git-Client ueber die git-CLI. Formen und Namen wie in src/lib/git.ts.
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct FileChange {
    path: String,
    orig: Option<String>,
    index: String,
    worktree: String,
    conflict: bool,
}

#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum RepoState {
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Status {
    branch: Option<String>,
    head: String,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    state: RepoState,
    files: Vec<FileChange>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Commit {
    sha: String,
    parents: Vec<String>,
    author: String,
    email: String,
    date: String,
    subject: String,
    refs: Vec<String>,
}

#[derive(Serialize)]
pub struct CommitDetail {
    commit: Commit,
    body: String,
    diff: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Branch {
    name: String,
    remote: bool,
    current: bool,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    sha: String,
    date: String,
    subject: String,
    /// Commits, die noch nicht in `base` sind; None ohne Basis-Branch.
    unmerged: Option<u32>,
    base: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Stash {
    index: u32,
    message: String,
    date: String,
    branch: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Tag {
    name: String,
    sha: String,
    date: String,
    message: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Remote {
    name: String,
    url: String,
}

#[derive(Serialize)]
pub struct Conflict {
    path: String,
    base: Option<String>,
    ours: Option<String>,
    theirs: Option<String>,
    merged: String,
}

// ---------- Prozess ----------

/// git im Repo, ohne Prompts, Editor und Farben; englische Meldungen zum Parsen.
fn git(repo: &str) -> Command {
    let mut cmd = crate::quiet("git");
    cmd.arg("-C")
        .arg(repo)
        .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
        // Patches brauchen a/- und b/-Praefixe (Frontend-Parser, git apply -p1)
        .args(["-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    cmd
}

/// Fuer Nutzerpfade: keine Pathspec-Magie. Nicht global, stash nutzt intern selbst Magie.
fn lit(repo: &str) -> Command {
    let mut cmd = git(repo);
    cmd.env("GIT_LITERAL_PATHSPECS", "1");
    cmd
}

fn exec(cmd: &mut Command, input: Option<&[u8]>) -> Result<Output, String> {
    let res = match input {
        None => cmd.output(),
        Some(data) => cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                // stdin schliessen (drop), sonst wartet git ewig
                let written = child.stdin.take().map(|mut s| s.write_all(data));
                let out = child.wait_with_output()?;
                written.transpose()?;
                Ok(out)
            }),
    };
    res.map_err(|e| format!("git konnte nicht gestartet werden: {e}"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Fuer den Merge-Editor: lossy dekodiert wuerde resolve() die Datei beschaedigen.
fn strict(bytes: Vec<u8>) -> Result<String, String> {
    match String::from_utf8(bytes) {
        Ok(s) if !s.contains('\0') => Ok(s),
        _ => Err("Binär- oder Nicht-UTF-8-Datei: Ours oder Theirs komplett übernehmen".into()),
    }
}

fn fail(o: &Output) -> String {
    let err = text(&o.stderr).trim().to_string();
    if err.is_empty() {
        text(&o.stdout).trim().to_string()
    } else {
        err
    }
}

/// stdout bei Erfolg, sonst stderr als Fehler.
fn out(cmd: &mut Command) -> Result<String, String> {
    let o = exec(cmd, None)?;
    if o.status.success() {
        Ok(text(&o.stdout))
    } else {
        Err(fail(&o))
    }
}

/// Fuer Aktionen: die Meldung (stdout + stderr) geht so oder so ans Frontend.
fn talk(cmd: &mut Command) -> Result<String, String> {
    let o = exec(cmd, None)?;
    let msg = format!("{}\n{}", text(&o.stdout).trim(), text(&o.stderr).trim())
        .trim()
        .to_string();
    if o.status.success() {
        Ok(msg)
    } else {
        Err(msg)
    }
}

fn ok(cmd: &mut Command) -> bool {
    exec(cmd, None).is_ok_and(|o| o.status.success())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("Git abgebrochen: {e}"))?
}

// ---------- Eingaben pruefen ----------

/// Refs/Revs aus dem Frontend duerfen nie als Option durchgehen.
fn arg(s: &str) -> Result<&str, String> {
    if s.is_empty() || s.starts_with('-') {
        Err(format!("Ungültiger Name: „{s}“"))
    } else {
        Ok(s)
    }
}

fn branch_name<'a>(repo: &str, name: &'a str) -> Result<&'a str, String> {
    arg(name)?;
    if ok(git(repo).args(["check-ref-format", "--branch", name])) {
        Ok(name)
    } else {
        Err(format!("Ungültiger Branch-Name: „{name}“"))
    }
}

/// Relativer Pfad im Repo, ohne `..`, absolute Anteile oder `.git`.
fn inside(repo: &str, path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let bad = |c: Component| match c {
        // Windows oeffnet ".git." bzw. ".git " als .git, der 8.3-Kurzname ist GIT~1.
        Component::Normal(n) => {
            let n = n.to_string_lossy();
            let n = n.trim_end_matches(['.', ' ']);
            n.eq_ignore_ascii_case(".git") || n.eq_ignore_ascii_case("git~1")
        }
        _ => true,
    };
    if path.is_empty() || p.components().any(bad) {
        return Err(format!("Ungültiger Pfad: {path}"));
    }
    Ok(Path::new(repo).join(p))
}

fn ref_exists(repo: &str, full: &str) -> bool {
    ok(git(repo).args(["show-ref", "--verify", "--quiet", full]))
}

fn has_head(repo: &str) -> bool {
    ok(git(repo).args(["rev-parse", "--verify", "--quiet", "HEAD"]))
}

fn has_conflicts(repo: &str) -> bool {
    out(git(repo).args(["diff", "--name-only", "--diff-filter=U"]))
        .is_ok_and(|s| !s.trim().is_empty())
}

/// Konflikte sind kein Fehler: das Frontend liest danach den Status.
fn tolerate(repo: &str, cmd: &mut Command) -> Result<String, String> {
    let before = has_conflicts(repo);
    match talk(cmd) {
        // Stash: Konflikt im getrackten Teil, aber untracked Dateien fehlen -> echter Fehler.
        Err(msg)
            if !before
                && has_conflicts(repo)
                && !msg.contains("could not restore untracked files") =>
        {
            Ok(msg)
        }
        res => res,
    }
}

// ---------- Parser ----------

fn parse_status(raw: &str) -> Status {
    let mut s = Status {
        branch: None,
        head: String::new(),
        upstream: None,
        ahead: 0,
        behind: 0,
        state: RepoState::Clean,
        files: Vec::new(),
    };
    let change = |xy: &str, path: &str, orig: Option<&str>, conflict: bool| FileChange {
        path: path.to_string(),
        orig: orig.map(str::to_string),
        index: xy.get(0..1).unwrap_or(".").to_string(),
        worktree: xy.get(1..2).unwrap_or(".").to_string(),
        conflict,
    };
    let mut recs = raw.split('\0');
    while let Some(rec) = recs.next() {
        if let Some(h) = rec.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" if val != "(initial)" => s.head = val.to_string(),
                "branch.head" if val != "(detached)" => s.branch = Some(val.to_string()),
                "branch.upstream" => s.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            s.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            s.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        // Pfad ist immer das letzte Feld und darf Leerzeichen enthalten.
        let f: Vec<&str> = match rec.as_bytes().first() {
            Some(b'1') => rec.splitn(9, ' ').collect(),
            Some(b'2') => rec.splitn(10, ' ').collect(),
            Some(b'u') => rec.splitn(11, ' ').collect(),
            Some(b'?') => {
                if let Some(p) = rec.strip_prefix("? ") {
                    s.files.push(change("??", p, None, false));
                }
                continue;
            }
            _ => continue,
        };
        match (f[0], f.len()) {
            ("1", 9) => s.files.push(change(f[1], f[8], None, false)),
            // Umbenennung: der alte Pfad folgt als eigener NUL-Eintrag.
            ("2", 10) => {
                let orig = recs.next();
                s.files.push(change(f[1], f[9], orig, false));
            }
            ("u", 11) => s.files.push(change(f[1], f[10], None, true)),
            _ => {}
        }
    }
    s
}

const FMT: &str = "%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%s%x1f%D";

/// Ein Datensatz aus FMT, optional gefolgt vom Body (git show).
fn parse_commit(rec: &str) -> Option<(Commit, String)> {
    let f: Vec<&str> = rec.splitn(8, '\x1f').collect();
    if f.len() < 7 {
        return None;
    }
    let commit = Commit {
        sha: f[0].to_string(),
        parents: f[1].split_whitespace().map(str::to_string).collect(),
        author: f[2].to_string(),
        email: f[3].to_string(),
        date: f[4].to_string(),
        subject: f[5].to_string(),
        refs: f[6]
            .split(", ")
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string)
            .collect(),
    };
    Some((commit, f.get(7).unwrap_or(&"").trim_end().to_string()))
}

fn parse_log(raw: &str) -> Vec<Commit> {
    raw.split('\x1e')
        .map(|r| r.trim_start_matches(['\r', '\n']))
        .filter(|r| !r.is_empty())
        .filter_map(parse_commit)
        .map(|(c, _)| c)
        .collect()
}

fn parse_track(track: &str) -> (u32, u32) {
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

const BRANCH_FMT: &str = "%(refname)%1f%(HEAD)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(objectname)%1f%(committerdate:iso-strict)%1f%(contents:subject)";

/// raw optional mit 8. Feld aus %(ahead-behind:<base>) ("vor hinter").
fn parse_branches(raw: &str, base: Option<&str>) -> Vec<Branch> {
    raw.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(8, '\x1f').collect();
            if f.len() < 7 {
                return None;
            }
            let (name, remote) = if let Some(n) = f[0].strip_prefix("refs/heads/") {
                (n, false)
            } else {
                (f[0].strip_prefix("refs/remotes/")?, true)
            };
            // origin/HEAD ist nur ein Zeiger, kein Branch.
            if remote && (name == "HEAD" || name.ends_with("/HEAD")) {
                return None;
            }
            let (ahead, behind) = parse_track(f[3]);
            Some(Branch {
                name: name.to_string(),
                remote,
                current: f[1] == "*",
                upstream: (!f[2].is_empty()).then(|| f[2].to_string()),
                ahead,
                behind,
                sha: f[4].to_string(),
                date: f[5].to_string(),
                subject: f[6].to_string(),
                unmerged: f.get(7).and_then(|ab| ab.split(' ').next()?.parse().ok()),
                base: base.map(str::to_string),
            })
        })
        .collect()
}

fn parse_stashes(raw: &str) -> Vec<Stash> {
    raw.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(3, '\x1f').collect();
            if f.len() < 3 {
                return None;
            }
            let index = f[0].split_once('{')?.1.strip_suffix('}')?.parse().ok()?;
            let gs = f[1];
            let (branch, message) = match gs
                .strip_prefix("WIP on ")
                .or_else(|| gs.strip_prefix("On "))
            {
                Some(rest) => rest.split_once(": ").unwrap_or((rest, "")),
                None => ("", gs),
            };
            Some(Stash {
                index,
                message: message.to_string(),
                date: f[2].to_string(),
                branch: branch.to_string(),
            })
        })
        .collect()
}

const TAG_FMT: &str = "%(refname:strip=2)%1f%(objecttype)%1f%(objectname)%1f%(*objectname)%1f%(creatordate:iso-strict)%1f%(contents:subject)";

fn parse_tags(raw: &str) -> Vec<Tag> {
    raw.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(6, '\x1f').collect();
            if f.len() < 6 {
                return None;
            }
            // Annotiert: Tag-Objekt auf den Commit aufloesen; leichtgewichtig hat keine eigene Nachricht.
            let annotated = f[1] == "tag";
            Some(Tag {
                name: f[0].to_string(),
                sha: if annotated && !f[3].is_empty() {
                    f[3]
                } else {
                    f[2]
                }
                .to_string(),
                date: f[4].to_string(),
                message: if annotated { f[5] } else { "" }.to_string(),
            })
        })
        .collect()
}

fn parse_remotes(raw: &str) -> Vec<Remote> {
    raw.lines()
        .filter_map(|l| l.strip_suffix(" (fetch)"))
        .filter_map(|l| l.split_once('\t'))
        .map(|(name, url)| Remote {
            name: name.to_string(),
            url: url.to_string(),
        })
        .collect()
}

/// "origin/feature/x" -> ("origin", "feature/x"); laengster passender Remote-Name gewinnt.
fn split_remote<'a>(remotes: &[Remote], name: &'a str) -> Option<(String, &'a str)> {
    remotes
        .iter()
        .filter_map(|r| {
            let rest = name.strip_prefix(r.name.as_str())?.strip_prefix('/')?;
            Some((r.name.clone(), rest))
        })
        .max_by_key(|(r, _)| r.len())
}

// ---------- Logik ----------

fn repo_state(repo: &str) -> Result<RepoState, String> {
    let dir = PathBuf::from(out(git(repo).args(["rev-parse", "--absolute-git-dir"]))?.trim());
    Ok(
        if dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists() {
            RepoState::Rebase
        } else if dir.join("MERGE_HEAD").exists() {
            RepoState::Merge
        } else if dir.join("CHERRY_PICK_HEAD").exists() {
            RepoState::CherryPick
        } else if dir.join("REVERT_HEAD").exists() {
            RepoState::Revert
        } else {
            RepoState::Clean
        },
    )
}

fn status(repo: &str) -> Result<Status, String> {
    let raw = out(git(repo).args([
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        "--untracked-files=all",
    ]))?;
    let mut s = parse_status(&raw);
    s.state = repo_state(repo)?;
    Ok(s)
}

fn remotes(repo: &str) -> Result<Vec<Remote>, String> {
    Ok(parse_remotes(&out(git(repo).args(["remote", "-v"]))?))
}

fn untracked(repo: &str, paths: &[String]) -> Result<Vec<String>, String> {
    let raw = out(lit(repo)
        .args(["ls-files", "--others", "--exclude-standard", "-z", "--"])
        .args(paths))?;
    Ok(raw
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

fn diff(repo: &str, path: Option<&str>, staged: bool) -> Result<String, String> {
    const OPTS: [&str; 3] = ["diff", "--no-color", "--no-ext-diff"];
    if let (Some(p), false) = (path, staged) {
        if !untracked(repo, &[p.to_string()])?.is_empty() {
            // Neue Datei gegen nichts; Exit 1 heisst bei --no-index nur "unterschiedlich".
            let o = exec(
                git(repo)
                    .args(OPTS)
                    .args(["--no-index", "--", "/dev/null", p]),
                None,
            )?;
            return match o.status.code() {
                Some(0 | 1) => Ok(text(&o.stdout)),
                _ => Err(fail(&o)),
            };
        }
    }
    let mut cmd = lit(repo);
    cmd.args(OPTS);
    if staged {
        cmd.arg("--cached");
    }
    cmd.arg("--").args(path);
    out(&mut cmd)
}

fn log(repo: &str, rev: Option<&str>, skip: u32, limit: u32) -> Result<Vec<Commit>, String> {
    let mut cmd = git(repo);
    cmd.args(["log", "--topo-order"])
        .arg(format!("--format={FMT}%x1e"))
        .arg(format!("--skip={skip}"))
        .arg(format!("--max-count={limit}"));
    match rev {
        Some(r) => cmd.arg(arg(r)?),
        // Sichtbarer Graph: ohne Stash-, Notes- oder Backup-Refs.
        None => cmd.args(["--branches", "--remotes", "--tags"]),
    };
    // HEAD fuer detached; im leeren Repo gibt es ihn nicht.
    if rev.is_none() && has_head(repo) {
        cmd.arg("HEAD");
    }
    Ok(parse_log(&out(cmd.arg("--"))?))
}

fn show(repo: &str, sha: &str) -> Result<CommitDetail, String> {
    arg(sha)?;
    let meta = out(git(repo)
        .args(["show", "-s"])
        .arg(format!("--format={FMT}%x1f%b"))
        .args([sha, "--"]))?;
    let (commit, body) = parse_commit(&meta).ok_or("Commit nicht lesbar")?;
    // Merge-Commits: Diff gegen den ersten Parent.
    let diff = out(git(repo).args([
        "show",
        "--format=",
        "--patch",
        "-M",
        "-m",
        "--first-parent",
        "--no-color",
        "--no-ext-diff",
        sha,
        "--",
    ]))?;
    Ok(CommitDetail { commit, body, diff })
}

fn stage(repo: &str, paths: &[String]) -> Result<(), String> {
    out(lit(repo).args(["add", "-A", "--"]).args(paths)).map(|_| ())
}

/// Bei Umbenennungen gehoert der alte Pfad mit dazu, sonst bleibt dessen Loeschung gestaged.
fn with_origs(repo: &str, paths: &[String]) -> Result<Vec<String>, String> {
    let mut all = paths.to_vec();
    for f in status(repo)?.files {
        if let Some(orig) = f.orig {
            if paths.contains(&f.path) && !all.contains(&orig) {
                all.push(orig);
            }
        }
    }
    Ok(all)
}

fn unstage(repo: &str, paths: &[String]) -> Result<(), String> {
    let all = with_origs(repo, paths)?;
    let mut cmd = lit(repo);
    if has_head(repo) {
        cmd.args(["restore", "--staged", "--"]);
    } else {
        // Noch kein Commit: es gibt kein HEAD zum Zuruecksetzen.
        cmd.args(["rm", "--cached", "-r", "-q", "--ignore-unmatch", "--"]);
    }
    out(cmd.args(&all)).map(|_| ())
}

/// Arbeitskopie auf den Index-Stand zurueck; untracked Dateien loeschen. Gestagtes bleibt.
fn discard(repo: &str, paths: &[String]) -> Result<(), String> {
    let loose = untracked(repo, paths)?;
    let tracked: Vec<&String> = paths.iter().filter(|p| !loose.contains(p)).collect();
    if !loose.is_empty() {
        out(lit(repo).args(["clean", "-f", "-q", "--"]).args(&loose))?;
    }
    if !tracked.is_empty() {
        out(lit(repo)
            .args(["restore", "--worktree", "--"])
            .args(tracked))?;
    }
    Ok(())
}

fn apply(repo: &str, patch: &str, cached: bool, reverse: bool) -> Result<(), String> {
    let mut cmd = git(repo);
    cmd.args(["apply", "--recount", "--whitespace=nowarn"]);
    if cached {
        cmd.arg("--cached");
    }
    if reverse {
        cmd.arg("--reverse");
    }
    let o = exec(cmd.arg("-"), Some(patch.as_bytes()))?;
    if o.status.success() {
        Ok(())
    } else if patch.contains('\u{FFFD}') {
        // diff() dekodiert lossy; Nicht-UTF-8-Zeilen passen dann nicht mehr.
        Err("Datei ist nicht UTF-8, bitte nur die ganze Datei stagen/verwerfen.".into())
    } else {
        Err(fail(&o))
    }
}

fn commit(repo: &str, message: &str, amend: bool) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["commit", "-q", "-F", "-"]);
    if amend {
        cmd.arg("--amend");
    }
    let o = exec(&mut cmd, Some(message.as_bytes()))?;
    if !o.status.success() {
        return Err(fail(&o));
    }
    Ok(out(git(repo).args(["rev-parse", "HEAD"]))?
        .trim()
        .to_string())
}

fn checkout(repo: &str, target: &str, create: bool, start: Option<&str>) -> Result<String, String> {
    if create {
        let mut cmd = git(repo);
        cmd.args(["switch", "-c", branch_name(repo, target)?]);
        if let Some(s) = start {
            cmd.arg(arg(s)?);
            // "Neuer Branch ab origin/main" soll nicht origin/main tracken, sonst scheitert push.
            if split_remote(&remotes(repo)?, s).is_some_and(|(_, b)| b != target) {
                cmd.arg("--no-track");
            }
        }
        return talk(&mut cmd);
    }
    arg(target)?;
    if ref_exists(repo, &format!("refs/heads/{target}")) {
        return talk(git(repo).args(["switch", target]));
    }
    if ref_exists(repo, &format!("refs/remotes/{target}")) {
        if let Some((_, local)) = split_remote(&remotes(repo)?, target) {
            // Remote-Branch: lokalen Tracking-Branch nehmen bzw. anlegen.
            if ref_exists(repo, &format!("refs/heads/{local}")) {
                return talk(git(repo).args(["switch", local]));
            }
            return talk(git(repo).args(["switch", "-c", local, "--track", target]));
        }
    }
    // SHA oder Tag: detached.
    talk(git(repo).args(["switch", "--detach", target]))
}

fn branch_delete(repo: &str, name: &str, force: bool, remote: bool) -> Result<String, String> {
    arg(name)?;
    if remote {
        let (r, branch) = split_remote(&remotes(repo)?, name)
            .ok_or_else(|| format!("Kein Remote zu „{name}“ gefunden"))?;
        return talk(git(repo).args(["push", "--delete", "--", r.as_str(), branch]));
    }
    talk(git(repo).args(["branch", if force { "-D" } else { "-d" }, name]))
}

/// Setzt je nach Zustand fort oder bricht ab.
fn sequencer(repo: &str, step: &str) -> Result<String, String> {
    let op = match repo_state(repo)? {
        RepoState::Merge => "merge",
        RepoState::Rebase => "rebase",
        RepoState::CherryPick => "cherry-pick",
        RepoState::Revert => "revert",
        RepoState::Clean => {
            return Err("Kein laufender Merge, Rebase, Cherry-Pick oder Revert.".into())
        }
    };
    let res = tolerate(repo, git(repo).args([op, step]));
    // Leer nach Loesung = ours: git will --skip, sonst haengt der Zustand.
    if res.is_err()
        && step == "--continue"
        && matches!(op, "cherry-pick" | "revert")
        && !has_conflicts(repo)
        && ok(git(repo).args(["diff", "--cached", "--quiet", "HEAD", "--"]))
    {
        return talk(git(repo).args([op, "--skip"]));
    }
    res
}

fn is_merge(repo: &str, sha: &str) -> Result<bool, String> {
    let line = out(git(repo).args(["rev-list", "--parents", "-n", "1", sha, "--"]))?;
    Ok(line.split_whitespace().count() > 2)
}

/// cherry-pick/revert; bei Merge-Commits gegen den ersten Parent.
fn pick(repo: &str, op: &str, sha: &str) -> Result<String, String> {
    arg(sha)?;
    let mut cmd = git(repo);
    cmd.arg(op);
    if op == "revert" {
        cmd.arg("--no-edit");
    }
    if is_merge(repo, sha)? {
        cmd.args(["-m", "1"]);
    }
    tolerate(repo, cmd.arg(sha))
}

fn push(repo: &str, force: bool) -> Result<String, String> {
    let st = status(repo)?;
    let mut cmd = git(repo);
    cmd.arg("push");
    if force {
        cmd.arg("--force-with-lease");
    }
    if st.upstream.is_none() {
        let branch = st.branch.ok_or("Detached HEAD: kein Branch zum Pushen.")?;
        let list = remotes(repo)?;
        let remote = list
            .iter()
            .find(|r| r.name == "origin")
            .or(list.first())
            .ok_or("Kein Remote eingerichtet.")?;
        cmd.args(["-u", remote.name.as_str(), branch.as_str()]);
    }
    talk(&mut cmd)
}

/// Pro Repo fest eingestellter Production-Branch (lokale git config), None = automatisch.
fn configured_base(repo: &str) -> Option<String> {
    out(git(repo).args(["config", "--get", "ocui.base"]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn set_base(repo: &str, base: Option<&str>) -> Result<(), String> {
    match base {
        Some(b) => {
            let rev = format!("{}^{{commit}}", arg(b)?);
            if !ok(git(repo).args(["rev-parse", "--verify", "--quiet", &rev])) {
                return Err(format!("Branch nicht gefunden: {b}"));
            }
            out(git(repo).args(["config", "ocui.base", b])).map(|_| ())
        }
        // Exit 5 = war nicht gesetzt, auch gut.
        None => {
            let _ = out(git(repo).args(["config", "--unset", "ocui.base"]));
            Ok(())
        }
    }
}

/// Hauptbranch, gegen den "noch nicht gemergt" gezaehlt wird: eingestellt, sonst origin/HEAD, sonst die ueblichen Namen.
fn base_branch(repo: &str) -> Option<String> {
    // Geloeschter oder umbenannter Branch in der Config: auf automatisch zurueckfallen.
    let valid = |b: &String| ok(git(repo).args(["rev-parse", "--verify", "--quiet", &format!("{b}^{{commit}}")]));
    if let Some(b) = configured_base(repo).filter(valid) {
        return Some(b);
    }
    // Neueres git liefert auch ein verwaistes origin/HEAD, daher ebenfalls pruefen.
    let head = out(git(repo).args(["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(valid);
    head.or_else(|| {
        ["origin/main", "origin/master", "main", "master"]
            .into_iter()
            .find(|b| {
                let full = match b.strip_prefix("origin/") {
                    Some(_) => format!("refs/remotes/{b}"),
                    None => format!("refs/heads/{b}"),
                };
                ok(git(repo).args(["show-ref", "--verify", "--quiet", &full]))
            })
            .map(str::to_string)
    })
}

fn branches(repo: &str) -> Result<Vec<Branch>, String> {
    let list = |fmt: String| {
        out(git(repo)
            .arg("for-each-ref")
            .arg(format!("--format={fmt}"))
            .args(["refs/heads", "refs/remotes"]))
    };
    if let Some(base) = base_branch(repo) {
        // %(ahead-behind:) gibt es erst ab git 2.41; aelteres git bekommt die Liste ohne Zaehler.
        if let Ok(raw) = list(format!("{BRANCH_FMT}%1f%(ahead-behind:{base})")) {
            return Ok(parse_branches(&raw, Some(&base)));
        }
    }
    Ok(parse_branches(&list(BRANCH_FMT.to_string())?, None))
}

fn merge(repo: &str, rev: &str, no_ff: bool) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["merge", "--no-edit"]);
    if no_ff {
        cmd.arg("--no-ff");
    }
    tolerate(repo, cmd.arg(arg(rev)?))
}

fn rebase_onto(repo: &str, onto: &str) -> Result<String, String> {
    tolerate(repo, git(repo).args(["rebase", arg(onto)?]))
}

fn stash_ref(index: u32) -> String {
    format!("stash@{{{index}}}")
}

fn stashes(repo: &str) -> Result<Vec<Stash>, String> {
    let raw = out(git(repo).args(["stash", "list", "--format=%gd%x1f%gs%x1f%aI"]))?;
    Ok(parse_stashes(&raw))
}

fn stash_push(repo: &str, message: Option<&str>, untracked: bool) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["stash", "push"]);
    if untracked {
        cmd.arg("--include-untracked");
    }
    if let Some(m) = message.filter(|m| !m.trim().is_empty()) {
        cmd.arg(format!("--message={m}"));
    }
    talk(&mut cmd)
}

fn stash_show(repo: &str, index: u32) -> Result<String, String> {
    out(git(repo)
        .args([
            "stash",
            "show",
            "-p",
            "--include-untracked",
            "--no-color",
            "--no-ext-diff",
        ])
        .arg(stash_ref(index)))
}

fn tags(repo: &str) -> Result<Vec<Tag>, String> {
    let raw = out(git(repo)
        .args(["for-each-ref", "--sort=-creatordate"])
        .arg(format!("--format={TAG_FMT}"))
        .arg("refs/tags"))?;
    Ok(parse_tags(&raw))
}

fn tag_create(repo: &str, name: &str, sha: &str, message: Option<&str>) -> Result<(), String> {
    arg(name)?;
    arg(sha)?;
    if !ok(git(repo)
        .arg("check-ref-format")
        .arg(format!("refs/tags/{name}")))
    {
        return Err(format!("Ungültiger Tag-Name: „{name}“"));
    }
    let mut cmd = git(repo);
    cmd.arg("tag");
    if let Some(m) = message.filter(|m| !m.trim().is_empty()) {
        cmd.arg("-a").arg(format!("--message={m}"));
    }
    out(cmd.args([name, sha])).map(|_| ())
}

fn conflict(repo: &str, path: &str) -> Result<Conflict, String> {
    let file = inside(repo, path)?;
    let stage = |n: u8| {
        exec(
            git(repo)
                .args(["cat-file", "blob"])
                .arg(format!(":{n}:{path}")),
            None,
        )
        .ok()
        .filter(|o| o.status.success())
        .map(|o| strict(o.stdout))
        .transpose()
    };
    Ok(Conflict {
        path: path.to_string(),
        base: stage(1)?,
        ours: stage(2)?,
        theirs: stage(3)?,
        // Geloescht auf einer Seite: Arbeitsdatei kann fehlen.
        merged: std::fs::read(&file).map_or(Ok(String::new()), strict)?,
    })
}

#[derive(Serialize)]
pub struct Sides {
    ours: String,
    theirs: String,
}

/// Lesbare Namen fuer "ours"/"theirs" im laufenden Vorgang, z. B. main / feature/x.
/// Auch fuer Binaerdateien, die keine Konfliktmarker mit Labels haben.
fn sides(repo: &str) -> Sides {
    let exists = |rev: &str| ok(git(repo).args(["rev-parse", "--verify", "--quiet", rev]));
    let name = |rev: &str| {
        let n = out(git(repo).args(["name-rev", "--name-only", "--no-undefined", rev]))
            .ok()
            .map(|s| s.trim().trim_start_matches("remotes/").to_string())
            .filter(|s| !s.is_empty());
        n.or_else(|| out(git(repo).args(["rev-parse", "--short", rev])).ok().map(|s| s.trim().to_string()))
            .unwrap_or_else(|| rev.to_string())
    };
    let head = out(git(repo).args(["rev-parse", "--abbrev-ref", "HEAD"])).map(|s| s.trim().to_string());
    // Beim Rebase ist HEAD losgeloest, "ours" ist der Branch, auf den rebased wird.
    let onto = || {
        let dir = PathBuf::from(out(git(repo).args(["rev-parse", "--absolute-git-dir"])).ok()?.trim());
        let sha = ["rebase-merge/onto", "rebase-apply/onto"]
            .iter()
            .find_map(|f| std::fs::read_to_string(dir.join(f)).ok())?;
        Some(name(sha.trim()))
    };
    let ours = match head {
        Ok(h) if h != "HEAD" => h,
        _ => onto().unwrap_or_else(|| "HEAD".into()),
    };
    let theirs = ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "REBASE_HEAD"]
        .into_iter()
        .find(|r| exists(r))
        .map(name)
        .unwrap_or_else(|| "Theirs".into());
    Sides { ours, theirs }
}

#[tauri::command]
pub async fn git_conflict_sides(repo: String) -> Result<Sides, String> {
    blocking(move || Ok(sides(&repo))).await
}

/// Ganze Seite uebernehmen, auch fuer Binaerdateien, die der Merge-Editor nicht laden kann.
fn resolve_side(repo: &str, path: &str, theirs: bool) -> Result<(), String> {
    inside(repo, path)?;
    if out(lit(repo).args(["ls-files", "-u", "--", path]))?.trim().is_empty() {
        return Err(format!("{path} steht nicht im Konflikt."));
    }
    let side = if theirs { "--theirs" } else { "--ours" };
    out(lit(repo).args(["checkout", side, "--", path]))?;
    out(lit(repo).args(["add", "--", path])).map(|_| ())
}

fn resolve(repo: &str, path: &str, content: &str) -> Result<(), String> {
    let file = inside(repo, path)?;
    // Nur Dateien schreiben, die wirklich im Konflikt stehen.
    if out(lit(repo).args(["ls-files", "-u", "--", path]))?.trim().is_empty() {
        return Err(format!("{path} steht nicht im Konflikt."));
    }
    std::fs::write(&file, content).map_err(|e| format!("Konnte {path} nicht schreiben: {e}"))?;
    out(lit(repo).args(["add", "--", path])).map(|_| ())
}

// ---------- Commands ----------

#[tauri::command]
pub async fn git_status(repo: String) -> Result<Status, String> {
    blocking(move || status(&repo)).await
}

#[tauri::command]
pub async fn git_log(
    repo: String,
    rev: Option<String>,
    skip: u32,
    limit: u32,
) -> Result<Vec<Commit>, String> {
    blocking(move || log(&repo, rev.as_deref(), skip, limit)).await
}

#[tauri::command]
pub async fn git_show(repo: String, sha: String) -> Result<CommitDetail, String> {
    blocking(move || show(&repo, &sha)).await
}

#[tauri::command]
pub async fn git_diff(repo: String, path: Option<String>, staged: bool) -> Result<String, String> {
    blocking(move || diff(&repo, path.as_deref(), staged)).await
}

#[tauri::command]
pub async fn git_stage(repo: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    blocking(move || stage(&repo, &paths)).await
}

#[tauri::command]
pub async fn git_unstage(repo: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    blocking(move || unstage(&repo, &paths)).await
}

#[tauri::command]
pub async fn git_discard(repo: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    blocking(move || discard(&repo, &paths)).await
}

#[tauri::command]
pub async fn git_apply(
    repo: String,
    patch: String,
    cached: bool,
    reverse: bool,
) -> Result<(), String> {
    blocking(move || apply(&repo, &patch, cached, reverse)).await
}

#[tauri::command]
pub async fn git_commit(repo: String, message: String, amend: bool) -> Result<String, String> {
    blocking(move || commit(&repo, &message, amend)).await
}

#[tauri::command]
pub async fn git_branches(repo: String) -> Result<Vec<Branch>, String> {
    blocking(move || branches(&repo)).await
}

#[tauri::command]
pub async fn git_checkout(
    repo: String,
    target: String,
    create: bool,
    start: Option<String>,
) -> Result<String, String> {
    blocking(move || checkout(&repo, &target, create, start.as_deref())).await
}

#[tauri::command]
pub async fn git_branch_delete(
    repo: String,
    name: String,
    force: bool,
    remote: bool,
) -> Result<String, String> {
    blocking(move || branch_delete(&repo, &name, force, remote)).await
}

#[tauri::command]
pub async fn git_branch_rename(repo: String, from: String, to: String) -> Result<(), String> {
    blocking(move || {
        arg(&from)?;
        let to = branch_name(&repo, &to)?;
        out(git(&repo).args(["branch", "-m", &from, to])).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn git_merge(repo: String, rev: String, no_ff: bool) -> Result<String, String> {
    blocking(move || merge(&repo, &rev, no_ff)).await
}

#[tauri::command]
pub async fn git_rebase(repo: String, onto: String) -> Result<String, String> {
    blocking(move || rebase_onto(&repo, &onto)).await
}

#[tauri::command]
pub async fn git_abort(repo: String) -> Result<String, String> {
    blocking(move || sequencer(&repo, "--abort")).await
}

#[tauri::command]
pub async fn git_continue(repo: String) -> Result<String, String> {
    blocking(move || sequencer(&repo, "--continue")).await
}

#[tauri::command]
pub async fn git_cherry_pick(repo: String, sha: String) -> Result<String, String> {
    blocking(move || pick(&repo, "cherry-pick", &sha)).await
}

#[tauri::command]
pub async fn git_revert(repo: String, sha: String) -> Result<String, String> {
    blocking(move || pick(&repo, "revert", &sha)).await
}

#[tauri::command]
pub async fn git_reset(repo: String, sha: String, mode: String) -> Result<String, String> {
    blocking(move || {
        let flag = match mode.as_str() {
            "soft" => "--soft",
            "mixed" => "--mixed",
            "hard" => "--hard",
            _ => return Err(format!("Unbekannter Reset-Modus: {mode}")),
        };
        talk(git(&repo).args(["reset", "-q", flag, arg(&sha)?, "--"]))
    })
    .await
}

#[tauri::command]
pub async fn git_fetch(repo: String) -> Result<String, String> {
    blocking(move || talk(git(&repo).args(["fetch", "--all", "--prune"]))).await
}

#[tauri::command]
pub async fn git_pull(repo: String, rebase: bool) -> Result<String, String> {
    blocking(move || {
        let mode = if rebase { "--rebase" } else { "--no-rebase" };
        tolerate(&repo, git(&repo).args(["pull", mode]))
    })
    .await
}

#[tauri::command]
pub async fn git_push(repo: String, force: bool) -> Result<String, String> {
    blocking(move || push(&repo, force)).await
}

#[tauri::command]
pub async fn git_remotes(repo: String) -> Result<Vec<Remote>, String> {
    blocking(move || remotes(&repo)).await
}

#[tauri::command]
pub async fn git_stashes(repo: String) -> Result<Vec<Stash>, String> {
    blocking(move || stashes(&repo)).await
}

#[tauri::command]
pub async fn git_stash_push(
    repo: String,
    message: Option<String>,
    untracked: bool,
) -> Result<String, String> {
    blocking(move || stash_push(&repo, message.as_deref(), untracked)).await
}

#[tauri::command]
pub async fn git_stash_apply(repo: String, index: u32, pop: bool) -> Result<String, String> {
    blocking(move || {
        let op = if pop { "pop" } else { "apply" };
        tolerate(
            &repo,
            git(&repo).args(["stash", op, stash_ref(index).as_str()]),
        )
    })
    .await
}

#[tauri::command]
pub async fn git_stash_drop(repo: String, index: u32) -> Result<(), String> {
    blocking(move || out(git(&repo).args(["stash", "drop", stash_ref(index).as_str()])).map(|_| ()))
        .await
}

#[tauri::command]
pub async fn git_stash_show(repo: String, index: u32) -> Result<String, String> {
    blocking(move || stash_show(&repo, index)).await
}

#[tauri::command]
pub async fn git_tags(repo: String) -> Result<Vec<Tag>, String> {
    blocking(move || tags(&repo)).await
}

#[tauri::command]
pub async fn git_tag_create(
    repo: String,
    name: String,
    sha: String,
    message: Option<String>,
) -> Result<(), String> {
    blocking(move || tag_create(&repo, &name, &sha, message.as_deref())).await
}

#[tauri::command]
pub async fn git_tag_delete(repo: String, name: String) -> Result<(), String> {
    blocking(move || out(git(&repo).args(["tag", "-d", arg(&name)?])).map(|_| ())).await
}

/// Welche der Branches `names` stecken schon in anderen aus `names`? Name -> enthaltende Namen.
/// merge-base --independent sortiert alle nicht enthaltenen in einem Aufruf aus, nur der Rest
/// kostet je ein for-each-ref (eingeschraenkt auf `names`).
fn nesting(repo: &str, names: &[String]) -> Result<HashMap<String, Vec<String>>, String> {
    let mut res = HashMap::new();
    if names.len() < 2 {
        return Ok(res);
    }
    for n in names {
        arg(n)?;
    }
    let shas: Vec<String> = out(git(repo).arg("rev-parse").args(names))?
        .lines()
        .map(str::to_string)
        .collect();
    let free: HashSet<String> = out(git(repo).args(["merge-base", "--independent"]).args(&shas))?
        .lines()
        .map(str::to_string)
        .collect();
    // Muster fuer lokal und remote; nicht existierende matchen einfach nichts.
    let patterns: Vec<String> = names
        .iter()
        .flat_map(|n| [format!("refs/heads/{n}"), format!("refs/remotes/{n}")])
        .collect();
    let nested: Vec<(&String, &String)> = names.iter().zip(&shas).filter(|(_, s)| !free.contains(*s)).collect();
    // Ein --contains kostet in grossen Repos ~0,25 s; nacheinander waeren 30 Branches schon 7 s.
    let lanes = nested.len().div_ceil(8).max(1);
    let found: Vec<(String, Vec<String>)> = std::thread::scope(|s| {
        let workers: Vec<_> = nested
            .chunks(lanes)
            .map(|chunk| {
                let patterns = &patterns;
                s.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|(name, sha)| {
                            let raw = out(git(repo)
                                .args(["for-each-ref", "--format=%(refname:short)", "--contains", sha])
                                .args(patterns))
                            .ok()?;
                            let by = raw.lines().filter(|n| n != name).map(str::to_string).collect();
                            Some(((*name).clone(), by))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    res.extend(found);
    Ok(res)
}

#[tauri::command]
pub async fn git_nesting(repo: String, names: Vec<String>) -> Result<HashMap<String, Vec<String>>, String> {
    blocking(move || nesting(&repo, &names)).await
}

#[tauri::command]
pub async fn git_get_base(repo: String) -> Result<Option<String>, String> {
    blocking(move || Ok(configured_base(&repo))).await
}

#[tauri::command]
pub async fn git_set_base(repo: String, base: Option<String>) -> Result<(), String> {
    blocking(move || set_base(&repo, base.as_deref())).await
}

#[tauri::command]
pub async fn git_conflict(repo: String, path: String) -> Result<Conflict, String> {
    blocking(move || conflict(&repo, &path)).await
}

#[tauri::command]
pub async fn git_resolve(repo: String, path: String, content: String) -> Result<(), String> {
    blocking(move || resolve(&repo, &path, &content)).await
}

#[tauri::command]
pub async fn git_resolve_side(repo: String, path: String, theirs: bool) -> Result<(), String> {
    blocking(move || resolve_side(&repo, &path, theirs)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_porcelain_v2() {
        let raw = "# branch.oid abc123\0# branch.head feature/ä\0# branch.upstream origin/feature/ä\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/mit leer.rs\0\
2 R. N... 100644 100644 100644 aaa bbb R100 neu.txt\0alt.txt\0\
u UU N... 100644 100644 100644 100644 a b c konflikt.txt\0\
? grüße.md\0! ignoriert\0";
        let s = parse_status(raw);
        assert_eq!(s.branch.as_deref(), Some("feature/ä"));
        assert_eq!(s.head, "abc123");
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/ä"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        let paths: Vec<_> = s.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["src/mit leer.rs", "neu.txt", "konflikt.txt", "grüße.md"]
        );
        assert_eq!(
            (s.files[0].index.as_str(), s.files[0].worktree.as_str()),
            (".", "M")
        );
        assert_eq!(s.files[1].orig.as_deref(), Some("alt.txt"));
        assert!(s.files[2].conflict);
        assert_eq!(
            (s.files[3].index.as_str(), s.files[3].worktree.as_str()),
            ("?", "?")
        );
    }

    #[test]
    fn status_leer_und_detached() {
        let s = parse_status("# branch.oid (initial)\0# branch.head master\0");
        assert_eq!((s.head.as_str(), s.branch.as_deref()), ("", Some("master")));
        let s = parse_status("# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!(s.branch, None);
    }

    #[test]
    fn log_und_show() {
        let raw = "a1\x1fp1 p2\x1fMarlon\x1fm@x.de\x1f2026-01-01T10:00:00+01:00\x1fMerge\x1fHEAD -> main, origin/main, tag: v1\x1e\n\
b2\x1f\x1fMarlon\x1fm@x.de\x1f2026-01-01T09:00:00+01:00\x1fStart\x1f\x1e\n";
        let log = parse_log(raw);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].parents, ["p1", "p2"]);
        assert_eq!(log[0].refs, ["HEAD -> main", "origin/main", "tag: v1"]);
        assert!(log[1].parents.is_empty() && log[1].refs.is_empty());

        let (c, body) =
            parse_commit("a\x1f\x1fx\x1fy\x1fd\x1fs\x1f\x1fZeile 1\n\nZeile 2\n\n").unwrap();
        assert_eq!(c.subject, "s");
        assert_eq!(body, "Zeile 1\n\nZeile 2");
    }

    #[test]
    fn branches_parsen() {
        let raw = "refs/heads/main\x1f*\x1forigin/main\x1fahead 1, behind 3\x1fs1\x1fd\x1fsub\n\
refs/heads/lokal\x1f \x1f\x1f\x1fs2\x1fd\x1fsub\n\
refs/remotes/origin/HEAD\x1f \x1f\x1f\x1fs1\x1fd\x1fsub\n\
refs/remotes/origin/feature/x\x1f \x1f\x1f\x1fs3\x1fd\x1fsub: mit doppelpunkt\n";
        let b = parse_branches(raw, None);
        assert_eq!(b.len(), 3);
        assert!(b[0].current && !b[0].remote);
        assert_eq!((b[0].ahead, b[0].behind), (1, 3));
        assert_eq!(b[1].upstream, None);
        assert_eq!(b[2].name, "origin/feature/x");
        assert!(b[2].remote);
        assert_eq!(parse_track("gone"), (0, 0));
        assert_eq!(b[0].unmerged, None);

        let with = parse_branches("refs/heads/f\x1f \x1f\x1f\x1fs\x1fd\x1fsub\x1f4 2\n", Some("origin/main"));
        assert_eq!((with[0].unmerged, with[0].base.as_deref()), (Some(4), Some("origin/main")));
    }

    #[test]
    fn stashes_und_tags() {
        let raw = "stash@{0}\x1fOn main: mein text\x1fd0\nstash@{12}\x1fWIP on feature/y: abc123 subj\x1fd1\n";
        let s = parse_stashes(raw);
        assert_eq!(
            (s[0].index, s[0].branch.as_str(), s[0].message.as_str()),
            (0, "main", "mein text")
        );
        assert_eq!(
            (s[1].index, s[1].branch.as_str(), s[1].message.as_str()),
            (12, "feature/y", "abc123 subj")
        );

        let raw = "v2\x1ftag\x1ftagobj\x1fcommit2\x1fd\x1fRelease 2\nv1\x1fcommit\x1fcommit1\x1f\x1fd\x1fcommit-subject\n";
        let t = parse_tags(raw);
        assert_eq!(
            (t[0].sha.as_str(), t[0].message.as_str()),
            ("commit2", "Release 2")
        );
        assert_eq!((t[1].sha.as_str(), t[1].message.as_str()), ("commit1", ""));
    }

    #[test]
    fn remotes_und_pfade() {
        let r = parse_remotes("origin\thttps://x/a.git (fetch)\norigin\thttps://x/a.git (push)\nup/stream\tgit@x:b.git (fetch)\n");
        assert_eq!(r.len(), 2);
        assert_eq!(
            split_remote(&r, "up/stream/feat/z"),
            Some(("up/stream".into(), "feat/z"))
        );
        assert_eq!(
            split_remote(&r, "origin/main"),
            Some(("origin".into(), "main"))
        );
        assert_eq!(split_remote(&r, "nix/main"), None);

        assert!(inside("C:/r", "a/b.txt").is_ok());
        assert!(inside("C:/r", "../x").is_err());
        #[cfg(windows)]
        assert!(inside("C:/r", "C:/x").is_err());
        assert!(inside("C:/r", "/x").is_err());
        assert!(inside("C:/r", "").is_err());
        assert!(inside("C:/r", ".GIT/hooks/pre-commit").is_err());
        assert!(inside("C:/r", ".git./config").is_err());
        assert!(inside("C:/r", "a/.git . /config").is_err());
        assert!(inside("C:/r", "GIT~1/config").is_err());
        assert!(inside("C:/r", ".gitignore").is_ok());
        assert!(arg("-x").is_err() && arg("main").is_ok());
    }

    /// Durchlauf gegen ein echtes Wegwerf-Repo: Konflikt, Rename, Umlaute, Stash, Tag.
    #[test]
    fn echtes_repo() {
        for (k, v) in [
            ("GIT_AUTHOR_NAME", "Test"),
            ("GIT_AUTHOR_EMAIL", "t@x.de"),
            ("GIT_COMMITTER_NAME", "Test"),
            ("GIT_COMMITTER_EMAIL", "t@x.de"),
        ] {
            std::env::set_var(k, v);
        }
        let dir = std::env::temp_dir().join(format!("ocui-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = dir.to_str().unwrap();
        let write = |p: &str, s: &str| std::fs::write(dir.join(p), s).unwrap();
        let all = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        out(git(r).args(["init", "-q", "-b", "main"])).unwrap();
        out(git(r).args(["config", "commit.gpgsign", "false"])).unwrap();

        // Leeres Repo
        assert!(log(r, None, 0, 10).unwrap().is_empty());
        let s = status(r).unwrap();
        assert_eq!((s.branch.as_deref(), s.head.as_str()), (Some("main"), ""));

        write("a.txt", "eins\nzwei\ndrei\n");
        write("grüße ä.txt", "hallo\n");
        let st = status(r).unwrap();
        assert!(st
            .files
            .iter()
            .any(|f| f.path == "grüße ä.txt" && f.index == "?"));
        assert!(diff(r, Some("grüße ä.txt"), false)
            .unwrap()
            .contains("+hallo"));
        stage(r, &all(&["a.txt", "grüße ä.txt"])).unwrap();
        unstage(r, &all(&["grüße ä.txt"])).unwrap(); // ohne HEAD
        stage(r, &all(&["grüße ä.txt"])).unwrap();
        let c1 = commit(r, "Start\n\nMit Body", false).unwrap();
        assert_eq!(status(r).unwrap().head, c1);

        // Konflikt: feature und main aendern dieselbe Zeile
        checkout(r, "feature/ü", true, None).unwrap();
        write("a.txt", "eins\nFEATURE\ndrei\n");
        stage(r, &all(&["a.txt"])).unwrap();
        commit(r, "feature", false).unwrap();
        checkout(r, "main", false, None).unwrap();
        write("a.txt", "eins\nMAIN\ndrei\n");
        stage(r, &all(&["a.txt"])).unwrap();
        commit(r, "main", false).unwrap();
        merge(r, "feature/ü", false).unwrap();
        let st = status(r).unwrap();
        assert_eq!(st.state, RepoState::Merge);
        assert!(st.files.iter().any(|f| f.path == "a.txt" && f.conflict));
        let c = conflict(r, "a.txt").unwrap();
        assert!(c.base.unwrap().contains("zwei"));
        assert!(c.ours.unwrap().contains("MAIN") && c.theirs.unwrap().contains("FEATURE"));
        assert!(c.merged.contains("<<<<<<<"));
        resolve(r, "a.txt", "eins\nBEIDE\ndrei\n").unwrap();
        sequencer(r, "--continue").unwrap();
        assert_eq!(status(r).unwrap().state, RepoState::Clean);
        let l = log(r, None, 0, 50).unwrap();
        assert_eq!(l[0].parents.len(), 2);
        assert!(l[0].refs.iter().any(|x| x == "HEAD -> main"));
        assert!(show(r, &l[0].sha).unwrap().diff.contains("+BEIDE"));
        assert_eq!(show(r, &c1).unwrap().body, "Mit Body");

        // Umbenennen gestaged, dann komplett unstagen
        std::fs::rename(dir.join("a.txt"), dir.join("b.txt")).unwrap();
        stage(r, &all(&["a.txt", "b.txt"])).unwrap();
        let st = status(r).unwrap();
        let ren = st.files.iter().find(|f| f.path == "b.txt").unwrap();
        assert_eq!(
            (ren.index.as_str(), ren.orig.as_deref()),
            ("R", Some("a.txt"))
        );
        unstage(r, &all(&["b.txt"])).unwrap();
        assert!(status(r)
            .unwrap()
            .files
            .iter()
            .all(|f| f.index == "." || f.index == "?"));
        std::fs::rename(dir.join("b.txt"), dir.join("a.txt")).unwrap();

        // Hunk stagen per Patch, verwerfen
        write("a.txt", "eins\nBEIDE\ndrei\nvier\n");
        let patch = diff(r, Some("a.txt"), false).unwrap();
        apply(r, &patch, true, false).unwrap();
        assert!(diff(r, Some("a.txt"), true).unwrap().contains("+vier"));
        apply(r, &patch, true, true).unwrap();
        discard(r, &all(&["a.txt"])).unwrap();
        write("weg.txt", "x");
        discard(r, &all(&["weg.txt"])).unwrap();
        assert!(!dir.join("weg.txt").exists());
        assert!(status(r).unwrap().files.is_empty());

        // Stash mit untracked Umlaut-Datei
        write("neu ö.txt", "neu\n");
        stash_push(r, Some("mein stash"), true).unwrap();
        let s = stashes(r).unwrap();
        assert_eq!(
            (s[0].index, s[0].branch.as_str(), s[0].message.as_str()),
            (0, "main", "mein stash")
        );
        assert!(!dir.join("neu ö.txt").exists());
        assert!(stash_show(r, 0).unwrap().contains("neu ö.txt"));
        tolerate(r, git(r).args(["stash", "pop", "stash@{0}"])).unwrap();
        assert!(dir.join("neu ö.txt").exists() && stashes(r).unwrap().is_empty());

        // Tags und Branches
        tag_create(r, "v1", "HEAD", Some("Release 1")).unwrap();
        tag_create(r, "leicht", &c1, None).unwrap();
        assert!(tag_create(r, "-x", "HEAD", None).is_err());
        let t = tags(r).unwrap();
        let v1 = t.iter().find(|t| t.name == "v1").unwrap();
        assert_eq!(
            (v1.sha.as_str(), v1.message.as_str()),
            (l[0].sha.as_str(), "Release 1")
        );
        assert_eq!(t.iter().find(|t| t.name == "leicht").unwrap().sha, c1);
        let b = branches(r).unwrap();
        assert!(b.iter().any(|b| b.name == "main" && b.current));
        assert!(b.iter().any(|b| b.name == "feature/ü" && !b.current));
        assert!(checkout(r, "bad..name", true, None).is_err());
        checkout(r, &c1, false, None).unwrap();
        assert_eq!(status(r).unwrap().branch, None);
        checkout(r, "main", false, None).unwrap();

        // Rebase mit Konflikt, dann abbrechen
        discard(r, &all(&["neu ö.txt"])).unwrap();
        checkout(r, "konflikt", true, Some(&c1)).unwrap();
        write("a.txt", "eins\nANDERS\ndrei\n");
        stage(r, &all(&["a.txt"])).unwrap();
        commit(r, "anders", false).unwrap();
        rebase_onto(r, "main").unwrap();
        assert_eq!(status(r).unwrap().state, RepoState::Rebase);
        sequencer(r, "--abort").unwrap();
        assert_eq!(status(r).unwrap().state, RepoState::Clean);
        checkout(r, "main", false, None).unwrap();

        // Cherry-Pick mit Loesung = ours ist leer: continue ueberspringt
        pick(r, "cherry-pick", "konflikt").unwrap();
        assert_eq!(status(r).unwrap().state, RepoState::CherryPick);
        assert!(resolve(r, ".git/config", "x").is_err());
        resolve(r, "a.txt", "eins\nBEIDE\ndrei\n").unwrap();
        sequencer(r, "--continue").unwrap();
        assert_eq!(status(r).unwrap().state, RepoState::Clean);

        // Remote: Push ohne Upstream, Remote-Branch auschecken und loeschen
        let bare = format!("{r}-remote.git");
        out(git(r).args(["init", "-q", "--bare", &bare])).unwrap();
        out(git(r).args(["remote", "add", "origin", &bare])).unwrap();
        push(r, false).unwrap();
        assert_eq!(status(r).unwrap().upstream.as_deref(), Some("origin/main"));
        out(git(r).args(["push", "-q", "origin", "konflikt:remote/x"])).unwrap();
        out(git(r).args(["fetch", "-q"])).unwrap();
        assert!(branches(r)
            .unwrap()
            .iter()
            .any(|b| b.name == "origin/remote/x" && b.remote));
        checkout(r, "origin/remote/x", false, None).unwrap();
        assert_eq!(status(r).unwrap().branch.as_deref(), Some("remote/x"));
        checkout(r, "main", false, None).unwrap();
        branch_delete(r, "origin/remote/x", false, true).unwrap();
        assert!(!branches(r)
            .unwrap()
            .iter()
            .any(|b| b.name == "origin/remote/x"));

        let _ = std::fs::remove_dir_all(&bare);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_als_kebab_case() {
        assert_eq!(
            serde_json::to_string(&RepoState::CherryPick).unwrap(),
            "\"cherry-pick\""
        );
    }

    // ---------- Szenario-Matrix gegen echte Repos ----------

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn cfg(r: &str, name: &str) {
        for (k, val) in [
            ("user.name", name),
            ("user.email", "t@x.de"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("pull.rebase", "false"),
        ] {
            out(git(r).args(["config", k, val])).unwrap();
        }
    }

    /// Frisches Repo (main, eigener Autor, autocrlf aus) in %TEMP%.
    fn fresh(name: &str) -> (Tmp, String) {
        let dir = std::env::temp_dir().join(format!("ocui-m-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = dir.to_str().unwrap().to_string();
        out(git(&r).args(["init", "-q", "-b", "main"])).unwrap();
        cfg(&r, "Test");
        (Tmp(dir), r)
    }

    fn put(r: &str, p: &str, s: impl AsRef<[u8]>) {
        let f = Path::new(r).join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    }

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    fn ci(r: &str, p: &str, s: &str, msg: &str) -> String {
        put(r, p, s);
        stage(r, &v(&[p])).unwrap();
        commit(r, msg, false).unwrap()
    }

    fn file<'a>(s: &'a Status, p: &str) -> Option<&'a FileChange> {
        s.files.iter().find(|f| f.path == p)
    }

    fn bare(r: &str) -> (Tmp, String) {
        let b = format!("{r}-remote.git");
        let _ = std::fs::remove_dir_all(&b);
        out(git(r).args(["init", "-q", "--bare", "-b", "main", &b])).unwrap();
        out(git(r).args(["remote", "add", "origin", &b])).unwrap();
        (Tmp(PathBuf::from(&b)), b)
    }

    fn clone(src: &str, name: &str) -> (Tmp, String) {
        let dir = std::env::temp_dir().join(format!("ocui-m-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = dir.to_str().unwrap().to_string();
        out(git(src).args(["clone", "-q", src, &r])).unwrap();
        cfg(&r, "Zwei");
        (Tmp(dir), r)
    }

    #[test]
    fn m_leeres_repo() {
        let (_t, r) = fresh("leer");
        assert!(log(&r, None, 0, 10).unwrap().is_empty());
        assert!(branches(&r).unwrap().is_empty());
        assert!(tags(&r).unwrap().is_empty());
        assert!(stashes(&r).unwrap().is_empty());
        assert!(remotes(&r).unwrap().is_empty());
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
        assert!(push(&r, false).is_err());
        assert!(commit(&r, "leer", false).is_err());
        assert!(commit(&r, "x", true).is_err());
        put(&r, "a.txt", "a\n");
        assert!(diff(&r, Some("a.txt"), false).unwrap().contains("+a"));
        discard(&r, &v(&["a.txt"])).unwrap();
        assert!(!Path::new(&r).join("a.txt").exists());
        put(&r, "b.txt", "1\n2\n");
        stage(&r, &v(&["b.txt"])).unwrap();
        assert!(diff(&r, Some("b.txt"), true).unwrap().contains("+2"));
        unstage(&r, &v(&["b.txt"])).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "b.txt").unwrap().index, "?");
        assert!(stash_push(&r, None, true).is_err());
    }

    #[test]
    fn m_detached_head() {
        let (_t, r) = fresh("detached");
        let c1 = ci(&r, "a.txt", "1\n", "eins");
        ci(&r, "a.txt", "2\n", "zwei");
        checkout(&r, &c1, false, None).unwrap();
        let s = status(&r).unwrap();
        assert_eq!((s.branch.as_deref(), s.head.as_str()), (None, c1.as_str()));
        let c3 = ci(&r, "a.txt", "3\n", "lose");
        assert!(log(&r, None, 0, 10).unwrap().iter().any(|c| c.sha == c3));
        assert!(push(&r, false).unwrap_err().contains("Detached"));
        assert!(branches(&r).unwrap().iter().all(|b| !b.current));
        checkout(&r, "rettung", true, None).unwrap();
        assert_eq!(status(&r).unwrap().branch.as_deref(), Some("rettung"));
    }

    #[test]
    fn m_upstream_fehlt_und_gone() {
        let (_t, r) = fresh("gone");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "eins");
        push(&r, false).unwrap();
        checkout(&r, "feature/tief/x", true, None).unwrap();
        assert_eq!(status(&r).unwrap().upstream, None);
        ci(&r, "b.txt", "b\n", "b");
        push(&r, false).unwrap();
        let s = status(&r).unwrap();
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/tief/x"));
        assert_eq!((s.ahead, s.behind), (0, 0));
        ci(&r, "b.txt", "bb\n", "bb");
        assert_eq!(status(&r).unwrap().ahead, 1);
        out(git(&r).args(["push", "-q", "origin", "--delete", "feature/tief/x"])).unwrap();
        talk(git(&r).args(["fetch", "--all", "--prune"])).unwrap();
        let s = status(&r).unwrap();
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/tief/x"));
        assert_eq!((s.ahead, s.behind), (0, 0));
        let b = branches(&r).unwrap();
        let f = b.iter().find(|b| b.name == "feature/tief/x").unwrap();
        assert_eq!((f.ahead, f.behind), (0, 0));
        push(&r, false).unwrap();
        assert!(ref_exists(&r, "refs/remotes/origin/feature/tief/x"));
    }

    #[test]
    fn m_push_neuer_branch_ab_remote_branch() {
        // Sidebar "Neuer Branch ab hier" auf origin/main: checkout(v, true, "origin/main")
        let (_t, r) = fresh("ab-remote");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "eins");
        push(&r, false).unwrap();
        checkout(&r, "feature/neu", true, Some("origin/main")).unwrap();
        ci(&r, "b.txt", "b\n", "b");
        let res = push(&r, false);
        assert!(res.is_ok(), "{res:?}");
        assert!(ref_exists(&r, "refs/remotes/origin/feature/neu"));
    }

    #[test]
    fn m_remote_checkout_lokal_existiert() {
        let (_t, r) = fresh("rlokal");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "eins");
        push(&r, false).unwrap();
        checkout(&r, "x/y", true, None).unwrap();
        push(&r, false).unwrap();
        ci(&r, "a.txt", "2\n", "lokal weiter");
        checkout(&r, "main", false, None).unwrap();
        // Backend-Weg: vorhandenen lokalen Branch nehmen
        checkout(&r, "origin/x/y", false, None).unwrap();
        assert_eq!(status(&r).unwrap().branch.as_deref(), Some("x/y"));
        checkout(&r, "main", false, None).unwrap();
        // Frontend-Weg (Sidebar checkoutRemote): create=true scheitert, wenn der lokale existiert
        let res = checkout(&r, "x/y", true, Some("origin/x/y"));
        assert!(res.is_err(), "{res:?}");
    }

    fn git_rename(r: &str, from: &str, to: &str) -> Result<(), String> {
        arg(from)?;
        let to = branch_name(r, to)?;
        out(git(r).args(["branch", "-m", from, to])).map(|_| ())
    }

    #[test]
    fn m_pfade_und_namen_mit_sonderzeichen() {
        let (_t, r) = fresh("sonder");
        ci(&r, "start.txt", "s\n", "start");
        for p in [
            "ordner mit leer/dätei ü.txt",
            "it's.txt",
            "[x].txt",
            "-strich.txt",
            "#raute.txt",
            "tab\u{e9}.txt",
        ] {
            put(&r, p, "x\n");
            let st = status(&r).unwrap();
            assert!(file(&st, p).is_some(), "{p} fehlt in {:?}", st.files);
            let patch = diff(&r, Some(p), false).unwrap();
            assert!(patch.contains("+x"), "{p}");
            // Neue Datei per Hunk-Patch ins Index
            apply(&r, &patch, true, false).unwrap_or_else(|e| panic!("{p}: {e}"));
            assert_eq!(file(&status(&r).unwrap(), p).unwrap().index, "A", "{p}");
            unstage(&r, &v(&[p])).unwrap();
            stage(&r, &v(&[p])).unwrap();
            commit(&r, &format!("add {p}"), false).unwrap();
            put(&r, p, "y\n");
            let patch = diff(&r, Some(p), false).unwrap();
            apply(&r, &patch, false, true).unwrap_or_else(|e| panic!("{p}: {e}"));
            assert!(status(&r).unwrap().files.is_empty(), "{p}");
        }
        for b in ["feature/ümlaut", "it's", "a.b/c-d_e"] {
            checkout(&r, b, true, None).unwrap_or_else(|e| panic!("{b}: {e}"));
            assert_eq!(status(&r).unwrap().branch.as_deref(), Some(b));
            checkout(&r, "main", false, None).unwrap();
        }
        assert!(branches(&r).unwrap().iter().any(|b| b.name == "it's"));
        git_rename(&r, "it's", "umbenannt/ö").unwrap();
        assert!(branches(&r).unwrap().iter().any(|b| b.name == "umbenannt/ö"));
        for bad in ["mit leer", "a..b", "x~1", "-x", "", "HEAD", "a:b", "ende."] {
            assert!(checkout(&r, bad, true, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn m_renames() {
        let (_t, r) = fresh("rename");
        ci(&r, "alt ä.txt", "inhalt\nzeile\n", "start");
        std::fs::rename(Path::new(&r).join("alt ä.txt"), Path::new(&r).join("neu ö.txt")).unwrap();
        let st = status(&r).unwrap();
        assert_eq!(file(&st, "alt ä.txt").unwrap().worktree, "D");
        assert_eq!(file(&st, "neu ö.txt").unwrap().index, "?");
        stage(&r, &v(&["alt ä.txt", "neu ö.txt"])).unwrap();
        let st = status(&r).unwrap();
        let f = file(&st, "neu ö.txt").unwrap();
        assert_eq!((f.index.as_str(), f.orig.as_deref()), ("R", Some("alt ä.txt")));
        put(&r, "neu ö.txt", "inhalt\nzeile\nmehr\n");
        let st = status(&r).unwrap();
        let f = file(&st, "neu ö.txt").unwrap();
        assert_eq!((f.index.as_str(), f.worktree.as_str()), ("R", "M"));
        assert!(diff(&r, Some("neu ö.txt"), false).unwrap().contains("+mehr"));
        discard(&r, &v(&["neu ö.txt"])).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "neu ö.txt").unwrap().worktree, ".");
        let c = commit(&r, "rename", false).unwrap();
        let d = show(&r, &c).unwrap().diff;
        assert!(d.contains("rename from alt ä.txt") && d.contains("rename to neu ö.txt"), "{d}");
    }

    #[test]
    fn m_binaer_crlf_newline_gross() {
        let (_t, r) = fresh("binaer");
        let bin: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        put(&r, "bild.bin", &bin);
        assert!(diff(&r, Some("bild.bin"), false).unwrap().contains("Binary"));
        stage(&r, &v(&["bild.bin"])).unwrap();
        put(&r, "crlf.txt", "a\r\nb\r\nc\r\n");
        put(&r, "ohne.txt", "a\nb");
        stage(&r, &v(&["crlf.txt", "ohne.txt"])).unwrap();
        let c = commit(&r, "start", false).unwrap();
        assert!(show(&r, &c).unwrap().diff.contains("Binary"));
        put(&r, "bild.bin", &bin[..100]);
        assert!(diff(&r, Some("bild.bin"), false).unwrap().contains("Binary"));
        discard(&r, &v(&["bild.bin"])).unwrap();
        assert_eq!(std::fs::read(Path::new(&r).join("bild.bin")).unwrap(), bin);

        // CRLF im Repo: Hunk stagen und in der Arbeitskopie zuruecknehmen
        put(&r, "crlf.txt", "a\r\nB\r\nc\r\nd\r\n");
        let p = diff(&r, Some("crlf.txt"), false).unwrap();
        assert!(p.contains("+B\r\n"));
        apply(&r, &p, true, false).unwrap();
        assert!(diff(&r, Some("crlf.txt"), false).unwrap().is_empty());
        apply(&r, &p, true, true).unwrap();
        apply(&r, &p, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(Path::new(&r).join("crlf.txt")).unwrap(), "a\r\nb\r\nc\r\n");

        // Ohne Newline am Ende
        put(&r, "ohne.txt", "a\nb\nc");
        let p = diff(&r, Some("ohne.txt"), false).unwrap();
        assert!(p.contains("\\ No newline at end of file"));
        apply(&r, &p, true, false).unwrap();
        assert!(diff(&r, Some("ohne.txt"), true).unwrap().contains("+c"));
        apply(&r, &p, true, true).unwrap();
        apply(&r, &p, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(Path::new(&r).join("ohne.txt")).unwrap(), "a\nb");

        // Gross: ~5 MB Textdatei, Patch ueber stdin
        let big: String = (0..400_000).map(|i| format!("zeile {i}\n")).collect();
        put(&r, "gross.txt", &big);
        stage(&r, &v(&["gross.txt"])).unwrap();
        commit(&r, "gross", false).unwrap();
        let big2: String = (0..400_000).map(|i| format!("{} {i}\n", if i % 2 == 0 { "ZEILE" } else { "zeile" })).collect();
        put(&r, "gross.txt", &big2);
        let p = diff(&r, Some("gross.txt"), false).unwrap();
        assert!(p.len() > 1_000_000);
        apply(&r, &p, true, false).unwrap();
        assert!(diff(&r, Some("gross.txt"), false).unwrap().is_empty());
    }

    #[test]
    fn m_crlf_mit_autocrlf() {
        // Git fuer Windows setzt core.autocrlf=true systemweit.
        let (_t, r) = fresh("autocrlf");
        out(git(&r).args(["config", "core.autocrlf", "true"])).unwrap();
        ci(&r, "a.txt", "eins\ndrei\n", "start");
        put(&r, "a.txt", "eins\r\nzwei\r\ndrei\r\nvier\r\n");
        let p = diff(&r, Some("a.txt"), false).unwrap();
        apply(&r, &p, true, false).unwrap();
        assert!(diff(&r, Some("a.txt"), false).unwrap().is_empty());
        apply(&r, &p, true, true).unwrap();
        apply(&r, &p, false, true).unwrap();
        let s = status(&r).unwrap();
        assert!(s.files.is_empty(), "{:?}", s.files);
    }

    #[test]
    fn m_submodul() {
        let (_t, sub) = fresh("sub-quelle");
        ci(&sub, "s.txt", "s\n", "s");
        let (_t2, r) = fresh("sub-haupt");
        ci(&r, "a.txt", "a\n", "a");
        out(git(&r).args(["-c", "protocol.file.allow=always", "submodule", "add", "-q", &sub, "libs/sub"])).unwrap();
        let st = status(&r).unwrap();
        assert!(file(&st, "libs/sub").is_some(), "{:?}", st.files);
        commit(&r, "submodul", false).unwrap();
        let inner = format!("{r}/libs/sub");
        cfg(&inner, "Innen");
        ci(&inner, "s.txt", "neu\n", "neu");
        let st = status(&r).unwrap();
        assert_eq!(file(&st, "libs/sub").unwrap().worktree, "M");
        assert!(diff(&r, Some("libs/sub"), false).unwrap().contains("Subproject commit"));
        stage(&r, &v(&["libs/sub"])).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "libs/sub").unwrap().index, "M");
        unstage(&r, &v(&["libs/sub"])).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "libs/sub").unwrap().index, ".");
        put(&inner, "s.txt", "dreckig\n");
        assert!(file(&status(&r).unwrap(), "libs/sub").is_some());
    }

    #[test]
    fn m_merge_konflikt_abort_und_modify_delete() {
        let (_t, r) = fresh("merge");
        ci(&r, "a.txt", "1\n2\n3\n", "start");
        ci(&r, "weg.txt", "w\n", "weg");
        checkout(&r, "f", true, None).unwrap();
        ci(&r, "a.txt", "1\nF\n3\n", "f");
        ci(&r, "weg.txt", "f aendert\n", "f weg");
        checkout(&r, "main", false, None).unwrap();
        ci(&r, "a.txt", "1\nM\n3\n", "m");
        std::fs::remove_file(Path::new(&r).join("weg.txt")).unwrap();
        stage(&r, &v(&["weg.txt"])).unwrap();
        commit(&r, "m loescht", false).unwrap();

        merge(&r, "f", false).unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Merge);
        assert!(sequencer(&r, "--continue").is_err());
        sequencer(&r, "--abort").unwrap();
        let s = status(&r).unwrap();
        assert_eq!(s.state, RepoState::Clean);
        assert!(s.files.is_empty());

        merge(&r, "f", true).unwrap();
        let st = status(&r).unwrap();
        assert!(file(&st, "weg.txt").unwrap().conflict, "{:?}", st.files);
        let c = conflict(&r, "weg.txt").unwrap();
        assert!(c.ours.is_none() && c.theirs.is_some() && c.base.is_some());
        resolve(&r, "a.txt", "1\nMF\n3\n").unwrap();
        resolve(&r, "weg.txt", "f aendert\n").unwrap();
        assert!(resolve(&r, "a.txt", "x").is_err(), "schon geloest");
        sequencer(&r, "--continue").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
        let l = log(&r, Some("main"), 0, 1).unwrap();
        assert_eq!(l[0].parents.len(), 2);
        // Merge scheitert wegen lokaler Aenderungen: echter Fehler, kein Zustand
        checkout(&r, "g", true, Some("f")).unwrap();
        ci(&r, "a.txt", "G\n", "g");
        checkout(&r, "main", false, None).unwrap();
        put(&r, "a.txt", "lokal\n");
        assert!(merge(&r, "g", false).is_err());
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
    }

    #[test]
    fn m_binaer_konflikt() {
        let (_t, r) = fresh("binkonf");
        put(&r, "b.bin", [0u8, 1, 2, 3]);
        stage(&r, &v(&["b.bin"])).unwrap();
        commit(&r, "start", false).unwrap();
        checkout(&r, "f", true, None).unwrap();
        put(&r, "b.bin", [0u8, 9, 9]);
        stage(&r, &v(&["b.bin"])).unwrap();
        commit(&r, "f", false).unwrap();
        checkout(&r, "main", false, None).unwrap();
        put(&r, "b.bin", [0u8, 7, 7]);
        stage(&r, &v(&["b.bin"])).unwrap();
        commit(&r, "m", false).unwrap();
        merge(&r, "f", false).unwrap();
        assert!(file(&status(&r).unwrap(), "b.bin").unwrap().conflict);
        assert!(conflict(&r, "b.bin").is_err());
        assert!(resolve_side(&r, "nicht-im-konflikt.txt", true).is_err());
        resolve_side(&r, "b.bin", true).unwrap();
        sequencer(&r, "--continue").unwrap();
        assert_eq!(std::fs::read(Path::new(&r).join("b.bin")).unwrap(), [0u8, 9, 9]);
    }

    fn konflikt_setup(name: &str) -> (Tmp, String, String) {
        let (t, r) = fresh(name);
        ci(&r, "a.txt", "1\n2\n3\n", "start");
        checkout(&r, "f", true, None).unwrap();
        let f = ci(&r, "a.txt", "1\nF\n3\n", "f");
        checkout(&r, "main", false, None).unwrap();
        ci(&r, "a.txt", "1\nM\n3\n", "m");
        (t, r, f)
    }

    #[test]
    fn m_rebase_konflikt_geloest_als_leer() {
        let (_t, r, _) = konflikt_setup("rebase");
        checkout(&r, "f", false, None).unwrap();
        ci(&r, "b.txt", "b\n", "f2");
        rebase_onto(&r, "main").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Rebase);
        // Loesung = main-Stand: der Commit wird leer
        resolve(&r, "a.txt", "1\nM\n3\n").unwrap();
        let res = sequencer(&r, "--continue");
        let s = status(&r).unwrap();
        assert_eq!(s.state, RepoState::Clean, "{res:?}");
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(s.branch.as_deref(), Some("f"));
        assert!(Path::new(&r).join("b.txt").exists());
    }

    #[test]
    fn m_rebase_konflikt_mehrfach() {
        let (_t, r, _) = konflikt_setup("rebase2");
        checkout(&r, "f", false, None).unwrap();
        ci(&r, "a.txt", "1\nF2\n3\n", "f2");
        rebase_onto(&r, "main").unwrap();
        resolve(&r, "a.txt", "1\nMF\n3\n").unwrap();
        // Naechster Commit konfliktet erneut: kein Fehler, weiter im Rebase
        sequencer(&r, "--continue").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Rebase);
        assert!(has_conflicts(&r));
        resolve(&r, "a.txt", "1\nMF2\n3\n").unwrap();
        sequencer(&r, "--continue").unwrap();
        let s = status(&r).unwrap();
        assert_eq!((s.state, s.branch.as_deref()), (RepoState::Clean, Some("f")));
        let l = log(&r, Some("f"), 0, 10).unwrap();
        let subj: Vec<_> = l.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subj, ["f2", "f", "m", "start"]);
    }

    #[test]
    fn m_cherry_pick_und_revert() {
        let (_t, r, f) = konflikt_setup("pick");
        pick(&r, "cherry-pick", &f).unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::CherryPick);
        sequencer(&r, "--abort").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
        pick(&r, "cherry-pick", &f).unwrap();
        resolve(&r, "a.txt", "1\nF\n3\n").unwrap();
        sequencer(&r, "--continue").unwrap();
        let l = log(&r, Some("main"), 0, 1).unwrap();
        assert_eq!((l[0].subject.as_str(), status(&r).unwrap().state), ("f", RepoState::Clean));
        // Schon enthalten: leerer Pick; Zustand darf nicht haengen bleiben
        let res = pick(&r, "cherry-pick", &f);
        if status(&r).unwrap().state == RepoState::CherryPick {
            sequencer(&r, "--continue").unwrap();
        }
        assert_eq!(status(&r).unwrap().state, RepoState::Clean, "{res:?}");

        // Revert mit Konflikt: "m" zuruecknehmen kollidiert mit "f"
        let m = log(&r, Some("main"), 0, 3).unwrap()[1].sha.clone();
        pick(&r, "revert", &m).unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Revert);
        sequencer(&r, "--abort").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
        pick(&r, "revert", &m).unwrap();
        resolve(&r, "a.txt", "1\n2\n3\n").unwrap();
        sequencer(&r, "--continue").unwrap();
        assert_eq!(status(&r).unwrap().state, RepoState::Clean);
        assert!(log(&r, Some("main"), 0, 1).unwrap()[0].subject.starts_with("Revert"));
        // Merge-Commit reverten/picken (gegen ersten Parent)
        checkout(&r, "g", true, Some("f")).unwrap();
        ci(&r, "g.txt", "g\n", "g");
        checkout(&r, "main", false, None).unwrap();
        merge(&r, "g", true).unwrap();
        let mc = log(&r, Some("main"), 0, 1).unwrap()[0].sha.clone();
        pick(&r, "revert", &mc).unwrap();
        assert!(!Path::new(&r).join("g.txt").exists());
        pick(&r, "cherry-pick", &mc).unwrap();
        assert!(Path::new(&r).join("g.txt").exists());
    }

    #[test]
    fn m_stash() {
        let (_t, r) = fresh("stash");
        ci(&r, "a.txt", "1\n", "start");
        put(&r, "a.txt", "2\n");
        put(&r, "neu/ü datei.txt", "neu\n");
        stash_push(&r, Some("mit: doppelpunkt"), true).unwrap();
        let s = stashes(&r).unwrap();
        assert_eq!((s[0].branch.as_str(), s[0].message.as_str()), ("main", "mit: doppelpunkt"));
        let sh = stash_show(&r, 0).unwrap();
        assert!(sh.contains("neu/ü datei.txt") && sh.contains("+2"), "{sh}");
        assert!(status(&r).unwrap().files.is_empty());
        // ohne untracked: neue Datei bleibt liegen
        put(&r, "a.txt", "3\n");
        put(&r, "lose.txt", "l\n");
        stash_push(&r, None, false).unwrap();
        assert!(Path::new(&r).join("lose.txt").exists());
        let s = stashes(&r).unwrap();
        assert_eq!((s.len(), s[0].branch.as_str(), s[1].index), (2, "main", 1));
        assert!(stash_show(&r, 0).unwrap().contains("+3"));
        // Pop mit Konflikt: kein Fehler, Stash bleibt
        ci(&r, "a.txt", "4\n", "vier");
        tolerate(&r, git(&r).args(["stash", "pop", &stash_ref(0)])).unwrap();
        assert!(has_conflicts(&r));
        assert_eq!(stashes(&r).unwrap().len(), 2);
        resolve(&r, "a.txt", "4\n").unwrap();
        out(git(&r).args(["stash", "drop", &stash_ref(0)])).unwrap();
        // Untracked-Stash auf Datei, die inzwischen existiert (ohne Konflikt): Fehler
        ci(&r, "a.txt", "2\n", "wie stash");
        put(&r, "neu/ü datei.txt", "anders\n");
        assert!(tolerate(&r, git(&r).args(["stash", "apply", &stash_ref(0)])).is_err());
        std::fs::remove_file(Path::new(&r).join("neu/ü datei.txt")).unwrap();
        discard(&r, &v(&["lose.txt"])).unwrap();
        out(git(&r).args(["reset", "-q", "--hard"])).unwrap();
        tolerate(&r, git(&r).args(["stash", "apply", &stash_ref(0)])).unwrap();
        assert!(Path::new(&r).join("neu/ü datei.txt").exists());
        // Stash im detached HEAD
        let head = status(&r).unwrap().head;
        out(git(&r).args(["stash", "push", "-q", "-u"])).unwrap();
        checkout(&r, &head, false, None).unwrap();
        put(&r, "a.txt", "det\n");
        stash_push(&r, Some("losgeloest"), false).unwrap();
        assert_eq!(stashes(&r).unwrap()[0].message, "losgeloest");
    }

    #[test]
    fn m_tags() {
        let (_t, r) = fresh("tags");
        let c1 = ci(&r, "a.txt", "1\n", "eins");
        let c2 = ci(&r, "a.txt", "2\n", "zwei");
        tag_create(&r, "v1.0", &c1, Some("Erste: Version")).unwrap();
        tag_create(&r, "leicht/ü", "HEAD", None).unwrap();
        tag_create(&r, "leer-nachricht", "HEAD", Some("   ")).unwrap();
        assert!(tag_create(&r, "v1.0", "HEAD", None).is_err());
        assert!(tag_create(&r, "bad name", "HEAD", None).is_err());
        assert!(tag_create(&r, "x", "gibtsnicht", None).is_err());
        let t = tags(&r).unwrap();
        let get = |n: &str| t.iter().find(|t| t.name == n).unwrap();
        assert_eq!((get("v1.0").sha.as_str(), get("v1.0").message.as_str()), (c1.as_str(), "Erste: Version"));
        assert_eq!((get("leicht/ü").sha.as_str(), get("leicht/ü").message.as_str()), (c2.as_str(), ""));
        let l = log(&r, None, 0, 10).unwrap();
        assert!(l.iter().find(|c| c.sha == c1).unwrap().refs.iter().any(|x| x == "tag: v1.0"));
        checkout(&r, "v1.0", false, None).unwrap();
        let s = status(&r).unwrap();
        assert_eq!((s.branch.as_deref(), s.head.as_str()), (None, c1.as_str()));
        checkout(&r, "main", false, None).unwrap();
        out(git(&r).args(["tag", "-d", "leicht/ü"])).unwrap();
        assert_eq!(tags(&r).unwrap().len(), 2);
        // Tag und Branch gleichen Namens: checkout nimmt den Branch
        tag_create(&r, "gleich", &c1, None).unwrap();
        checkout(&r, "gleich", true, None).unwrap();
        ci(&r, "a.txt", "3\n", "drei");
        checkout(&r, "main", false, None).unwrap();
        checkout(&r, "gleich", false, None).unwrap();
        assert_eq!(status(&r).unwrap().branch.as_deref(), Some("gleich"));
    }

    #[test]
    fn m_remote_push_fetch_pull() {
        let (_t, r) = fresh("remote");
        let (_bt, b) = bare(&r);
        ci(&r, "a.txt", "1\n2\n3\n", "start");
        push(&r, false).unwrap();
        checkout(&r, "team/x/y", true, None).unwrap();
        push(&r, false).unwrap();
        checkout(&r, "main", false, None).unwrap();
        let (_t2, r2) = clone(&b, "remote-klon");
        assert!(branches(&r2).unwrap().iter().any(|b| b.name == "origin/team/x/y" && b.remote));
        // Klon pusht, erster ist behind
        ci(&r2, "a.txt", "1\nZWEI\n3\n", "vom klon");
        push(&r2, false).unwrap();
        talk(git(&r).args(["fetch", "--all", "--prune"])).unwrap();
        let s = status(&r).unwrap();
        assert_eq!((s.ahead, s.behind), (0, 1));
        let bm = branches(&r).unwrap();
        let m = bm.iter().find(|b| b.name == "main").unwrap();
        assert_eq!((m.ahead, m.behind), (0, 1));
        // Divergenz + Konflikt beim Pull (merge)
        ci(&r, "a.txt", "1\nEINS\n3\n", "lokal");
        let p = tolerate(&r, git(&r).args(["pull", "--no-rebase"]));
        assert!(p.is_ok(), "{p:?}");
        assert_eq!(status(&r).unwrap().state, RepoState::Merge);
        sequencer(&r, "--abort").unwrap();
        // Pull mit Rebase und Konflikt
        let p = tolerate(&r, git(&r).args(["pull", "--rebase"]));
        assert!(p.is_ok(), "{p:?}");
        assert_eq!(status(&r).unwrap().state, RepoState::Rebase);
        resolve(&r, "a.txt", "1\nBEIDE\n3\n").unwrap();
        sequencer(&r, "--continue").unwrap();
        let s = status(&r).unwrap();
        assert_eq!((s.state, s.ahead, s.behind), (RepoState::Clean, 1, 0));
        push(&r, false).unwrap();
        // Abgelehnter Push ist Fehler; force-with-lease klappt nach fetch
        ci(&r2, "c.txt", "c\n", "klon2");
        assert!(push(&r2, false).is_err());
        talk(git(&r2).args(["fetch", "--all", "--prune"])).unwrap();
        push(&r2, true).unwrap();
        // Remote-Branch mit Slashes loeschen
        branch_delete(&r2, "origin/team/x/y", false, true).unwrap();
        talk(git(&r).args(["fetch", "--all", "--prune"])).unwrap();
        assert!(!branches(&r).unwrap().iter().any(|b| b.name == "origin/team/x/y"));
        assert!(branch_delete(&r, "team/x/y", false, false).is_ok());
    }

    #[test]
    fn m_base_und_unmerged() {
        let (_t, r) = fresh("base");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "start");
        push(&r, false).unwrap();
        checkout(&r, "f", true, None).unwrap();
        ci(&r, "f.txt", "f\n", "f1");
        ci(&r, "f.txt", "ff\n", "f2");
        checkout(&r, "main", false, None).unwrap();
        let um = |r: &str, n: &str| {
            branches(r).unwrap().into_iter().find(|b| b.name == n).map(|b| (b.unmerged, b.base))
        };
        assert_eq!(um(&r, "f"), Some((Some(2), Some("origin/main".into()))));
        assert_eq!(um(&r, "origin/main"), Some((Some(0), Some("origin/main".into()))));
        set_base(&r, Some("f")).unwrap();
        assert_eq!(configured_base(&r).as_deref(), Some("f"));
        assert_eq!(um(&r, "main"), Some((Some(0), Some("f".into()))));
        assert!(set_base(&r, Some("gibtsnicht")).is_err());
        assert!(set_base(&r, Some("-x")).is_err());
        // Eingestellter Branch geloescht -> automatisch
        checkout(&r, "g", true, None).unwrap();
        out(git(&r).args(["branch", "-D", "f"])).unwrap();
        assert_eq!(um(&r, "g").unwrap().1.as_deref(), Some("origin/main"));
        set_base(&r, None).unwrap();
        set_base(&r, None).unwrap();
        assert_eq!(configured_base(&r), None);
        out(git(&r).args(["remote", "set-head", "origin", "main"])).unwrap();
        assert_eq!(base_branch(&r).as_deref(), Some("origin/main"));
        out(git(&r).args(["remote", "remove", "origin"])).unwrap();
        assert_eq!(base_branch(&r).as_deref(), Some("main"));
    }

    #[test]
    fn m_base_origin_head_verwaist() {
        // Remote hat master -> main umbenannt; origin/HEAD zeigt nach fetch --prune ins Leere.
        let (_t, r) = fresh("verwaist");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "start");
        out(git(&r).args(["push", "-q", "origin", "main:master", "main:main"])).unwrap();
        out(git(&r).args(["fetch", "-q"])).unwrap();
        out(git(&r).args(["remote", "set-head", "origin", "master"])).unwrap();
        out(git(&r).args(["push", "-q", "origin", "--delete", "master"])).unwrap();
        talk(git(&r).args(["fetch", "--all", "--prune"])).unwrap();
        checkout(&r, "f", true, None).unwrap();
        ci(&r, "f.txt", "f\n", "f1");
        let f = branches(&r).unwrap().into_iter().find(|b| b.name == "f").unwrap();
        assert_eq!(f.unmerged, Some(1), "base={:?}", base_branch(&r));
    }

    #[test]
    fn m_nesting_ketten_und_duplikate() {
        let (_t, r) = fresh("nest");
        ci(&r, "a.txt", "1\n", "start");
        checkout(&r, "a", true, None).unwrap();
        ci(&r, "a.txt", "a\n", "a");
        checkout(&r, "b", true, None).unwrap();
        ci(&r, "a.txt", "b\n", "b");
        checkout(&r, "c", true, None).unwrap();
        ci(&r, "a.txt", "c\n", "c");
        checkout(&r, "c-kopie", true, None).unwrap();
        checkout(&r, "main", false, None).unwrap();
        checkout(&r, "frei", true, None).unwrap();
        ci(&r, "z.txt", "z\n", "frei");
        let n = nesting(&r, &v(&["a", "b", "c", "frei", "a"])).unwrap();
        let mut a = n.get("a").cloned().unwrap_or_default();
        a.sort();
        assert_eq!(a, ["b", "c"]);
        assert_eq!(n.get("b").cloned().unwrap_or_default(), ["c"]);
        assert!(!n.contains_key("c") && !n.contains_key("frei"));
        // Gleicher Stand: gegenseitig enthalten (Frontend filtert o.sha !== b.sha)
        let n = nesting(&r, &v(&["b", "c", "c-kopie"])).unwrap();
        let mut b = n.get("b").cloned().unwrap_or_default();
        b.sort();
        assert_eq!(b, ["c", "c-kopie"]);
        assert!(nesting(&r, &v(&["a"])).unwrap().is_empty());
        assert!(nesting(&r, &v(&["-x", "a"])).is_err());
    }

    #[test]
    fn m_nesting_mit_remote() {
        let (_t, r) = fresh("nestremote");
        let _b = bare(&r);
        ci(&r, "a.txt", "1\n", "start");
        push(&r, false).unwrap();
        checkout(&r, "team/a", true, None).unwrap();
        ci(&r, "a.txt", "2\n", "a");
        push(&r, false).unwrap();
        checkout(&r, "team/b", true, None).unwrap();
        ci(&r, "a.txt", "3\n", "b");
        let n = nesting(&r, &v(&["origin/team/a", "team/b"])).unwrap();
        assert_eq!(n.get("origin/team/a").cloned().unwrap_or_default(), ["team/b"]);
    }

    #[test]
    fn m_nesting_branch_wie_ordner() {
        let (_t, r) = fresh("nestdir");
        ci(&r, "docs/readme.md", "1\n", "start");
        checkout(&r, "docs", true, None).unwrap();
        ci(&r, "docs/readme.md", "2\n", "docs");
        checkout(&r, "docs2", true, None).unwrap();
        ci(&r, "docs/readme.md", "3\n", "docs2");
        let n = nesting(&r, &v(&["docs", "docs2"]));
        assert!(n.is_ok(), "{n:?}");
        assert_eq!(n.unwrap().get("docs").cloned().unwrap_or_default(), ["docs2"]);
    }

    #[test]
    fn m_reset_und_ungueltige_eingaben() {
        let (_t, r) = fresh("reset");
        let c1 = ci(&r, "a.txt", "1\n", "eins");
        ci(&r, "a.txt", "2\n", "zwei");
        // wie git_reset
        let reset = |mode: &str, sha: &str| talk(git(&r).args(["reset", "-q", mode, arg(sha)?, "--"]));
        reset("--soft", &c1).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "a.txt").unwrap().index, "M");
        reset("--mixed", &c1).unwrap();
        assert_eq!(file(&status(&r).unwrap(), "a.txt").unwrap().worktree, "M");
        reset("--hard", &c1).unwrap();
        assert!(status(&r).unwrap().files.is_empty());
        assert!(reset("--hard", "--all").is_err());
        assert!(log(&r, Some("--all"), 0, 1).is_err());
        assert!(show(&r, "-p").is_err());
        assert!(merge(&r, "--abort", false).is_err());
        assert!(pick(&r, "cherry-pick", "-n").is_err());
        assert!(conflict(&r, "../x").is_err());
        assert!(resolve(&r, "a.txt", "x").is_err());
        assert!(sequencer(&r, "--continue").is_err());
        assert!(diff(&r, Some("gibtsnicht.txt"), false).unwrap().is_empty());
        // Pathspec-Magie wird nicht ausgewertet
        put(&r, "b.txt", "b\n");
        assert!(stage(&r, &v(&[":/"])).is_err());
        assert!(file(&status(&r).unwrap(), "b.txt").unwrap().index == "?");
    }

    #[test]
    fn m_amend_und_commit_nachricht() {
        let (_t, r) = fresh("amend");
        ci(&r, "a.txt", "1\n", "eins");
        let c = commit(&r, "# kein Kommentar\n\nBody mit # Raute", true).unwrap();
        let d = show(&r, &c).unwrap();
        assert_eq!(d.commit.subject, "# kein Kommentar");
        assert_eq!(d.body, "Body mit # Raute");
        assert!(commit(&r, "   \n", true).is_err());
        assert_eq!(log(&r, None, 0, 10).unwrap().len(), 1);
    }

    #[test]
    fn m_stash_untracked_teilweise() {
        let (_t, r) = fresh("stashteil");
        ci(&r, "a.txt", "1\n", "start");
        put(&r, "a.txt", "2\n");
        put(&r, "neu.txt", "aus dem stash\n");
        stash_push(&r, None, true).unwrap();
        ci(&r, "a.txt", "3\n", "konkurriert");
        put(&r, "neu.txt", "anders\n");
        // wie git_stash_apply(pop=true)
        let res = tolerate(&r, git(&r).args(["stash", "pop", &stash_ref(0)]));
        // Tracked-Teil kam mit Konflikt, neu.txt aus dem Stash fehlt: das ist kein Erfolg.
        assert!(res.is_err(), "{res:?}");
    }

    #[test]
    fn m_hunk_und_zeilen_apply() {
        let (_t, r) = fresh("hunks");
        let base: String = (1..=20).map(|i| format!("z{i}\n")).collect();
        ci(&r, "a.txt", &base, "start");
        let neu = base.replace("z2\n", "z2\nNEU-A\nNEU-B\n").replace("z18\n", "z18\nNEU-C\n");
        put(&r, "a.txt", &neu);
        let p = diff(&r, Some("a.txt"), false).unwrap();
        let hunks: Vec<&str> = p.split("\n@@").collect();
        assert_eq!(hunks.len(), 3, "{p}");
        // Nur den zweiten Hunk stagen
        let second = format!("{}\n@@{}", hunks[0], hunks[2]);
        apply(&r, &second, true, false).unwrap();
        let staged = diff(&r, Some("a.txt"), true).unwrap();
        assert!(staged.contains("+NEU-C") && !staged.contains("NEU-A"), "{staged}");
        // Einzelne Zeile aus dem ersten Hunk: NEU-B weglassen, Zaehler bleiben falsch (--recount)
        let first = format!("{}\n@@{}\n", hunks[0], hunks[1]).replace("+NEU-B\n", "");
        apply(&r, &first, true, false).unwrap();
        let staged = diff(&r, Some("a.txt"), true).unwrap();
        assert!(staged.contains("+NEU-A") && !staged.contains("NEU-B"), "{staged}");
        // Zeile aus dem Index zurueck (reverse cached) und Hunk in der Arbeitskopie verwerfen
        let s = diff(&r, Some("a.txt"), true).unwrap();
        let sh: Vec<&str> = s.split("\n@@").collect();
        apply(&r, &format!("{}\n@@{}", sh[0], sh[2]), true, true).unwrap();
        assert!(!diff(&r, Some("a.txt"), true).unwrap().contains("NEU-C"));
        let w = diff(&r, Some("a.txt"), false).unwrap();
        apply(&r, &w, false, true).unwrap();
        let now = std::fs::read_to_string(Path::new(&r).join("a.txt")).unwrap();
        assert!(now.contains("NEU-A") && !now.contains("NEU-B") && !now.contains("NEU-C"), "{now}");
        // Veralteter Patch (Datei inzwischen geaendert) wird abgelehnt statt falsch angewandt
        put(&r, "a.txt", base.replace("z3\n", "ANDERS\n"));
        assert!(apply(&r, &first, false, true).is_err());
        // Nicht-UTF-8-Datei: sprechende Meldung
        put(&r, "latin.txt", b"gr\xfc\xdfe\n".as_slice());
        stage(&r, &v(&["latin.txt"])).unwrap();
        commit(&r, "latin", false).unwrap();
        put(&r, "latin.txt", b"gr\xfc\xdfe\nneu\n".as_slice());
        let p = diff(&r, Some("latin.txt"), false).unwrap();
        let e = apply(&r, &p.replace("+neu\n", "+neu\n+neu2\n"), true, false);
        assert!(e.is_ok() || e.unwrap_err().contains("UTF-8"));
    }

    #[test]
    fn m_discard_varianten() {
        let (_t, r) = fresh("discard");
        ci(&r, "a.txt", "1\n", "start");
        // Gestaged neu + Arbeitskopie geaendert: nur Arbeitskopie zurueck
        put(&r, "n.txt", "idx\n");
        stage(&r, &v(&["n.txt"])).unwrap();
        put(&r, "n.txt", "wt\n");
        discard(&r, &v(&["n.txt"])).unwrap();
        assert_eq!(std::fs::read_to_string(Path::new(&r).join("n.txt")).unwrap(), "idx\n");
        // Geloescht in der Arbeitskopie
        std::fs::remove_file(Path::new(&r).join("a.txt")).unwrap();
        discard(&r, &v(&["a.txt"])).unwrap();
        assert!(Path::new(&r).join("a.txt").exists());
        // Untracked in tiefem Ordner + tracked gemischt
        put(&r, "x/y/z.txt", "z\n");
        put(&r, "a.txt", "2\n");
        discard(&r, &v(&["x/y/z.txt", "a.txt"])).unwrap();
        assert!(!Path::new(&r).join("x/y/z.txt").exists());
        assert_eq!(std::fs::read_to_string(Path::new(&r).join("a.txt")).unwrap(), "1\n");
        // Ignorierte Dateien bleiben unangetastet
        put(&r, ".gitignore", "*.log\n");
        put(&r, "b.log", "log\n");
        discard(&r, &v(&["b.log"])).unwrap_or(());
        assert!(Path::new(&r).join("b.log").exists());
    }
}
