use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::agents;
use crate::model::AgentKind;

/// Hand the line to the live agent. A local echo is not delivery.
pub struct Delivery {
    pub note: String,
    pub screen: String,
}

impl Delivery {
    fn note(text: impl Into<String>) -> Self {
        Self {
            note: text.into(),
            screen: String::new(),
        }
    }

    fn with_screen(note: impl Into<String>, screen: impl Into<String>) -> Self {
        Self {
            note: note.into(),
            screen: screen.into(),
        }
    }
}

pub fn send(home: &Path, agent: AgentKind, id: &str, text: &str) -> Delivery {
    let text = text.trim();
    if text.is_empty() || id.is_empty() {
        return Delivery::note("not live");
    }
    if std::env::var_os("GRANTAP_TERM_NO_SEND").is_some() {
        return Delivery::note("not live");
    }
    let slash = text.starts_with('/');
    match agent {
        AgentKind::Grok => {
            let Some(pid) = grok_pid(home, id) else {
                return Delivery::note("not live");
            };
            inject(pid, text)
        }
        AgentKind::Claude => {
            if slash && let Some(pid) = claude_pid(home, id) {
                return inject(pid, text);
            }
            match claude_target(home, id) {
                Some(target) => Delivery::note(claude_pipe(&target, text)),
                None => Delivery::note("not live"),
            }
        }
        AgentKind::Codex => {
            if !codex_listed(home, id) {
                return Delivery::note("not live");
            }
            if slash && let Some(pid) = cli_pid("codex", id) {
                return inject(pid, text);
            }
            Delivery::note(codex_queue(id, text))
        }
    }
}

fn grok_pid(home: &Path, id: &str) -> Option<u32> {
    agents::grok_sessions(home)
        .into_iter()
        .find(|session| session.alive && session.id == id)
        .map(|session| session.pid)
}

fn claude_pid(home: &Path, id: &str) -> Option<u32> {
    claude_target(home, id).map(|target| target.pid)
}

struct ClaudeTarget {
    pid: u32,
    pipe: String,
    token: String,
}

fn claude_target(home: &Path, id: &str) -> Option<ClaudeTarget> {
    let dir = home.join(".claude/sessions");
    let entries = fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if json_string(&text, "sessionId").as_deref() != Some(id) {
            continue;
        }
        let pid = json_number(&text, "pid")?;
        if !agents::pid_alive(pid) {
            continue;
        }
        let pipe = json_string(&text, "messagingSocketPath")?;
        let token = peer_token(&dir, pid)?;
        return Some(ClaudeTarget { pid, pipe, token });
    }
    None
}

fn peer_token(dir: &Path, pid: u32) -> Option<String> {
    let prefix = format!("{pid}.");
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".key") {
            let text = fs::read_to_string(entry.path()).ok()?;
            return json_string(&text, "peerToken");
        }
    }
    None
}

fn claude_pipe(target: &ClaudeTarget, text: &str) -> String {
    let id = message_id();
    let body = format!(
        "{{\"type\":\"auth\",\"token\":\"{}\"}}\n{{\"msgV\":1,\"msg_id\":\"{id}\",\"type\":\"user\",\"priority\":\"now\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}\n",
        json_escape(&target.token),
        json_escape(text)
    );
    let path = target.pipe.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(write_pipe(&path, body.as_bytes()));
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => "sent".into(),
        Ok(Err(err)) => clip_note("not sent", &err),
        Err(_) => "not sent  pipe timed out".into(),
    }
}

fn write_pipe(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .or_else(|_| OpenOptions::new().write(true).open(path))
        .map_err(|err| err.to_string())?;
    file.write_all(bytes).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn codex_listed(home: &Path, id: &str) -> bool {
    let Ok(text) = fs::read_to_string(home.join(".codex/session_index.jsonl")) else {
        return false;
    };
    text.lines()
        .any(|line| json_string(line, "id").as_deref() == Some(id))
}

fn codex_queue(id: &str, text: &str) -> String {
    let Some((node, script)) = codex_script() else {
        return "not sent  codex is not on PATH".into();
    };
    let mut cmd = Command::new(node);
    cmd.arg(script)
        .args(["queue", "--thread", id, "--message", text])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => "sent".into(),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().find(|line| !line.trim().is_empty()).unwrap_or("codex queue failed");
            clip_note("not sent", line)
        }
        Err(err) => clip_note("not sent", &err.to_string()),
    }
}

fn codex_script() -> Option<(PathBuf, PathBuf)> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cmd = dir.join("codex.cmd");
        let node = dir.join("node.exe");
        let script = dir.join("node_modules/@openai/codex/bin/codex.js");
        if cmd.is_file() && node.is_file() && script.is_file() {
            return Some((node, script));
        }
    }
    None
}

