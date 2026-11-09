use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::model::{AgentKind, Chat};

const CAP: usize = 80;

pub fn load(home: &Path) -> Vec<Chat> {
    let mut codex = Vec::new();
    load_codex(&home.join(".codex/session_index.jsonl"), &mut codex);
    if codex.len() > 40 {
        codex.drain(0..codex.len() - 40);
    }
    let mut claude = Vec::new();
    load_claude(&home.join(".claude/projects"), &mut claude);
    let mut grok = Vec::new();
    load_grok(&home.join(".grok/sessions"), &mut grok);
    attach_codex_rollouts(&home.join(".codex/session_index.jsonl"), &mut codex);
    let mut chats = codex;
    chats.extend(claude);
    chats.extend(grok);
    mark_live(home, &mut chats);
    chats
}

fn mark_live(home: &Path, chats: &mut Vec<Chat>) {
    for session in crate::agents::grok_sessions(home) {
        if !session.alive {
            continue;
        }
        if let Some(chat) = chats
            .iter_mut()
            .find(|chat| chat.agent == AgentKind::Grok && chat.id == session.id)
        {
            chat.live = true;
            continue;
        }
        let path = crate::agents::session_path(home, &session.cwd, &session.id);
        if !path.is_dir() {
            continue;
        }
        let (at, cwd_from_summary) = grok_facts(&path);
        let title = crate::agents::generated_title(&path).unwrap_or_else(|| session.id.clone());
        let cwd = if cwd_from_summary.is_empty() {
            session.cwd
        } else {
            cwd_from_summary
        };
        let at = if at == 0 { mtime(&path) } else { at };
        chats.insert(
            0,
            Chat {
                agent: AgentKind::Grok,
                id: session.id,
                title,
                path: path.clone(),
                cwd,
                live: true,
                at,
                tokens: grok_tokens(&path),
            },
        );
    }
    mark_cli_live(chats, &cli_commands());
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.live));
}

fn cli_commands() -> String {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-ax", "-o", "command="])
        .output()
    else {
        return String::new();
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.len() > 256 * 1024 {
        text.truncate(256 * 1024);
    }
    text
}

fn mark_cli_live(chats: &mut [Chat], ps: &str) {
    for chat in chats.iter_mut() {
        if chat.live || chat.id.len() < 8 {
            continue;
        }
        let name = match chat.agent {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Grok => continue,
        };
        if ps
            .lines()
            .any(|line| is_cli(line, name) && contains_id(line, &chat.id))
        {
            chat.live = true;
        }
    }
}

fn is_cli(line: &str, name: &str) -> bool {
    let token = line.split_whitespace().next().unwrap_or("");
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    base == name
}

fn contains_id(line: &str, id: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(found) = line[start..].find(id) {
        let at = start + found;
        let before = at == 0 || !is_id_byte(bytes[at - 1]);
        let after_at = at + id.len();
        let after = after_at >= bytes.len() || !is_id_byte(bytes[after_at]);
        if before && after {
            return true;
        }
        start = at + 1;
    }
    false
}

fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn load_codex(index: &Path, out: &mut Vec<Chat>) {
    let file = match File::open(index) {
        Ok(file) => file,
        Err(_) => return,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Some(id) = json_string(&line, "id") else {
            continue;
        };
        let title = json_string(&line, "thread_name").unwrap_or_else(|| id.clone());
        let at = json_string(&line, "updated_at")
            .and_then(|text| unix_time(&text))
            .unwrap_or(0);
        out.push(Chat {
            agent: AgentKind::Codex,
            id,
            title,
            path: index.to_path_buf(),
            cwd: String::new(),
            live: false,
            at,
            tokens: None,
        });
    }
}

fn load_claude(root: &Path, out: &mut Vec<Chat>) {
    let mut found = Vec::new();
    let Ok(projects) = fs::read_dir(root) else {
        return;
    };
    for project in projects.flatten() {
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            found.push((mtime(&path), path));
            if found.len() == 4000 {
                break;
            }
        }
        if found.len() == 4000 {
            break;
        }
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.truncate(CAP);
    for (at, path) in found {
        let id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "chat".into());
        let cwd = path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = claude_title(&path).unwrap_or_else(|| id.clone());
        let tokens = claude_tokens(&path);
        out.push(Chat {
            agent: AgentKind::Claude,
            title,
            id,
            path,
            cwd,
            live: false,
            at,
            tokens,
        });
    }
}

fn load_grok(root: &Path, out: &mut Vec<Chat>) {
    let mut found = Vec::new();
    collect_grok(root, &mut found, 0);
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.truncate(CAP);
    for (_, path) in found {
        if let Some(chat) = grok_session(&path) {
            out.push(chat);
        }
    }
}

fn collect_grok(path: &Path, out: &mut Vec<(i64, PathBuf)>, depth: u8) {
    if depth > 4 || out.len() >= 4000 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut children = Vec::new();
    let mut saw_file = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let child = entry.path();
        if child.is_dir() {
            children.push(child);
        } else {
            saw_file = true;
        }
    }
    if depth > 0 && (saw_file || children.is_empty()) {
        let summary = path.join("summary.json");
        let at = if summary.is_file() {
            mtime(&summary)
        } else {
            mtime(path)
        };
        out.push((at, path.to_path_buf()));
        if saw_file {
            return;
        }
    }
    for child in children {
        if out.len() >= 4000 {
            break;
        }
        collect_grok(&child, out, depth + 1);
    }
}

