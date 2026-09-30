//! Vorschau des eingebauten Editors: URI-Schema `preview` (liefert Projektdateien an iframe/img),
//! WPF-Vorschau ueber den C#-Helfer und die Rueckfrage vor dem Beenden bei ungespeicherten Aenderungen.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use tauri::http::{header, Method, Request, Response, StatusCode};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::files::within;
use crate::git::blocking;

/// Projekte, deren Editor gerade offen ist (kanonische Pfade). Nur daraus liefert das Schema Dateien,
/// sonst koennte jede Seite in der WebView ueber die URL beliebige Pfade der Platte laden.
/// Alle Projekte teilen sich einen Origin: die Vorschau-iframes laufen deshalb in einer Sandbox ohne
/// allow-same-origin (PreviewPane) und koennen die Dateien einbinden, aber nicht per fetch lesen.
#[derive(Default)]
pub struct PreviewRoots(Mutex<HashSet<PathBuf>>);

/// `on` false: Editor des Projekts geschlossen, das Schema liefert nichts mehr daraus.
#[tauri::command]
pub fn preview_allow(repo: String, on: bool, state: tauri::State<'_, PreviewRoots>) -> Result<(), String> {
    let root = Path::new(&repo).canonicalize().map_err(|e| format!("Projekt nicht gefunden: {e}"))?;
    let mut roots = state.0.lock().unwrap();
    if on {
        roots.insert(root);
    } else {
        roots.remove(&root);
    }
    Ok(())
}

/// `%41` -> `A`. None bei kaputter Kodierung, ungueltigem UTF-8 oder NUL.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            // Ziffern einzeln: from_str_radix naehme auch "+1".
            let hex = |c: &u8| (*c as char).to_digit(16);
            out.push((hex(b.get(i + 1)?)? * 16 + hex(b.get(i + 2)?)?) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().filter(|s| !s.contains('\0'))
}

/// URL-Pfad `/<repo kodiert>/<pfad>` -> Datei. Das Repo ist genau ein Segment und muss freigegeben sein;
/// der Rest laeuft durch denselben Waechter wie der Editor (kein `..`, kein `.git`, kein Symlink nach draussen).
fn resolve(roots: &HashSet<PathBuf>, url_path: &str) -> Result<PathBuf, StatusCode> {
    let (repo, rest) = url_path.trim_start_matches('/').split_once('/').unwrap_or((url_path.trim_start_matches('/'), ""));
    let (Some(repo), Some(rest)) = (percent_decode(repo), percent_decode(rest)) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let allowed = Path::new(&repo).canonicalize().is_ok_and(|root| roots.contains(&root));
    if repo.is_empty() || !allowed {
        return Err(StatusCode::FORBIDDEN);
    }
    let rest = rest.trim_end_matches('/');
    let rest = if rest.is_empty() { "index.html" } else { rest };
    let mut file = within(&repo, rest).map_err(|_| StatusCode::FORBIDDEN)?;
    if file.is_dir() {
        // Wieder durch den Waechter: index.html koennte selbst ein Symlink nach draussen sein.
        file = within(&repo, &format!("{rest}/index.html")).map_err(|_| StatusCode::FORBIDDEN)?;
    }
    if file.is_file() { Ok(file) } else { Err(StatusCode::NOT_FOUND) }
}

fn content_type(path: &Path) -> &'static str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "wasm" => "application/wasm",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    let mut res = Response::new(code.canonical_reason().unwrap_or_default().as_bytes().to_vec());
    *res.status_mut() = code;
    res.headers_mut().insert(header::CONTENT_TYPE, header::HeaderValue::from_static("text/plain; charset=utf-8"));
    res
}

/// Antwort des Schemas. Die Query (`?v=`) ist nur Cache-Brecher und steckt nicht in `uri().path()`.
// ponytail: keine Range-Anfragen, Videos laden komplett (WKWebView spielt sie evtl. nicht). Nachruesten, wenn Videos wichtig werden.
fn respond(roots: &HashSet<PathBuf>, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let file = match resolve(roots, req.uri().path()) {
        Ok(f) => f,
        Err(code) => return status(code),
    };
    let Ok(body) = std::fs::read(&file) else {
        return status(StatusCode::NOT_FOUND);
    };
    let mut res = Response::new(if req.method() == Method::HEAD { Vec::new() } else { body });
    let head = res.headers_mut();
    head.insert(header::CONTENT_TYPE, header::HeaderValue::from_static(content_type(&file)));
    // Die Datei aendert sich beim Speichern: nie aus dem Cache.
    head.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    res
}