fn cli_pid(name: &str, id: &str) -> Option<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = format!(
            "Get-CimInstance Win32_Process | ForEach-Object {{ if ($_.CommandLine -and $_.ProcessId) {{ \"$($_.ProcessId)`t$($_.CommandLine)\" }} }}"
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let Some((pid, command)) = line.split_once('\t') else {
                continue;
            };
            if command_is(command, name) && command.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_').any(|part| part == id)
            {
                if let Ok(pid) = pid.trim().parse::<u32>()
                    && agents::pid_alive(pid)
                {
                    return Some(pid);
                }
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = (name, id);
        None
    }
}

fn command_is(line: &str, name: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains(&format!("{name}.exe"))
        || lower.contains(&format!("{name}.cmd"))
        || lower.contains(&format!("/{name}"))
        || lower.contains(&format!("\\{name}"))
        || lower.split_whitespace().any(|token| token == name)
}

fn inject(pid: u32, text: &str) -> Delivery {
    if pid == 0 || pid == std::process::id() {
        return Delivery::note("not sent");
    }
    let Ok(exe) = std::env::current_exe() else {
        return Delivery::note("not sent");
    };
    let mut cmd = Command::new(exe);
    cmd.arg("--inject")
        .arg(pid.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The desk already owns a console. AttachConsole from that process hangs.
        cmd.creation_flags(0x0000_0008);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return Delivery::note(clip_note("not sent", &err.to_string())),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut screen = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut screen);
                }
                return Delivery::with_screen("sent", screen);
            }
            Ok(Some(_)) => {
                let mut err = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                let line = err
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("inject failed");
                return Delivery::note(clip_note("not sent", line));
            }
            Ok(None) if start.elapsed() > Duration::from_secs(3) => {
                let _ = child.kill();
                return Delivery::note("not sent  inject timed out");
            }
            Ok(None) => thread::sleep(Duration::from_millis(30)),
            Err(err) => return Delivery::note(clip_note("not sent", &err.to_string())),
        }
    }
}

pub fn inject_attached(pid: u32, text: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        inject_windows(pid, text)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, text);
        Err("console inject is a windows console".into())
    }
}

