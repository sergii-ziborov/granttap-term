use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A shell whose stdout is lines on the Term tab. It does not use the desk console.
pub struct Shell {
    lines: Vec<String>,
    stdin: Option<ChildStdin>,
    child: Option<Child>,
    rx: Option<Receiver<String>>,
    /// Kill the process tree on drop. Hosted helpers spawn their own children.
    tree: bool,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            lines: vec!["Linux shell. Enter runs ls, git, and the rest.".into()],
            stdin: None,
            child: None,
            rx: None,
            tree: false,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn submit(&mut self, cwd: &Path, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.ensure(cwd);
        self.lines.push(format!("> {line}"));
        match &mut self.stdin {
            Some(stdin) => {
                let _ = stdin.write_all(line.as_bytes());
                let _ = stdin.write_all(b"\n");
                let _ = stdin.flush();
            }
            None => self.lines.push("shell is not running".into()),
        }
        self.trim();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        self.trim();
    }

    pub fn pump(&mut self) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        let mut grew = false;
        while let Ok(line) = rx.try_recv() {
            self.lines.push(line);
            grew = true;
        }
        if grew {
            self.trim();
        }
        grew
    }

    fn ensure(&mut self, cwd: &Path) {
        if self.child.is_some() {
            return;
        }
        match spawn(cwd) {
            Ok(child) => self.attach(child),
            Err(err) => self.lines.push(format!("shell  {err}")),
        }
    }

    fn attach(&mut self, mut child: Child) {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (tx, rx) = mpsc::channel();
        if let Some(out) = stdout {
            let tx = tx.clone();
            thread::spawn(move || read_lines(out, tx));
        }
        if let Some(err) = stderr {
            thread::spawn(move || read_lines(err, tx));
        }
        self.stdin = child.stdin.take();
        self.rx = Some(rx);
        self.child = Some(child);
    }

    fn trim(&mut self) {
        if self.lines.len() > 400 {
            let extra = self.lines.len() - 400;
            self.lines.drain(0..extra);
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            if self.tree {
                kill_tree(child.id());
            }
            let _ = child.kill();
        }
    }
}

fn kill_tree(id: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &id.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = id;
    }
}

/// The Term tab can hold more than one shell. The chat, when open, keeps the other column.
pub struct Shells {
    items: Vec<Shell>,
    at: usize,
    opened: Vec<String>,
    help: Arc<Mutex<Vec<(String, Vec<String>)>>>,
}

impl Shells {
    pub fn new() -> Self {
        Self {
            items: vec![Shell::new()],
            at: 0,
            opened: Vec::new(),
            help: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn lines(&self) -> &[String] {
        self.current().lines()
    }

    pub fn side_lines(&self) -> Vec<String> {
        if self.items.len() < 2 {
            return Vec::new();
        }
        let other = if self.at + 1 < self.items.len() {
            self.at + 1
        } else {
            self.at - 1
        };
        self.items.get(other).map(|shell| shell.lines().to_vec()).unwrap_or_default()
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.current_mut().push_line(line);
    }

    pub fn submit(&mut self, cwd: &Path, line: &str) {
        self.remember(line);
        self.current_mut().submit(cwd, line);
    }

    pub fn pump(&mut self) -> bool {
        let mut active = false;
        for (index, shell) in self.items.iter_mut().enumerate() {
            if shell.pump() && index == self.at {
                active = true;
            }
        }
        active
    }

    pub fn split(&mut self) {
        self.items.push(Shell::new());
        self.at = self.items.len() - 1;
        let n = self.count();
        self.push_line(format!("shell {n}"));
    }

    /// A helper beside the shell. Its output stays on the Term tab, and the
    /// selected shell does not change. The process has no console window.
    pub fn host(&mut self, title: &str, program: &Path, args: &[String], cwd: &Path) {
        self.items.push(Shell::hosted(title, program, args, cwd));
    }

    pub fn cycle(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.at = (self.at + 1) % self.items.len();
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn label(&self) -> String {
        format!("shell {} of {}", self.at + 1, self.items.len())
    }

    pub fn opened(&self) -> &[String] {
        &self.opened
    }

    pub fn subs(&self, tool: &str) -> Vec<String> {
        if let Some(list) = catalog(tool) {
            return list.iter().map(|name| (*name).to_string()).collect();
        }
        self.help
            .lock()
            .ok()
            .and_then(|rows| {
                rows.iter()
                    .find(|(name, _)| name == tool)
                    .map(|(_, subs)| subs.clone())
            })
            .unwrap_or_default()
    }

    pub fn remember(&mut self, line: &str) {
        let Some(tool) = tool_token(line) else {
            return;
        };
        if !self.opened.iter().any(|have| have == &tool) {
            self.opened.push(tool.clone());
        }
        if catalog(&tool).is_some() {
            return;
        }
        let help = Arc::clone(&self.help);
        if help
            .lock()
            .ok()
            .is_some_and(|rows| rows.iter().any(|(name, _)| name == &tool))
        {
            return;
        }
        if let Ok(mut rows) = help.lock() {
            rows.push((tool.clone(), Vec::new()));
        }
        thread::spawn(move || {
            let subs = fetch_help(&tool);
            if let Ok(mut rows) = help.lock()
                && let Some(slot) = rows.iter_mut().find(|(name, _)| name == &tool)
            {
                slot.1 = subs;
            }
        });
    }

    fn current(&self) -> &Shell {
        &self.items[self.at]
    }

    fn current_mut(&mut self) -> &mut Shell {
        &mut self.items[self.at]
    }
}

impl Shell {
    fn hosted(title: &str, program: &Path, args: &[String], cwd: &Path) -> Self {
        let mut shell = Self {
            lines: vec![title.to_string()],
            stdin: None,
            child: None,
            rx: None,
            tree: true,
        };
        let mut cmd = Command::new(program);
        cmd.args(args);
        pipe(&mut cmd, cwd);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
            if program.file_name().and_then(|name| name.to_str()) == Some("node.exe") {
                hide_node_console(&mut cmd);
            }
        }
        match cmd.spawn() {
            Ok(child) => shell.attach(child),
            Err(err) => shell.lines.push(format!("shell  {err}")),
        }
        shell
    }
}

#[cfg(windows)]
fn hide_node_console(cmd: &mut Command) {
    let Ok(profile) = std::env::var("USERPROFILE") else {
        return;
    };
    let script = Path::new(&profile).join("AppData/Local/granttap-term/hide-console.cjs");
    if !script.is_file() {
        return;
    }
    let flag = format!(
        "--require {}",
        script.display().to_string().replace('\\', "/")
    );
    let mut opts = std::env::var("NODE_OPTIONS").unwrap_or_default();
    if !opts.contains("hide-console.cjs") {
        if !opts.is_empty() && !opts.ends_with(' ') {
            opts.push(' ');
        }
        opts.push_str(&flag);
    }
    cmd.env("NODE_OPTIONS", opts);
}

fn read_lines(reader: impl Read, tx: mpsc::Sender<String>) {
    let mut lines = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let clean = line.trim_end_matches(['\r', '\n']).to_string();
                if tx.send(clean).is_err() {
                    break;
                }
            }
        }
    }
}