fn grok_session(path: &Path) -> Option<Chat> {
    let id = path.file_name()?.to_string_lossy().into_owned();
    if id.starts_with('.') {
        return None;
    }
    let (at, cwd_from_summary) = grok_facts(path);
    let title = crate::agents::generated_title(path).unwrap_or(id.clone());
    let cwd = if cwd_from_summary.is_empty() {
        path.parent()
            .and_then(|parent| parent.file_name())
            .map(|name| crate::agents::percent_decode(&name.to_string_lossy()))
            .unwrap_or_default()
    } else {
        cwd_from_summary
    };
    let at = if at == 0 { mtime(path) } else { at };
    Some(Chat {
        agent: AgentKind::Grok,
        title,
        id,
        path: path.to_path_buf(),
        cwd,
        live: false,
        at,
        tokens: grok_tokens(path),
    })
}

pub fn transcript(path: &Path) -> Vec<String> {
    turns(path, 64 * 1024, 24)
}

pub fn transcript_full(path: &Path) -> Vec<String> {
    turns(path, 8 * 1024 * 1024, 6000)
}

fn turns(path: &Path, cap: u64, keep: usize) -> Vec<String> {
    let file = if path.is_dir() {
        let history = path.join("chat_history.jsonl");
        if history.is_file() {
            history
        } else {
            path.join("summary.json")
        }
    } else {
        path.to_path_buf()
    };
    let Some(text) = read_tail_cap(&file, cap) else {
        return vec!["No transcript.".into()];
    };
    let mut lines = Vec::new();
    for line in text.lines() {
        push_turn(&mut lines, turn_lines(line));
    }
    if lines.is_empty() {
        return vec!["No transcript.".into()];
    }
    lines = thin_tools(lines, 400);
    if lines.len() > keep {
        let mut start = lines.len() - keep;
        let floor = start.saturating_sub(400);
        while start > floor && !turn_head(&lines[start]) {
            start -= 1;
        }
        lines.drain(..start);
    }
    lines
}

fn turn_head(line: &str) -> bool {
    line.starts_with("user  ") || line.starts_with("assistant  ") || line.starts_with("tool  ")
}

fn push_turn(lines: &mut Vec<String>, piece: Vec<String>) {
    let Some(head) = piece.first() else {
        return;
    };
    let turn = head.starts_with("user  ") || head.starts_with("assistant  ");
    if turn && lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.extend(piece);
}

fn message_lines(prefix: &str, text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut labeled = false;
    for para in text.split('\n') {
        let para = one_line(para.trim_end());
        if para.trim().is_empty() {
            if labeled && out.last().is_some_and(|line: &String| !line.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let shown = clip_msg(&para);
        if labeled {
            out.push(shown);
        } else {
            out.push(format!("{prefix}{shown}"));
            labeled = true;
        }
    }
    while out.last().is_some_and(|line: &String| line.is_empty()) {
        out.pop();
    }
    out
}

fn thin_tools(lines: Vec<String>, tool_cap: usize) -> Vec<String> {
    let tools = lines
        .iter()
        .filter(|line| line.starts_with("tool  "))
        .count();
    let mut drop_tools = tools.saturating_sub(tool_cap);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if drop_tools > 0 && line.starts_with("tool  ") {
            drop_tools -= 1;
            continue;
        }
        out.push(line);
    }
    out
}

enum Piece {
    Text(String),
    Tool(String),
}

fn assistant_lines(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pieces = json_value_after(line, "content")
        .map(content_pieces)
        .unwrap_or_default();
    let had_tool = pieces.iter().any(|piece| matches!(piece, Piece::Tool(_)));
    emit_pieces(&mut out, "assistant  ", &pieces);
    if !had_tool {
        for name in tool_names(line) {
            push_tool(&mut out, &name);
        }
    }
    out
}

fn emit_pieces(out: &mut Vec<String>, prefix: &str, pieces: &[Piece]) {
    let mut buf = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(text) => {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(text);
            }
            Piece::Tool(name) => {
                flush_text(out, prefix, &buf);
                buf.clear();
                push_tool(out, name);
            }
        }
    }
    flush_text(out, prefix, &buf);
}

fn flush_text(out: &mut Vec<String>, prefix: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    if out.last().is_some_and(|line| !line.is_empty()) {
        out.push(String::new());
    }
    out.extend(message_lines(prefix, text));
}

fn push_tool(out: &mut Vec<String>, name: &str) {
    if !is_ident(name) {
        return;
    }
    let row = format!("tool  {name}");
    if out.last().map(String::as_str) == Some(row.as_str()) {
        return;
    }
    if out.last().is_some_and(|line| !line.is_empty()) {
        out.push(String::new());
    }
    out.push(row);
}

fn content_pieces(value: &str) -> Vec<Piece> {
    if value.starts_with('"') {
        return match parse_json_string(value) {
            Some(text) if !text.is_empty() => vec![Piece::Text(text)],
            _ => Vec::new(),
        };
    }
    let Some(array) = bracket_slice(value) else {
        return Vec::new();
    };
    let pieces = pieces_in_array(array);
    if !pieces.is_empty() {
        return pieces;
    }
    json_all(array, "text")
        .into_iter()
        .filter(|text| !text.is_empty())
        .map(Piece::Text)
        .collect()
}