#[cfg(windows)]
fn inject_windows(pid: u32, text: &str) -> Result<(), String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn FreeConsole() -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut core::ffi::c_void,
            disposition: u32,
            flags: u32,
            template: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
        fn WriteConsoleInputW(
            handle: *mut core::ffi::c_void,
            records: *const InputRecord,
            count: u32,
            wrote: *mut u32,
        ) -> i32;
        fn GetLastError() -> u32;
        fn Sleep(millis: u32);
    }
    // INPUT_RECORD is 20 bytes: event type, pad, then KEY_EVENT_RECORD.
    #[repr(C)]
    struct InputRecord {
        event_type: u16,
        _pad: u16,
        key_down: i32,
        repeat: u16,
        vk: u16,
        scan: u16,
        unicode: u16,
        control: u32,
    }
    const _: () = assert!(std::mem::size_of::<InputRecord>() == 20);
    const KEY_EVENT: u16 = 1;
    const VK_RETURN: u16 = 0x0D;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    fn record(ch: u16, vk: u16, down: bool) -> InputRecord {
        InputRecord {
            event_type: KEY_EVENT,
            _pad: 0,
            key_down: i32::from(down),
            repeat: 1,
            vk,
            scan: 0,
            unicode: ch,
            control: 0,
        }
    }
    let mut records = Vec::new();
    for ch in text.encode_utf16() {
        records.push(record(ch, 0, true));
        records.push(record(ch, 0, false));
    }
    records.push(record(b'\r' as u16, VK_RETURN, true));
    records.push(record(b'\r' as u16, VK_RETURN, false));
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(format!("attach {}", GetLastError()));
        }
        // Stdin of --inject is a pipe. GetStdHandle stays on that pipe.
        // CONIN$ is the console we just attached.
        let name: Vec<u16> = "CONIN$\0".encode_utf16().collect();
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle.is_null() || handle == (-1isize as *mut core::ffi::c_void) {
            let err = GetLastError();
            FreeConsole();
            return Err(format!("input {err}"));
        }
        let mut wrote = 0u32;
        let ok = WriteConsoleInputW(handle, records.as_ptr(), records.len() as u32, &mut wrote);
        let err = GetLastError();
        CloseHandle(handle);
        if ok == 0 || wrote != records.len() as u32 {
            FreeConsole();
            return Err(format!("write {err} wrote {wrote}"));
        }
        // The slash UI paints on the agent console. Read it before we detach
        // so the desk can show the reply that never lands in the transcript.
        Sleep(300);
        let screen = console_bottom();
        FreeConsole();
        if !screen.is_empty() {
            println!("{screen}");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn console_bottom() -> String {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut core::ffi::c_void,
            disposition: u32,
            flags: u32,
            template: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
        fn GetConsoleScreenBufferInfo(
            handle: *mut core::ffi::c_void,
            info: *mut BufferInfo,
        ) -> i32;
        fn ReadConsoleOutputW(
            handle: *mut core::ffi::c_void,
            buffer: *mut CharInfo,
            size: Coord,
            coord: Coord,
            region: *mut SmallRect,
        ) -> i32;
    }
    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }
    #[repr(C)]
    struct BufferInfo {
        size: Coord,
        cursor: Coord,
        attributes: u16,
        window: SmallRect,
        maximum: Coord,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CharInfo {
        ch: u16,
        attr: u16,
    }
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    unsafe {
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle.is_null() || handle == (-1isize as *mut core::ffi::c_void) {
            return String::new();
        }
        let mut info = BufferInfo {
            size: Coord { x: 0, y: 0 },
            cursor: Coord { x: 0, y: 0 },
            attributes: 0,
            window: SmallRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            maximum: Coord { x: 0, y: 0 },
        };
        if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
            CloseHandle(handle);
            return String::new();
        }
        let width = (info.window.right - info.window.left + 1).clamp(1, 180);
        let height = (info.window.bottom - info.window.top + 1).clamp(1, 80);
        let take = 14.min(height);
        let top = info.window.bottom - take + 1;
        let mut region = SmallRect {
            left: info.window.left,
            top,
            right: info.window.left + width - 1,
            bottom: info.window.bottom,
        };
        let cells = (width as usize).saturating_mul(take as usize);
        let mut buffer = vec![CharInfo { ch: 0, attr: 0 }; cells];
        let ok = ReadConsoleOutputW(
            handle,
            buffer.as_mut_ptr(),
            Coord { x: width, y: take },
            Coord { x: 0, y: 0 },
            &mut region,
        );
        CloseHandle(handle);
        if ok == 0 {
            return String::new();
        }
        let mut lines = Vec::new();
        for row in 0..take as usize {
            let start = row * width as usize;
            let end = start + width as usize;
            let units: Vec<u16> = buffer[start..end]
                .iter()
                .map(|cell| if cell.ch == 0 { b' ' as u16 } else { cell.ch })
                .collect();
            let text = String::from_utf16_lossy(&units);
            let text = text.trim();
            if text.chars().any(|ch| ch.is_alphanumeric()) {
                lines.push(text.to_string());
            }
        }
        lines.join("\n")
    }
}

fn message_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("cc-msg-{n:032x}")
}

fn clip_note(head: &str, detail: &str) -> String {
    let detail = detail.trim().replace(['\n', '\r'], " ");
    let detail: String = detail.chars().take(80).collect();
    if detail.is_empty() {
        head.into()
    } else {
        format!("{head}  {detail}")
    }
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    let value = after.split('"').nth(1)?;
    if value.is_empty() {
        None
    } else {
        Some(unescape(value))
    }
}

fn json_number(text: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_session_is_not_live_and_does_not_spawn() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-send-{n}"));
        fs::create_dir_all(&home).unwrap();
        assert_eq!(
            send(&home, AgentKind::Grok, "no-such-session", "hi").note,
            "not live"
        );
        assert_eq!(
            send(&home, AgentKind::Codex, "no-such-thread", "hi").note,
            "not live"
        );
        assert_eq!(
            send(&home, AgentKind::Claude, "no-such-session", "hi").note,
            "not live"
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn claude_reports_a_missing_pipe_without_a_live_session() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-pipe-{n}"));
        let dir = home.join(".claude/sessions");
        fs::create_dir_all(&dir).unwrap();
        let pid = std::process::id();
        fs::write(
            dir.join(format!("{pid}.json")),
            format!(
                "{{\"pid\":{pid},\"sessionId\":\"sess-desk\",\"messagingSocketPath\":\"\\\\\\\\.\\\\pipe\\\\granttap-term-missing-{n}\"}}"
            ),
        )
        .unwrap();
        fs::write(
            dir.join(format!("{pid}.abc.key")),
            "{\"peerToken\":\"desk-token\"}\n",
        )
        .unwrap();
        let note = send(&home, AgentKind::Claude, "sess-desk", "hi").note;
        assert!(note.starts_with("not sent"), "{note}");
        assert_eq!(
            send(&home, AgentKind::Claude, "sess-desk", "/compact").note,
            "not sent"
        );
        let _ = fs::remove_dir_all(home);
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('/') => out.push('/'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