/// Handler fuer `register_asynchronous_uri_scheme_protocol("preview", ...)`; liest auf einem Worker-Thread.
pub fn serve(app: &tauri::AppHandle, req: Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let roots = app.state::<PreviewRoots>().0.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || responder.respond(respond(&roots, &req)));
}

// ---------- WPF ----------

const XAML_TIMEOUT: Duration = Duration::from_secs(15);
/// Exit-Code des .NET-Apphosts, wenn die passende Runtime fehlt (FrameworkMissingFailure).
const FRAMEWORK_MISSING: u32 = 0x8000_8096;

/// Naechste App.xaml vom Ordner der Datei aufwaerts bis zum Projektordner: ihre Ressourcen (Styles, Farben) gelten mit.
fn app_xaml(repo: &str, path: &str) -> Option<PathBuf> {
    let file = within(repo, path).ok()?;
    let root = Path::new(repo);
    file.ancestors().skip(1).take_while(|d| d.starts_with(root)).map(|d| d.join("App.xaml")).find(|f| f.is_file())
}

/// Startet den Prozess, gibt `input` auf stdin und wartet hoechstens `timeout`; danach wird er beendet.
fn run(cmd: &mut Command, input: Vec<u8>, timeout: Duration) -> Result<(ExitStatus, Vec<u8>, String), String> {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    // Alle drei Pipes in eigenen Threads: sonst blockiert ein voller Puffer beide Seiten.
    let mut stdin = child.stdin.take();
    std::thread::spawn(move || stdin.as_mut().map(|s| s.write_all(&input)));
    let (mut so, mut se) = (child.stdout.take(), child.stderr.take());
    let out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = so.as_mut().map(|s| s.read_to_end(&mut buf));
        buf
    });
    let err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = se.as_mut().map(|s| s.read_to_end(&mut buf));
        String::from_utf8_lossy(&buf).trim().to_string()
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Zeitüberschreitung".into());
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    Ok((status, out.join().unwrap_or_default(), err.join().unwrap_or_default()))
}

/// Ergebnis des Helfers -> Base64-PNG oder Fehlertext; fehlende .NET-Runtime als eigener Code.
fn xaml_result(code: Option<i32>, png: &[u8], stderr: &str) -> Result<String, String> {
    if code == Some(0) && !png.is_empty() {
        return Ok(base64::engine::general_purpose::STANDARD.encode(png));
    }
    // Der Apphost meldet "You must install or update .NET to run this application."
    if code.map(|c| c as u32) == Some(FRAMEWORK_MISSING) || stderr.contains("You must install") {
        return Err("DOTNET_FEHLT".into());
    }
    Err(if stderr.is_empty() { format!("WPF-Vorschau fehlgeschlagen (Code {code:?}).") } else { stderr.to_string() })
}

fn render(helper: &Path, repo: &str, path: &str, content: String) -> Result<String, String> {
    if !helper.is_file() {
        return Err(format!("WPF-Vorschau ist nicht installiert ({} fehlt).", helper.display()));
    }
    let mut cmd = crate::quiet(&helper.to_string_lossy());
    // Argument 1: Projektordner (nur daraus bindet der Helfer weitere XAML-Dateien ein), Argument 2: App.xaml.
    cmd.arg(repo).args(app_xaml(repo, path));
    // Relative Verweise im XAML (Bilder) gelten ab dem Ordner der Datei.
    if let Some(dir) = within(repo, path).ok().and_then(|f| f.parent().map(Path::to_path_buf)).filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    let (status, png, stderr) = run(&mut cmd, content.into_bytes(), XAML_TIMEOUT)
        .map_err(|e| format!("WPF-Vorschau konnte nicht gestartet werden: {e}"))?;
    xaml_result(status.code(), &png, &stderr)
}

/// Rendert XAML (ungespeicherter Stand) ueber den mitgelieferten Helfer zu einem PNG (Base64).
#[tauri::command]
pub async fn xaml_render(app: tauri::AppHandle, repo: String, path: String, content: String) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("NUR_WINDOWS".into());
    }
    // Gebuendelt wie im Dev-Modus: tauri-build kopiert die Ressourcen neben die exe.
    let dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    blocking(move || render(&dir.join("xaml-preview").join("xaml-preview.exe"), &repo, &path, content)).await
}

// ---------- Beenden ----------

/// Der Editor hat ungespeicherte Aenderungen; das Frontend haelt es aktuell.
static DIRTY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn editor_dirty(dirty: bool) {
    DIRTY.store(dirty, Ordering::Relaxed);
}

