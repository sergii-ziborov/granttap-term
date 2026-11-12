use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::model::{AgentKind, Chat};

const CAP: usize = 80;
const EACH: usize = 10;

pub fn load(home: &Path) -> Vec<Chat> {
    let mut codex = Vec::new();
    load_codex(&home.join(".codex/session_index.jsonl"), &mut codex);
    let mut claude = Vec::new();
    load_claude(&home.join(".claude/projects"), &mut claude);
    let mut grok = Vec::new();
    load_grok(&home.join(".grok/sessions"), &mut grok);
    attach_codex_rollouts(&home.join(".codex/session_index.jsonl"), &mut codex);
    let mut chats = codex;
    chats.extend(claude);
    chats.extend(grok);
    mark_live(home, &mut chats);
    for chat in &mut chats {
        if let Some(path) = resolve_cwd(&chat.cwd) {
            chat.cwd = path.display().to_string();
        }
    }
    keep_each(&mut chats, EACH);
    chats
}

/// Ten newest chats for each agent. Equal times keep the later record.
fn keep_each(chats: &mut Vec<Chat>, each: usize) {
    let mut pick = Vec::new();
    for agent in [AgentKind::Grok, AgentKind::Claude, AgentKind::Codex] {
        let mut slots: Vec<usize> = chats
            .iter()
            .enumerate()
            .filter(|(_, chat)| chat.agent == agent)
            .map(|(index, _)| index)
            .collect();
        slots.sort_by(|&left, &right| {
            chats[right]
                .at
                .cmp(&chats[left].at)
                .then(usize::from(chats[right].live).cmp(&usize::from(chats[left].live)))
                .then(right.cmp(&left))
        });
        slots.truncate(each);
        pick.extend(slots);
    }
    pick.sort_unstable();
    let mut next = 0usize;
    let mut index = 0usize;
    chats.retain(|_| {
        let keep = next < pick.len() && pick[next] == index;
        if keep {
            next += 1;
        }
        index += 1;
        keep
    });
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
                window: grok_window(&path),
            },
        );
    }
    mark_cli_live(chats, &cli_commands());
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.live));
}

fn cli_commands() -> String {
    let Ok(output) = process_list() else {
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

#[cfg(unix)]
fn process_list() -> std::io::Result<std::process::Output> {
    std::process::Command::new("ps")
        .args(["-ax", "-o", "command="])
        .output()
}

#[cfg(windows)]
fn process_list() -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process | ForEach-Object { if ($_.CommandLine) { $_.CommandLine } }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

fn is_cli(line: &str, name: &str) -> bool {
    let token = first_token(line);
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    if base == name {
        return true;
    }
    let lower = base.to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".cmd"))
        .or_else(|| lower.strip_suffix(".bat"))
        == Some(name)
}

fn first_token(line: &str) -> &str {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('"') {
        return rest.split('"').next().unwrap_or("");
    }
    line.split_whitespace().next().unwrap_or("")
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
            window: None,
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
            window: None,
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
    let mut session_file = false;
    let mut saw_file = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let child = entry.path();
        if child.is_dir() {
            children.push(child);
            continue;
        }
        saw_file = true;
        // prompt_history.jsonl sits on the encoded cwd folder. The chat is the
        // child directory that holds summary.json or chat_history.jsonl.
        if name == "summary.json" || name == "chat_history.jsonl" {
            session_file = true;
        }
    }
    if depth > 0 && (session_file || (children.is_empty() && !saw_file)) {
        let summary = path.join("summary.json");
        let at = if summary.is_file() {
            mtime(&summary)
        } else {
            mtime(path)
        };
        out.push((at, path.to_path_buf()));
        return;
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
        window: grok_window(path),
    })
}

pub fn transcript(path: &Path) -> Vec<String> {
    turns(path, 64 * 1024, 24)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn transcript_full(path: &Path) -> Vec<String> {
    turns(path, 8 * 1024 * 1024, 6000)
}

/// One screen of a chat, read from the end of the file. `from` is the byte
/// where those lines start. Zero means the file has nothing older.
pub struct HistPage {
    pub lines: Vec<String>,
    pub from: u64,
    pub path: PathBuf,
}

const PAGE_BYTES: u64 = 48 * 1024;
const PAGE_MAX: u64 = 256 * 1024;

pub fn transcript_tail(path: &Path) -> HistPage {
    let file = history_file(path);
    let len = fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
    read_page(&file, len)
}

/// The page that ends where `end` begins. Used when the reader scrolls up.
pub fn transcript_before(path: &Path, end: u64) -> HistPage {
    read_page(path, end)
}

fn history_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        let history = path.join("chat_history.jsonl");
        if history.is_file() {
            history
        } else {
            path.join("summary.json")
        }
    } else {
        path.to_path_buf()
    }
}

fn read_page(file: &Path, end: u64) -> HistPage {
    let len = fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);
    let end = end.min(len);
    let empty = HistPage {
        lines: vec!["No transcript.".into()],
        from: 0,
        path: file.to_path_buf(),
    };
    if end == 0 {
        return empty;
    }
    let mut window = PAGE_BYTES.min(end);
    loop {
        let start = end - window;
        let Some(buf) = read_span(file, start, end) else {
            return empty;
        };
        let mut skip = 0usize;
        if start > 0 {
            while skip < buf.len() && skip < 3 && (buf[skip] & 0xC0) == 0x80 {
                skip += 1;
            }
        }
        let data = &buf[skip..];
        let rel = if start == 0 {
            0usize
        } else {
            match data.iter().position(|byte| *byte == b'\n') {
                Some(at) => at + 1,
                None => {
                    // The window sits inside one record. A long last line used
                    // to become "No transcript." and the turn above it vanished.
                    if window >= PAGE_MAX || window >= end {
                        return long_record_page(file, end);
                    }
                    window = (window + PAGE_BYTES).min(PAGE_MAX).min(end);
                    continue;
                }
            }
        };
        let body = String::from_utf8_lossy(&data[rel..]);
        if body.trim().is_empty() {
            if window >= PAGE_MAX || window >= end {
                return long_record_page(file, end);
            }
            window = (window + PAGE_BYTES).min(PAGE_MAX).min(end);
            continue;
        }
        let mut lines = Vec::new();
        for line in body.lines() {
            push_turn(&mut lines, turn_lines(line));
        }
        // Tool dumps and reasoning hide the latest user line when they fill
        // the window. Step back until a real turn is in the page.
        let from = start + skip as u64 + rel as u64;
        if !has_talk(&lines) && from > 0 && window < LONG_BACK && window < end {
            window = (window + PAGE_BYTES).min(LONG_BACK).min(end);
            continue;
        }
        if lines.is_empty() {
            if from > 0 {
                return long_record_page(file, end);
            }
            lines.push("No transcript.".into());
        } else {
            lines = thin_tools(lines, 80);
        }
        return HistPage {
            lines,
            from,
            path: file.to_path_buf(),
        };
    }
}

const LONG_BACK: u64 = 1024 * 1024;

fn has_talk(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|line| line.starts_with("user  ") || line.starts_with("assistant  "))
}

/// The last record is longer than a page. Keep the turns just before it, and
/// a clip of that record, instead of replacing the pane with "No transcript."
fn long_record_page(file: &Path, end: u64) -> HistPage {
    let empty = HistPage {
        lines: vec!["No transcript.".into()],
        from: 0,
        path: file.to_path_buf(),
    };
    let back = LONG_BACK.min(end);
    if back == 0 {
        return empty;
    }
    let origin = end - back;
    let Some(buf) = read_span(file, origin, end) else {
        return empty;
    };
    let mut skip = 0usize;
    if origin > 0 {
        while skip < buf.len() && skip < 3 && (buf[skip] & 0xC0) == 0x80 {
            skip += 1;
        }
    }
    let data = &buf[skip..];
    let abs = origin + skip as u64;
    let mut last_nl = None;
    let mut prev_nl = None;
    for (index, byte) in data.iter().enumerate() {
        if *byte == b'\n' {
            prev_nl = last_nl;
            last_nl = Some(index);
        }
    }
    let record_at = if data.last() == Some(&b'\n') {
        prev_nl.map(|index| index + 1).unwrap_or(0)
    } else {
        last_nl.map(|index| index + 1).unwrap_or(0)
    };
    let mut prior = &data[..record_at];
    let mut from = abs;
    if abs > 0 {
        if let Some(cut) = prior.iter().position(|byte| *byte == b'\n') {
            from = abs + cut as u64 + 1;
            prior = &prior[cut + 1..];
        } else if !prior.is_empty() {
            prior = &[];
            from = abs + record_at as u64;
        }
    }
    let mut lines = Vec::new();
    let prior_text = String::from_utf8_lossy(prior);
    for line in prior_text.lines() {
        push_turn(&mut lines, turn_lines(line));
    }
    let mut record = &data[record_at..];
    if record.last() == Some(&b'\n') {
        record = &record[..record.len() - 1];
    }
    if record.last() == Some(&b'\r') {
        record = &record[..record.len() - 1];
    }
    let take = utf8_cut(record, PAGE_BYTES as usize);
    let record_text = String::from_utf8_lossy(&record[..take]);
    let record_text = record_text.trim_end();
    let kind = leading_type(record_text).unwrap_or_default();
    let hidden = matches!(kind.as_str(), "tool_result" | "reasoning" | "system");
    if !hidden {
        let parsed = turn_lines(record_text);
        if !parsed.is_empty() {
            push_turn(&mut lines, parsed);
        } else if let Some(text) = loose_content(record_text) {
            let prefix = if kind == "user" { "user  " } else { "assistant  " };
            push_turn(&mut lines, message_lines(prefix, &text));
        } else if !has_talk(&lines) {
            let shown = clip(&one_line(record_text), 4000);
            if !shown.is_empty() {
                lines.push(shown);
            }
        }
    }
    if lines.is_empty() {
        return empty;
    }
    HistPage {
        lines: thin_tools(lines, 80),
        from,
        path: file.to_path_buf(),
    }
}

