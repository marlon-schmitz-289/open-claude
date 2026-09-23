//! ocui-sh: schlanke Shell fuers App-Terminal. Startet erst `claude`, danach eine REPL.
//! Einfache Zeilen laufen direkt (Konsole wird vererbt), alles mit Pipes, Umleitungen,
//! Variablen oder Globs geht an eine echte Shell (Git-Bash, sonst cmd; Unix: sh).

use std::borrow::Cow;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use reedline::{
    default_emacs_keybindings, Color, ColumnarMenu, Completer, DefaultHinter, Emacs,
    FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode,
    PromptHistorySearch, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion,
};

const BUILTINS: [&str; 6] = ["cd", "exit", "clear", "pwd", "history", "export"];

fn main() {
    let no_claude = std::env::args().any(|a| a == "--no-claude");
    // Ctrl+C gehoert dem Kind: Handler (nicht vererbt) haelt nur die Shell am Leben.
    let _ = ctrlc::set_handler(|| {});
    // Geerbtes "Ctrl+C ignorieren" (z. B. vom GUI-Parent) loeschen, wie cmd.exe es tut;
    // das Flag vererbt sich sonst an alle Kinder und Ctrl+C erreicht sie nie.
    #[cfg(windows)]
    {
        extern "system" {
            fn SetConsoleCtrlHandler(h: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        }
        unsafe { SetConsoleCtrlHandler(None, 0) };
    }
    let mut sh = Shell::new();
    if !no_claude {
        sh.run("claude");
    }
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        repl(&mut sh);
    } else {
        // Pipe/Datei als Eingabe: einfach zeilenweise, ohne Editor und Prompt.
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if !sh.run(&line) {
                break;
            }
        }
    }
    std::process::exit(sh.status);
}

fn repl(sh: &mut Shell) {
    let history = FileBackedHistory::with_file(10_000, home().join(".ocui_history"))
        .map(Box::new)
        .ok();
    let mut kb = default_emacs_keybindings();
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".into()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let mut ed = Reedline::create()
        .with_edit_mode(Box::new(Emacs::new(kb)))
        .with_completer(Box::new(Comp { cmds: path_commands() }))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name("completion_menu"),
        )))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .with_hinter(Box::new(DefaultHinter::default()));
    if let Some(h) = history {
        ed = ed.with_history(h);
    }

    loop {
        println!();
        let prompt = ShPrompt { status: sh.status };
        match ed.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let cont = sh.run(&line);
                // reedline schreibt die History nur beim sauberen Drop; die App killt den
                // Prozess beim Schliessen des Terminals (TerminateProcess) statt ihn zu beenden,
                // Drop laeuft dann nie -> ohne Sync waere die ganze Session-History weg.
                let _ = ed.sync_history();
                if !cont {
                    break;
                }
            }
            Ok(Signal::CtrlC) => continue,
            Ok(Signal::CtrlD) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("ocui-sh: {e}");
                break;
            }
        }
    }
}

struct Shell {
    status: i32,
    oldpwd: Option<PathBuf>,
    #[cfg_attr(not(windows), allow(dead_code))]
    bash: Option<PathBuf>,
}

impl Shell {
    fn new() -> Self {
        let git = if cfg!(windows) { git_root() } else { None };
        if cfg!(windows) {
            // claude liegt oft in ~/.local/bin, Unix-Tools (ls, grep, ...) in <Git>\usr\bin.
            // Git-Tools hinten anhaengen, damit Windows-find/sort und MSVC-link vorn bleiben.
            let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
            let local = home().join(".local").join("bin");
            if local.is_dir() && !dirs.contains(&local) {
                dirs.insert(0, local);
            }
            if let Some(g) = &git {
                let usr = g.join("usr").join("bin");
                if !dirs.contains(&usr) {
                    dirs.push(usr);
                }
            }
            if let Ok(p) = std::env::join_paths(dirs) {
                std::env::set_var("PATH", p);
            }
            if std::env::var_os("LANG").is_none() {
                std::env::set_var("LANG", "C.UTF-8");
            }
        }
        Shell { status: 0, oldpwd: None, bash: git.map(|g| g.join("bin").join("bash.exe")) }
    }

