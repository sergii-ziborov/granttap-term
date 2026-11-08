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
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
}

pub fn handle(line: &str, home: &Path) -> String {
    let id = json_number(line, "id").unwrap_or(0);
    let method = json_string(line, "method").unwrap_or_default();
    match method.as_str() {
        "initialize" => ok(
            id,
            r#"{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"granttap-term","version":"0.1.0"}}"#,
        ),
        "tools/list" => ok(id, TOOLS),
        "tools/call" => ok(id, &call(line, home)),
        _ => err(id, "method not found"),
    }
}

const TOOLS: &str = r#"{"tools":[{"name":"list_chats","description":"Local Grok, Claude, and Codex chats on this computer.","inputSchema":{"type":"object","properties":{}}},{"name":"preview_image","description":"Cell preview of a local image for a test.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]}"#;

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
                    "{{\"agent\":\"{}\",\"id\":\"{}\",\"title\":\"{}\"}}",
                    chat.agent.label(),
                    escape(&chat.id),
                    escape(&chat.title)
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
        _ => r#"{"content":[{"type":"text","text":"unknown tool"}],"isError":true}"#.into(),
    }
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
        assert!(response.contains("granttap-term"));
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
}