/// Beendet die App; bei ungespeicherten Aenderungen erst nach Rueckfrage.
/// Nicht blockierend: beide Aufrufer laufen auf dem Haupt-Thread, dort wuerde ein blockierender Dialog haengen.
pub fn quit(app: &tauri::AppHandle) {
    if !DIRTY.load(Ordering::Relaxed) {
        return app.exit(0);
    }
    // Aus dem Tray heraus ist das Fenster versteckt: zeigen, worum es geht.
    crate::reopen(app);
    let mut dialog = app
        .dialog()
        .message("Ungespeicherte Änderungen im Editor verwerfen?")
        .title("Open Claude")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Verwerfen".into(), "Abbrechen".into()));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    let app = app.clone();
    dialog.show(move |ok| {
        if ok {
            app.exit(0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocui-preview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub dir/.git")).unwrap();
        std::fs::write(dir.join("index.html"), "<p>").unwrap();
        std::fs::write(dir.join("sub dir/index.html"), "<b>").unwrap();
        std::fs::write(dir.join("sub dir/ä.css"), "a{}").unwrap();
        std::fs::write(dir.join("sub dir/.git/config"), "x").unwrap();
        dir
    }

    /// Kodiert wie encodeURIComponent: alles ausser unreservierten Zeichen.
    fn enc(s: &str) -> String {
        s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    }

    #[test]
    fn dekodiert_prozent() {
        assert_eq!(percent_decode("a%20b%2Fc").as_deref(), Some("a b/c"));
        assert_eq!(percent_decode("C%3A%5CDev%5C%C3%A4").as_deref(), Some("C:\\Dev\\ä"));
        assert_eq!(percent_decode("").as_deref(), Some(""));
        for bad in ["%", "%2", "%zz", "%ff", "a%00b", "%+1"] {
            assert_eq!(percent_decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn nur_freigegebene_projekte() {
        let (repo, fremd) = (tmp("erlaubt"), tmp("fremd"));
        let roots = HashSet::from([repo.canonicalize().unwrap()]);
        let url = |root: &Path, rest: &str| format!("/{}/{rest}", enc(&root.to_string_lossy()));
        let at = |rest: &str| resolve(&roots, &url(&repo, rest));

        assert_eq!(at("index.html").unwrap(), repo.join("index.html"));
        // Leerer Rest und Ordner liefern index.html; Segmente sind einzeln kodiert.
        assert_eq!(at("").unwrap(), repo.join("index.html"));
        assert_eq!(resolve(&roots, &format!("/{}", enc(&repo.to_string_lossy()))).unwrap(), repo.join("index.html"));
        assert_eq!(at("sub%20dir").unwrap(), repo.join("sub dir/index.html"));
        assert_eq!(at("sub%20dir/").unwrap(), repo.join("sub dir/index.html"));
        assert_eq!(at("sub%20dir/%C3%A4.css").unwrap(), repo.join("sub dir/ä.css"));
        assert_eq!(at("fehlt.css"), Err(StatusCode::NOT_FOUND));

        // Nicht freigegeben, auch nicht ueber ein freigegebenes Projekt hinweg.
        assert_eq!(resolve(&roots, &url(&fremd, "index.html")), Err(StatusCode::FORBIDDEN));
        assert_eq!(resolve(&roots, "/%2Fetc/passwd"), Err(StatusCode::FORBIDDEN));
        assert_eq!(resolve(&roots, "/"), Err(StatusCode::FORBIDDEN));
        assert_eq!(resolve(&HashSet::new(), &url(&repo, "index.html")), Err(StatusCode::FORBIDDEN));
        let raus = format!("..%2F{}%2Findex.html", fremd.file_name().unwrap().to_string_lossy());
        for bad in ["../x", "%2E%2E/x", "sub%20dir/../../x", "sub%20dir/.git/config", "%2Fetc%2Fpasswd", raus.as_str()] {
            assert_eq!(at(bad), Err(StatusCode::FORBIDDEN), "{bad}");
        }
        assert_eq!(at("%zz"), Err(StatusCode::BAD_REQUEST));
    }

    #[cfg(unix)]
    #[test]
    fn kein_symlink_nach_draussen() {
        let (repo, fremd) = (tmp("link"), tmp("link-ziel"));
        std::os::unix::fs::symlink(&fremd, repo.join("raus")).unwrap();
        std::fs::create_dir_all(repo.join("ordner")).unwrap();
        std::os::unix::fs::symlink(fremd.join("index.html"), repo.join("ordner/index.html")).unwrap();
        let roots = HashSet::from([repo.canonicalize().unwrap()]);
        for bad in ["raus/index.html", "raus", "ordner", "ordner/index.html"] {
            let url = format!("/{}/{bad}", enc(&repo.to_string_lossy()));
            assert_eq!(resolve(&roots, &url), Err(StatusCode::FORBIDDEN), "{bad}");
        }
    }

    #[test]
    fn antwort_mit_typ_ohne_cache() {
        let repo = tmp("antwort");
        let roots = HashSet::from([repo.canonicalize().unwrap()]);
        let req = |method: &str, rest: &str| {
            let uri = format!("preview://localhost/{}/{rest}", enc(&repo.to_string_lossy()));
            respond(&roots, &Request::builder().method(method).uri(uri).body(Vec::new()).unwrap())
        };
        // Die Query ist nur Cache-Brecher.
        let res = req("GET", "sub%20dir/%C3%A4.css?v=17");
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "text/css; charset=utf-8");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(res.body(), b"a{}");
        let head = req("HEAD", "index.html");
        assert_eq!((head.status(), head.body().len()), (StatusCode::OK, 0));
        assert_eq!(head.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(req("POST", "index.html").status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(req("GET", "fehlt.png").status(), StatusCode::NOT_FOUND);
        assert_eq!(req("GET", "sub%20dir/.git/config").status(), StatusCode::FORBIDDEN);
        assert_eq!(content_type(Path::new("a/B.PNG")), "image/png");
        assert_eq!(content_type(Path::new("a/ohne")), "application/octet-stream");
    }

    #[test]
    fn findet_app_xaml_aufwaerts() {
        let repo = tmp("appxaml");
        std::fs::create_dir_all(repo.join("App/Views/Tief")).unwrap();
        let r = repo.to_string_lossy().into_owned();
        assert_eq!(app_xaml(&r, "App/Views/Tief/Main.xaml"), None);
        std::fs::write(repo.join("App/App.xaml"), "<Application/>").unwrap();
        assert_eq!(app_xaml(&r, "App/Views/Tief/Main.xaml"), Some(repo.join("App/App.xaml")));
        assert_eq!(app_xaml(&r, "App/Main.xaml"), Some(repo.join("App/App.xaml")));
        // Nicht ueber den Projektordner hinaus und nicht aus einem Nachbarordner.
        assert_eq!(app_xaml(&r, "Main.xaml"), None);
        assert_eq!(app_xaml(&r, "sub dir/Main.xaml"), None);
        assert_eq!(app_xaml(&r, "../x.xaml"), None);
    }

    #[test]
    fn deutet_helfer_ergebnis() {
        assert_eq!(xaml_result(Some(0), b"PNG", "").unwrap(), "UE5H");
        assert_eq!(xaml_result(Some(1), b"", "Fehler in <Button> (Zeile 3): x").unwrap_err(), "Fehler in <Button> (Zeile 3): x");
        assert_eq!(xaml_result(Some(FRAMEWORK_MISSING as i32), b"", "").unwrap_err(), "DOTNET_FEHLT");
        let host = "You must install or update .NET to run this application.\n\nApp: x.exe";
        assert_eq!(xaml_result(Some(-2147450749), b"", host).unwrap_err(), "DOTNET_FEHLT");
        assert!(xaml_result(Some(0), b"", "").is_err());
        let fehlt = render(Path::new("/gibt/es/nicht.exe"), "/tmp", "a.xaml", String::new()).unwrap_err();
        assert!(fehlt.contains("nicht installiert"), "{fehlt}");
    }

    #[cfg(unix)]
    #[test]
    fn prozess_mit_stdin_und_zeitlimit() {
        // Mehr als ein Pipe-Puffer, damit ein Deadlock auffiele.
        let big = vec![b'x'; 300_000];
        let (status, out, err) = run(&mut Command::new("cat"), big.clone(), XAML_TIMEOUT).unwrap();
        assert!(status.success() && out == big && err.is_empty());
        let (status, _, err) = run(Command::new("sh").args(["-c", "echo kaputt >&2; exit 3"]), big, XAML_TIMEOUT).unwrap();
        assert_eq!((status.code(), err.as_str()), (Some(3), "kaputt"));
        let slow = run(Command::new("sleep").arg("5"), Vec::new(), Duration::from_millis(100));
        assert_eq!(slow.unwrap_err(), "Zeitüberschreitung");
    }
}
