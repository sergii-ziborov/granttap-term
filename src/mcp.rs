use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::chats;
use crate::draw;

pub fn serve() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle(&line, &home());
        out.write_all(response.as_bytes())?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}

fn home() -> std::path::PathBuf {
    crate::store::user_home().unwrap_or_default()
}

pub fn handle(line: &str, home: &Path) -> String {
    let id = json_number(line, "id").unwrap_or(0);
    let method = json_string(line, "method").unwrap_or_default();
    match method.as_str() {
        "initialize" => ok(
            id,
            r#"{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"granttap-local","version":"1.0.0"}}"#,
        ),
        "tools/list" => ok(id, TOOLS),
        "tools/call" => ok(id, &call(line, home)),
        _ => err(id, "method not found"),
    }
}

const TOOLS: &str = r#"{"tools":[{"name":"list_chats","description":"Local Grok, Claude, and Codex chats on this computer.","inputSchema":{"type":"object","properties":{}}},{"name":"preview_image","description":"Cell preview of a local image for a test.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},{"name":"git_history","description":"Recent commits through Weavatrix git.","inputSchema":{"type":"object","properties":{"root":{"type":"string"}}}},{"name":"git_read_blob","description":"UTF-8 file at a revision through Weavatrix git.","inputSchema":{"type":"object","properties":{"root":{"type":"string"},"file":{"type":"string"},"revision":{"type":"string"}},"required":["file"]}},{"name":"browse_page","description":"Text rendering of a local page or http(s) URL for a test.","inputSchema":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}},{"name":"list_mesh","description":"Repo, Weavatrix registry, and linked repos for a root.","inputSchema":{"type":"object","properties":{"root":{"type":"string"}}}},{"name":"list_skills","description":"Claude, Codex, and shared skills visible on this computer.","inputSchema":{"type":"object","properties":{}}},{"name":"read_handoff","description":"Read a chat handoff written by move.","inputSchema":{"type":"object","properties":{"chat":{"type":"string"}},"required":["chat"]}},{"name":"read_skill","description":"Read a Claude, Codex, or shared skill file.","inputSchema":{"type":"object","properties":{"skill":{"type":"string"}},"required":["skill"]}},{"name":"browser_session","description":"The page open in the GrantTap Term desk: url, js, live, size, hits, and text.","inputSchema":{"type":"object","properties":{}}},{"name":"browser_act","description":"Drive the open desk browser: open, back, forward, reload, click, type, key, js, live, mark.","inputSchema":{"type":"object","properties":{"action":{"type":"string"},"text":{"type":"string"}},"required":["action"]}}]}"#;

