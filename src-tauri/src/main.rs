// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Sonst zeigt WKWebView beim Halten einer Taste das Akzent-Popup statt sie zu wiederholen.
    // Nur fuer unsere App-Domain, vor dem Start von AppKit.
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("defaults")
        .args(["write", "com.marlonschmitz.openclaude", "ApplePressAndHoldEnabled", "-bool", "false"])
        .status();
    ocui_lib::run()
}
