//! GitHub/GitLab-Anbindung: Konten, Repo-Suche, PRs/MRs, Klonen.
//! Token liegen nur im Windows-Anmeldeinfo-Speicher und tauchen nie in argv, Logs oder Fehlern auf.

use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

#[derive(Serialize)]
pub struct Account {
    kind: String,
    host: String,
    user: String,
    avatar: Option<String>,
}

#[derive(Serialize)]
pub struct RemoteRepo {
    kind: String,
    host: String,
    full_name: String,
    name: String,
    description: Option<String>,
    private: bool,
    fork: bool,
    archived: bool,
    stars: u64,
    lang: Option<String>,
    updated_at: String,
    default_branch: Option<String>,
    clone_http: String,
    clone_ssh: String,
    web_url: String,
}

#[derive(Serialize)]
pub struct PullRequest {
    number: u64,
    title: String,
    author: String,
    source_branch: String,
    target_branch: String,
    draft: bool,
    updated_at: String,
    web_url: String,
}

/// Option<ForgeList> serialisiert zu {kind, items} | null wie ForgeList<T> in git.ts.
#[derive(Serialize)]
pub struct ForgeList<T> {
    kind: &'static str,
    items: Vec<T>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ReleaseAsset {
    name: String,
    size: u64,
    downloads: u64,
    url: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Release {
    id: String,
    tag: String,
    name: String,
    body: String,
    draft: bool,
    prerelease: bool,
    created_at: String,
    published_at: Option<String>,
    author: String,
    web_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
pub struct ReleaseInput {
    tag: String,
    name: String,
    body: String,
    draft: bool,
    prerelease: bool,
    target: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Run {
    id: u64,
    name: String,
    title: String,
    branch: String,
    sha: String,
    event: String,
    status: &'static str,
    created_at: String,
    duration_s: Option<u64>,
    web_url: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Job {
    id: u64,
    name: String,
    stage: Option<String>,
    status: &'static str,
    started_at: Option<String>,
    duration_s: Option<u64>,
    web_url: String,
}

#[derive(Serialize, Clone)]
struct CloneProgress {
    line: String,
    percent: Option<u8>,
}

fn check_kind(kind: &str) -> Result<(), String> {
    match kind {
        "github" | "gitlab" => Ok(()),
        _ => Err(format!("Unbekannter Anbieter: {kind}")),
    }
}

/// "https://GitLab.firma.de/" -> "gitlab.firma.de". Nur Hostname[:Port], sonst ginge der Token sonstwohin.
fn norm_host(host: &str) -> Result<String, String> {
    let h = host.trim();
    // API spricht immer https (api_base), http:// wird also nur abgeschnitten.
    let h = h.to_ascii_lowercase();
    let h = h.strip_prefix("https://").or(h.strip_prefix("http://")).unwrap_or(&h);
    let h = h.trim_end_matches('/').to_string();
    let ok = !h.is_empty()
        && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    if ok {
        Ok(h)
    } else {
        Err(format!("Ungültiger Host: {host}"))
    }
}

fn api_base(kind: &str, host: &str) -> String {
    match (kind, host) {
        ("github", "github.com") => "https://api.github.com".into(),
        ("github", _) => format!("https://{host}/api/v3"),
        _ => format!("https://{host}/api/v4"),
    }
}

fn entry(kind: &str, host: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("ocui", &format!("{kind}:{host}"))
        .map_err(|e| format!("Anmeldeinfo-Speicher: {e}"))
}

/// None = kein Konto hinterlegt.
fn token(kind: &str, host: &str) -> Result<Option<String>, String> {
    match entry(kind, host)?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Anmeldeinfo-Speicher: {e}")),
    }
}

/// Erstes Konto (GitHub vor GitLab), das zu diesem Host passt.
fn token_for_host(host: &str) -> Result<Option<(&'static str, String)>, String> {
    for kind in ["github", "gitlab"] {
        if let Some(t) = token(kind, host)? {
            return Ok(Some((kind, t)));
        }
    }
    Ok(None)
}

async fn api_get(kind: &str, host: &str, tok: &str, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
    api(kind, host, tok, reqwest::Method::GET, path, query, None).await
}

async fn api(
    kind: &str,
    host: &str,
    tok: &str,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .user_agent("ocui")
        .timeout(Duration::from_secs(30))
        // Umbenannte Repos antworten mit 301 auf denselben Host. Fremde Hosts nie:
        // reqwest wuerde PRIVATE-TOKEN dorthin mitschicken.
        .redirect(reqwest::redirect::Policy::custom(|a| {
            let first = &a.previous()[0];
            let same = a.url().scheme() == "https"
                && a.url().host_str() == first.host_str()
                && a.url().port_or_known_default() == first.port_or_known_default();
            if same && a.previous().len() < 5 { a.follow() } else { a.stop() }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let req = client.request(method, format!("{}{path}", api_base(kind, host))).query(query);
    let req = match body {
        Some(b) => req.json(b),
        None => req,
    };
    let req = if kind == "github" {
        req.bearer_auth(tok).header("Accept", "application/vnd.github+json")
    } else {
        req.header("PRIVATE-TOKEN", tok)
    };
    // reqwest-Fehler nennen nur die URL; der Token steckt im Header.
    let res = req.send().await.map_err(|e| format!("{host} nicht erreichbar: {e}"))?;
    let status = res.status();
    // DELETE antwortet mit 204 ohne Body -> Null.
    let body: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(api_error(host, status, &body));
    }
    Ok(body)
}

fn api_error(host: &str, status: reqwest::StatusCode, body: &Value) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return format!("{host}: Token ungültig oder abgelaufen (401)");
    }
    let msg = body["message"].as_str().or(body["error"].as_str()).unwrap_or("");
    // GitLab schickt z. B. {"message":"403 Forbidden"}, das stuende sonst doppelt da.
    let msg = if status.to_string().contains(msg) { "" } else { msg };
    let hint = if status == reqwest::StatusCode::FORBIDDEN { " (Token ohne Rechte?)" } else { "" };
    format!("{host} antwortet mit {status}{hint} {msg}").trim().to_string()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn opt(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(String::from)
}

async fn verify(kind: String, host: String, tok: String) -> Result<Account, String> {
    check_kind(&kind)?;
    let host = norm_host(&host)?;
    let tok = tok.trim().to_string();
    if tok.is_empty() {
        return Err("Token fehlt".into());
    }
    let me = api_get(&kind, &host, &tok, "/user", &[]).await?;
    let user = if kind == "github" { s(&me["login"]) } else { s(&me["username"]) };
    entry(&kind, &host)?
        .set_password(&tok)
        .map_err(|e| format!("Anmeldeinfo-Speicher: {e}"))?;
    Ok(Account { avatar: opt(&me["avatar_url"]), kind, host, user })
}

#[tauri::command]
pub async fn forge_login(kind: String, host: String, token: String) -> Result<Account, String> {
    verify(kind, host, token).await
}

#[tauri::command]
pub fn forge_logout(kind: String, host: String) -> Result<(), String> {
    check_kind(&kind)?;
    match entry(&kind, &norm_host(&host)?)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Anmeldeinfo-Speicher: {e}")),
    }
}

/// Token aus `gh auth token` bzw. `glab config get token` holen und wie beim Login pruefen.
#[tauri::command]
pub async fn forge_import_cli(kind: String, host: String) -> Result<Account, String> {
    check_kind(&kind)?;
    let h = norm_host(&host)?;
    let (prog, args) = if kind == "github" {
        ("gh", vec!["auth", "token", "--hostname", &h])
    } else {
        ("glab", vec!["config", "get", "token", "--host", &h])
    };
    let out = crate::quiet(prog)
        .args(args)
        .output()
        .map_err(|_| format!("{prog} ist nicht installiert"))?;
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || tok.is_empty() {
        // stderr von gh/glab enthaelt keinen Token.
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("{prog}: kein Token für {h}. {err}").trim().to_string());
    }
    verify(kind, h, tok).await
}

#[tauri::command]
pub async fn forge_repos(kind: String, host: String, query: String, page: u32) -> Result<Vec<RemoteRepo>, String> {
    check_kind(&kind)?;
    let host = norm_host(&host)?;
    let tok = token(&kind, &host)?.ok_or_else(|| format!("Kein Konto für {host}"))?;
    let page = page.max(1).to_string();
    let q = query.trim().to_string();
    let per = ("per_page", "50".to_string());

    if kind == "github" {
        let body = if q.is_empty() {
            let aff = "owner,collaborator,organization_member".to_string();
            let qs = [("sort", "pushed".into()), ("affiliation", aff), per, ("page", page)];
            api_get(&kind, &host, &tok, "/user/repos", &qs).await?
        } else {
            let qs = [("q", q), per, ("page", page)];
            api_get(&kind, &host, &tok, "/search/repositories", &qs).await?["items"].take()
        };
        let list = body.as_array().cloned().unwrap_or_default();
        return Ok(list
            .iter()
            .map(|r| RemoteRepo {
                kind: kind.clone(),
                host: host.clone(),
                full_name: s(&r["full_name"]),
                name: s(&r["name"]),
                description: opt(&r["description"]),
                private: r["private"].as_bool().unwrap_or(false),
                fork: r["fork"].as_bool().unwrap_or(false),
                archived: r["archived"].as_bool().unwrap_or(false),
                stars: r["stargazers_count"].as_u64().unwrap_or(0),
                lang: opt(&r["language"]),
                updated_at: opt(&r["pushed_at"]).unwrap_or_else(|| s(&r["updated_at"])),
                default_branch: opt(&r["default_branch"]),
                clone_http: s(&r["clone_url"]),
                clone_ssh: s(&r["ssh_url"]),
                web_url: s(&r["html_url"]),
            })
            .collect());
    }

    let mut qs = vec![
        ("membership", "true".to_string()),
        ("order_by", "last_activity_at".into()),
        ("simple", "false".into()),
        per,
        ("page", page),
    ];
    if !q.is_empty() {
        qs.push(("search", q));
    }
    let body = api_get(&kind, &host, &tok, "/projects", &qs).await?;
    let list = body.as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .map(|r| RemoteRepo {
            kind: kind.clone(),
            host: host.clone(),
            full_name: s(&r["path_with_namespace"]),
            // path statt name: taugt als Ordnername.
            name: s(&r["path"]),
            description: opt(&r["description"]),
            private: r["visibility"].as_str() != Some("public"),
            fork: r["forked_from_project"].is_object(),
            archived: r["archived"].as_bool().unwrap_or(false),
            stars: r["star_count"].as_u64().unwrap_or(0),
            // Sprache kostet bei GitLab einen Extra-Aufruf pro Projekt.
            lang: None,
            updated_at: s(&r["last_activity_at"]),
            default_branch: opt(&r["default_branch"]),
            clone_http: s(&r["http_url_to_repo"]),
            clone_ssh: s(&r["ssh_url_to_repo"]),
            web_url: s(&r["web_url"]),
        })
        .collect())
}

/// Remote-URL -> (Host, "owner/repo"). https (mit Port), ssh:// (Port ist SSH-Port, faellt weg) und scp-artig.
fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        let (auth, path) = rest.split_once('/')?;
        let host = auth.rsplit('@').next()?;
        let host = match scheme {
            "https" | "http" => host,
            "ssh" | "git" => host.split(':').next()?,
            _ => return None,
        };
        (host, path)
    } else {
        // git@host:owner/repo.git; "C:/..." ist ein Windows-Pfad, kein Host.
        let (auth, path) = url.split_once(':')?;
        let host = auth.rsplit('@').next()?;
        if host.len() < 2 || auth.contains('/') {
            return None;
        }
        (host, path)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || !path.contains('/') {
        return None;
    }
    Some((host.to_ascii_lowercase(), path.to_string()))
}

/// Pfad fuer GitLabs /projects/{id}: alles ausser unreservierten Zeichen kodieren.
fn encode(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Remote eines lokalen Repos mit passendem Konto; base ist "/repos/o/r" bzw. "/projects/<kodiert>".
struct Repo {
    kind: &'static str,
    host: String,
    base: String,
    tok: String,
}

impl Repo {
    fn github(&self) -> bool {
        self.kind == "github"
    }

    async fn call(&self, m: reqwest::Method, sub: &str, qs: &[(&str, String)], body: Option<&Value>) -> Result<Value, String> {
        api(self.kind, &self.host, &self.tok, m, &format!("{}{sub}", self.base), qs, body).await
    }

    async fn get(&self, sub: &str, qs: &[(&str, String)]) -> Result<Value, String> {
        self.call(reqwest::Method::GET, sub, qs, None).await
    }
}

/// None = Remote nicht parsebar oder kein Konto fuer den Host.
fn resolve(remote_url: &str) -> Result<Option<Repo>, String> {
    let Some((host, path)) = parse_remote(remote_url) else {
        return Ok(None);
    };
    Ok(token_for_host(&host)?.map(|(kind, tok)| {
        let base = if kind == "github" { format!("/repos/{path}") } else { format!("/projects/{}", encode(&path)) };
        Repo { kind, host, base, tok }
    }))
}

/// Fuer Schreibzugriffe: ohne Konto ist das ein Fehler, kein leeres Ergebnis.
fn require(remote_url: &str) -> Result<Repo, String> {
    resolve(remote_url)?.ok_or_else(|| "Kein Konto für dieses Remote-Repository hinterlegt".to_string())
}

#[tauri::command]
pub async fn forge_pulls(remote_url: String) -> Result<Vec<PullRequest>, String> {
    let Some(repo) = resolve(&remote_url)? else {
        return Ok(Vec::new());
    };
    let per = ("per_page", "50".to_string());
    if repo.github() {
        let qs = [("state", "open".to_string()), per];
        let body = repo.get("/pulls", &qs).await?;
        return Ok(body
            .as_array()
            .map(|l| {
                l.iter()
                    .map(|p| PullRequest {
                        number: p["number"].as_u64().unwrap_or(0),
                        title: s(&p["title"]),
                        author: s(&p["user"]["login"]),
                        source_branch: s(&p["head"]["ref"]),
                        target_branch: s(&p["base"]["ref"]),
                        draft: p["draft"].as_bool().unwrap_or(false),
                        updated_at: s(&p["updated_at"]),
                        web_url: s(&p["html_url"]),
                    })
                    .collect()
            })
            .unwrap_or_default());
    }
    let qs = [("state", "opened".to_string()), per];
    let body = repo.get("/merge_requests", &qs).await?;
    Ok(body
        .as_array()
        .map(|l| {
            l.iter()
                .map(|m| PullRequest {
                    number: m["iid"].as_u64().unwrap_or(0),
                    title: s(&m["title"]),
                    author: s(&m["author"]["username"]),
                    source_branch: s(&m["source_branch"]),
                    target_branch: s(&m["target_branch"]),
                    draft: m["draft"].as_bool().or(m["work_in_progress"].as_bool()).unwrap_or(false),
                    updated_at: s(&m["updated_at"]),
                    web_url: s(&m["web_url"]),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// ISO-8601 ("2024-05-01T12:00:03.5Z", "...+02:00") -> Unix-Sekunden. Ohne chrono, das waere eine neue Dependency.
fn unix_secs(ts: &str) -> Option<i64> {
    let n = |s: &str, r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(ts, 0..4)?, n(ts, 5..7)?, n(ts, 8..10)?);
    let (h, mi, se) = (n(ts, 11..13)?, n(ts, 14..16)?, n(ts, 17..19)?);
    let rest = ts.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest.chars().next() {
        None | Some('Z') => 0,
        Some(sign @ ('+' | '-')) => {
            let o = n(rest, 1..3)? * 3600 + n(rest, 4..6)? * 60;
            if sign == '-' { -o } else { o }
        }
        _ => return None,
    };
    // Tage seit 1970 nach Howard Hinnants days_from_civil.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se - off)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Sekunden von start bis end; end fehlt = laeuft noch, dann bis now.
fn span(start: &Value, end: &Value, now: i64) -> Option<u64> {
    let a = unix_secs(start.as_str()?)?;
    let b = end.as_str().map_or(Some(now), unix_secs)?;
    Some((b - a).max(0) as u64)
}

// Wartet auf einen Menschen (Freigabe, manueller Job, Zeitplan): nicht "queued",
// sonst gilt der Run als aktiv und die UI pollt womoeglich wochenlang. "skipped" ist das Naechstliegende.
fn gh_status(status: &str, conclusion: &str) -> &'static str {
    match (status, conclusion) {
        ("in_progress", _) => "running",
        ("waiting", _) => "skipped",
        ("completed", "success") => "success",
        ("completed", "cancelled") => "cancelled",
        ("completed", "skipped" | "neutral" | "stale") => "skipped",
        ("completed", _) => "failure",
        _ => "queued",
    }
}

fn gl_status(status: &str) -> &'static str {
    match status {
        "running" | "canceling" => "running",
        "success" => "success",
        "failed" => "failure",
        "canceled" => "cancelled",
        "skipped" | "manual" | "scheduled" => "skipped",
        _ => "queued",
    }
}

fn gh_release(r: &Value) -> Release {
    let assets = r["assets"].as_array().map(Vec::as_slice).unwrap_or_default();
    Release {
        id: r["id"].as_u64().unwrap_or(0).to_string(),
        tag: s(&r["tag_name"]),
        name: s(&r["name"]),
        body: s(&r["body"]),
        draft: r["draft"].as_bool().unwrap_or(false),
        prerelease: r["prerelease"].as_bool().unwrap_or(false),
        created_at: s(&r["created_at"]),
        published_at: opt(&r["published_at"]),
        author: s(&r["author"]["login"]),
        web_url: s(&r["html_url"]),
        assets: assets
            .iter()
            .map(|a| ReleaseAsset {
                name: s(&a["name"]),
                size: a["size"].as_u64().unwrap_or(0),
                downloads: a["download_count"].as_u64().unwrap_or(0),
                url: s(&a["browser_download_url"]),
            })
            .collect(),
    }
}

fn gl_release(r: &Value) -> Release {
    let links = r["assets"]["links"].as_array().map(Vec::as_slice).unwrap_or_default();
    Release {
        id: s(&r["tag_name"]),
        tag: s(&r["tag_name"]),
        name: s(&r["name"]),
        body: s(&r["description"]),
        draft: false,
        prerelease: false,
        created_at: s(&r["created_at"]),
        published_at: opt(&r["released_at"]),
        author: s(&r["author"]["username"]),
        web_url: s(&r["_links"]["self"]),
        assets: links
            .iter()
            .map(|a| ReleaseAsset { name: s(&a["name"]), size: 0, downloads: 0, url: s(&a["url"]) })
            .collect(),
    }
}

fn gh_run(r: &Value, now: i64) -> Run {
    let status = gh_status(r["status"].as_str().unwrap_or(""), r["conclusion"].as_str().unwrap_or(""));
    let end = if r["status"] == "completed" { &r["updated_at"] } else { &Value::Null };
    let title = opt(&r["display_title"])
        .unwrap_or_else(|| r["head_commit"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("").into());
    Run {
        id: r["id"].as_u64().unwrap_or(0),
        name: s(&r["name"]),
        title,
        branch: s(&r["head_branch"]),
        sha: s(&r["head_sha"]),
        event: s(&r["event"]),
        status,
        created_at: s(&r["created_at"]),
        duration_s: span(&r["run_started_at"], end, now),
        web_url: s(&r["html_url"]),
    }
}

fn gl_run(r: &Value) -> Run {
    Run {
        id: r["id"].as_u64().unwrap_or(0),
        name: format!("Pipeline #{}", r["iid"].as_u64().unwrap_or(0)),
        // Die Liste liefert keinen Commit-Titel.
        title: String::new(),
        branch: s(&r["ref"]),
        sha: s(&r["sha"]),
        event: s(&r["source"]),
        status: gl_status(r["status"].as_str().unwrap_or("")),
        created_at: s(&r["created_at"]),
        // Liste hat weder duration noch started_at; updated_at-created_at enthielte Wartezeit und spaete Retries.
        duration_s: None,
        web_url: s(&r["web_url"]),
    }
}

fn gh_job(j: &Value, now: i64) -> Job {
    Job {
        id: j["id"].as_u64().unwrap_or(0),
        name: s(&j["name"]),
        stage: None,
        status: gh_status(j["status"].as_str().unwrap_or(""), j["conclusion"].as_str().unwrap_or("")),
        started_at: opt(&j["started_at"]),
        duration_s: if j["status"] == "queued" { None } else { span(&j["started_at"], &j["completed_at"], now) },
        web_url: s(&j["html_url"]),
    }
}

fn gl_job(j: &Value) -> Job {
    Job {
        id: j["id"].as_u64().unwrap_or(0),
        name: s(&j["name"]),
        stage: opt(&j["stage"]),
        status: gl_status(j["status"].as_str().unwrap_or("")),
        started_at: opt(&j["started_at"]),
        duration_s: j["duration"].as_f64().map(|d| d.max(0.0).round() as u64),
        web_url: s(&j["web_url"]),
    }
}

fn items<T>(v: &Value, f: impl Fn(&Value) -> T) -> Vec<T> {
    v.as_array().map(|l| l.iter().map(f).collect()).unwrap_or_default()
}

fn page_qs(page: u32) -> Vec<(&'static str, String)> {
    vec![("per_page", "30".into()), ("page", page.max(1).to_string())]
}

/// Die ID landet im URL-Pfad: GitHub nur Ziffern, GitLab-Tag kodiert. Sonst ginge "../" an eine andere API-Route.
fn release_path(repo: &Repo, id: &str) -> Result<String, String> {
    if !repo.github() {
        return Ok(format!("/releases/{}", encode(id)));
    }
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("Ungültige Release-ID: {id}"));
    }
    Ok(format!("/releases/{id}"))
}

#[tauri::command]
pub async fn forge_releases(remote_url: String, page: u32) -> Result<Option<ForgeList<Release>>, String> {
    let Some(repo) = resolve(&remote_url)? else {
        return Ok(None);
    };
    let body = repo.get("/releases", &page_qs(page)).await?;
    let items = if repo.github() { items(&body, gh_release) } else { items(&body, gl_release) };
    Ok(Some(ForgeList { kind: repo.kind, items }))
}

#[tauri::command]
pub async fn forge_release_save(remote_url: String, id: Option<String>, input: ReleaseInput) -> Result<Release, String> {
    use reqwest::Method;
    let repo = require(&remote_url)?;
    let target = input.target.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if repo.github() {
        let mut body = serde_json::json!({
            "tag_name": input.tag,
            "name": input.name,
            "body": input.body,
            "draft": input.draft,
            "prerelease": input.prerelease,
        });
        if let Some(t) = target {
            body["target_commitish"] = t.into();
        }
        let r = match id {
            None => repo.call(Method::POST, "/releases", &[], Some(&body)).await?,
            Some(id) => repo.call(Method::PATCH, &release_path(&repo, &id)?, &[], Some(&body)).await?,
        };
        return Ok(gh_release(&r));
    }
    let r = match id {
        None => {
            let mut body = serde_json::json!({ "tag_name": input.tag, "name": input.name, "description": input.body });
            if let Some(t) = target {
                body["ref"] = t.into();
            }
            repo.call(Method::POST, "/releases", &[], Some(&body)).await?
        }
        Some(tag) => {
            if input.tag != tag {
                return Err("GitLab kann den Tag eines Releases nicht ändern. Release löschen und neu anlegen.".into());
            }
            let body = serde_json::json!({ "name": input.name, "description": input.body });
            repo.call(Method::PUT, &release_path(&repo, &tag)?, &[], Some(&body)).await?
        }
    };
    Ok(gl_release(&r))
}

#[tauri::command]
pub async fn forge_release_delete(remote_url: String, id: String) -> Result<(), String> {
    let repo = require(&remote_url)?;
    repo.call(reqwest::Method::DELETE, &release_path(&repo, &id)?, &[], None).await?;
    Ok(())
}

#[tauri::command]
pub async fn forge_runs(remote_url: String, branch: String, page: u32) -> Result<Option<ForgeList<Run>>, String> {
    let Some(repo) = resolve(&remote_url)? else {
        return Ok(None);
    };
    let mut qs = page_qs(page);
    let branch = branch.trim().to_string();
    if repo.github() {
        if !branch.is_empty() {
            qs.push(("branch", branch));
        }
        let body = repo.get("/actions/runs", &qs).await?;
        let now = now_secs();
        let items = items(&body["workflow_runs"], |r| gh_run(r, now));
        return Ok(Some(ForgeList { kind: repo.kind, items }));
    }
    if !branch.is_empty() {
        qs.push(("ref", branch));
    }
    let body = repo.get("/pipelines", &qs).await?;
    Ok(Some(ForgeList { kind: repo.kind, items: items(&body, gl_run) }))
}

#[tauri::command]
pub async fn forge_jobs(remote_url: String, run_id: u64) -> Result<Vec<Job>, String> {
    let repo = require(&remote_url)?;
    let qs = [("per_page", "100".to_string())];
    if repo.github() {
        let body = repo.get(&format!("/actions/runs/{run_id}/jobs"), &qs).await?;
        let now = now_secs();
        return Ok(items(&body["jobs"], |j| gh_job(j, now)));
    }
    let body = repo.get(&format!("/pipelines/{run_id}/jobs"), &qs).await?;
    Ok(items(&body, gl_job))
}

/// "Receiving objects:  45% (45/100)" -> 45.
fn parse_percent(line: &str) -> Option<u8> {
    let head = &line[..line.find('%')?];
    let digits: String = head.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    digits.chars().rev().collect::<String>().parse().ok().filter(|p| *p <= 100)
}

#[tauri::command]
pub async fn forge_clone(app: AppHandle, id: String, url: String, dest: String) -> Result<String, String> {
    let event = format!("clone:{id}");
    let emit = move |p: CloneProgress| {
        let _ = app.emit(&event, p);
    };
    tauri::async_runtime::spawn_blocking(move || clone_blocking(url, dest, emit))
        .await
        .map_err(|e| format!("Klonen abgebrochen: {e}"))?
}

fn clone_blocking(url: String, dest: String, emit: impl Fn(CloneProgress)) -> Result<String, String> {
    let url = url.trim().to_string();
    if url.is_empty() || url.starts_with('-') {
        return Err(format!("Ungültige URL: {url}"));
    }
    let target = std::path::Path::new(&dest);
    if target.exists() && std::fs::read_dir(target).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("Zielordner ist nicht leer: {dest}"));
    }

    let mut cmd = crate::quiet("git");
    cmd.args(["clone", "--progress", "--", &url, &dest])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    // HTTPS mit Konto: Header nur fuer diesen Aufruf und nur fuer diesen Host, per Env statt argv.
    if url.starts_with("https://") {
        if let Some((host, _)) = parse_remote(&url) {
            if let Some((kind, tok)) = token_for_host(&host)? {
                let user = if kind == "github" { "x-access-token" } else { "oauth2" };
                let basic = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{tok}"));
                cmd.env("GIT_CONFIG_COUNT", "1")
                    .env("GIT_CONFIG_KEY_0", format!("http.https://{host}/.extraHeader"))
                    .env("GIT_CONFIG_VALUE_0", format!("Authorization: Basic {basic}"));
            }
        }
    }

    let mut child = cmd.spawn().map_err(|e| format!("git nicht startbar: {e}"))?;
    let mut err = child.stderr.take().ok_or("stderr fehlt")?;
    let mut buf = [0u8; 4096];
    let mut pending: Vec<u8> = Vec::new();
    // Nur Meldungen ohne Prozentangabe, fuer die Fehlermeldung.
    let mut msgs: Vec<String> = Vec::new();
    let mut done = false;
    // git schreibt Fortschritt mit \r in dieselbe Zeile, daher an \r und \n trennen.
    while !done {
        let n = err.read(&mut buf).unwrap_or(0);
        if n == 0 {
            done = true;
            pending.push(b'\n');
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(i) = pending.iter().position(|b| matches!(b, b'\r' | b'\n')) {
            let line = String::from_utf8_lossy(&pending[..i]).trim().to_string();
            pending.drain(..=i);
            if line.is_empty() {
                continue;
            }
            let percent = parse_percent(&line);
            if percent.is_none() {
                msgs.push(line.clone());
            }
            emit(CloneProgress { line, percent });
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        let tail = msgs.len().saturating_sub(10);
        let msg = msgs[tail..].join("\n");
        return Err(if msg.is_empty() { "Klonen fehlgeschlagen".into() } else { msg });
    }
    Ok(dest)
}

/// Nur http(s): sonst koennte explorer/open/xdg-open beliebige Programme oder Pfade oeffnen.
fn valid_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && url.len() > 8
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !valid_url(&url) {
        return Err(format!("Keine http(s)-URL: {url}"));
    }
    crate::open_system(&url)
        .map_err(|e| format!("Browser konnte nicht geöffnet werden: {e}"))
}

/// Browser-URL zum Remote (ssh/scp-artig -> https); None, wenn nicht parsebar.
#[tauri::command]
pub fn forge_web_url(remote_url: String) -> Option<String> {
    parse_remote(&remote_url).map(|(host, path)| format!("https://{host}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn web_url_aus_remote() {
        assert_eq!(forge_web_url("git@github.com:o/r.git".into()).as_deref(), Some("https://github.com/o/r"));
        assert_eq!(forge_web_url("C:/repos/x".into()), None);
    }

    fn p(u: &str) -> Option<(String, String)> {
        parse_remote(u)
    }

    fn hp(h: &str, path: &str) -> Option<(String, String)> {
        Some((h.into(), path.into()))
    }

    #[test]
    fn parst_remote_urls() {
        assert_eq!(p("https://github.com/octocat/Hello-World.git"), hp("github.com", "octocat/Hello-World"));
        assert_eq!(p("https://github.com/octocat/Hello-World/"), hp("github.com", "octocat/Hello-World"));
        assert_eq!(p("https://user@GitLab.firma.de:8443/a/b/c.git"), hp("gitlab.firma.de:8443", "a/b/c"));
        assert_eq!(p("git@github.com:octocat/Hello-World.git"), hp("github.com", "octocat/Hello-World"));
        assert_eq!(p("git@gitlab.com:group/sub/proj.git"), hp("gitlab.com", "group/sub/proj"));
        assert_eq!(p("ssh://git@gitlab.firma.de:2222/group/sub/proj.git"), hp("gitlab.firma.de", "group/sub/proj"));
        assert_eq!(p("ssh://git@github.com/o/r"), hp("github.com", "o/r"));
        assert_eq!(p("C:/Users/x/repo"), None);
        assert_eq!(p("https://github.com/nurowner"), None);
        assert_eq!(p("file:///c/repo"), None);
    }

    #[test]
    fn liest_fortschritt() {
        assert_eq!(parse_percent("Receiving objects:  45% (45/100), 1.2 MiB | 3 MiB/s"), Some(45));
        assert_eq!(parse_percent("Resolving deltas: 100% (3/3), done."), Some(100));
        assert_eq!(parse_percent("Cloning into 'x'..."), None);
        assert_eq!(parse_percent("remote: Compressing objects:   0% (1/200)"), Some(0));
    }

    #[test]
    fn prueft_host_und_url() {
        assert_eq!(norm_host(" https://GitLab.Firma.de/ ").unwrap(), "gitlab.firma.de");
        assert!(norm_host("evil.com/x").is_err());
        assert!(norm_host("a@b").is_err());
        assert_eq!(api_base("github", "github.com"), "https://api.github.com");
        assert_eq!(api_base("github", "ghe.firma.de"), "https://ghe.firma.de/api/v3");
        assert_eq!(api_base("gitlab", "gitlab.com"), "https://gitlab.com/api/v4");
        assert_eq!(encode("group/sub/proj"), "group%2Fsub%2Fproj");
        assert!(valid_url("https://github.com/o/r/pull/1"));
        assert!(!valid_url("file:///C:/Windows/system32/calc.exe"));
        assert!(!valid_url("https://x.de\" & calc"));
        assert!(!valid_url("C:\\Windows"));
    }

    #[test]
    fn parst_weitere_remote_formen() {
        assert_eq!(p("http://GitLab.local/g/p"), hp("gitlab.local", "g/p"));
        assert_eq!(p("github.com:o/r.git"), hp("github.com", "o/r"));
        assert_eq!(p("git://github.com/o/r.git"), hp("github.com", "o/r"));
        assert_eq!(p("  https://github.com/o/r.git/  "), hp("github.com", "o/r"));
        assert_eq!(p("https://user:pa@ss@github.com/o/r"), hp("github.com", "o/r"));
        assert_eq!(p("D:\\x\\repo"), None);
        assert_eq!(p(""), None);
        assert_eq!(encode("a b/ü"), "a%20b%2F%C3%BC");
    }

    #[test]
    fn open_url_grenzen() {
        assert!(valid_url("HTTPS://github.com/x"));
        assert!(!valid_url("https://"));
        assert!(!valid_url("javascript:alert(1)"));
        assert!(!valid_url("https://a.de\nb"));
        assert!(!valid_url("ms-settings:"));
    }

    // Nutzer tippen die Host-URL oft mit http:// oder in Grossbuchstaben ab.
    #[test]
    fn norm_host_nimmt_http_und_grossgeschriebenes_schema() {
        assert_eq!(norm_host("HTTPS://gitlab.com").unwrap(), "gitlab.com");
        assert_eq!(norm_host("http://gitlab.firma.de").unwrap(), "gitlab.firma.de");
        assert!(norm_host("ftp://x.de").is_err());
    }

    #[test]
    fn api_fehlermeldung_lesbar_und_ohne_dopplung() {
        use reqwest::StatusCode as S;
        let e = api_error("gitlab.com", S::UNAUTHORIZED, &serde_json::json!({"message": "401 Unauthorized"}));
        assert_eq!(e, "gitlab.com: Token ungültig oder abgelaufen (401)");
        let e = api_error("gitlab.com", S::FORBIDDEN, &serde_json::json!({"message": "403 Forbidden"}));
        assert_eq!(e, "gitlab.com antwortet mit 403 Forbidden (Token ohne Rechte?)");
        let e = api_error("github.com", S::NOT_FOUND, &serde_json::json!({"message": "Not Found"}));
        assert_eq!(e, "github.com antwortet mit 404 Not Found");
        let e = api_error("github.com", S::UNPROCESSABLE_ENTITY, &serde_json::json!({"message": "Validation Failed"}));
        assert!(e.ends_with("Validation Failed"), "{e}");
    }

    #[test]
    fn parst_zeitstempel() {
        assert_eq!(unix_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_secs("2024-02-29T12:00:00Z"), Some(1709208000));
        assert_eq!(unix_secs("2024-02-29T12:00:00.123Z"), Some(1709208000));
        assert_eq!(unix_secs("2024-02-29T14:00:00.5+02:00"), Some(1709208000));
        assert_eq!(unix_secs("2024-02-29T07:30:00-04:30"), Some(1709208000));
        assert_eq!(unix_secs("kaputt"), None);
        assert_eq!(unix_secs("2024-02-29T12:00:00X"), None);
        let (a, b) = (json!("2024-01-01T00:00:00Z"), json!("2024-01-01T00:01:30Z"));
        assert_eq!(span(&a, &b, 0), Some(90));
        assert_eq!(span(&a, &Value::Null, 1704067210), Some(10));
        assert_eq!(span(&Value::Null, &b, 0), None);
    }

    #[test]
    fn mappt_status() {
        assert_eq!(gh_status("queued", ""), "queued");
        assert_eq!(gh_status("waiting", ""), "skipped");
        assert_eq!(gh_status("pending", ""), "queued");
        assert_eq!(gh_status("in_progress", ""), "running");
        assert_eq!(gh_status("completed", "success"), "success");
        assert_eq!(gh_status("completed", "timed_out"), "failure");
        assert_eq!(gh_status("completed", "action_required"), "failure");
        assert_eq!(gh_status("completed", "cancelled"), "cancelled");
        assert_eq!(gh_status("completed", "neutral"), "skipped");
        assert_eq!(gl_status("manual"), "skipped");
        assert_eq!(gl_status("scheduled"), "skipped");
        assert_eq!(gl_status("pending"), "queued");
        assert_eq!(gl_status("waiting_for_resource"), "queued");
        assert_eq!(gl_status("running"), "running");
        assert_eq!(gl_status("failed"), "failure");
        assert_eq!(gl_status("canceled"), "cancelled");
        assert_eq!(gl_status("skipped"), "skipped");
    }

    #[test]
    fn mappt_releases() {
        let r = gh_release(&json!({
            "id": 42, "tag_name": "v1.0", "name": "Eins", "body": "<script>x</script>",
            "draft": true, "prerelease": false, "created_at": "2024-01-01T00:00:00Z", "published_at": null,
            "author": {"login": "octo"}, "html_url": "https://github.com/o/r/releases/tag/v1.0",
            "assets": [{"name": "a.zip", "size": 10, "download_count": 3, "browser_download_url": "https://x/a.zip"}]
        }));
        assert_eq!((r.id.as_str(), r.tag.as_str(), r.author.as_str()), ("42", "v1.0", "octo"));
        assert!(r.draft && r.published_at.is_none());
        assert_eq!(r.body, "<script>x</script>");
        assert_eq!(r.assets, vec![ReleaseAsset { name: "a.zip".into(), size: 10, downloads: 3, url: "https://x/a.zip".into() }]);

        let r = gl_release(&json!({
            "tag_name": "v2", "name": "Zwei", "description": "Notes", "created_at": "c", "released_at": "r",
            "author": {"username": "gl"}, "_links": {"self": "https://gitlab.com/g/p/-/releases/v2"},
            "assets": {"links": [{"name": "bin", "url": "https://x/bin"}]}
        }));
        assert_eq!((r.id.as_str(), r.tag.as_str(), r.body.as_str()), ("v2", "v2", "Notes"));
        assert_eq!(r.published_at.as_deref(), Some("r"));
        assert_eq!(r.web_url, "https://gitlab.com/g/p/-/releases/v2");
        assert_eq!(r.assets[0].url, "https://x/bin");
        assert!(!r.draft && !r.prerelease);
    }

    #[test]
    fn mappt_runs_und_jobs() {
        let run = json!({
            "id": 7, "name": "CI", "display_title": "", "head_commit": {"message": "Fix\n\nDetails"},
            "head_branch": "main", "head_sha": "abc", "event": "push", "status": "completed", "conclusion": "failure",
            "created_at": "2024-01-01T00:00:00Z", "run_started_at": "2024-01-01T00:00:10Z",
            "updated_at": "2024-01-01T00:01:10Z", "html_url": "https://github.com/o/r/actions/runs/7"
        });
        let r = gh_run(&run, 0);
        assert_eq!((r.id, r.title.as_str(), r.status, r.duration_s), (7, "Fix", "failure", Some(60)));
        let mut laufend = run.clone();
        laufend["status"] = json!("in_progress");
        laufend["display_title"] = json!("Titel");
        let r = gh_run(&laufend, unix_secs("2024-01-01T00:00:40Z").unwrap());
        assert_eq!((r.title.as_str(), r.status, r.duration_s), ("Titel", "running", Some(30)));

        let pl = json!({"id": 99, "iid": 5, "ref": "main", "sha": "def", "source": "push", "status": "success",
            "created_at": "2024-01-01T00:00:00Z", "updated_at": "2024-01-01T00:02:00Z", "web_url": "https://gl/p/99"});
        let r = gl_run(&pl);
        assert_eq!((r.name.as_str(), r.event.as_str(), r.status, r.duration_s), ("Pipeline #5", "push", "success", None));

        let j = gh_job(&json!({"id": 1, "name": "build", "status": "completed", "conclusion": "success",
            "started_at": "2024-01-01T00:00:00Z", "completed_at": "2024-01-01T00:00:05Z", "html_url": "u"}), 0);
        assert_eq!((j.stage, j.status, j.duration_s), (None, "success", Some(5)));
        let j = gh_job(&json!({"id": 2, "status": "queued", "started_at": "2024-01-01T00:00:00Z"}), 99);
        assert_eq!(j.duration_s, None);
        let j = gl_job(&json!({"id": 3, "name": "test", "stage": "test", "status": "failed",
            "started_at": "s", "duration": 12.6, "web_url": "u"}));
        assert_eq!((j.stage.as_deref(), j.status, j.duration_s), (Some("test"), "failure", Some(13)));
    }

    #[test]
    fn release_id_landet_sicher_im_pfad() {
        let repo = |kind| Repo { kind, host: "h".into(), base: String::new(), tok: String::new() };
        assert_eq!(release_path(&repo("github"), "123").unwrap(), "/releases/123");
        assert!(release_path(&repo("github"), "../../user").is_err());
        assert!(release_path(&repo("github"), "").is_err());
        assert_eq!(release_path(&repo("gitlab"), "v1.0/../x").unwrap(), "/releases/v1.0%2F..%2Fx");
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ocui-forge-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git").current_dir(dir).args(args).status().unwrap().success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn klont_lokal_mit_fortschritt_und_prueft_ziel() {
        let root = tmp("clone");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        git(&src, &["init", "-q", "-b", "main"]);
        std::fs::write(src.join("a.txt"), "x").unwrap();
        git(&src, &["add", "."]);
        git(&src, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        // Unix-Pfad beginnt schon mit /
        let url = format!("file:///{}", src.display().to_string().replace('\\', "/").trim_start_matches('/'));

        // Leerer, existierender Ordner ist erlaubt.
        let empty = root.join("leer");
        std::fs::create_dir_all(&empty).unwrap();
        let lines = std::sync::Mutex::new(Vec::new());
        let dest = empty.display().to_string();
        let r = clone_blocking(url.clone(), dest.clone(), |p| lines.lock().unwrap().push((p.line, p.percent)));
        assert_eq!(r, Ok(dest));
        assert!(empty.join("a.txt").is_file());
        assert!(!lines.lock().unwrap().is_empty());

        // Nicht leer -> Fehler, kein git-Aufruf.
        let r = clone_blocking(url.clone(), empty.display().to_string(), |_| {});
        assert!(r.unwrap_err().contains("nicht leer"));
        // Ziel ist eine Datei.
        let file = root.join("datei");
        std::fs::write(&file, "x").unwrap();
        assert!(clone_blocking(url.clone(), file.display().to_string(), |_| {}).is_err());
        // Option-Injection.
        assert!(clone_blocking("--upload-pack=calc".into(), root.join("n").display().to_string(), |_| {}).is_err());
        // Nicht existierende Quelle: git-Meldung kommt durch, Ziel bleibt nicht zurueck.
        let bad = root.join("bad");
        let e = clone_blocking(format!("{url}-fehlt"), bad.display().to_string(), |_| {}).unwrap_err();
        assert!(e.contains("fatal") || e.contains("does not"), "{e}");
        assert!(!bad.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn api_fehler_bei_netzfehler_ohne_token_im_text() {
        let e = tauri::async_runtime::block_on(api_get("gitlab", "127.0.0.1:1", "GEHEIM123", "/user", &[]))
            .unwrap_err();
        assert!(e.contains("nicht erreichbar"), "{e}");
        assert!(!e.contains("GEHEIM123"));
    }

    // Echter, lesender Aufruf gegen gitlab.com; offline wird nur uebersprungen.
    #[test]
    fn api_401_mit_falschem_token() {
        match tauri::async_runtime::block_on(api_get("gitlab", "gitlab.com", "glpat-falsch", "/user", &[])) {
            Err(e) if e.contains("nicht erreichbar") => {}
            Err(e) => {
                assert!(e.contains("401"), "{e}");
                assert!(!e.contains("glpat-falsch"));
            }
            Ok(v) => panic!("unerwartet ok: {v}"),
        }
    }

    // gitlab-org/gitlab-ce wurde umbenannt; die API antwortet mit 301 auf denselben Host.
    #[test]
    fn api_folgt_redirect_auf_gleichem_host() {
        let r = tauri::async_runtime::block_on(api_get(
            "gitlab",
            "gitlab.com",
            "",
            "/projects/gitlab-org%2Fgitlab-ce/merge_requests",
            &[("state", "opened".into()), ("per_page", "1".into())],
        ));
        // offline: uebersprungen wie api_401_mit_falschem_token
        assert!(r.is_ok() || r.as_ref().is_err_and(|e| e.contains("nicht erreichbar")), "{r:?}");
    }
}