fn call(line: &str, home: &Path) -> String {
    let name = json_string(line, "name").unwrap_or_default();
    match name.as_str() {
        "list_chats" => {
            let chats = chats::load(home);
            let mut body = String::from("[");
            for (index, chat) in chats.iter().enumerate() {
                if index > 0 {
                    body.push(',');
                }
                body.push_str(&format!(
                    "{{\"agent\":\"{}\",\"id\":\"{}\",\"title\":\"{}\",\"live\":{}}}",
                    chat.agent.label(),
                    escape(&chat.id),
                    escape(&chat.title),
                    if chat.live { "true" } else { "false" }
                ));
            }
            body.push(']');
            format!(
                "{{\"content\":[{{\"type\":\"text\",\"text\":{body_json}}}]}}",
                body_json = quote(&body)
            )
        }
        "preview_image" => {
            let Some(path) = json_string(line, "path") else {
                return r#"{"content":[{"type":"text","text":"path is required"}],"isError":true}"#
                    .into();
            };
            match draw::inspect(Path::new(&path), 1, 32, 8) {
                Ok(preview) => {
                    let text = format!(
                        "{}  {}  {}x{}  {} bytes\n{}",
                        preview.mark.token(),
                        preview.mark.format,
                        preview.mark.width,
                        preview.mark.height,
                        preview.mark.bytes,
                        preview.picture
                    );
                    let text = format!("{text}\n{}\n{}", preview.sixel, preview.kitty);
                    format!(
                        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}",
                        quote(&text)
                    )
                }
                Err(err) => format!(
                    "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":true}}",
                    quote(&err)
                ),
            }
        }
        "git_history" => text_result(&history_text(&root_arg(line))),
        "git_read_blob" => {
            let Some(file) = json_string(line, "file") else {
                return r#"{"content":[{"type":"text","text":"file is required"}],"isError":true}"#
                    .into();
            };
            let revision = json_string(line, "revision").unwrap_or_else(|| "HEAD".into());
            match crate::gitread::blob_text(Path::new(&root_arg(line)), &revision, &file, 8_000) {
                Ok(text) => text_result(&text),
                Err(err) => error_text(&err),
            }
        }
        "browse_page" => {
            let Some(url) = json_string(line, "url") else {
                return r#"{"content":[{"type":"text","text":"url is required"}],"isError":true}"#
                    .into();
            };
            match crate::page::load(&url, &cwd()) {
                Ok(lines) => text_result(&lines.join("\n")),
                Err(err) => error_text(&err),
            }
        }
        "list_mesh" => text_result(&mesh_text(home, &root_arg(line))),
        "browser_session" => text_result(&session_text(home)),
        "browser_act" => {
            let Some(action) = json_string(line, "action") else {
                return r#"{"content":[{"type":"text","text":"action is required"}],"isError":true}"#
                    .into();
            };
            let text = json_string(line, "text").unwrap_or_default();
            match queue_browser_act(home, &action, &text) {
                Ok(body) => text_result(&body),
                Err(err) => error_text(&err),
            }
        }
        "read_handoff" => {
            let Some(chat) = json_string(line, "chat") else {
                return r#"{"content":[{"type":"text","text":"chat is required"}],"isError":true}"#
                    .into();
            };
            match chats::handoff_text(home, &chat) {
                Ok(text) => text_result(&text),
                Err(err) => error_text(&err),
            }
        }
        "read_skill" => {
            let Some(name) = json_string(line, "skill") else {
                return r#"{"content":[{"type":"text","text":"skill is required"}],"isError":true}"#
                    .into();
            };
            let skills = crate::skills::load(home, &cwd());
            let Some(skill) = skills.iter().find(|skill| skill.name == name) else {
                return error_text(&format!("no skill {name}"));
            };
            match std::fs::read_to_string(&skill.path) {
                Ok(text) => {
                    let mut clipped = text;
                    if clipped.len() > 8_000 {
                        clipped.truncate(8_000);
                    }
                    text_result(&clipped)
                }
                Err(err) => error_text(&err.to_string()),
            }
        }
        "list_skills" => {
            let skills = crate::skills::load(home, &cwd());
            if skills.is_empty() {
                text_result("none")
            } else {
                let body = skills
                    .iter()
                    .map(|skill| format!("{}  {}", skill.agent, skill.name))
                    .collect::<Vec<_>>()
                    .join("\n");
                text_result(&body)
            }
        }
        _ => r#"{"content":[{"type":"text","text":"unknown tool"}],"isError":true}"#.into(),
    }
}

fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn root_arg(line: &str) -> String {
    json_string(line, "root").unwrap_or_else(|| cwd().display().to_string())
}

fn history_text(root: &str) -> String {
    match crate::gitread::recent(Path::new(root), 12) {
        Ok(revs) if revs.is_empty() => "No commits.".into(),
        Ok(revs) => revs
            .iter()
            .map(|rev| format!("{}  {}  {}", rev.id, rev.when, rev.subject))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(err) => err,
    }
}

fn mesh_text(home: &Path, root: &str) -> String {
    let root_path = Path::new(root);
    let repo = crate::mesh::git_root(root_path)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "none".into());
    let known = crate::gitread::known_repos(home);
    let names = if known.is_empty() {
        "none".into()
    } else {
        known
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let board = crate::health::scan(root_path, false, false);
    let mut faults = String::new();
    for (rel, why) in board.faults().take(12) {
        faults.push_str(&format!("! {rel}  {why}\n"));
    }
    if faults.is_empty() {
        faults = "none\n".into();
    }
    format!(
        "Repo  {repo}\nWeavatrix  {names}\nUnhealthy  {}\n{faults}",
        board.unhealthy_count()
    )
}

fn browser_dir(home: &Path) -> Option<std::path::PathBuf> {
    if !home.is_dir() {
        return None;
    }
    let dir = home.join(".granttap-term");
    fs_create(&dir)?;
    Some(dir)
}

fn fs_create(dir: &Path) -> Option<()> {
    if dir.is_dir() || std::fs::create_dir_all(dir).is_ok() {
        Some(())
    } else {
        None
    }
}