    /// Fuehrt eine Zeile aus; false = Shell beenden.
    fn run(&mut self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return true;
        }
        // Reine Zuweisung(en) wie "FOO=bar" sonst: is_complex faengt sie (wegen '=') ab,
        // geht an eine neue Subshell und ist danach wirkungslos, "echo $FOO" bleibt leer.
        if let Some(assigns) = as_assignments(line) {
            for (k, v) in assigns {
                std::env::set_var(k, v);
            }
            self.status = 0;
            return true;
        }
        if is_complex(line) {
            self.status = self.delegate(line);
            return true;
        }
        let mut words = split(line, &home().to_string_lossy());
        if let Some(a) = alias(&words[0]) {
            words.splice(0..1, a.split(' ').map(String::from));
        }
        self.status = match words[0].as_str() {
            "exit" => {
                self.status = words.get(1).and_then(|c| c.parse().ok()).unwrap_or(self.status);
                return false;
            }
            "cd" => self.cd(words.get(1).map(String::as_str)),
            "pwd" => {
                println!("{}", cwd().display());
                0
            }
            "clear" => {
                print!("\x1b[2J\x1b[3J\x1b[H");
                let _ = std::io::stdout().flush();
                0
            }
            "history" => {
                let text = std::fs::read_to_string(home().join(".ocui_history")).unwrap_or_default();
                for (i, l) in text.lines().enumerate() {
                    println!("{:5}  {l}", i + 1);
                }
                0
            }
            "export" => {
                for (k, v) in words[1..].iter().filter_map(|w| w.split_once('=')) {
                    std::env::set_var(k, v);
                }
                0
            }
            _ => self.spawn(&words, line),
        };
        true
    }

    fn cd(&mut self, arg: Option<&str>) -> i32 {
        let target = match arg {
            None => home(),
            Some("-") => match &self.oldpwd {
                Some(p) => p.clone(),
                None => {
                    eprintln!("cd: OLDPWD nicht gesetzt");
                    return 1;
                }
            },
            Some(a) => PathBuf::from(a),
        };
        let prev = cwd();
        match std::env::set_current_dir(&target) {
            Ok(()) => {
                if arg == Some("-") {
                    println!("{}", cwd().display());
                }
                self.oldpwd = Some(prev);
                0
            }
            Err(e) => {
                eprintln!("cd: {}: {e}", target.display());
                1
            }
        }
    }

    fn spawn(&self, words: &[String], line: &str) -> i32 {
        let mut cmd = if cfg!(windows) {
            let path = std::env::var("PATH").unwrap_or_default();
            let ext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
            match resolve(&words[0], &path, &ext, |p| p.is_file()) {
                // .bat/.cmd startet std selbst sicher ueber cmd.exe.
                Some(p) if has_ext(&p, &[".exe", ".com", ".bat", ".cmd"]) => Command::new(p),
                Some(p) => {
                    let mut c = Command::new("cmd");
                    c.arg("/d").arg("/c").arg(p);
                    c
                }
                // Nicht gefunden: evtl. cmd-Builtin oder Bash-Funktion, die echte Shell meldet sonst selbst.
                None => return self.delegate(line),
            }
        } else {
            Command::new(&words[0])
        };
        match cmd.args(&words[1..]).status() {
            Ok(s) => exit_code(s),
            Err(e) => {
                eprintln!("ocui-sh: {}: {e}", words[0]);
                if e.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
            }
        }
    }

    fn delegate(&self, line: &str) -> i32 {
        let mut cmd = self.real_shell(line);
        match cmd.status() {
            Ok(s) => exit_code(s),
            Err(e) => {
                eprintln!("ocui-sh: Shell nicht startbar: {e}");
                126
            }
        }
    }

    // ponytail: Windows-Pfade mit Backslash werden in bash zu Escapes; bei Bedarf / nutzen.
    #[cfg(windows)]
    fn real_shell(&self, line: &str) -> Command {
        use std::os::windows::process::CommandExt;
        match &self.bash {
            Some(b) => {
                let mut c = Command::new(b);
                c.arg("-c").arg(line);
                c
            }
            None => {
                let mut c = Command::new("cmd");
                c.raw_arg(format!("/d /s /c \"{line}\""));
                c
            }
        }
    }

    #[cfg(not(windows))]
    fn real_shell(&self, line: &str) -> Command {
        let mut c = Command::new("sh");
        c.arg("-c").arg(line);
        c
    }
}