fn pieces_in_array(array: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = array;
    while out.len() < 128 {
        let Some((kind, at)) = next_type(rest) else {
            break;
        };
        let from = after_type_value(&rest[at..]);
        match kind.as_str() {
            "text" | "input_text" | "output_text" => {
                if let Some(text) = json_string(from, "text")
                    && !text.is_empty()
                {
                    out.push(Piece::Text(text));
                }
            }
            "tool_use" => {
                if let Some(name) = json_string(from, "name") {
                    out.push(Piece::Tool(name));
                }
            }
            _ => {}
        }
        let skip = at + "\"type\"".len();
        if skip >= rest.len() {
            break;
        }
        rest = &rest[skip..];
    }
    out
}

fn next_type(line: &str) -> Option<(String, usize)> {
    let at = find_string_key(line, "\"type\"")?;
    let kind = string_at(line, "\"type\"", at)?;
    Some((kind, at))
}

fn after_type_value(from: &str) -> &str {
    let Some(after) = from.strip_prefix("\"type\"") else {
        return from;
    };
    let Some(after) = after.trim_start().strip_prefix(':') else {
        return from;
    };
    let Some(body) = after.trim_start().strip_prefix('"') else {
        return from;
    };
    match body.find('"') {
        Some(end) => &body[end + 1..],
        None => from,
    }
}

fn json_value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut rest = line;
    while let Some(at) = rest.find(&needle) {
        let after = rest[at + needle.len()..].trim_start();
        if let Some(value) = after.strip_prefix(':') {
            return Some(value.trim_start());
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    None
}

fn parse_json_string(value: &str) -> Option<String> {
    let body = value.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            unescape(&mut out, &mut chars);
            continue;
        }
        if ch == '"' {
            return Some(out);
        }
        out.push(ch);
    }
    None
}

fn bracket_slice(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (index, &byte) in bytes.iter().enumerate() {
        if in_str {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_str = false;
            }
            continue;
        }
        match byte {
            b'"' => in_str = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&value[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn read_tail_cap(path: &Path, cap: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > cap {
        file.seek(SeekFrom::End(-(cap as i64))).ok()?;
    }
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    if len > cap
        && let Some(at) = buf.find('\n')
    {
        buf.drain(..=at);
    }
    if buf.is_empty() { None } else { Some(buf) }
}

fn turn_lines(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !trimmed.starts_with('{') {
        return vec![clip(&one_line(trimmed), 4000)];
    }
    let Some(kind) = speaker(trimmed) else {
        return Vec::new();
    };
    match kind {
        "user" => {
            let mut out = Vec::new();
            if let Some(value) = json_value_after(trimmed, "content") {
                emit_pieces(&mut out, "user  ", &content_pieces(value));
            } else if let Some(text) = json_string(trimmed, "content") {
                emit_pieces(&mut out, "user  ", &[Piece::Text(text)]);
            }
            out
        }
        "assistant" => assistant_lines(trimmed),
        _ => Vec::new(),
    }
}

fn tool_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while names.len() < 32 {
        let Some(at) = rest.find("\"arguments\"") else {
            break;
        };
        let start = at.saturating_sub(96);
        if let Some(name) = json_string(&rest[start..at], "name")
            && is_ident(&name)
        {
            names.push(name);
        }
        let next = at + "\"arguments\"".len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    names
}

fn is_ident(name: &str) -> bool {
    let mut count = 0usize;
    for ch in name.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '_') {
            return false;
        }
        count += 1;
        if count > 40 {
            return false;
        }
    }
    count > 0
}

fn clip_msg(text: &str) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index == 50_000 {
            out.push('~');
            break;
        }
        out.push(ch);
    }
    out
}

fn speaker(line: &str) -> Option<&'static str> {
    if let Some(top) = leading_type(line) {
        return match top.as_str() {
            "user" => Some("user"),
            "assistant" => Some("assistant"),
            "response_item" => match json_string(line, "role").as_deref() {
                Some("user") => Some("user"),
                Some("assistant") => Some("assistant"),
                _ => None,
            },
            _ => None,
        };
    }
    match json_string(line, "role").as_deref() {
        Some("user") => Some("user"),
        Some("assistant") => Some("assistant"),
        _ => None,
    }
}

fn leading_type(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix('{')?.trim_start();
    let rest = rest.strip_prefix("\"type\"")?.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let kind = &rest[..end];
    if kind.is_empty() {
        None
    } else {
        Some(kind.to_string())
    }
}