fn spawn(cwd: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new(shell_program());
    shell_args(&mut cmd, cwd);
    pipe(&mut cmd, cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        cmd.env("WSL_UTF8", "1");
    }
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(err) => fallback(cwd).map_err(|_| err),
    }
}

fn pipe(cmd: &mut Command, cwd: &Path) {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cwd.is_dir() {
        cmd.current_dir(cwd);
    }
}

fn shell_program() -> &'static str {
    #[cfg(windows)]
    {
        "wsl.exe"
    }
    #[cfg(not(windows))]
    {
        "bash"
    }
}

fn shell_args(cmd: &mut Command, cwd: &Path) {
    #[cfg(windows)]
    {
        // Same Linux commands on Windows as on a Unix desk: bash inside WSL.
        cmd.arg("--cd").arg(cwd).args([
            "-e",
            "stdbuf",
            "-oL",
            "-eL",
            "bash",
            "--noprofile",
            "--norc",
        ]);
    }
    #[cfg(not(windows))]
    {
        let _ = cwd;
        cmd.args(["--noprofile", "--norc"]);
    }
}

fn tool_token(line: &str) -> Option<String> {
    let token = line.split_whitespace().next().unwrap_or("");
    let token = token.trim_start_matches(['.', '/', '\\']);
    let token = token.rsplit(['/', '\\']).next().unwrap_or(token);
    if token.is_empty() || token.starts_with('-') || is_shell_word(token) {
        return None;
    }
    if !token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return None;
    }
    Some(token.to_string())
}

fn is_shell_word(token: &str) -> bool {
    matches!(
        token,
        "cd" | "ls"
            | "pwd"
            | "echo"
            | "cat"
            | "export"
            | "set"
            | "alias"
            | "source"
            | "true"
            | "false"
            | "sudo"
            | "time"
            | "clear"
            | "which"
            | "type"
            | "exit"
            | "pushd"
            | "popd"
            | "umask"
            | "wait"
            | "kill"
            | "jobs"
            | "fg"
            | "bg"
            | "history"
            | "man"
            | "less"
            | "more"
            | "head"
            | "tail"
            | "wc"
            | "grep"
            | "find"
            | "sort"
            | "uniq"
            | "awk"
            | "sed"
            | "tr"
            | "cut"
            | "xargs"
            | "chmod"
            | "chown"
            | "mkdir"
            | "rm"
            | "cp"
            | "mv"
            | "touch"
            | "ln"
            | "df"
            | "du"
            | "ps"
            | "top"
            | "env"
            | "printenv"
            | "uname"
            | "date"
            | "sleep"
            | "test"
            | "bash"
            | "sh"
            | "wsl"
            | "stdbuf"
            | "if"
            | "then"
            | "fi"
            | "for"
            | "while"
            | "do"
            | "done"
    )
}