/// Text from a record that was cut before its closing quote.
fn loose_content(raw: &str) -> Option<String> {
    for key in ["\"text\":\"", "\"content\":\""] {
        let Some(at) = raw.find(key) else {
            continue;
        };
        let rest = &raw[at + key.len()..];
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                unescape(&mut out, &mut chars);
                continue;
            }
            if ch == '"' {
                break;
            }
            if out.chars().count() >= 4000 {
                out.push('~');
                break;
            }
            out.push(ch);
        }
        if !out.trim().is_empty() {
            return Some(out);
        }
    }
    None
}

fn read_span(path: &Path, start: u64, end: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let take = usize::try_from(end.saturating_sub(start)).ok()?;
    let mut buf = vec![0u8; take];
    let mut filled = 0usize;
    while filled < take {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    buf.truncate(filled);
    Some(buf)
}

fn turns(path: &Path, cap: u64, keep: usize) -> Vec<String> {
    let file = history_file(path);
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
    let cleaned;
    let text = if prefix.starts_with("user") {
        cleaned = visible_user_text(text);
        cleaned.as_str()
    } else {
        text
    };
    if text.trim().is_empty() {
        return Vec::new();
    }
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
    Think,
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
            Piece::Think => {
                flush_text(out, prefix, &buf);
                buf.clear();
                push_think(out);
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

fn push_think(out: &mut Vec<String>) {
    if out.last().map(String::as_str) == Some("think") {
        return;
    }
    if out.last().is_some_and(|line| !line.is_empty()) {
        out.push(String::new());
    }
    out.push("think".into());
}

/// Name, then a file tail and `+N -N` when the call actually carries them.
fn tool_label(name: &str, from: &str) -> String {
    let Some(path) = tool_path(from) else {
        return name.to_string();
    };
    match tool_delta(from) {
        Some((add, del)) => format!("{name}  {path}  +{add}  -{del}"),
        None => format!("{name}  {path}"),
    }
}

fn tool_path(from: &str) -> Option<String> {
    for key in ["path", "file_path", "filePath", "target_file"] {
        let Some(value) = json_string(from, key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > 120
            || value.chars().any(|ch| ch == '\n' || ch == '\r' || ch == '\t')
        {
            continue;
        }
        let tail = value.rsplit(['/', '\\']).next().unwrap_or(value);
        if tail.is_empty() || tail.chars().count() > 60 {
            continue;
        }
        return Some(tail.to_string());
    }
    None
}

fn tool_delta(from: &str) -> Option<(u64, u64)> {
    let add = json_u64(from, "additions", false).or_else(|| json_u64(from, "added", false));
    let del = json_u64(from, "deletions", false).or_else(|| json_u64(from, "removed", false));
    if add.is_some() || del.is_some() {
        return Some((add.unwrap_or(0), del.unwrap_or(0)));
    }
    None
}

fn push_tool(out: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if name.is_empty() || !name.chars().next().is_some_and(|ch| is_ident_char(ch)) {
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
            "tool_use" | "function_call" | "custom_tool_call" | "tool_call" => {
                if let Some(name) = call_name(from) {
                    out.push(Piece::Tool(tool_label(&name, from)));
                }
            }
            "CommandExecution" | "ReadFile" | "FileChange" | "list_files" => {
                out.push(Piece::Tool(tool_label(&kind, from)));
            }
            "thinking" | "reasoning" | "redacted_thinking" => out.push(Piece::Think),
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
    if harness_record(trimmed) {
        return Vec::new();
    }
    match speaker(trimmed) {
        Some("user") => {
            let mut out = Vec::new();
            if let Some(value) = json_value_after(trimmed, "content") {
                emit_pieces(&mut out, "user  ", &content_pieces(value));
            } else if let Some(text) = json_string(trimmed, "content") {
                emit_pieces(&mut out, "user  ", &[Piece::Text(text)]);
            }
            out
        }
        Some("assistant") => assistant_lines(trimmed),
        _ => spine(trimmed),
    }
}

fn spine(line: &str) -> Vec<String> {
    if let Some("developer" | "system" | "tool") = json_string(line, "role").as_deref() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = line;
    let mut saw_text = false;
    while out.len() < 64 {
        let Some((kind, at)) = next_type(rest) else {
            break;
        };
        let from = after_type_value(&rest[at..]);
        match kind.as_str() {
            "tool_use" | "function_call" | "custom_tool_call" | "tool_call" => {
                if let Some(name) = call_name(from) {
                    push_tool(&mut out, &tool_label(&name, from));
                }
            }
            "CommandExecution" | "ReadFile" | "FileChange" | "list_files" => {
                push_tool(&mut out, &tool_label(&kind, from));
            }
            "thinking" | "reasoning" | "redacted_thinking" => push_think(&mut out),
            "AgentMessage" | "output_text" | "input_text" | "text" | "Text" if !saw_text => {
                if let Some(text) = message_text(from)
                    && !text.is_empty()
                {
                    let prefix = if kind == "input_text" {
                        "user  "
                    } else {
                        "assistant  "
                    };
                    emit_pieces(&mut out, prefix, &[Piece::Text(text)]);
                    saw_text = true;
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

fn call_name(from: &str) -> Option<String> {
    let end = ["\"arguments\"", "\"input\"", "\"command\"", "\"output\""]
        .iter()
        .filter_map(|needle| from.find(needle))
        .min()
        .unwrap_or(from.len());
    let name = json_string(&from[..end], "name")?;
    if is_ident(&name) { Some(name) } else { None }
}

fn message_text(from: &str) -> Option<String> {
    let end = [
        "\"arguments\"",
        "\"input\"",
        "\"command\"",
        "\"output\"",
        "\"encrypted_content\"",
    ]
    .iter()
    .filter_map(|needle| from.find(needle))
    .min()
    .unwrap_or(from.len());
    let slice = &from[..end];
    if let Some(text) = json_string(slice, "text")
        && !text.is_empty()
    {
        return Some(text);
    }
    let value = json_value_after(slice, "content")?;
    if !value.starts_with('"') {
        return None;
    }
    let text = parse_json_string(value)?;
    if text.is_empty() { None } else { Some(text) }
}

fn tool_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while names.len() < 32 {
        let Some(at) = rest.find("\"arguments\"") else {
            break;
        };
        // 96 bytes can land inside a multibyte character such as "ь".
        let start = char_floor(rest, at.saturating_sub(96));
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

fn char_floor(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
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

pub fn worked(path: &Path) -> Option<String> {
    let text = read_capped(&path.join("summary.json"), 64 * 1024)?;
    let start = json_string(&text, "created_at").and_then(|value| unix_time(&value))?;
    let end = ["last_active_at", "updated_at"]
        .into_iter()
        .find_map(|key| json_string(&text, key).and_then(|value| unix_time(&value)))
        .unwrap_or(start);
    Some(span_label(end.saturating_sub(start).max(0) as u64))
}

fn span_label(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {rest}m")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveUpdate {
    Agent(String),
    Tool(String),
    User(i64, String),
    Pulse(i64, Option<u64>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnMark {
    pub at: i64,
    pub head: String,
    pub done: i64,
    pub tokens: Option<u64>,
}

/// New rows from a Grok `updates.jsonl` after `offset`. An unfinished last
/// line stays unconsumed so the next read can finish it.
pub fn read_updates(path: &Path, offset: u64) -> (u64, Vec<LiveUpdate>) {
    let Ok(mut file) = File::open(path) else {
        return (offset, Vec::new());
    };
    let Ok(len) = file.metadata().map(|meta| meta.len()) else {
        return (offset, Vec::new());
    };
    if offset > len {
        return (len, Vec::new());
    }
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return (offset, Vec::new());
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return (offset, Vec::new());
    }
    let mut events = Vec::new();
    let mut consumed = offset;
    let bytes = buf.as_bytes();
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let line = &buf[start..index];
        start = index + 1;
        consumed = offset + start as u64;
        if let Some(update) = live_update(line) {
            events.push(update);
        }
        if let Some(pulse) = live_pulse(line) {
            events.push(pulse);
        }
    }
    let _ = bytes;
    (consumed, events)
}

fn live_update(line: &str) -> Option<LiveUpdate> {
    let kind = json_key(line, "sessionUpdate")?;
    match kind.as_str() {
        "agent_message_chunk" => {
            let text = json_key(line, "text")?;
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(LiveUpdate::Agent(text.to_string()))
            }
        }
        "tool_call" => {
            let title = json_key(line, "title")?;
            let title = title.trim();
            if title.is_empty() {
                None
            } else {
                let title: String = title.chars().take(80).collect();
                Some(LiveUpdate::Tool(title))
            }
        }
        "user_message_chunk" => {
            let text = json_key(line, "text").unwrap_or_default();
            let at = json_u64(head_str(line, 80), "timestamp", false)
                .and_then(|n| i64::try_from(n).ok())
                .unwrap_or(0);
            Some(LiveUpdate::User(at, user_pane_text(&text)))
        }
        _ => None,
    }
}

fn live_pulse(line: &str) -> Option<LiveUpdate> {
    let window = head_str(line, 500);
    let kind = json_string(window, "sessionUpdate")?;
    if !matches!(
        kind.as_str(),
        "agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update"
    ) {
        return None;
    }
    let at = json_u64(window, "timestamp", false)
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(0);
    let tokens = tail_u64(line, "totalTokens");
    if at == 0 && tokens.is_none() {
        return None;
    }
    Some(LiveUpdate::Pulse(at, tokens))
}

fn tail_u64(line: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let at = line.rfind(&needle)?;
    json_u64(&line[at..], key, false)
}

fn flatten_head(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(48).collect()
}

/// What the open pane should show for a user chunk. The typed line, not the
/// harness around it. A long line is clipped; turn marks still use the short head.
fn user_pane_text(raw: &str) -> String {
    clip(&visible_user_text(raw), 4000)
}

/// History and the live seat share this. A `<user_query>` shows its inner text.
/// A record that is only a reminder, user_info, or rules is not a chat line.
fn visible_user_text(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if tagged(raw, "user_query").is_none() && raw.starts_with("This session is being continued") {
        return String::new();
    }
    let text = tagged(raw, "user_query").unwrap_or_else(|| raw.to_string());
    let text = strip_harness(&text);
    let mut out = String::new();
    for line in text.split('\n') {
        if is_lone_tag(line.trim()) {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out.trim().to_string()
}

fn harness_names() -> &'static [&'static str] {
    &[
        "system-reminder",
        "user_info",
        "user_rules",
        "rules",
        "memory-context",
        "local-command-stdout",
        "local-command-caveat",
        "command-name",
        "bash-input",
        "bash-stdout",
    ]
}

fn strip_harness(text: &str) -> String {
    let mut out = text.to_string();
    for name in harness_names() {
        out = remove_marked(&out, &format!("<{name}"), ">", &format!("</{name}>"));
        out = remove_marked(
            &out,
            &format!("&lt;{name}"),
            "&gt;",
            &format!("&lt;/{name}&gt;"),
        );
    }
    out
}

/// Drops a closed harness block. An unclosed tag drops the rest only when
/// nothing real sits in front of it, so a mention of the word stays.
fn remove_marked(text: &str, open_key: &str, gt: &str, close: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find(open_key) else {
            out.push_str(rest);
            break;
        };
        let after = &rest[start + open_key.len()..];
        let boundary = after.starts_with(gt) || after.starts_with(|ch: char| ch.is_whitespace());
        if !boundary {
            let mut chars = rest[start..].chars();
            let Some(ch) = chars.next() else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..start]);
            out.push(ch);
            rest = chars.as_str();
            continue;
        }
        let Some(gt_at) = after.find(gt) else {
            if rest[..start].trim().is_empty() {
                break;
            }
            out.push_str(&rest[..start]);
            out.push_str(open_key);
            rest = after;
            continue;
        };
        let body = &after[gt_at + gt.len()..];
        if let Some(end) = body.find(close) {
            out.push_str(&rest[..start]);
            rest = &body[end + close.len()..];
            continue;
        }
        if rest[..start].trim().is_empty() {
            break;
        }
        out.push_str(&rest[..start]);
        out.push_str(open_key);
        rest = after;
    }
    out
}

fn is_lone_tag(line: &str) -> bool {
    let line = line.trim();
    if matches!(
        line,
        "<user_query>" | "</user_query>" | "&lt;user_query&gt;" | "&lt;/user_query&gt;"
    ) {
        return true;
    }
    harness_names().iter().any(|name| {
        line == format!("<{name}>")
            || line == format!("</{name}>")
            || line == format!("&lt;{name}&gt;")
            || line == format!("&lt;/{name}&gt;")
    })
}

fn harness_record(line: &str) -> bool {
    if leading_type(line).as_deref() == Some("system") {
        return true;
    }
    if matches!(
        json_string(line, "role").as_deref(),
        Some("system" | "developer")
    ) {
        return true;
    }
    json_string(line, "synthetic_reason").as_deref() == Some("system_reminder")
}

fn tagged(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    let inner = rest[..end].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

fn text_head(window: &str) -> String {
    if let Some(text) = json_string(window, "text") {
        return flatten_head(&text);
    }
    let Some((_, rest)) = window.split_once("\"text\":\"") else {
        return String::new();
    };
    flatten_head(rest)
}

fn head_str(text: &str, max: usize) -> &str {
    let end = utf8_prefix(text.as_bytes(), max).len();
    &text[..end]
}

fn utf8_prefix(buf: &[u8], max: usize) -> &[u8] {
    let end = utf8_cut(buf, max.min(buf.len()));
    &buf[..end]
}

fn utf8_cut(buf: &[u8], index: usize) -> usize {
    let mut end = index.min(buf.len());
    if end < buf.len() {
        while end > 0 && (buf[end] & 0xC0) == 0x80 {
            end -= 1;
        }
    }
    end
}

fn tail_u64_buf(buf: &[u8], key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let needle = needle.as_bytes();
    if needle.is_empty() || buf.len() < needle.len() {
        return None;
    }
    let region_from = buf.len().saturating_sub(2048);
    let region = &buf[region_from..];
    let rel = region.windows(needle.len()).rposition(|window| window == needle)?;
    let at = region_from + rel;
    let end = utf8_cut(buf, (at + 48).min(buf.len()));
    if end < at {
        return None;
    }
    let text = String::from_utf8_lossy(&buf[at..end]);
    json_u64(&text, key, false)
}

/// One pass over a Grok `updates.jsonl`. Each user line opens a turn.
/// `done == 0` means the latest turn is still being written.
/// A long log is read from the tail, so opening a chat does not scan the whole file.
const MARK_CAP: u64 = 1024 * 1024;

pub fn turn_marks(path: &Path) -> Vec<TurnMark> {
    let file_path = if path.is_dir() {
        path.join("updates.jsonl")
    } else {
        return Vec::new();
    };
    let Ok(mut file) = File::open(&file_path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let tailed = len > MARK_CAP;
    if tailed && file.seek(SeekFrom::End(-(MARK_CAP as i64))).is_err() {
        return Vec::new();
    }
    let mut reader = BufReader::new(file);
    if tailed {
        let mut skip = Vec::new();
        if reader.read_until(b'\n', &mut skip).is_err() {
            return Vec::new();
        }
    }
    let mut buf = Vec::new();
    let mut marks = Vec::new();
    let mut open_at: i64 = 0;
    let mut head = String::new();
    let mut last_at: i64 = 0;
    let mut last_tokens: Option<u64> = None;
    loop {
        buf.clear();
        let Ok(n) = reader.read_until(b'\n', &mut buf) else {
            break;
        };
        if n == 0 {
            break;
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if !contains_slice(&buf, b"sessionUpdate") {
            continue;
        }
        let window = String::from_utf8_lossy(utf8_prefix(&buf, 600));
        let Some(kind) = json_string(&window, "sessionUpdate") else {
            continue;
        };
        let at = json_u64(&window, "timestamp", false)
            .and_then(|n| i64::try_from(n).ok())
            .unwrap_or(0);
        match kind.as_str() {
            "user_message_chunk" => {
                if open_at > 0 {
                    marks.push(TurnMark {
                        at: open_at,
                        head: std::mem::take(&mut head),
                        done: last_at,
                        tokens: last_tokens,
                    });
                }
                let text_window = String::from_utf8_lossy(utf8_prefix(&buf, 4096));
                open_at = at;
                head = text_head(&text_window);
                last_at = at;
                last_tokens = None;
            }
            "agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update" => {
                if at > 0 {
                    last_at = at;
                }
                if let Some(n) = tail_u64_buf(&buf, "totalTokens") {
                    last_tokens = Some(n);
                }
            }
            _ => {}
        }
    }
    if open_at > 0 {
        let running = turn_running(&file_path);
        marks.push(TurnMark {
            at: open_at,
            head,
            done: if running { 0 } else { last_at },
            tokens: last_tokens,
        });
    }
    marks
}

/// How long a turn may sit without a new `updates.jsonl` write and still be
/// in progress. A think pause is longer than a flush.
const WORK_QUIET_SECS: u64 = 20;

pub fn updates_fresh(path: &Path) -> bool {
    updates_age(path).is_some_and(|age| age < WORK_QUIET_SECS)
}

/// The latest turn is still going: the log was touched recently, or a tool
/// call has not come back completed.
pub fn turn_running(path: &Path) -> bool {
    if updates_fresh(path) {
        return true;
    }
    tool_open(path)
}

pub fn tool_open(path: &Path) -> bool {
    let Some(file_path) = updates_file(path) else {
        return false;
    };
    let Ok(mut file) = File::open(&file_path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|meta| meta.len()) else {
        return false;
    };
    if len == 0 {
        return false;
    }
    let take = len.min(128 * 1024);
    let from_start = take == len;
    if file.seek(SeekFrom::End(-(take as i64))).is_err() {
        return false;
    }
    let mut buf = vec![0u8; take as usize];
    let Ok(read) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(read);
    let text = String::from_utf8_lossy(&buf);
    // A mid-file read starts on a cut line. A whole-file read does not.
    let body = if from_start {
        text.as_ref()
    } else {
        text.find('\n').map(|index| &text[index + 1..]).unwrap_or("")
    };
    let mut open = Vec::new();
    for line in body.split('\n') {
        let Some(kind) = session_kind(line) else {
            continue;
        };
        match kind {
            "user_message_chunk" => open.clear(),
            "tool_call" => note_tool(&mut open, tool_call_id(line)),
            "tool_call_update" => {
                let Some(id) = tool_call_id(line) else {
                    continue;
                };
                if tool_finished(line) {
                    open.retain(|have| have != id);
                } else {
                    note_tool(&mut open, Some(id));
                }
            }
            _ => {}
        }
    }
    !open.is_empty()
}

fn note_tool(open: &mut Vec<String>, id: Option<&str>) {
    let Some(id) = id else {
        return;
    };
    if open.iter().any(|have| have == id) {
        return;
    }
    open.push(id.to_string());
}

/// `status` is the key that follows `toolCallId` on a finished update.
/// A later copy of that word inside tool output does not close the call.
fn tool_finished(line: &str) -> bool {
    let Some(rest) = line.split_once("\"toolCallId\":\"").map(|(_, rest)| rest) else {
        return false;
    };
    let Some(after) = rest.split_once('"').map(|(_, after)| after) else {
        return false;
    };
    after.starts_with(",\"status\":\"completed\"")
}

fn session_kind(line: &str) -> Option<&str> {
    let rest = line.split_once("\"sessionUpdate\":\"")?.1;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn tool_call_id(line: &str) -> Option<&str> {
    let rest = line.split_once("\"toolCallId\":\"")?.1;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn updates_file(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        let file = path.join("updates.jsonl");
        return file.is_file().then_some(file);
    }
    if path
        .file_name()
        .is_some_and(|name| name == "updates.jsonl")
        && path.is_file()
    {
        return Some(path.to_path_buf());
    }
    None
}

pub fn updates_age(path: &Path) -> Option<u64> {
    let file = updates_file(path)?;
    let modified = std::fs::metadata(file).and_then(|meta| meta.modified()).ok()?;
    Some(modified.elapsed().map(|age| age.as_secs()).unwrap_or(0))
}

fn contains_slice(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|window| window == needle)
}

fn json_key(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let rest = line.split_once(&needle)?.1;
    unescape_json(rest)
}

fn unescape_json(rest: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let Some(code) = u32::from_str_radix(&hex, 16).ok() else {
                        continue;
                    };
                    if let Some(decoded) = char::from_u32(code) {
                        out.push(decoded);
                    }
                }
                Some(other) => out.push(other),
                None => return None,
            }
            continue;
        }
        if ch == '"' {
            return Some(out);
        }
        out.push(ch);
    }
    None
}

pub fn transcript_stamp(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(history_file(path))
        .and_then(|meta| meta.modified())
        .ok()
}

pub fn resolve_cwd(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let direct = PathBuf::from(raw);
    if direct.is_absolute() && direct.is_dir() {
        return Some(direct);
    }
    if !looks_encoded(raw) {
        return None;
    }
    if let Some(found) = cached_encoded(raw) {
        return found;
    }
    let found = decode_encoded(raw);
    remember_encoded(raw, found.clone());
    found
}

pub fn walk_encoded(root: &Path, rest: &str) -> Option<PathBuf> {
    walk_at(root, rest)
}

fn looks_encoded(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(drive) = chars.next() else {
        return false;
    };
    drive.is_ascii_alphabetic() && chars.as_str().starts_with("--")
}

fn cached_encoded(raw: &str) -> Option<Option<PathBuf>> {
    let Ok(rows) = encoded_cache().lock() else {
        return None;
    };
    rows.iter()
        .find(|(key, _)| key == raw)
        .map(|(_, path)| path.clone())
}

fn remember_encoded(raw: &str, path: Option<PathBuf>) {
    if let Ok(mut rows) = encoded_cache().lock() {
        if rows.iter().any(|(key, _)| key == raw) {
            return;
        }
        rows.push((raw.to_string(), path));
        if rows.len() > 64 {
            rows.remove(0);
        }
    }
}

fn encoded_cache() -> &'static std::sync::Mutex<Vec<(String, Option<PathBuf>)>> {
    use std::sync::Mutex;
    static CACHE: std::sync::OnceLock<Mutex<Vec<(String, Option<PathBuf>)>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn decode_encoded(raw: &str) -> Option<PathBuf> {
    let mut chars = raw.chars();
    let drive = chars.next()?;
    let rest = chars.as_str().strip_prefix("--")?;
    if rest.is_empty() {
        return None;
    }
    let upper = drive.to_ascii_uppercase();
    let lower = drive.to_ascii_lowercase();
    let mut letters = vec![upper];
    if lower != upper {
        letters.push(lower);
    }
    for letter in letters {
        let root = PathBuf::from(format!("{letter}:\\"));
        if let Some(found) = walk_encoded(&root, rest) {
            return Some(found);
        }
    }
    None
}

fn walk_at(dir: &Path, rest: &str) -> Option<PathBuf> {
    if rest.is_empty() {
        return dir.is_dir().then(|| dir.to_path_buf());
    }
    let mut choices = Vec::new();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        let encoded = encode_component(&name);
        if encoded.is_empty() {
            continue;
        }
        if let Some(next) = take_component(rest, &encoded) {
            choices.push((encoded.len(), name.to_string(), next));
        }
    }
    choices.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    for (_, name, next) in choices {
        if let Some(found) = walk_at(&dir.join(name), next) {
            return Some(found);
        }
    }
    None
}

fn take_component<'a>(rest: &'a str, encoded: &str) -> Option<&'a str> {
    let after = rest.strip_prefix(encoded)?;
    if after.is_empty() {
        Some(after)
    } else {
        after.strip_prefix('-')
    }
}

fn encode_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out
}

/// Lines typed in the desk that are not in the history yet.
/// A line already stored in the chat stays where it was sent. It is not glued
/// to the bottom of the open page again.
pub fn pending_outbox(home: &Path, id: &str, chat: &Path) -> Vec<String> {
    let stored = read_outbox(home, id);
    if stored.is_empty() {
        return Vec::new();
    }
    let history = history_file(chat);
    let len = fs::metadata(&history).map(|meta| meta.len()).unwrap_or(0);
    let mut kept = Vec::new();
    let mut shown = Vec::new();
    for (offset, text) in stored {
        let landed = history_has(&history, offset.unwrap_or(0), len, &text);
        if landed {
            continue;
        }
        let line = match offset {
            Some(offset) => format!("{offset}\t{text}"),
            None => text.clone(),
        };
        kept.push(line);
        shown.push(format!("user  {text}"));
    }
    let path = outbox_path(home, id);
    let next = if kept.is_empty() {
        String::new()
    } else {
        format!("{}\n", kept.join("\n"))
    };
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current != next {
        if next.is_empty() {
            let _ = fs::remove_file(&path);
        } else if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
            let _ = fs::write(&path, next);
        }
    }
    shown
}

pub fn append_outbox(
    home: &Path,
    id: &str,
    history: Option<&Path>,
    text: &str,
) -> std::io::Result<()> {
    let safe = safe_id(id);
    if safe.is_empty() {
        return Err(std::io::Error::other("chat id is empty"));
    }
    let path = outbox_path(home, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let offset = history
        .map(history_file)
        .and_then(|file| fs::metadata(file).ok())
        .map(|meta| meta.len())
        .unwrap_or(0);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    use std::io::Write;
    writeln!(file, "{offset}\t{text}")?;
    Ok(())
}

fn outbox_path(home: &Path, id: &str) -> PathBuf {
    home.join(".granttap-term/outbox")
        .join(format!("{}.txt", safe_id(id)))
}

/// `(Some(offset), text)` is a send recorded after `offset` bytes of history.
/// `None` is an older line with no mark. Both count as landed once `text`
/// shows up in the history at or after that byte.
fn read_outbox(home: &Path, id: &str) -> Vec<(Option<u64>, String)> {
    let Ok(text) = fs::read_to_string(outbox_path(home, id)) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            if let Some((mark, rest)) = line.split_once('\t')
                && let Ok(offset) = mark.parse::<u64>()
            {
                (Some(offset), rest.to_string())
            } else {
                (None, line.to_string())
            }
        })
        .collect()
}

fn history_has(file: &Path, offset: u64, len: u64, needle: &str) -> bool {
    if needle.is_empty() || offset >= len {
        return false;
    }
    const CAP: u64 = 8 * 1024 * 1024;
    let end = len.min(offset.saturating_add(CAP));
    let Some(buf) = read_span(file, offset, end) else {
        return false;
    };
    let text = String::from_utf8_lossy(&buf);
    if text.contains(needle) {
        return true;
    }
    if end < len {
        let tail_from = len.saturating_sub(1024 * 1024).max(end);
        if let Some(tail) = read_span(file, tail_from, len) {
            return String::from_utf8_lossy(&tail).contains(needle);
        }
    }
    false
}

/// Context filled in this chat. The lifetime sum in usage.json is a different number.
pub fn context_tokens(path: &Path) -> Option<u64> {
    context_pair(path).0
}

pub fn context_window(path: &Path) -> Option<u64> {
    context_pair(path).1
}

fn grok_window(path: &Path) -> Option<u64> {
    context_window(path)
}

/// Used and window from one read of signals.json. Neither number is invented.
pub fn context_pair(path: &Path) -> (Option<u64>, Option<u64>) {
    let Some(text) = read_capped(&path.join("signals.json"), 16 * 1024) else {
        return (None, None);
    };
    let used = json_u64(&text, "contextTokensUsed", false);
    let window = json_u64(&text, "contextWindowTokens", false).filter(|n| *n > 0);
    (used, window)
}

/// Fields the session file actually has. Weekly quota is not among them.
pub struct ContextFacts {
    pub used: Option<u64>,
    pub window: Option<u64>,
    pub model: String,
    pub turns: Option<u64>,
    pub compactions: Option<u64>,
    pub before: Option<u64>,
    pub tools: Option<u64>,
}

pub fn context_facts(path: &Path) -> ContextFacts {
    let text = read_capped(&path.join("signals.json"), 16 * 1024).unwrap_or_default();
    let model = json_string(&text, "primaryModelId").unwrap_or_default();
    ContextFacts {
        used: json_u64(&text, "contextTokensUsed", false),
        window: json_u64(&text, "contextWindowTokens", false).filter(|n| *n > 0),
        model,
        turns: json_u64(&text, "turnCount", false),
        compactions: json_u64(&text, "compactionCount", false),
        before: json_u64(&text, "totalTokensBeforeCompaction", false),
        tools: json_u64(&text, "toolCallCount", false),
    }
}

/// The context modal's split. The three buckets add up to `used`.
/// Tool, skill, workflow, and MCP figures are already inside those buckets.
#[derive(Clone, Debug, Default)]
pub struct ContextSplit {
    pub system: u64,
    pub messages: u64,
    pub reasoning: u64,
    pub tools_tokens: u64,
    pub tools: u64,
    pub skills_tokens: u64,
    pub skills: u64,
    pub flows_tokens: u64,
    pub flows: u64,
    pub mcp_tokens: u64,
    pub mcps: u64,
}

pub fn context_split(path: &Path, used: u64) -> ContextSplit {
    let session = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let system = file_tokens(&session.join("system_prompt.txt"));
    let (messages, reasoning) = history_tokens(&session.join("chat_history.jsonl"), used);
    let (system, messages, reasoning) = fit_parts(used, system, messages, reasoning);
    let (tools_tokens, tools) = tool_catalog(&session.join("tool_definitions.json"));
    let grok = grok_home(&session);
    let (skills_tokens, skills) = skill_catalog(grok.as_deref());
    let (flows_tokens, flows) = flow_catalog(grok.as_deref());
    let (mcp_tokens, mcps) = mcp_catalog(grok.as_deref());
    ContextSplit {
        system,
        messages,
        reasoning,
        tools_tokens: tools_tokens.min(used.max(1)),
        tools,
        skills_tokens: skills_tokens.min(used.max(1)),
        skills,
        flows_tokens: flows_tokens.min(used.max(1)),
        flows,
        mcp_tokens: mcp_tokens.min(used.max(1)),
        mcps,
    }
}

/// One decimal under 100k, a rounded integer k at 100k and above, raw below 1k.
pub fn grok_token_face(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    if n >= 100_000 {
        return format!("{}k", n.saturating_add(500) / 1000);
    }
    let tenths = n.saturating_add(50) / 100;
    format!("{}.{}k", tenths / 10, tenths % 10)
}

/// Scale the three measured buckets so they add up to `used`.
/// A shortfall is overhead and stays on the reasoning bucket.
pub fn fit_parts(used: u64, system: u64, messages: u64, reasoning: u64) -> (u64, u64, u64) {
    if used == 0 {
        return (0, 0, 0);
    }
    let sum = system.saturating_add(messages).saturating_add(reasoning);
    if sum == 0 {
        return (0, used, 0);
    }
    if sum <= used {
        return (system, messages, reasoning.saturating_add(used - sum));
    }
    let system = used.saturating_mul(system) / sum;
    let messages = used.saturating_mul(messages) / sum;
    let reasoning = used.saturating_sub(system).saturating_sub(messages);
    (system, messages, reasoning)
}

fn file_tokens(path: &Path) -> u64 {
    let len = fs::metadata(path).ok().map(|meta| meta.len()).unwrap_or(0);
    len.saturating_add(3) / 4
}

fn history_tokens(path: &Path, budget: u64) -> (u64, u64) {
    let Some(text) = read_tail(path, 1_500_000) else {
        return (0, 0);
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let mut messages = 0u64;
    let mut reasoning = 0u64;
    let mut started = false;
    for line in lines.iter().rev() {
        if line.len() < 8 {
            continue;
        }
        let tokens = (line.len() as u64).saturating_add(3) / 4;
        let reason = line.contains("\"type\":\"reasoning\"") || line.contains("\"type\": \"reasoning\"");
        let room = budget.saturating_sub(messages.saturating_add(reasoning));
        if tokens > room && started {
            if reason {
                reasoning = reasoning.saturating_add(room);
            } else {
                messages = messages.saturating_add(room);
            }
            break;
        }
        if reason {
            reasoning = reasoning.saturating_add(tokens);
        } else {
            messages = messages.saturating_add(tokens);
        }
        started = true;
        if messages.saturating_add(reasoning) >= budget {
            break;
        }
    }
    (messages, reasoning)
}

fn read_tail(path: &Path, cap: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.seek(SeekFrom::End(0)).ok()?;
    let take = (cap as u64).min(len);
    file.seek(SeekFrom::Start(len - take)).ok()?;
    let mut buf = vec![0u8; take as usize];
    file.read_exact(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if take < len
        && let Some(at) = text.find('\n')
    {
        text.drain(..=at);
    }
    if text.is_empty() { None } else { Some(text) }
}

fn tool_catalog(path: &Path) -> (u64, u64) {
    let tokens = file_tokens(path);
    let Some(text) = read_capped(path, 256 * 1024) else {
        return (0, 0);
    };
    let mut count = 0u64;
    let mut rest = text.as_str();
    while let Some(at) = rest.find("\"name\"") {
        count += 1;
        rest = &rest[at + 6..];
        if count > 64 {
            break;
        }
    }
    (tokens, count)
}

fn grok_home(session: &Path) -> Option<PathBuf> {
    let home = session.parent()?.parent()?.parent()?;
    if home.join("bundled").is_dir() || home.join("config.toml").is_file() {
        Some(home.to_path_buf())
    } else {
        None
    }
}

fn skill_catalog(home: Option<&Path>) -> (u64, u64) {
    let Some(home) = home else {
        return (0, 0);
    };
    let mut tokens = 0u64;
    let mut count = 0u64;
    for root in [home.join("bundled/skills"), home.join("skills")] {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let skill = entry.path().join("SKILL.md");
            if !skill.is_file() {
                continue;
            }
            count += 1;
            tokens = tokens.saturating_add(file_tokens(&skill).min(200));
            if count >= 80 {
                return (tokens, count);
            }
        }
    }
    (tokens, count)
}

fn flow_catalog(home: Option<&Path>) -> (u64, u64) {
    let Some(home) = home else {
        return (0, 0);
    };
    let mut tokens = 0u64;
    let mut count = 0u64;
    for root in [home.join("bundled/workflows"), home.join("workflows")] {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rhai") {
                continue;
            }
            count += 1;
            tokens = tokens.saturating_add(flow_blurb(&path));
            if count >= 32 {
                return (tokens, count);
            }
        }
    }
    (tokens, count)
}

fn flow_blurb(path: &Path) -> u64 {
    let Some(text) = read_capped(path, 1200) else {
        return 0;
    };
    let Some(at) = text.find("description:") else {
        return file_tokens(path).min(80);
    };
    let rest = text[at + "description:".len()..].trim_start();
    let body = rest.trim_start_matches('"');
    let end = body.find('"').unwrap_or(body.len().min(180));
    (end as u64).saturating_add(3) / 4
}

fn mcp_catalog(home: Option<&Path>) -> (u64, u64) {
    let Some(home) = home else {
        return (0, 0);
    };
    let Some(text) = read_capped(&home.join("config.toml"), 64 * 1024) else {
        return (0, 0);
    };
    let mut count = 0u64;
    let mut rest = text.as_str();
    while let Some(at) = rest.find("[mcp_servers.") {
        count += 1;
        rest = &rest[at + 13..];
        if count > 32 {
            break;
        }
    }
    (count.saturating_mul(48), count)
}

fn grok_tokens(path: &Path) -> Option<u64> {
    if let Some(used) = context_tokens(path) {
        return Some(used);
    }
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
    fn grok_token_face_matches_the_modal_rounding() {
        assert_eq!(grok_token_face(186), "186");
        assert_eq!(grok_token_face(2_000), "2.0k");
        assert_eq!(grok_token_face(32_900), "32.9k");
        assert_eq!(grok_token_face(135_614), "136k");
        assert_eq!(grok_token_face(256_000), "256k");
    }

    #[test]
    fn fit_parts_adds_the_gap_to_reasoning_and_never_passes_used() {
        assert_eq!(fit_parts(100, 10, 40, 10), (10, 40, 50));
        assert_eq!(fit_parts(50, 0, 0, 0), (0, 50, 0));
        let (system, messages, reasoning) = fit_parts(100, 80, 80, 40);
        assert_eq!(system + messages + reasoning, 100);
        assert_eq!((system, messages, reasoning), (40, 40, 20));
    }

    #[test]
    fn context_split_scales_a_short_history_up_to_used() {
        let home = scratch();
        let session = home.join(".grok/sessions/work/one");
        std::fs::write(session.join("system_prompt.txt"), "abcd").unwrap();
        std::fs::write(
            session.join("chat_history.jsonl"),
            "{\"type\":\"message\",\"text\":\"hello desk\"}\n{\"type\":\"reasoning\",\"text\":\"why\"}\n",
        )
        .unwrap();
        std::fs::write(
            session.join("tool_definitions.json"),
            "{\"name\":\"read_file\"}\n{\"name\":\"grep\"}\n",
        )
        .unwrap();
        let split = context_split(&session, 100);
        assert_eq!(split.system + split.messages + split.reasoning, 100);
        assert!(split.system > 0, "{split:?}");
        assert!(split.messages > 0, "{split:?}");
        assert!(split.reasoning > split.messages, "{split:?}");
        assert_eq!(split.tools, 2);
        assert!(split.tools_tokens > 0 && split.tools_tokens <= 100, "{split:?}");
        assert_eq!(split.skills, 0);
        assert_eq!(split.flows, 0);
        assert_eq!(split.mcps, 0);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_user_chunk_keeps_the_typed_line() {
        let short = live_update(
            r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"какой итог"}}}"#,
        );
        assert_eq!(
            short,
            Some(LiveUpdate::User(1000, "какой итог".into()))
        );
        let long_text = "a".repeat(80);
        let long = live_update(&format!(
            r#"{{"timestamp":2,"update":{{"sessionUpdate":"user_message_chunk","content":{{"text":"{long_text}"}}}}}}"#
        ));
        match long {
            Some(LiveUpdate::User(2, text)) => assert!(text.chars().count() > 48, "{text}"),
            other => panic!("{other:?}"),
        }
        let wrapped = live_update(
            r#"{"timestamp":3,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"before <user_query>\nкакой итог\n</user_query> after"}}}"#,
        );
        assert_eq!(
            wrapped,
            Some(LiveUpdate::User(3, "какой итог".into()))
        );
        let noise = live_update(
            r#"{"timestamp":4,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"<system-reminder>\nkeep out\n</system-reminder>"}}}"#,
        );
        assert_eq!(noise, Some(LiveUpdate::User(4, String::new())));
    }

    #[test]
    fn updates_return_the_agent_reply_and_keep_a_partial_line() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        std::fs::write(
            &path,
            "{\"update\":{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{\"type\":\"text\",\"text\":\"hidden\"}}}\n{\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Extra High (active)\"}}}\n{\"update\":{\"sessionUpdate\":\"tool_call\",\"title\":\"read_file\"}}\n{\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"still",
        )
        .unwrap();
        let (at, events) = read_updates(&path, 0);
        assert!(at > 0 && at < std::fs::metadata(&path).unwrap().len());
        assert_eq!(
            events,
            vec![
                LiveUpdate::Agent("Extra High (active)".into()),
                LiveUpdate::Tool("read_file".into()),
            ]
        );
        let (next, more) = read_updates(&path, at);
        assert!(more.is_empty(), "{more:?}");
        assert_eq!(next, at);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn turn_marks_keep_the_sent_time_and_the_spend() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        std::fs::write(
            &path,
            "{\"timestamp\":1000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\"hello desk from the seat\"}}}\n{\"timestamp\":1010,\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"text\":\"ok\"}},\"_meta\":{\"totalTokens\":2400}}\n{\"timestamp\":2000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\"second question from the seat\"}}}\n{\"timestamp\":2010,\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"text\":\"done\"}},\"_meta\":{\"totalTokens\":3000}}\n",
        )
        .unwrap();
        let marks = turn_marks(&home);
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert_eq!(marks[0].at, 1000);
        assert_eq!(marks[0].done, 1010);
        assert_eq!(marks[0].tokens, Some(2400));
        assert!(marks[0].head.contains("hello desk"), "{marks:?}");
        assert_eq!(marks[1].at, 2000);
        assert_eq!(marks[1].done, 0, "{marks:?}");
        assert_eq!(marks[1].tokens, Some(3000));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn turn_marks_read_the_tail_of_a_long_log() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        let mut body = String::new();
        body.push_str(
            "{\"timestamp\":1000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\"early desk from the seat\"}}}\n",
        );
        body.push_str(
            "{\"timestamp\":1010,\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"text\":\"old\"}},\"_meta\":{\"totalTokens\":10}}\n",
        );
        while body.len() < 1024 * 1024 + 80_000 {
            body.push_str(
                "{\"timestamp\":1500,\"update\":{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{\"text\":\"xxxxxxxx\"}}}\n",
            );
        }
        body.push_str(
            "{\"timestamp\":2000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\"late desk from the seat\"}}}\n",
        );
        body.push_str(
            "{\"timestamp\":2010,\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"text\":\"new\"}},\"_meta\":{\"totalTokens\":80}}\n",
        );
        std::fs::write(&path, body).unwrap();
        let started = std::time::Instant::now();
        let marks = turn_marks(&home);
        assert!(
            started.elapsed().as_millis() < 1500,
            "tail scan took {:?}",
            started.elapsed()
        );
        assert!(
            marks.iter().any(|mark| mark.head.contains("late desk")),
            "{marks:?}"
        );
        assert!(
            marks.iter().all(|mark| !mark.head.contains("early desk")),
            "{marks:?}"
        );
        assert_eq!(marks.last().map(|mark| mark.tokens), Some(Some(80)));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_long_history_opens_on_its_last_page() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        let mut body = String::new();
        for index in 0..400 {
            body.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"paragraph {index:03} {}\"}}\n",
                "word ".repeat(30)
            ));
        }
        std::fs::write(&path, &body).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 48 * 1024);
        let tail = transcript_tail(&path);
        let joined = tail.lines.join("\n");
        assert!(tail.from > 0, "{}", tail.from);
        assert!(joined.contains("paragraph 399"), "{joined}");
        assert!(!joined.contains("paragraph 000"), "{joined}");
        let older = transcript_before(&tail.path, tail.from);
        let older_text = older.lines.join("\n");
        assert!(older.from < tail.from, "{} {}", older.from, tail.from);
        assert!(!older_text.contains("paragraph 399"), "{older_text}");
        assert!(older_text.contains("paragraph "), "{older_text}");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_long_last_line_keeps_the_turn_above_it() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        let mut body = String::from("{\"type\":\"user\",\"content\":\"keep-earlier\"}\n");
        body.push_str("{\"type\":\"user\",\"content\":\"tail-sentence-visible ");
        body.push_str(&"word ".repeat(80_000));
        body.push_str("\"}\n");
        assert!(body.len() > 256 * 1024, "{}", body.len());
        std::fs::write(&path, &body).unwrap();
        let tail = transcript_tail(&path);
        let joined = tail.lines.join("\n");
        assert!(
            joined.contains("keep-earlier"),
            "earlier turn dropped: {joined}"
        );
        assert!(
            joined.contains("tail-sentence-visible"),
            "long line dropped: {joined}"
        );
        assert_ne!(tail.lines, vec!["No transcript.".to_string()]);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn an_old_desk_line_is_not_stuck_under_the_tail() {
        let home = scratch();
        let chat = home.join("chat");
        std::fs::create_dir_all(&chat).unwrap();
        let mut body = String::new();
        body.push_str("{\"type\":\"user\",\"content\":\"say hi\"}\n");
        body.push_str("{\"type\":\"user\",\"content\":\"/effort\"}\n");
        body.push_str("{\"type\":\"user\",\"content\":\"какой итог\"}\n");
        for index in 0..400 {
            body.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"paragraph {index:03} {}\"}}\n",
                "word ".repeat(30)
            ));
        }
        body.push_str("{\"type\":\"user\",\"content\":\"latest desk line\"}\n");
        let history = chat.join("chat_history.jsonl");
        std::fs::write(&history, &body).unwrap();
        assert!(std::fs::metadata(&history).unwrap().len() > 48 * 1024);
        let len = std::fs::metadata(&history).unwrap().len();
        let out = home.join(".granttap-term/outbox");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("desk.txt"),
            format!("say hi\n/effort\nкакой итог\n{len}\tstill pending\n"),
        )
        .unwrap();
        let pending = pending_outbox(&home, "desk", &chat);
        assert_eq!(pending, vec!["user  still pending".to_string()], "{pending:?}");
        let tail = transcript_tail(&chat);
        let joined = tail.lines.join("\n");
        assert!(joined.contains("latest desk line"), "{joined}");
        assert!(!joined.contains("say hi"), "{joined}");
        let _ = std::fs::remove_dir_all(home);
    }

    fn age(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let when = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(secs))
            .unwrap();
        file.set_modified(when).unwrap();
    }

    #[test]
    fn a_quiet_open_tool_keeps_the_turn_running() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"hello desk from the seat"}}}"#,
                "\n",
                r#"{"timestamp":1010,"update":{"sessionUpdate":"tool_call","toolCallId":"call-1","title":"run"}}"#,
                "\n",
                r#"{"timestamp":1020,"update":{"sessionUpdate":"tool_call_update","toolCallId":"call-1","kind":"execute","content":[{"text":"see \"status\":\"completed\" later"}]}}"#,
                "\n",
            ),
        )
        .unwrap();
        age(&path, 40);
        assert!(tool_open(&home));
        assert!(turn_running(&home));
        let marks = turn_marks(&home);
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].done, 0, "{marks:?}");
        std::fs::write(
            &path,
            "{\"timestamp\":1010,\"update\":{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"call-1\",\"title\":\"run\"}}\n",
        )
        .unwrap();
        age(&path, 40);
        assert!(tool_open(&path));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_finished_tool_closes_the_turn_once_the_log_is_quiet() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"hello desk from the seat"}}}"#,
                "\n",
                r#"{"timestamp":1010,"update":{"sessionUpdate":"tool_call","toolCallId":"call-1","title":"run"}}"#,
                "\n",
                r#"{"timestamp":1040,"update":{"sessionUpdate":"tool_call_update","toolCallId":"call-1","status":"completed"}}"#,
                "\n",
            ),
        )
        .unwrap();
        age(&path, 40);
        assert!(!tool_open(&home));
        assert!(!turn_running(&home));
        let marks = turn_marks(&home);
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].at, 1000);
        assert_eq!(marks[0].done, 1040, "{marks:?}");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_tool_update_keeps_the_turn_open_after_the_call_scrolls_away() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        let mut body = String::new();
        body.push_str(
            "{\"timestamp\":1000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\"hello desk from the seat\"}}}\n",
        );
        body.push_str(
            "{\"timestamp\":1010,\"update\":{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"call-1\",\"title\":\"run\"}}\n",
        );
        while body.len() < 200_000 {
            body.push_str(
                "{\"timestamp\":1011,\"update\":{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{\"text\":\"xxxxxxxx\"}}}\n",
            );
        }
        body.push_str(
            "{\"timestamp\":1020,\"update\":{\"sessionUpdate\":\"tool_call_update\",\"toolCallId\":\"call-1\",\"kind\":\"execute\"}}\n",
        );
        std::fs::write(&path, body).unwrap();
        age(&path, 40);
        assert!(path.metadata().unwrap().len() > 128 * 1024);
        assert!(tool_open(&home));
        assert!(turn_running(&home));
        let marks = turn_marks(&home);
        assert_eq!(marks.last().map(|mark| mark.done), Some(0), "{marks:?}");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn turn_marks_survive_a_cyrillic_cut() {
        let home = scratch();
        let path = home.join("updates.jsonl");
        let mut user_prefix =
            "{\"timestamp\":1000,\"update\":{\"sessionUpdate\":\"user_message_chunk\",\"content\":{\"text\":\""
                .to_string();
        if (600 - user_prefix.len()) % 2 == 0 {
            user_prefix.push(' ');
        }
        let mut user = user_prefix;
        user.push_str(&"й".repeat(3000));
        user.push_str("\"}}}\n");
        assert!(
            !user.is_char_boundary(600),
            "byte 600 must split й"
        );
        let mut agent_prefix =
            "{\"timestamp\":1010,\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"text\":\""
                .to_string();
        if (500 - agent_prefix.len()) % 2 == 0 {
            agent_prefix.push(' ');
        }
        let mut agent = agent_prefix;
        agent.push_str(&"й".repeat(1500));
        agent.push_str("\"}},\"_meta\":{\"totalTokens\":2400}}\n");
        assert!(!agent.is_char_boundary(500), "byte 500 must split й");
        let early = format!("x{}", "й".repeat(50));
        assert!(!early.is_char_boundary(80));
        assert!(std::str::from_utf8(utf8_prefix(early.as_bytes(), 80)).is_ok());
        std::fs::write(&path, format!("{user}{agent}")).unwrap();
        assert!(live_update(user.trim_end()).is_some());
        assert!(live_pulse(agent.trim_end()).is_some());
        let marks = turn_marks(&home);
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].at, 1000);
        assert_eq!(marks[0].tokens, Some(2400));
        assert!(marks[0].head.contains('й'), "{marks:?}");
        let _ = std::fs::remove_dir_all(home);
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
        assert_eq!(codex_n, 10, "{codex_n}");
        assert!(chats.iter().any(|chat| chat.title == "codex 89"));
        assert!(chats.iter().all(|chat| chat.title != "codex 00"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn each_agent_keeps_its_ten_newest() {
        let home = scratch();
        let mut codex = String::new();
        for index in 0..12 {
            codex.push_str(&format!(
                "{{\"id\":\"c{index:02}\",\"thread_name\":\"codex {index}\",\"updated_at\":\"{at}\"}}\n",
                at = 1_000 + index
            ));
        }
        fs::write(home.join(".codex/session_index.jsonl"), codex).unwrap();
        fs::write(home.join(".claude/projects/demo/abc.jsonl"), "{}\n").unwrap();
        for index in 0..12 {
            let dir = home.join(format!(".grok/sessions/work/g{index:02}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("summary.json"),
                format!(
                    "{{\"updated_at\":\"{at}\",\"generated_title\":\"grok {index}\"}}\n",
                    at = 2_000 + index
                ),
            )
            .unwrap();
        }
        let chats = load(&home);
        let codex_n = chats
            .iter()
            .filter(|chat| chat.agent == AgentKind::Codex)
            .count();
        let grok_n = chats
            .iter()
            .filter(|chat| chat.agent == AgentKind::Grok)
            .count();
        assert_eq!(codex_n, 10);
        assert_eq!(grok_n, 10);
        assert!(chats.iter().any(|chat| chat.id == "abc"));
        assert!(chats.iter().any(|chat| chat.title == "codex 11"));
        assert!(chats.iter().all(|chat| chat.title != "codex 00"));
        assert!(chats.iter().any(|chat| chat.title == "grok 11"));
        assert!(chats.iter().all(|chat| chat.title != "grok 0"));
        assert!(chats.len() <= 30, "{}", chats.len());
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
            window: None,
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
            window: None,
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
        assert!(is_cli(
            r"C:\Users\a\AppData\Roaming\npm\claude.cmd --resume 01234567-abcd",
            "claude"
        ));
        assert!(is_cli(
            r#""C:\Program Files\codex\codex.exe" --resume 01234567-abcd"#,
            "codex"
        ));
        assert!(!is_cli(
            "Claude Helper (Renderer) --session 01234567-abcd",
            "claude"
        ));
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
        fs::write(
            home.join(".grok/sessions/work/one/signals.json"),
            "{\"contextTokensUsed\":203543,\"contextWindowTokens\":256000,\"totalTokensBeforeCompaction\":10741332}\n",
        )
        .unwrap();
        fs::write(
            home.join(".grok/sessions/work/one/usage.json"),
            "{\"session\":{\"totalTokens\":227175484}}\n",
        )
        .unwrap();
        let chats = load(&home);
        let grok = chats.iter().find(|chat| chat.id == "one").unwrap();
        assert_eq!(grok.tokens, Some(203_543), "context, not the lifetime sum");
        let codex = chats.iter().find(|chat| chat.id == id).unwrap();
        assert_eq!(codex.at, 86_400);
        assert_eq!(codex.tokens, Some(80));
        assert_eq!(codex.cwd, "/tmp/night");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn grok_child_session_keeps_its_title_and_project_cwd() {
        let home = scratch();
        let encoded = "C%3A%5CUsers%5Csergii%5Cgranttap-term";
        let parent = home.join(".grok/sessions").join(encoded);
        let dir = parent.join("01a197e2-d3b2-7a81-9298-eec7243a81bd");
        fs::create_dir_all(&dir).unwrap();
        fs::write(parent.join("prompt_history.jsonl"), "{}\n").unwrap();
        fs::write(
            dir.join("summary.json"),
            r#"{"generated_title":"GrantTap Term Windows","last_active_at":"1970-01-03T00:00:00Z","info":{"cwd":"C:\\Users\\sergii\\granttap-term"}}"#,
        )
        .unwrap();
        let chats = load(&home);
        assert!(
            chats.iter().all(|chat| chat.cwd != "sessions" && chat.title != "sessions"),
            "{chats:?}"
        );
        let chat = chats
            .iter()
            .find(|chat| chat.id == "01a197e2-d3b2-7a81-9298-eec7243a81bd")
            .unwrap();
        assert_eq!(chat.title, "GrantTap Term Windows");
        assert_eq!(chat.cwd, r"C:\Users\sergii\granttap-term");
        assert_eq!(chat.at, 172_800);
        assert!(chats.iter().all(|chat| chat.id != encoded));
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
    fn cyrillic_before_arguments_does_not_panic() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        let filler = "ь".repeat(80);
        fs::write(
            &path,
            format!(
                "{{\"type\":\"assistant\",\"content\":\"{filler}\",\"name\":\"read_file\",\"arguments\":{{\"path\":\"a\"}}}}\n"
            ),
        )
        .unwrap();
        let joined = transcript(&path).join("\n");
        assert!(joined.contains("tool  read_file"), "{joined}");
        assert!(joined.contains("ь"), "{joined}");
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
    fn a_user_query_tag_stays_out_of_the_transcript() {
        let home = scratch();
        let path = home.join("chat_history.jsonl");
        fs::write(
            &path,
            "\
{\"type\":\"system\",\"content\":\"<user_query>secret prompt</user_query>\"}\n\
{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_info>\\nhost\\n</user_info>\\n<rules>\\nkeep out\\n</rules>\\n<user_rules>\\nmore\\n</user_rules>\"}]}\n\
{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"<user_query>\\nначинай паралель\\n</user_query>\"}]}\n\
{\"type\":\"user\",\"synthetic_reason\":\"system_reminder\",\"content\":[{\"type\":\"text\",\"text\":\"<system-reminder>\\nAvailable Skills\\n</system-reminder>\"}]}\n\
{\"type\":\"user\",\"content\":\"<system-reminder>\\nkeep out\\n</system-reminder>\"}\n\
{\"type\":\"user\",\"content\":\"&lt;system-reminder&gt;\\nskills\\n&lt;/system-reminder&gt;\"}\n\
{\"type\":\"user\",\"content\":\"This session is being continued from a previous conversation.\"}\n\
{\"type\":\"user\",\"content\":\"ship the desk and mention rules in prose\"}\n",
        )
        .unwrap();
        let lines = transcript_full(&path);
        let joined = lines.join("\n");
        assert!(
            lines.iter().any(|line| line == "user  начинай паралель"),
            "{joined}"
        );
        assert!(!joined.contains("user_query"), "{joined}");
        assert!(!joined.contains("</"), "{joined}");
        assert!(!joined.contains("user_info"), "{joined}");
        assert!(!joined.contains("Available Skills"), "{joined}");
        assert!(!joined.contains("keep out"), "{joined}");
        assert!(!joined.contains("secret prompt"), "{joined}");
        assert!(!joined.contains("skills"), "{joined}");
        assert!(!joined.contains("being continued"), "{joined}");
        assert!(
            lines
                .iter()
                .any(|line| line == "user  ship the desk and mention rules in prose"),
            "{joined}"
        );
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
    fn codex_calls_keep_the_name_and_the_reply() {
        let home = scratch();
        let path = home.join("rollout.jsonl");
        fs::write(
            &path,
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"shell\",\"arguments\":\"SECRETARGS\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"name\":\"read_file\",\"input\":\"SECRETARGS\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"item_completed\",\"item\":{\"type\":\"AgentMessage\",\"content\":[{\"type\":\"Text\",\"text\":\"the desk stays\"}]}}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"item_completed\",\"item\":{\"type\":\"CommandExecution\",\"command\":\"SECRETARGS\"}}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call_output\",\"output\":\"SECRETARGS\",\"text\":\"SECRETARGS\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"input_text\",\"text\":\"hidden\"}]}}\n",
        )
        .unwrap();
        let lines = transcript_full(&path);
        let joined = lines.join("\n");
        assert!(lines.iter().any(|line| line == "tool  shell"), "{joined}");
        assert!(lines.iter().any(|line| line == "tool  read_file"), "{joined}");
        assert!(
            lines.iter().any(|line| line == "assistant  the desk stays"),
            "{joined}"
        );
        assert!(
            lines.iter().any(|line| line == "tool  CommandExecution"),
            "{joined}"
        );
        assert!(!joined.contains("SECRETARGS"), "{joined}");
        assert!(!joined.contains("hidden"), "{joined}");
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

    #[test]
    fn worked_reads_the_summary_span() {
        let home = scratch();
        let dir = home.join("chat");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("summary.json"),
            "{\"created_at\":\"1970-01-01T00:00:00Z\",\"last_active_at\":\"1970-01-01T01:30:00Z\"}\n",
        )
        .unwrap();
        assert_eq!(worked(&dir).as_deref(), Some("1h 30m"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn encoded_cwd_walks_to_the_longest_directory() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-enc-{n}"));
        let tests = root.join("Users/Sergii/Documents/GitHub/edge-analytics-tests");
        let short = root.join("Users/Sergii/Documents/GitHub/edge");
        let claude = root.join("Users/Sergii/Documents/GitHub/.claude");
        fs::create_dir_all(&tests).unwrap();
        fs::create_dir_all(&short).unwrap();
        fs::create_dir_all(&claude).unwrap();
        let found = walk_encoded(
            &root,
            "Users-Sergii-Documents-GitHub-edge-analytics-tests",
        )
        .unwrap();
        assert_eq!(found, tests);
        let dot = walk_encoded(&root, "Users-Sergii-Documents-GitHub--claude").unwrap();
        assert_eq!(dot, claude);
        assert!(walk_encoded(&root, "Users-missing").is_none());
        let _ = fs::remove_dir_all(root);
    }
}
