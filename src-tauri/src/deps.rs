//! Voraussetzungen, beim Start geprueft: claude ueberall, Git unter Windows. Fehlt etwas, bietet die
//! Titelleiste die Installation an (offizieller Claude-Installer, Git per winget).

use std::path::PathBuf;
use std::process::Stdio;

pub(crate) fn on_path(name: &str) -> bool {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd"] } else { &[""] };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|d| exts.iter().any(|e| d.join(format!("{name}{e}")).is_file()))
}

/// Ziel des nativen Installers, auch bevor PATH es kennt.
fn local_claude() -> PathBuf {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".local").join("bin").join(format!("claude{}", std::env::consts::EXE_SUFFIX))
}

#[tauri::command]
pub fn deps_missing() -> Vec<&'static str> {
    // Git zuerst: Claude Code braucht unter Windows die Git-Bash.
    let mut out = Vec::new();
    if cfg!(windows) && !on_path("git") {
        out.push("git");
    }
    if !on_path("claude") && !local_claude().is_file() {
        out.push("claude");
    }
    out
}

#[tauri::command]
pub async fn deps_install(name: String) -> Result<String, String> {
    crate::git::blocking(move || install(&name)).await
}

fn install(name: &str) -> Result<String, String> {
    let (prog, args, manual): (&str, &[&str], &str) = match (name, cfg!(windows)) {
        ("claude", true) => (
            "powershell",
            &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://claude.ai/install.ps1 | iex"],
            "irm https://claude.ai/install.ps1 | iex",
        ),
        ("claude", false) => (
            "bash",
            &["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
            "curl -fsSL https://claude.ai/install.sh | bash",
        ),
        ("git", true) => (
            "winget",
            &["install", "--id", "Git.Git", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"],
            "https://git-scm.com",
        ),
        _ => return Err(format!("Unbekannte Abhängigkeit: {name}")),
    };
    let fail = |why: String| format!("{name} nicht installiert ({why}). Von Hand: {manual}");
    let out = crate::quiet(prog)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| fail(e.to_string()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let tail = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("").to_string();
    if !out.status.success() {
        return Err(fail(tail));
    }
    #[cfg(windows)]
    refresh_path();
    Ok(tail)
}

/// Installer tragen sich nur in den Registry-PATH ein. Ohne Auffrischen faenden Terminal und Git-Ansicht
/// das Neue erst nach einem Neustart der App (auch relaunch erbt die alte Umgebung).
#[cfg(windows)]
fn refresh_path() {
    let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
        [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')";
    let Ok(out) = crate::quiet("powershell").args(["-NoProfile", "-Command", script]).stdin(Stdio::null()).output() else {
        return;
    };
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // Unter Windows ist set_var threadsicher (die Win32-Umgebung hat ein eigenes Lock).
    if out.status.success() && path.len() > 1 {
        std::env::set_var("PATH", path);
    }
}