fn catalog(tool: &str) -> Option<&'static [&'static str]> {
    Some(match tool {
        "kubectl" => &[
            "get",
            "describe",
            "create",
            "delete",
            "apply",
            "edit",
            "explain",
            "logs",
            "exec",
            "port-forward",
            "scale",
            "rollout",
            "config",
            "cluster-info",
            "api-resources",
            "api-versions",
            "top",
            "cordon",
            "uncordon",
            "drain",
            "taint",
            "label",
            "annotate",
            "patch",
            "replace",
            "diff",
            "auth",
            "cp",
            "debug",
            "events",
            "expose",
            "proxy",
            "run",
            "set",
            "wait",
            "version",
        ],
        "git" => &[
            "status", "diff", "log", "add", "commit", "push", "pull", "fetch", "checkout",
            "switch", "branch", "merge", "rebase", "stash", "restore", "show", "remote",
            "clone", "init", "tag", "reset", "revert", "blame",
        ],
        "docker" => &[
            "ps", "images", "run", "exec", "build", "pull", "push", "logs", "stop", "start",
            "rm", "rmi", "compose", "volume", "network", "inspect", "info",
        ],
        "cargo" => &[
            "build", "test", "run", "check", "clippy", "fmt", "add", "update", "install",
            "clean", "doc", "tree",
        ],
        "npm" => &[
            "install", "run", "test", "start", "ci", "exec", "init", "publish", "outdated", "ls",
        ],
        "gh" => &["pr", "issue", "run", "repo", "api", "status", "release", "auth"],
        "helm" => &[
            "install", "upgrade", "uninstall", "list", "status", "repo", "template", "rollback",
            "history", "get",
        ],
        "az" => &[
            "group", "account", "aks", "vm", "network", "storage", "login", "deployment",
        ],
        "terraform" => &[
            "init", "plan", "apply", "destroy", "fmt", "validate", "state", "workspace", "output",
            "import",
        ],
        _ => return None,
    })
}

fn fetch_help(tool: &str) -> Vec<String> {
    if let Some(text) = run_help(tool) {
        parse_help(&text)
    } else {
        Vec::new()
    }
}

fn run_help(tool: &str) -> Option<String> {
    let mut attempts = Vec::new();
    #[cfg(windows)]
    {
        let mut wsl = Command::new("wsl.exe");
        wsl.args(["-e", tool, "--help"]);
        attempts.push(wsl);
    }
    let mut direct = Command::new(tool);
    direct.arg("--help");
    attempts.push(direct);
    for mut cmd in attempts {
        silence(&mut cmd);
        if let Some(text) = wait_output(&mut cmd, Duration::from_millis(1500))
            && !text.trim().is_empty()
        {
            return Some(text);
        }
    }
    None
}

fn silence(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
}

fn wait_output(cmd: &mut Command, limit: Duration) -> Option<String> {
    let mut child = cmd.spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            let mut out = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_string(&mut out);
            }
            return Some(out);
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            return None;
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn parse_help(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if !(line.starts_with("  ") || line.starts_with('\t')) {
            continue;
        }
        let token = line.trim().split_whitespace().next().unwrap_or("");
        if token.len() < 2
            || token.starts_with('-')
            || !token
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            continue;
        }
        if !out.iter().any(|have| have == token) {
            out.push(token.to_string());
        }
        if out.len() == 40 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hosted_command_stays_beside_the_shell() {
        let mut shells = Shells::new();
        let dir = std::env::temp_dir();
        shells.host(
            "mcp serve",
            Path::new("granttap-missing-host.exe"),
            &[],
            &dir,
        );
        assert_eq!(shells.count(), 2);
        assert_eq!(shells.label(), "shell 1 of 2");
        let side = shells.side_lines();
        assert!(side.iter().any(|line| line.contains("mcp serve")), "{side:?}");
        assert!(
            side.iter().any(|line| line.contains("shell")),
            "{side:?}"
        );
    }

    #[test]
    fn the_next_helper_stays_on_the_side() {
        let mut shells = Shells::new();
        let dir = std::env::temp_dir();
        let missing = Path::new("granttap-missing-host.exe");
        shells.host("mcp serve", missing, &[], &dir);
        shells.host("mcp monitor", missing, &[], &dir);
        assert!(
            shells.side_lines().iter().any(|line| line.contains("mcp serve")),
            "{:?}",
            shells.side_lines()
        );
        shells.cycle();
        assert!(
            shells.side_lines().iter().any(|line| line.contains("mcp monitor")),
            "{:?}",
            shells.side_lines()
        );
        shells.cycle();
        assert!(
            shells.side_lines().iter().any(|line| line.contains("mcp serve")),
            "{:?}",
            shells.side_lines()
        );
    }
}

fn fallback(cwd: &Path) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        let _ = cwd;
        Err(std::io::Error::other(
            "Linux shell needs WSL. wsl.exe did not start.",
        ))
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        pipe(&mut cmd, cwd);
        cmd.spawn()
    }
}