/// Ctrl+C-Abbruch wie in bash als 130 statt STATUS_CONTROL_C_EXIT bzw. Signal.
fn exit_code(s: std::process::ExitStatus) -> i32 {
    match s.code() {
        Some(c) if c as u32 == 0xC000_013A => 130,
        Some(c) => c,
        None => 130,
    }
}

fn alias(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "ls" => "ls --color=auto -F",
        "ll" => "ls --color=auto -F -lh",
        "la" => "ls --color=auto -F -lhA",
        "grep" => "grep --color=auto",
        _ => return None,
    })
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Zeile besteht nur aus NAME=wert (einer oder mehrere, simpel): dann direkt setzen statt delegieren.
fn as_assignments(line: &str) -> Option<Vec<(&str, &str)>> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let is_name = |n: &str| !n.is_empty() && n.chars().enumerate().all(|(i, c)| c == '_' || (i == 0 && c.is_ascii_alphabetic()) || (i > 0 && c.is_ascii_alphanumeric()));
    let mut out = Vec::with_capacity(words.len());
    for w in &words {
        let (k, v) = w.split_once('=')?;
        if !is_name(k) || v.contains(['"', '\'', '$', '`']) {
            return None;
        }
        out.push((k, v));
    }
    (!out.is_empty()).then_some(out)
}

/// Alles, was mehr als "Programm + Argumente" ist, bekommt die echte Shell.
fn is_complex(line: &str) -> bool {
    let first = line.split_whitespace().next().unwrap_or("");
    if first.contains('=') {
        return true; // FOO=1 cmd
    }
    let mut q: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (q, c) {
            // \" \$ \` \\ in "..." sind Escapes (bash-Regel); ohne den Check wuerde z.B.
            // "say \"hi\"" hier faelschlich als geschlossen gelten und der Rest der Zeile
            // (hi\") als eigene Woerter direkt ausgefuehrt statt an die echte Shell zu gehen.
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '$' | '`' | '\\')) => return true,
            (Some(e), c) if c == e => q = None,
            (Some('"'), '$' | '`') => return true,
            (Some(_), _) => {}
            (None, '\'' | '"') => q = Some(c),
            (None, '|' | '&' | ';' | '<' | '>' | '*' | '?' | '$' | '`' | '(' | ')' | '[' | ']' | '{' | '}') => return true,
            // Unter Windows ist \ Pfadtrenner, unter Unix ein Escape.
            (None, '\\') if !cfg!(windows) => return true,
            _ => {}
        }
    }
    q.is_some() // offenes Quote: die echte Shell soll meckern
}

/// Woerter mit '...'/"..."-Quotes; fuehrendes ungequotetes ~ wird zu home.
fn split(line: &str, home: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut q: Option<char> = None;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match q {
            Some(e) if c == e => q = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                q = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            None if c == '~' && !in_word && it.peek().is_none_or(|n| matches!(n, '/' | '\\') || n.is_whitespace()) => {
                cur.push_str(home);
                in_word = true;
            }
            None => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    let name = p.to_string_lossy().to_lowercase();
    exts.iter().any(|e| name.ends_with(&e.to_lowercase()))
}

/// Windows-Programmsuche wie cmd: PATH x PATHEXT; mit Pfadtrenner nur dieser Pfad.
fn resolve(name: &str, path: &str, pathext: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    let bases: Vec<PathBuf> = if name.contains(['/', '\\']) {
        vec![PathBuf::from(name)]
    } else {
        path.split(';').filter(|d| !d.trim().is_empty()).map(|d| Path::new(d).join(name)).collect()
    };
    bases.into_iter().find_map(|b| {
        if has_ext(&b, &exts) && is_file(&b) {
            return Some(b);
        }
        exts.iter()
            .map(|e| PathBuf::from(format!("{}{e}", b.display())))
            .find(|p| is_file(p))
    })
}

/// Git-Installation mit bin\bash.exe (Standardorte, sonst ueber git.exe im PATH).
fn git_root() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(pf).join("Git"));
    }
    if let Some(la) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(la).join("Programs").join("Git"));
    }
    // <Git>\cmd\git.exe oder <Git>\mingw64\bin\git.exe
    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(git) = resolve("git", &path, ".exe", |p| p.is_file()) {
        roots.extend(git.ancestors().skip(2).take(2).map(Path::to_path_buf));
    }
    roots.into_iter().find(|r| r.join("bin").join("bash.exe").is_file())
}

