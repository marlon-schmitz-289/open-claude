//! GitHub/GitLab-Anbindung: Konten, Repo-Suche, PRs/MRs, Klonen.
//! Token liegen nur im Windows-Anmeldeinfo-Speicher und tauchen nie in argv, Logs oder Fehlern auf.

use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
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
    let req = client.get(format!("{}{path}", api_base(kind, host))).query(query);
    let req = if kind == "github" {
        req.bearer_auth(tok).header("Accept", "application/vnd.github+json")
    } else {
        req.header("PRIVATE-TOKEN", tok)
    };
    // reqwest-Fehler nennen nur die URL; der Token steckt im Header.
    let res = req.send().await.map_err(|e| format!("{host} nicht erreichbar: {e}"))?;
    let status = res.status();
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

#[tauri::command]
pub async fn forge_pulls(remote_url: String) -> Result<Vec<PullRequest>, String> {
    let Some((host, path)) = parse_remote(&remote_url) else {
        return Ok(Vec::new());
    };
    let Some((kind, tok)) = token_for_host(&host)? else {
        return Ok(Vec::new());
    };
    let per = ("per_page", "50".to_string());
    if kind == "github" {
        let qs = [("state", "open".to_string()), per];
        let body = api_get(kind, &host, &tok, &format!("/repos/{path}/pulls"), &qs).await?;
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
    let body = api_get(kind, &host, &tok, &format!("/projects/{}/merge_requests", encode(&path)), &qs).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