pub(crate) fn write_browser_snapshot(home: &Path, body: &str) {
    let Some(dir) = browser_dir(home) else {
        return;
    };
    let _ = std::fs::write(dir.join("browser.txt"), body);
}

pub(crate) fn write_browser_ack(home: &Path, body: &str) {
    let Some(dir) = browser_dir(home) else {
        return;
    };
    let _ = std::fs::write(dir.join("browser-ack.txt"), body);
}

pub(crate) fn take_browser_command(home: &Path) -> Option<String> {
    let dir = home.join(".granttap-term");
    let path = dir.join("browser-cmd.txt");
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next()?.to_string();
    let rest = lines.collect::<Vec<_>>().join("\n");
    if !rest.is_empty() {
        let _ = std::fs::write(&path, format!("{rest}\n"));
    }
    Some(first)
}

fn read_snapshot(home: &Path) -> String {
    std::fs::read_to_string(home.join(".granttap-term/browser.txt")).unwrap_or_default()
}

fn read_ack(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".granttap-term/browser-ack.txt")).ok()?;
    let text = text.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn session_text(home: &Path) -> String {
    let raw = read_snapshot(home);
    if raw.trim().is_empty() {
        return "No browser session. Open GrantTap Term.".into();
    }
    format_session(&raw)
}