/// Branch aus .git/HEAD der naechsten Repo-Wurzel (auch Worktrees mit .git-Datei).
fn branch(start: &Path) -> Option<String> {
    for dir in start.ancestors() {
        let dot = dir.join(".git");
        let gitdir = if dot.is_dir() {
            dot
        } else if dot.is_file() {
            let s = std::fs::read_to_string(&dot).ok()?;
            dir.join(s.trim().strip_prefix("gitdir:")?.trim())
        } else {
            continue;
        };
        return head_branch(&std::fs::read_to_string(gitdir.join("HEAD")).ok()?);
    }
    None
}

fn head_branch(head: &str) -> Option<String> {
    let h = head.trim();
    Some(match h.strip_prefix("ref: ") {
        Some(r) => r.strip_prefix("refs/heads/").unwrap_or(r).to_string(),
        None => h.get(..7)?.to_string(), // detached: kurzer Hash
    })
}

struct ShPrompt {
    status: i32,
}

impl Prompt for ShPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let dir = cwd();
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
        let mut p = format!("\x1b[1;35m{name}\x1b[0m");
        if let Some(b) = branch(&dir) {
            p += &format!(" \x1b[90mon \x1b[36m⎇ {b}\x1b[0m");
        }
        if self.status != 0 {
            p += &format!(" \x1b[31m✘ {}\x1b[0m", self.status);
        }
        (p + "\n").into()
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        "".into()
    }
    fn render_prompt_indicator(&self, _: PromptEditMode) -> Cow<'_, str> {
        if self.status == 0 { "\x1b[1;35m❯\x1b[0m " } else { "\x1b[1;31m❯\x1b[0m " }.into()
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        "\x1b[90m…\x1b[0m ".into()
    }
    fn render_prompt_history_search_indicator(&self, h: PromptHistorySearch) -> Cow<'_, str> {
        format!("\x1b[90m(suche: {})\x1b[0m ", h.term).into()
    }
    // Farben stecken schon in den Strings; reedline soll nichts drueberlegen.
    fn get_prompt_color(&self) -> Color {
        Color::Reset
    }
    fn get_indicator_color(&self) -> Color {
        Color::Reset
    }
}