fn one_line(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '\t' {
            out.push_str("    ");
        } else if ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn write_handoff(home: &Path, chat: &Chat, to: AgentKind) -> Result<PathBuf, String> {
    let dir = home.join(".granttap-term/handoff");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let safe = safe_id(&chat.id);
    if safe.is_empty() {
        return Err("chat id is empty".into());
    }
    let path = dir.join(format!("{safe}.txt"));
    let mut body = format!(
        "from {}\nto {}\ntitle {}\npath {}\n---\n",
        chat.agent.label(),
        to.label(),
        chat.title.replace(['\n', '\r'], " "),
        chat.path.display()
    );
    for line in transcript(&chat.path) {
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(&path, body).map_err(|err| err.to_string())?;
    Ok(path)
}

pub fn handoff_text(home: &Path, id: &str) -> Result<String, String> {
    let safe = safe_id(id);
    if safe.is_empty() {
        return Err("chat id is empty".into());
    }
    let path = home
        .join(".granttap-term/handoff")
        .join(format!("{safe}.txt"));
    fs::read_to_string(path).map_err(|_| format!("no handoff {safe}"))
}

fn safe_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn clip(line: &str, max: usize) -> String {
    let mut out = String::new();
    for (index, ch) in line.chars().enumerate() {
        if index == max {
            out.push('~');
            break;
        }
        out.push(ch);
    }
    out
}

pub fn handoff(chats: &mut [Chat], id: &str, to: AgentKind) -> bool {
    let Some(chat) = chats.iter_mut().find(|chat| chat.id == id) else {
        return false;
    };
    chat.agent = to;
    true
}

fn json_string(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = line;
    while let Some(at) = rest.find(&needle) {
        let after = rest[at + needle.len()..].trim_start();
        if let Some(value) = after.strip_prefix(':') {
            let value = value.trim_start();
            if let Some(body) = value.strip_prefix('"') {
                let mut out = String::new();
                let mut chars = body.chars();
                while let Some(ch) = chars.next() {
                    if ch == '\\' {
                        unescape(&mut out, &mut chars);
                        continue;
                    }
                    if ch == '"' {
                        return if out.is_empty() { None } else { Some(out) };
                    }
                    out.push(ch);
                }
            }
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    None
}

fn unescape(out: &mut String, chars: &mut std::str::Chars<'_>) {
    match chars.next() {
        Some('n' | 'r') => out.push('\n'),
        Some('t') => out.push_str("    "),
        Some('u') => {
            let mut hex = String::new();
            for _ in 0..4 {
                match chars.next() {
                    Some(ch) if ch.is_ascii_hexdigit() => hex.push(ch),
                    _ => return,
                }
            }
            if let Ok(code) = u32::from_str_radix(&hex, 16)
                && let Some(ch) = char::from_u32(code)
            {
                out.push(ch);
            }
        }
        Some(other) => out.push(other),
        None => {}
    }
}

fn json_all(line: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    let needle = format!("\"{key}\"");
    while out.len() < 48 {
        let Some(at) = find_string_key(rest, &needle) else {
            break;
        };
        let hidden = key == "text" && hidden_text(&rest[..at]);
        let Some(text) = string_at(rest, &needle, at) else {
            let next = at + needle.len();
            if next >= rest.len() {
                break;
            }
            rest = &rest[next..];
            continue;
        };
        if !hidden && !text.is_empty() {
            out.push(text);
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    out
}

fn find_string_key(line: &str, needle: &str) -> Option<usize> {
    let mut rest = line;
    let mut base = 0usize;
    while let Some(at) = rest.find(needle) {
        let after = rest[at + needle.len()..].trim_start();
        if let Some(stripped) = after.strip_prefix(':') {
            let value = stripped.trim_start();
            if value.starts_with('"') {
                return Some(base + at);
            }
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        base += next;
        rest = &rest[next..];
    }
    None
}

fn string_at(line: &str, needle: &str, at: usize) -> Option<String> {
    let after = line[at + needle.len()..].trim_start();
    let value = after.strip_prefix(':')?.trim_start();
    parse_json_string(value)
}

fn hidden_text(before: &str) -> bool {
    let Some(at) = before.rfind("\"type\"") else {
        return false;
    };
    let Some(after) = before[at + 6..].trim_start().strip_prefix(':') else {
        return false;
    };
    let Some(after) = after.trim_start().strip_prefix('"') else {
        return false;
    };
    let kind = after.split('"').next().unwrap_or("");
    matches!(
        kind,
        "tool_result" | "thinking" | "reasoning" | "redacted_thinking" | "tool_use"
    )
}

fn attach_codex_rollouts(index: &Path, chats: &mut [Chat]) {
    if chats.is_empty() {
        return;
    }
    let Some(root) = index.parent() else {
        return;
    };
    let mut paths = Vec::new();
    collect_jsonl(&root.join("sessions"), &mut paths, 0);
    if paths.is_empty() {
        return;
    }
    paths.sort();
    if paths.len() > 80 {
        paths.drain(0..paths.len() - 80);
    }
    for path in paths {
        let Some((id, cwd)) = rollout_head(&path) else {
            continue;
        };
        if let Some(chat) = chats.iter_mut().find(|chat| chat.id == id) {
            chat.path = path.clone();
            if !cwd.is_empty() {
                chat.cwd = cwd;
            }
            if chat.at == 0 {
                chat.at = mtime(&path);
            }
            chat.tokens = codex_tokens(&path);
        }
    }
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 || out.len() > 400 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() > 400 {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out, depth + 1);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn rollout_head(path: &Path) -> Option<(String, String)> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 4096];
    let n = file.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    let id = json_string(&text, "session_id")?;
    let cwd = json_string(&text, "cwd").unwrap_or_default();
    Some((id, cwd))
}

fn claude_title(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut used = 0usize;
    let mut carry = String::new();
    let mut ai = None;
    while used < 256 * 1024 {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        used += n;
        carry.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(at) = carry.find('\n') {
            let line = carry[..at].to_string();
            carry.drain(..=at);
            if let Some(title) = json_string(&line, "customTitle") {
                return Some(title);
            }
            if ai.is_none() {
                ai = json_string(&line, "aiTitle");
            }
        }
        if carry.len() > 256 * 1024 {
            break;
        }
    }
    json_string(&carry, "customTitle")
        .or(ai)
        .or_else(|| json_string(&carry, "aiTitle"))
}

fn mtime(path: &Path) -> i64 {
    let Ok(meta) = fs::metadata(path) else {
        return 0;
    };
    let Ok(modified) = meta.modified() else {
        return 0;
    };
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| i64::try_from(dur.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn unix_time(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        let mut n: i64 = text.parse().ok()?;
        if n > 10_000_000_000 {
            n /= 1000;
        }
        return Some(n);
    }
    let bytes = text.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let month: u32 = text[5..7].parse().ok()?;
    let day: u32 = text[8..10].parse().ok()?;
    let hour: i64 = text[11..13].parse().ok()?;
    let minute: i64 = text[14..16].parse().ok()?;
    let second: i64 = text[17..19].parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    days.checked_mul(86400)?
        .checked_add(hour * 3600 + minute * 60 + second)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u32;
    let month_shift: i32 = if month > 2 { -3 } else { 9 };
    let doy = (153 * (month as i32 + month_shift) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u32;
    Some(i64::from(era) * 146097 + i64::from(doe) - 719468)
}

fn tail_text(path: &Path, cap: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > cap {
        file.seek(SeekFrom::End(-(cap as i64))).ok()?;
    }
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    if len > cap
        && let Some(at) = buf.find('\n')
    {
        buf.drain(..=at);
    }
    if buf.is_empty() { None } else { Some(buf) }
}

fn read_capped(path: &Path, cap: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let n = file.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[..n]).into_owned())
}

fn json_u64(text: &str, key: &str, last: bool) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let mut found = None;
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let after = rest[at + needle.len()..].trim_start();
        if let Some(value) = after.strip_prefix(':') {
            let digits: String = value
                .trim_start()
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if let Ok(number) = digits.parse::<u64>()
                && !digits.is_empty()
            {
                found = Some(number);
                if !last {
                    return found;
                }
            }
        }
        let next = at + needle.len();
        if next >= rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    found
}

fn codex_tokens(path: &Path) -> Option<u64> {
    let text = tail_text(path, 64 * 1024)?;
    let mut last = None;
    let mut rest = text.as_str();
    let marker = "thread_token_usage";
    while let Some(at) = rest.find(marker) {
        let window = &rest[at..];
        let end = window.len().min(400);
        if let Some(number) = json_u64(&window[..end], "total_tokens", false) {
            last = Some(number);
        }
        rest = &rest[at + marker.len()..];
    }
    last
}

fn claude_tokens(path: &Path) -> Option<u64> {
    let text = tail_text(path, 96 * 1024)?;
    let input = json_u64(&text, "input_tokens", true)?;
    let output = json_u64(&text, "output_tokens", true).unwrap_or(0);
    Some(input.saturating_add(output))
}

fn grok_facts(path: &Path) -> (i64, String) {
    let Some(text) = read_capped(&path.join("summary.json"), 64 * 1024) else {
        return (0, String::new());
    };
    let at = ["last_active_at", "updated_at", "created_at"]
        .into_iter()
        .find_map(|key| json_string(&text, key).and_then(|value| unix_time(&value)))
        .unwrap_or(0);
    let cwd = json_string(&text, "cwd").unwrap_or_default();
    (at, cwd)
}

fn grok_tokens(path: &Path) -> Option<u64> {
    let text = read_capped(&path.join("usage.json"), 256 * 1024)?;
    if let Some(total) = json_u64(&text, "totalTokens", false) {
        return Some(total);
    }
    let input = json_u64(&text, "inputTokens", false)?;
    let output = json_u64(&text, "outputTokens", false).unwrap_or(0);
    Some(input.saturating_add(output))
}

pub fn files(dir: &Path) -> Vec<crate::model::FileRow> {
    let mut rows = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return rows,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let dir_entry = path.is_dir();
        let mut label = name;
        if dir_entry {
            label.push('/');
        }
        rows.push(crate::model::FileRow {
            icon: icon(&label, dir_entry),
            dir: dir_entry,
            name: label,
            path,
        });
    }
    rows.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.cmp(&b.name)));
    if let Some(parent) = dir.parent().filter(|parent| *parent != dir) {
        rows.insert(
            0,
            crate::model::FileRow {
                name: "..".into(),
                icon: '<',
                dir: true,
                path: parent.to_path_buf(),
            },
        );
    }
    rows
}

fn icon(name: &str, dir: bool) -> char {
    if dir {
        return '>';
    }
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("rs") => 'R',
        Some("py") => 'P',
        Some("go") => 'G',
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx") => 'S',
        Some("html" | "htm" | "vue" | "svelte") => 'H',
        Some("css") => '%',
        Some("md") => 'M',
        Some("json") => 'J',
        Some("toml") => 'T',
        Some("yml" | "yaml") => 'Y',
        Some("png" | "jpg" | "jpeg" | "gif" | "webp") => 'I',
        Some("sh" | "bash" | "zsh") => '$',
        Some("c" | "h" | "cpp" | "hpp") => 'C',
        Some("java" | "kt" | "swift") => 'K',
        Some("sql") => 'Q',
        _ => '.',
    }
}

pub fn image_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim().trim_matches('"');
    let path = PathBuf::from(trimmed);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("granttap-term-{n}"));
        fs::create_dir_all(path.join(".codex")).unwrap();
        fs::create_dir_all(path.join(".claude/projects/demo")).unwrap();
        fs::create_dir_all(path.join(".grok/sessions/work/one")).unwrap();
        path
    }

    #[test]
    fn load_reads_real_files_only() {
        let home = scratch();
        fs::write(
            home.join(".codex/session_index.jsonl"),
            "{\"id\":\"c1\",\"thread_name\":\"Ship the desk\"}\n",
        )
        .unwrap();
        fs::write(home.join(".claude/projects/demo/abc.jsonl"), "{}\n").unwrap();
        let chats = load(&home);
        assert!(chats.iter().any(|chat| chat.title == "Ship the desk"));
        assert!(
            chats
                .iter()
                .any(|chat| chat.agent == AgentKind::Claude && chat.id == "abc")
        );
        assert!(
            chats
                .iter()
                .any(|chat| chat.agent == AgentKind::Grok && chat.id == "one")
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn claude_stays_when_codex_is_long() {
        let home = scratch();
        let mut codex = String::new();
        for index in 0..90 {
            codex.push_str(&format!(
                "{{\"id\":\"c{index:02}\",\"thread_name\":\"codex {index}\"}}\n"
            ));
        }
        fs::write(home.join(".codex/session_index.jsonl"), codex).unwrap();
        fs::write(home.join(".claude/projects/demo/abc.jsonl"), "{}\n").unwrap();
        let chats = load(&home);
        assert!(
            chats
                .iter()
                .any(|chat| chat.agent == AgentKind::Claude && chat.id == "abc")
        );
        assert!(
            chats
                .iter()
                .any(|chat| chat.agent == AgentKind::Grok && chat.id == "one")
        );
        let codex_n = chats
            .iter()
            .filter(|chat| chat.agent == AgentKind::Codex)
            .count();
        assert!(codex_n <= 40, "{codex_n}");
        assert!(chats.iter().any(|chat| chat.title == "codex 89"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn handoff_changes_owner() {
        let mut chats = vec![Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
        }];
        assert!(handoff(&mut chats, "abc", AgentKind::Grok));
        assert_eq!(chats[0].agent, AgentKind::Grok);
        assert!(!handoff(&mut chats, "missing", AgentKind::Codex));
    }

    #[test]
    fn live_title_and_handoff_come_from_the_session_files() {
        let home = scratch();
        fs::write(
            home.join(".grok/sessions/work/one/summary.json"),
            "{\"generated_title\":\"Desk towers\"}\n",
        )
        .unwrap();
        fs::write(
            home.join(".grok/sessions/work/one/chat_history.jsonl"),
            "hello from the chat\n",
        )
        .unwrap();
        fs::write(
            home.join(".grok/active_sessions.json"),
            format!(
                "[{{\"session_id\":\"one\",\"pid\":{},\"cwd\":\"/tmp\"}}]",
                std::process::id()
            ),
        )
        .unwrap();
        fs::write(home.join(".claude/projects/demo/abc.jsonl"), "{}\n").unwrap();
        let chats = load(&home);
        let chat = chats.iter().find(|chat| chat.id == "one").unwrap();
        assert_eq!(chat.title, "Desk towers");
        assert!(chat.live);
        let live_at = chats.iter().position(|chat| chat.id == "one").unwrap();
        let idle_at = chats.iter().position(|chat| chat.id == "abc").unwrap();
        assert!(live_at < idle_at);
        let path = write_handoff(&home, chat, AgentKind::Claude).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("from Grok"));
        assert!(text.contains("to Claude"));
        assert!(text.contains("hello from the chat"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn cli_argv_marks_the_matching_agent_only() {
        let chat = |agent, id: &str| Chat {
            agent,
            id: id.into(),
            title: id.into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
        };
        let mut chats = vec![
            chat(AgentKind::Claude, "01234567-abcd"),
            chat(AgentKind::Claude, "99999999-zzzz"),
            chat(AgentKind::Codex, "01234567-abcd"),
        ];
        let ps = "Claude Helper (Renderer) --session 01234567-abcd\n/opt/bin/claude --resume 01234567-abcd\n";
        mark_cli_live(&mut chats, ps);
        assert!(chats[0].live);
        assert!(!chats[1].live);
        assert!(!chats[2].live);
    }

    #[test]
    fn codex_rollout_keeps_cwd_and_the_user_line() {
        let home = scratch();
        let id = "01234567-89ab-cdef-0123-456789abcdef";
        fs::write(
            home.join(".codex/session_index.jsonl"),
            format!("{{\"id\":\"{id}\",\"thread_name\":\"Ship the desk\"}}\n"),
        )
        .unwrap();
        let day = home.join(".codex/sessions/2026/11/08");
        fs::create_dir_all(&day).unwrap();
        let rollout = day.join("rollout.jsonl");
        fs::write(
            &rollout,
            format!(
                "{{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{id}\",\"cwd\":\"/tmp/night\"}}}}\n{{\"timestamp\":\"t\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"hello desk\"}}]}}}}\n{{\"timestamp\":\"t\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"developer\",\"content\":[{{\"type\":\"input_text\",\"text\":\"hidden\"}}]}}}}\n"
            ),
        )
        .unwrap();
        let chats = load(&home);
        let chat = chats.iter().find(|chat| chat.id == id).unwrap();
        assert_eq!(chat.cwd, "/tmp/night");
        assert_eq!(chat.path, rollout);
        let tail = transcript(&chat.path);
        let joined = tail.join("\n");
        assert!(joined.contains("user  hello desk"), "{joined}");
        assert!(!joined.contains("hidden"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn claude_title_comes_from_ai_title_and_noise_stays_out() {
        let home = scratch();
        let session = "01234567-89ab-cdef-0123-456789abcdef";
        fs::write(
            home.join(format!(".claude/projects/demo/{session}.jsonl")),
            "{\"type\":\"queue-operation\",\"operation\":\"enqueue\"}\n{\"type\":\"ai-title\",\"aiTitle\":\"Name the desk\"}\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"keep this\"}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"secret\"},{\"type\":\"text\",\"text\":\"answer\"}]}}\n{\"parentUuid\":null,\"message\":{\"role\":\"user\",\"content\":\"later line\"},\"type\":\"user\"}\n",
        )
        .unwrap();
        let chats = load(&home);
        let chat = chats.iter().find(|chat| chat.id == session).unwrap();
        assert_eq!(chat.title, "Name the desk");
        let joined = transcript(&chat.path).join("\n");
        assert!(joined.contains("user  keep this"), "{joined}");
        assert!(joined.contains("assistant  answer"), "{joined}");
        assert_eq!(joined.matches("answer").count(), 1, "{joined}");
        assert!(joined.contains("user  later line"), "{joined}");
        assert!(!joined.contains("enqueue"), "{joined}");
        assert!(!joined.contains("secret"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn unix_time_reads_iso_and_millis() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time("1970-01-02T00:00:00.123456Z"), Some(86_400));
        assert_eq!(unix_time("86400"), Some(86_400));
        assert_eq!(unix_time("10000000001"), Some(10_000_000));
        assert_eq!(unix_time("not a date"), None);
    }

    #[test]
    fn custom_title_beats_ai_title_and_tokens_stay_real() {
        let home = scratch();
        let session = "01234567-89ab-cdef-0123-456789abcdef";
        fs::write(
            home.join(format!(".claude/projects/demo/{session}.jsonl")),
            "{\"type\":\"ai-title\",\"aiTitle\":\"AI name\"}\n{\"usage\":{\"cache_read_input_tokens\":500000,\"input_tokens\":1,\"output_tokens\":1}}\n{\"type\":\"custom-title\",\"customTitle\":\"Custom name\"}\n{\"usage\":{\"input_tokens\":100,\"output_tokens\":25,\"output_tokens_details\":{\"reasoning_tokens\":9}}}\n",
        )
        .unwrap();
        fs::write(
            home.join(".grok/sessions/work/one/summary.json"),
            "{\"generated_title\":\"Desk towers\",\"last_active_at\":\"1970-01-03T00:00:00Z\",\"cwd\":\"/tmp/night\"}\n",
        )
        .unwrap();
        fs::write(
            home.join(".grok/sessions/work/one/usage.json"),
            "{\"session\":{\"inputTokens\":3,\"outputTokens\":4,\"totalTokens\":12500}}\n",
        )
        .unwrap();
        let id = "fedcba98-7654-3210-fedc-ba9876543210";
        fs::write(
            home.join(".codex/session_index.jsonl"),
            format!("{{\"id\":\"{id}\",\"thread_name\":\"Ship\",\"updated_at\":\"1970-01-02T00:00:00Z\"}}\n"),
        )
        .unwrap();
        let day = home.join(".codex/sessions/2026/11/08");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("rollout.jsonl"),
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{id}\",\"cwd\":\"/tmp/night\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"total_tokens\":1}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"thread_token_usage\",\"total_tokens\":40}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"thread_token_usage\",\"total_tokens\":80}}}}\n{{\"type\":\"token_usage_record\",\"payload\":{{\"total_tokens\":9999}}}}\n"
            ),
        )
        .unwrap();
        let chats = load(&home);
        let claude = chats.iter().find(|chat| chat.id == session).unwrap();
        assert_eq!(claude.title, "Custom name");
        assert_eq!(claude.tokens, Some(125));
        let grok = chats.iter().find(|chat| chat.id == "one").unwrap();
        assert_eq!(grok.title, "Desk towers");
        assert_eq!(grok.cwd, "/tmp/night");
        assert_eq!(grok.at, 172_800);
        assert_eq!(grok.tokens, Some(12_500));
        let codex = chats.iter().find(|chat| chat.id == id).unwrap();
        assert_eq!(codex.at, 86_400);
        assert_eq!(codex.tokens, Some(80));
        assert_eq!(codex.cwd, "/tmp/night");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn missing_usage_stays_unknown() {
        let home = scratch();
        fs::write(
            home.join(".codex/session_index.jsonl"),
            "{\"id\":\"c1\",\"thread_name\":\"Ship the desk\"}\n",
        )
        .unwrap();
        let chats = load(&home);
        let chat = chats
            .iter()
            .find(|chat| chat.title == "Ship the desk")
            .unwrap();
        assert_eq!(chat.tokens, None);
        assert_eq!(chat.at, 0);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn grok_history_shows_the_user_the_reply_and_the_tool() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        fs::write(
            &path,
            "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"keep the desk\"}]}\n{\"type\":\"assistant\",\"content\":\"\",\"tool_calls\":[{\"id\":\"1\",\"name\":\"read_file\",\"arguments\":{\"path\":\"a\",\"text\":\"SECRETARGS\"}}]}\n{\"type\":\"tool_result\",\"tool_call_id\":\"1\",\"content\":\"SECRETARGS\"}\n{\"type\":\"assistant\",\"content\":\"the desk is open\"}\n",
        )
        .unwrap();
        let joined = transcript_full(&path).join("\n");
        assert!(joined.contains("user  keep the desk"), "{joined}");
        assert_eq!(joined.matches("keep the desk").count(), 1, "{joined}");
        assert!(joined.contains("tool  read_file"), "{joined}");
        assert!(joined.contains("assistant  the desk is open"), "{joined}");
        assert_eq!(joined.matches("the desk is open").count(), 1, "{joined}");
        assert!(!joined.contains("SECRETARGS"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn paragraphs_stay_on_their_own_lines() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        fs::write(
            &path,
            "{\"type\":\"user\",\"content\":\"first paragraph\\n\\nsecond paragraph\"}\n{\"type\":\"assistant\",\"content\":\"answer one\\nanswer two\"}\n",
        )
        .unwrap();
        let lines = transcript_full(&path);
        let joined = lines.join("\n");
        assert!(
            lines.iter().any(|line| line == "user  first paragraph"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "second paragraph"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "assistant  answer one"),
            "{joined}"
        );
        assert!(lines.iter().any(|line| line == "answer two"), "{joined}");
        assert!(!joined.contains("first paragraph second"), "{joined}");
        assert!(!joined.contains("answer one answer"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn indented_lines_and_later_text_parts_stay() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        let mut parts = String::new();
        for index in 0..8 {
            if index > 0 {
                parts.push(',');
            }
            parts.push_str(&format!("{{\"type\":\"text\",\"text\":\"part{index}\"}}"));
        }
        let body = "b".repeat(5000);
        fs::write(
            &path,
            format!(
                "{{\"type\":\"user\",\"content\":\"run this\\n\\n    let desk = 1;\\n    let other = 2;\"}}\n{{\"type\":\"assistant\",\"content\":[{parts}]}}\n{{\"type\":\"user\",\"content\":\"{body}\"}}\n{{\"type\":\"user\",\"content\":\"\",\"tool_calls\":[{{\"name\":\"read_file\",\"arguments\":{{\"text\":\"SECRETARGS\"}}}}]}}\n"
            ),
        )
        .unwrap();
        let lines = transcript_full(&path);
        let joined = lines.join("\n");
        assert!(
            lines.iter().any(|line| line == "user  run this"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "    let desk = 1;"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "    let other = 2;"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "assistant  part0"),
            "{joined}"
        );
        assert!(lines.iter().any(|line| line == "part7"), "{joined}");
        assert!(joined.contains(&body), "long line was cut");
        assert!(!joined.contains("SECRETARGS"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn markdown_lines_and_tool_uses_stay_in_order() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        let mut tools = String::new();
        for index in 0..9 {
            if index > 0 {
                tools.push(',');
            }
            tools.push_str(&format!(
                "{{\"type\":\"tool_use\",\"name\":\"tool{index}\",\"input\":{{\"command\":\"SECRETARGS\"}}}}"
            ));
        }
        fs::write(
            &path,
            format!(
                "{{\"type\":\"assistant\",\"content\":\"intro\\n\\n# Title\\n\\n```\\n    let desk = 1;\\n```\\n\\n- item\"}}\n{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"before\"}},{tools},{{\"type\":\"text\",\"text\":\"after\"}}]}}}}\n{{\"type\":\"user\",\"content\":\"\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442\"}}\n"
            ),
        )
        .unwrap();
        let lines = transcript_full(&path);
        let joined = lines.join("\n");
        assert!(lines.iter().any(|line| line == "assistant  intro"), "{joined}");
        assert!(lines.iter().any(|line| line == "# Title"), "{joined}");
        assert!(lines.iter().any(|line| line == "```"), "{joined}");
        assert!(lines.iter().any(|line| line == "    let desk = 1;"), "{joined}");
        assert!(lines.iter().any(|line| line == "- item"), "{joined}");
        assert!(lines.iter().any(|line| line == "assistant  before"), "{joined}");
        assert!(lines.iter().any(|line| line == "assistant  after"), "{joined}");
        assert!(lines.iter().any(|line| line == "tool  tool0"), "{joined}");
        assert!(lines.iter().any(|line| line == "tool  tool8"), "{joined}");
        let before = lines.iter().position(|line| line == "assistant  before").unwrap();
        let tool = lines.iter().position(|line| line == "tool  tool0").unwrap();
        let after = lines.iter().position(|line| line == "assistant  after").unwrap();
        assert!(before < tool && tool < after, "{joined}");
        assert!(lines.iter().any(|line| line == "user  Привет"), "{joined}");
        assert!(!joined.contains("SECRETARGS"), "{joined}");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn icons_follow_the_file_type() {
        assert_eq!(icon("main.rs", false), 'R');
        assert_eq!(icon("main.py", false), 'P');
        assert_eq!(icon("main.go", false), 'G');
        assert_eq!(icon("app.tsx", false), 'S');
        assert_eq!(icon("page.html", false), 'H');
        assert_eq!(icon("src", true), '>');
    }
}