fn format_session(raw: &str) -> String {
    let mut url = String::new();
    let mut js = String::new();
    let mut live = String::new();
    let mut bytes = String::new();
    let mut net = String::new();
    let mut busy = String::new();
    let mut open = String::new();
    let mut stamp = 0i64;
    let mut hits = Vec::new();
    let mut text = Vec::new();
    let mut mode = "";
    for line in raw.lines() {
        if line == "hits" {
            mode = "hits";
            continue;
        }
        if line == "text" {
            mode = "text";
            continue;
        }
        if mode == "hits" {
            hits.push(line.to_string());
            continue;
        }
        if mode == "text" {
            text.push(line.to_string());
            continue;
        }
        let Some((key, value)) = line.split_once('\t') else {
            continue;
        };
        match key {
            "stamp" => stamp = value.parse().unwrap_or(0),
            "url" => url = value.to_string(),
            "js" => js = if value == "1" { "on".into() } else { "off".into() },
            "live" => live = if value == "1" { "on".into() } else { "off".into() },
            "bytes" => bytes = value.to_string(),
            "net" => net = value.to_string(),
            "busy" => busy = if value == "1" { "yes".into() } else { "no".into() },
            "open" => open = if value == "1" { "yes".into() } else { "no".into() },
            _ => {}
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let fresh = stamp > 0 && now.saturating_sub(stamp) <= 3;
    let state = if fresh { "live" } else { "stale" };
    let mut out = format!(
        "session {state}\nurl  {url}\nopen {open}  js {js}  live {live}  busy {busy}\nsize {bytes}  net {net}\nhits\n"
    );
    if hits.is_empty() {
        out.push_str("none\n");
    } else {
        for hit in hits.iter().take(40) {
            out.push_str(hit);
            out.push('\n');
        }
    }
    out.push_str("text\n");
    if text.is_empty() {
        out.push_str("empty\n");
    } else {
        for line in text.iter().take(60) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn queue_browser_act(home: &Path, action: &str, text: &str) -> Result<String, String> {
    let line = match action {
        "open" | "type" | "key" | "click" => {
            if text.is_empty() {
                return Err(format!("{action} needs text"));
            }
            format!("{action}\t{text}")
        }
        "back" | "forward" | "reload" | "js" | "live" | "mark" => action.to_string(),
        _ => return Err(format!("unknown action {action}")),
    };
    let Some(dir) = browser_dir(home) else {
        return Err("no home for the desk".into());
    };
    let _ = std::fs::remove_file(dir.join("browser-ack.txt"));
    let path = dir.join("browser-cmd.txt");
    let mut previous = std::fs::read_to_string(&path).unwrap_or_default();
    if !previous.is_empty() && !previous.ends_with('\n') {
        previous.push('\n');
    }
    previous.push_str(&line);
    previous.push('\n');
    std::fs::write(&path, previous).map_err(|err| err.to_string())?;
    let spins = if cfg!(test) { 0 } else { 160 };
    for _ in 0..spins {
        if read_ack(home).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if let Some(ack) = read_ack(home) {
        return Ok(format!("{ack}\n{}", session_text(home)));
    }
    Ok(format!("queued\n{}", session_text(home)))
}

fn text_result(text: &str) -> String {
    format!(
        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}",
        quote(text)
    )
}

fn error_text(err: &str) -> String {
    format!(
        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":true}}",
        quote(err)
    )
}

fn ok(id: i64, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
}

fn err(id: i64, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32601,\"message\":{}}}}}",
        quote(message)
    )
}

fn quote(text: &str) -> String {
    format!("\"{}\"", escape(text))
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

fn json_string(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = line.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let value = rest.trim_start_matches('"').split('"').next()?;
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn json_number(line: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\"");
    let after = line.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let digits: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_names_the_server() {
        let response = handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            Path::new("/"),
        );
        assert!(response.contains("granttap-local"));
        assert!(response.contains("1.0.0"));
        assert!(response.contains("\"id\":1"));
    }

    #[test]
    fn unknown_method_is_an_error() {
        let response = handle(
            r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#,
            Path::new("/"),
        );
        assert!(response.contains("method not found"));
    }

    #[test]
    fn preview_without_a_path_is_an_error() {
        let response = handle(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"preview_image"}}"#,
            Path::new("/"),
        );
        assert!(response.contains("path is required"));
    }

    #[test]
    fn read_handoff_returns_the_bundle() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-handoff-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        let transcript = home.join("chat.txt");
        std::fs::write(&transcript, "hello desk\n").unwrap();
        let chat = crate::model::Chat {
            agent: crate::model::AgentKind::Claude,
            id: "ab/c".into(),
            title: "Desk".into(),
            path: transcript,
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
            window: None,
        };
        chats::write_handoff(&home, &chat, crate::model::AgentKind::Grok).unwrap();
        let response = handle(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read_handoff","arguments":{"chat":"ab/c"}}}"#,
            &home,
        );
        assert!(response.contains("from Claude"));
        assert!(response.contains("to Grok"));
        assert!(response.contains("hello desk"));
        assert!(response.contains("\"id\":4"));
        let missing = handle(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"read_handoff","arguments":{"chat":"missing"}}}"#,
            &home,
        );
        assert!(missing.contains("no handoff"));
        assert!(missing.contains("isError"));
        let listed = handle(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/list"}"#,
            Path::new("/"),
        );
        assert!(listed.contains("read_handoff"));
        std::fs::create_dir_all(home.join(".agents/skills/ship")).unwrap();
        std::fs::write(home.join(".agents/skills/ship/SKILL.md"), "Keep the desk\n").unwrap();
        let skill = handle(
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"read_skill","arguments":{"skill":"ship"}}}"#,
            &home,
        );
        assert!(skill.contains("Keep the desk"));
        assert!(skill.contains("\"id\":7"));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn the_browser_tools_read_and_queue_the_desk_session() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-mcp-browser-{n}"));
        std::fs::create_dir_all(home.join(".granttap-term")).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::write(
            home.join(".granttap-term/browser.txt"),
            format!("stamp\t{stamp}\nurl\thttps://example.com/desk\njs\t1\nlive\t1\nbytes\t12\nnet\t40\nbusy\t0\nopen\t1\nhits\n0\tlink\t0\t1\t9\t1\tDocs\thttps://example.com/docs\ntext\nHello desk\n"),
        )
        .unwrap();
        let listed = handle(
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/list"}"#,
            &home,
        );
        assert!(listed.contains("browser_session"), "{listed}");
        assert!(listed.contains("browser_act"), "{listed}");
        let seen = handle(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"browser_session"}}"#,
            &home,
        );
        assert!(seen.contains("https://example.com/desk"), "{seen}");
        assert!(seen.contains("Hello desk"), "{seen}");
        assert!(seen.contains("session live"), "{seen}");
        let queued = handle(
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"browser_act","arguments":{"action":"reload"}}}"#,
            &home,
        );
        assert!(queued.contains("queued"), "{queued}");
        let cmd = std::fs::read_to_string(home.join(".granttap-term/browser-cmd.txt")).unwrap();
        assert_eq!(cmd.trim(), "reload");
        assert_eq!(take_browser_command(&home).as_deref(), Some("reload"));
        assert!(take_browser_command(&home).is_none());
        let missing = handle(
            r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"browser_act","arguments":{"action":"open"}}}"#,
            &home,
        );
        assert!(missing.contains("isError"), "{missing}");
        let _ = std::fs::remove_dir_all(home);
    }
}