/// Programmnamen aus PATH (ohne Endung), einmal beim Start gesammelt.
// ponytail: kein Refresh nach Installationen waehrend der Session; Neustart reicht.
fn path_commands() -> Vec<String> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()).to_lowercase();
    let exts: Vec<&str> = exts.split(';').filter(|e| !e.is_empty()).collect();
    let mut out: Vec<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !cfg!(windows) {
                out.push(name);
            } else if let Some(stem) = exts.iter().find_map(|x| name.to_lowercase().strip_suffix(x).map(|s| s.len())) {
                out.push(name[..stem].to_string());
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

struct Comp {
    cmds: Vec<String>,
}

impl Completer for Comp {
    // ponytail: Pfade mit Leerzeichen werden nicht gequotet.
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let head = &line[..pos];
        let start = head.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
        let word = &head[start..];
        let span = Span::new(start, pos);
        let starts = |s: &str| s.to_lowercase().starts_with(&word.to_lowercase());
        let mut out = Vec::new();

        if head[..start].trim().is_empty() && !word.contains(['/', '\\']) {
            out.extend(self.cmds.iter().filter(|c| starts(c)).map(|c| Suggestion {
                value: c.clone(),
                span,
                append_whitespace: true,
                ..Default::default()
            }));
        }

        let (dir_part, prefix) = match word.rfind(['/', '\\']) {
            Some(i) => word.split_at(i + 1),
            None => ("", word),
        };
        let dir = match dir_part {
            "" => PathBuf::from("."),
            d if d.starts_with('~') => home().join(d[1..].trim_start_matches(['/', '\\'])),
            d => PathBuf::from(d),
        };
        let pre = prefix.to_lowercase();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.to_lowercase().starts_with(&pre) || (name.starts_with('.') && !prefix.starts_with('.')) {
                continue;
            }
            let is_dir = e.path().is_dir();
            out.push(Suggestion {
                value: format!("{dir_part}{name}{}", if is_dir { "/" } else { "" }),
                span,
                append_whitespace: !is_dir,
                ..Default::default()
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        for s in ["git status", "ls -la src", "echo 'a | b'", "code \"C:\\Program Files\\x\""] {
            assert!(!is_complex(s), "{s}");
        }
        for s in [
            "ls | wc", "a && b", "a; b", "echo $HOME", "echo \"$HOME\"", "ls *.rs", "x > f",
            "FOO=1 cmd", "echo `pwd`", "echo 'open", r#"echo "say \"hi\"""#, r#"echo "a\$b""#,
        ] {
            assert!(is_complex(s), "{s}");
        }
    }

    #[test]
    fn assignments() {
        assert_eq!(as_assignments("FOO=bar"), Some(vec![("FOO", "bar")]));
        assert_eq!(as_assignments("FOO=bar BAZ=1"), Some(vec![("FOO", "bar"), ("BAZ", "1")]));
        assert_eq!(as_assignments("FOO="), Some(vec![("FOO", "")]));
        assert_eq!(as_assignments("git status"), None);
        assert_eq!(as_assignments("FOO=1 cmd"), None); // echtes Prefix-Assignment: an die Shell
        assert_eq!(as_assignments("1FOO=bar"), None);
        assert_eq!(as_assignments("FOO=$HOME"), None);
    }

    #[test]
    fn quotes() {
        assert_eq!(split(r#"git commit -m "hallo welt" 'a b' """#, "/h"), ["git", "commit", "-m", "hallo welt", "a b", ""]);
        assert_eq!(split("cd ~/x ~ a~ '~'", "/h"), ["cd", "/h/x", "/h", "a~", "~"]);
        assert_eq!(split("  a   b  ", "/h"), ["a", "b"]);
    }

    #[test]
    fn pathext() {
        let files = ["C:\\n\\npm.cmd", "C:\\n\\npm", "C:\\g\\git.exe", "C:\\g\\tool.EXE"];
        // Windows-Dateisystem ignoriert Gross/Klein
        let is = |p: &Path| files.iter().any(|f| f.eq_ignore_ascii_case(p.to_str().unwrap()));
        let r = |n| resolve(n, "C:\\x;C:\\n;C:\\g", ".COM;.EXE;.BAT;.CMD", is).map(|p| p.display().to_string().to_lowercase());
        assert_eq!(r("npm").as_deref(), Some("c:\\n\\npm.cmd")); // nicht das Shell-Skript ohne Endung
        assert_eq!(r("git").as_deref(), Some("c:\\g\\git.exe"));
        assert_eq!(r("git.exe").as_deref(), Some("c:\\g\\git.exe"));
        assert_eq!(r("tool").as_deref(), Some("c:\\g\\tool.exe"));
        assert_eq!(r("C:\\g\\git").as_deref(), Some("c:\\g\\git.exe"));
        assert_eq!(r("nope"), None);
    }

    #[test]
    fn head() {
        assert_eq!(head_branch("ref: refs/heads/feature/x\n").as_deref(), Some("feature/x"));
        assert_eq!(head_branch("988dfa0123456789\n").as_deref(), Some("988dfa0"));
        let root = std::env::temp_dir().join(format!("ocui-sh-test-{}", std::process::id()));
        let sub = root.join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(branch(&sub).as_deref(), Some("main"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
