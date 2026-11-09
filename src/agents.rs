use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::model::{AgentKind, Permit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokLive {
    pub id: String,
    pub cwd: String,
    pub pid: u32,
    pub alive: bool,
}

pub fn grok_sessions(home: &Path) -> Vec<GrokLive> {
    let Ok(text) = fs::read_to_string(home.join(".grok/active_sessions.json")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for object in top_objects(&text) {
        let Some(id) = json_string(object, "session_id") else {
            continue;
        };
        let cwd = json_string(object, "cwd").unwrap_or_default();
        let pid = json_number(object, "pid").unwrap_or(0);
        out.push(GrokLive {
            alive: pid_alive(pid),
            id,
            cwd,
            pid,
        });
        if out.len() == 16 {
            break;
        }
    }
    out
}

pub fn session_path(home: &Path, cwd: &str, id: &str) -> PathBuf {
    home.join(".grok/sessions").join(percent(cwd)).join(id)
}

pub fn generated_title(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("summary.json")).ok()?;
    json_string(&text, "generated_title")
}

pub fn grok_usage(home: &Path) -> Option<String> {
    let session = grok_sessions(home).into_iter().find(|item| item.alive)?;
    let dir = session_path(home, &session.cwd, &session.id);
    let usage = fs::read_to_string(dir.join("usage.json")).ok()?;
    let object = json_object(&usage, "session")?;
    let input = number_depth1(object, "inputTokens")?;
    let output = number_depth1(object, "outputTokens").unwrap_or(0);
    let mut line = format!("Grok in {input} out {output}");
    if let Ok(signals) = fs::read_to_string(dir.join("signals.json"))
        && let Some(used) = number_depth1(&signals, "contextTokensUsed")
        && let Some(window) = number_depth1(&signals, "contextWindowTokens")
    {
        line.push_str(&format!("  context {used}/{window}"));
    }
    Some(line)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub agent: AgentKind,
    pub session_id: String,
    pub cwd: String,
    pub label: String,
}

pub fn asks(home: &Path) -> Vec<Ask> {
    live_asks(home).0
}

pub fn questions(home: &Path) -> String {
    let (asks, known) = live_asks(home);
    if !home.join(".grok/active_sessions.json").is_file() {
        return "no data".into();
    }
    if let Some(ask) = asks.first() {
        return ask.label.clone();
    }
    if grok_sessions(home).iter().any(|item| item.alive) {
        if known {
            "none".into()
        } else {
            "no data".into()
        }
    } else {
        "none".into()
    }
}

pub fn context_line(path: &Path) -> Option<String> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    let text = fs::read_to_string(dir.join("signals.json")).ok()?;
    let used = number_depth1(&text, "contextTokensUsed")?;
    let window = number_depth1(&text, "contextWindowTokens")?;
    if window == 0 {
        return None;
    }
    let pct = used.saturating_mul(100) / window;
    Some(format!("context {pct}%"))
}

fn live_asks(home: &Path) -> (Vec<Ask>, bool) {
    if !home.join(".grok/active_sessions.json").is_file() {
        return (Vec::new(), false);
    }
    let live: Vec<_> = grok_sessions(home)
        .into_iter()
        .filter(|item| item.alive)
        .collect();
    let mut asks = Vec::new();
    let mut known = false;
    for session in &live {
        let dir = session_path(home, &session.cwd, &session.id);
        if !dir.is_dir() {
            continue;
        }
        match plan_waiting(&dir) {
            Some(true) => asks.push(Ask {
                agent: AgentKind::Grok,
                session_id: session.id.clone(),
                cwd: session.cwd.clone(),
                label: "plan approval".into(),
            }),
            Some(false) => known = true,
            None => {}
        }
        if let Some(open) = permission_state(&dir) {
            match open {
                Some(tool) => asks.push(Ask {
                    agent: AgentKind::Grok,
                    session_id: session.id.clone(),
                    cwd: session.cwd.clone(),
                    label: format!("permission {tool}"),
                }),
                None => known = true,
            }
        }
    }
    (asks, known)
}

pub fn apply_permit(home: &Path, dir: &Path, permit: Permit) -> String {
    let grok = grok_mode(permit);
    let claude = claude_mode(permit);
    let codex = codex_mode(permit);
    [
        match set_toml(
            &home.join(".grok/config.toml"),
            "ui",
            "permission_mode",
            grok,
        ) {
            Ok(()) => format!("Grok {grok}"),
            Err(err) => format!("Grok {err}"),
        },
        match write_claude_mode(dir, claude) {
            Ok(()) => format!("Claude {claude}"),
            Err(err) => format!("Claude {err}"),
        },
        match set_toml(
            &home.join(".codex/config.toml"),
            "",
            "approval_policy",
            codex,
        ) {
            Ok(()) => format!("Codex {codex}"),
            Err(err) => format!("Codex {err}"),
        },
    ]
    .join("  ")
}

pub fn apply_model(home: &Path, dir: &Path, agent: Option<AgentKind>, model: &str) -> String {
    let Some(agent) = agent else {
        return "model kept on the desk".into();
    };
    let Ok(model) = plain(model) else {
        return "model needs a plain name".into();
    };
    match agent {
        AgentKind::Grok => {
            match set_toml(&home.join(".grok/config.toml"), "models", "default", model) {
                Ok(()) => format!("Grok model {model}"),
                Err(err) => format!("Grok {err}"),
            }
        }
        AgentKind::Claude => match write_project_model(dir, model) {
            Ok(()) => format!("Claude model {model}"),
            Err(err) => format!("Claude {err}"),
        },
        AgentKind::Codex => match set_toml(&home.join(".codex/config.toml"), "", "model", model) {
            Ok(()) => format!("Codex model {model}"),
            Err(err) => format!("Codex {err}"),
        },
    }
}

pub fn apply_effort(home: &Path, agent: AgentKind, effort: &str) -> String {
    let Ok(effort) = plain(effort) else {
        return "effort needs a plain name".into();
    };
    match agent {
        AgentKind::Grok => match set_toml(
            &home.join(".grok/config.toml"),
            "models",
            "default_reasoning_effort",
            effort,
        ) {
            Ok(()) => format!("Grok effort {effort}"),
            Err(err) => format!("Grok {err}"),
        },
        AgentKind::Claude => match set_claude_effort(home, effort) {
            Ok(()) => format!("Claude effort {effort}"),
            Err(err) => format!("Claude {err}"),
        },
        AgentKind::Codex => match set_toml(
            &home.join(".codex/config.toml"),
            "",
            "model_reasoning_effort",
            effort,
        ) {
            Ok(()) => format!("Codex effort {effort}"),
            Err(err) => format!("Codex {err}"),
        },
    }
}

fn grok_mode(permit: Permit) -> &'static str {
    match permit {
        Permit::Ask => "ask",
        Permit::Edits => "acceptEdits",
        Permit::Bypass => "always-approve",
        Permit::Plan => "plan",
    }
}

fn claude_mode(permit: Permit) -> &'static str {
    match permit {
        Permit::Ask => "default",
        Permit::Edits => "acceptEdits",
        Permit::Bypass => "bypassPermissions",
        Permit::Plan => "plan",
    }
}

fn codex_mode(permit: Permit) -> &'static str {
    match permit {
        Permit::Ask | Permit::Plan => "on-request",
        Permit::Edits => "on-failure",
        Permit::Bypass => "never",
    }
}

fn plain(value: &str) -> Result<&str, ()> {
    if value.is_empty() || value.contains(['"', '\'', '\n', '\r', '\\']) {
        Err(())
    } else {
        Ok(value)
    }
}

fn set_toml(path: &Path, section: &str, key: &str, value: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err("config missing".into());
    }
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let next = if let Some(next) = replace_toml_value(&text, key, value, section) {
        next
    } else if let Some(next) = insert_toml_line(&text, section, &format!("{key} = \"{value}\"")) {
        next
    } else {
        return Err(format!("no [{section}]"));
    };
    fs::write(path, next).map_err(|err| err.to_string())
}

fn set_claude_effort(home: &Path, effort: &str) -> Result<(), String> {
    let path = home.join(".claude/settings.json");
    if !path.is_file() {
        return Err("settings missing".into());
    }
    let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let Some(next) = replace_json_key(&text, "effortLevel", effort) else {
        return Err("effortLevel is absent".into());
    };
    fs::write(&path, next).map_err(|err| err.to_string())
}

fn write_claude_mode(dir: &Path, mode: &str) -> Result<(), String> {
    let path = dir.join(".claude/settings.local.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if !path.is_file() {
        let body =
            format!("{{\n  \"permissions\": {{\n    \"defaultMode\": \"{mode}\"\n  }}\n}}\n");
        return fs::write(&path, body).map_err(|err| err.to_string());
    }
    let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    if let Some(next) = replace_json_key(&text, "defaultMode", mode) {
        return fs::write(&path, next).map_err(|err| err.to_string());
    }
    let Some(next) = insert_after_key(&text, "permissions", "defaultMode", mode) else {
        return Err("permissions object is missing".into());
    };
    fs::write(&path, next).map_err(|err| err.to_string())
}

fn write_project_model(dir: &Path, model: &str) -> Result<(), String> {
    let path = dir.join(".claude/settings.local.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if !path.is_file() {
        let body = format!("{{\n  \"model\": \"{model}\"\n}}\n");
        return fs::write(&path, body).map_err(|err| err.to_string());
    }
    let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    if let Some(next) = replace_json_key(&text, "model", model) {
        return fs::write(&path, next).map_err(|err| err.to_string());
    }
    let next = insert_root_key(&text, "model", model)?;
    fs::write(&path, next).map_err(|err| err.to_string())
}

fn replace_toml_value(text: &str, key: &str, value: &str, section: &str) -> Option<String> {
    let mut current = String::new();
    let mut offset = 0usize;
    for piece in text.split_inclusive('\n') {
        let core = piece.trim_end_matches(['\n', '\r']);
        let trimmed = core.trim_start();
        if let Some(name) = header(trimmed) {
            current = name;
        } else if current == section
            && !trimmed.starts_with('#')
            && toml_key(trimmed) == Some(key)
            && let Some((start, end)) = quoted_span(trimmed)
        {
            let indent = core.len() - trimmed.len();
            let abs_start = offset + indent + start;
            let abs_end = offset + indent + end;
            let mut next = String::with_capacity(text.len() + value.len());
            next.push_str(&text[..abs_start]);
            next.push_str(value);
            next.push_str(&text[abs_end..]);
            return Some(next);
        }
        offset += piece.len();
    }
    None
}

fn insert_toml_line(text: &str, section: &str, line: &str) -> Option<String> {
    if section.is_empty() {
        return Some(format!("{line}\n{text}"));
    }
    let header = format!("[{section}]");
    let idx = text.find(&header)?;
    let nl = text[idx + header.len()..].find('\n')? + idx + header.len() + 1;
    let mut next = String::new();
    next.push_str(&text[..nl]);
    next.push_str(line);
    next.push('\n');
    next.push_str(&text[nl..]);
    Some(next)
}

fn header(trimmed: &str) -> Option<String> {
    if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
        Some(trimmed[1..trimmed.len() - 1].trim().to_string())
    } else {
        None
    }
}

fn toml_key(trimmed: &str) -> Option<&str> {
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Some(key)
    } else {
        None
    }
}

fn quoted_span(trimmed: &str) -> Option<(usize, usize)> {
    let eq = trimmed.find('=')?;
    let after = &trimmed[eq + 1..];
    let quote = after.trim_start().chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rel = after.find(quote)?;
    let start = eq + 1 + rel + 1;
    let end = start + trimmed[start..].find(quote)?;
    Some((start, end))
}

fn replace_json_key(text: &str, key: &str, value: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut search = 0usize;
    while let Some(rel) = text[search..].find(&needle) {
        let at = search + rel;
        let rest = &text[at + needle.len()..];
        let rest_trim = rest.trim_start();
        let ws1 = rest.len() - rest_trim.len();
        if !rest_trim.starts_with(':') {
            search = at + needle.len();
            continue;
        }
        let after_colon = &rest_trim[1..];
        let after_trim = after_colon.trim_start();
        let ws2 = after_colon.len() - after_trim.len();
        if !after_trim.starts_with('"') {
            search = at + needle.len();
            continue;
        }
        let val_start = at + needle.len() + ws1 + 1 + ws2 + 1;
        let end = text[val_start..].find('"')?;
        let mut next = String::with_capacity(text.len() + value.len());
        next.push_str(&text[..val_start]);
        next.push_str(value);
        next.push_str(&text[val_start + end..]);
        return Some(next);
    }
    None
}

fn insert_after_key(text: &str, object_key: &str, key: &str, value: &str) -> Option<String> {
    let needle = format!("\"{object_key}\"");
    let at = text.find(&needle)?;
    let brace = text[at + needle.len()..].find('{')? + at + needle.len();
    let mut next = String::new();
    next.push_str(&text[..=brace]);
    next.push_str(&format!("\n    \"{key}\": \"{value}\","));
    next.push_str(&text[brace + 1..]);
    Some(next)
}

fn insert_root_key(text: &str, key: &str, value: &str) -> Result<String, String> {
    let start = text.find('{').ok_or_else(|| "not an object".to_string())?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut close = None;
    for (offset, ch) in text[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or_else(|| "not an object".to_string())?;
    let comma = if text[..close].trim_end().ends_with('{') {
        ""
    } else {
        ","
    };
    let mut next = String::new();
    next.push_str(&text[..close]);
    next.push_str(&format!("{comma}\n  \"{key}\": \"{value}\"\n"));
    next.push_str(&text[close..]);
    Ok(next)
}

fn plan_waiting(dir: &Path) -> Option<bool> {
    let text = fs::read_to_string(dir.join("plan_mode.json")).ok()?;
    Some(json_bool(&text, "awaiting_plan_approval"))
}

fn permission_state(dir: &Path) -> Option<Option<String>> {
    let text = tail(&dir.join("events.jsonl"), 48 * 1024)?;
    let mut open = None;
    for line in text.lines() {
        if line.contains("\"permission_requested\"") {
            let tool = json_string(line, "tool_name").unwrap_or_else(|| "tool".into());
            open = Some(safe_tool(&tool));
        } else if line.contains("\"permission_resolved\"") {
            open = None;
        }
    }
    Some(open)
}

fn tail(path: &Path, max: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if len > max
        && let Some(pos) = text.find('\n')
    {
        text.drain(..=pos);
    }
    Some(text)
}

fn safe_tool(name: &str) -> String {
    let tool: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .take(40)
        .collect();
    if tool.is_empty() { "tool".into() } else { tool }
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &text[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn top_objects(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_str = false;
    let mut escape = false;
    for (idx, ch) in text.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = idx;
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    out.push(&text[start..=idx]);
                }
            }
            _ => {}
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
        Some(value.to_string())
    }
}

fn json_number(text: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn json_bool(text: &str, key: &str) -> bool {
    let needle = format!("\"{key}\"");
    let Some(after) = text.split_once(&needle).map(|(_, rest)| rest) else {
        return false;
    };
    after
        .trim_start()
        .trim_start_matches(':')
        .trim_start()
        .starts_with("true")
}

fn json_object<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let at = text.find(&needle)?;
    let after = &text[at + needle.len()..];
    let rel = after.find('{')?;
    let start = at + needle.len() + rel;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn number_depth1(text: &str, key: &str) -> Option<u64> {
    let mut depth = 0i32;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '"' {
            let rest = &text[idx + 1..];
            let end = rest.find('"')?;
            let raw = &rest[..end];
            let skip_to = idx + 1 + end + 1;
            while chars.peek().is_some_and(|(pos, _)| *pos < skip_to) {
                chars.next();
            }
            if raw == key && depth == 1 {
                let after = text[skip_to..]
                    .trim_start()
                    .trim_start_matches(':')
                    .trim_start();
                let digits: String = after
                    .chars()
                    .take_while(|item| item.is_ascii_digit())
                    .collect();
                return digits.parse().ok();
            }
            continue;
        }
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

pub struct Face {
    pub model: String,
    pub effort: String,
    pub permission: String,
}

pub fn face(home: &Path, dir: &Path, agent: AgentKind, permit: Permit) -> Face {
    let (model, effort, permission) = match agent {
        AgentKind::Grok => {
            let text = read_cap(&home.join(".grok/config.toml"));
            (
                toml_get(&text, "models", "default"),
                toml_get(&text, "models", "default_reasoning_effort"),
                toml_get(&text, "ui", "permission_mode"),
            )
        }
        AgentKind::Claude => {
            let settings = read_cap(&home.join(".claude/settings.json"));
            let local = read_cap(&dir.join(".claude/settings.local.json"));
            (
                json_string(&local, "model"),
                json_string(&settings, "effortLevel"),
                json_string(&local, "defaultMode"),
            )
        }
        AgentKind::Codex => {
            let text = read_cap(&home.join(".codex/config.toml"));
            (
                toml_get(&text, "", "model"),
                toml_get(&text, "", "model_reasoning_effort"),
                toml_get(&text, "", "approval_policy"),
            )
        }
    };
    let permission = permission.unwrap_or_else(|| match agent {
        AgentKind::Grok => grok_mode(permit).into(),
        AgentKind::Claude => claude_mode(permit).into(),
        AgentKind::Codex => codex_mode(permit).into(),
    });
    Face {
        model: model.unwrap_or_else(|| "—".into()),
        effort: effort.unwrap_or_else(|| "—".into()),
        permission,
    }
}

fn read_cap(path: &Path) -> String {
    let Ok(meta) = fs::metadata(path) else {
        return String::new();
    };
    if !meta.is_file() || meta.len() == 0 {
        return String::new();
    }
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; usize::try_from(meta.len().min(64 * 1024)).unwrap_or(0)];
    let Ok(n) = file.read(&mut buf) else {
        return String::new();
    };
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

fn toml_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = header(trimmed) {
            current = name;
            continue;
        }
        if current == section && toml_key(trimmed) == Some(key) {
            let (start, end) = quoted_span(trimmed)?;
            let value = trimmed[start..end].to_string();
            if value.is_empty() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("granttap-{name}-{n}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn permit_splices_one_value_and_leaves_the_rest() {
        let home = scratch("agent");
        let dir = scratch("proj");
        fs::create_dir_all(home.join(".grok")).unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(
            home.join(".grok/config.toml"),
            "[ui]\npermission_mode = \"ask\"\nyolo = false\n\n[models]\ndefault = \"grok-4\"\ndefault_reasoning_effort = \"medium\"\n",
        )
        .unwrap();
        fs::write(
            home.join(".claude/settings.json"),
            "{\"hooks\":{\"keep\":true},\"effortLevel\":\"medium\",\"tui\":true}\n",
        )
        .unwrap();
        fs::write(
            home.join(".codex/config.toml"),
            "model = \"old\"\nmodel_reasoning_effort = \"medium\"\napproval_policy = \"on-request\"\n\n[projects.demo]\nmodel = \"leave\"\n",
        )
        .unwrap();
        let note = apply_permit(&home, &dir, Permit::Bypass);
        assert!(note.contains("always-approve"));
        assert!(note.contains("bypassPermissions"));
        assert!(note.contains("never"));
        let grok = fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(grok.contains("permission_mode = \"always-approve\""));
        assert!(grok.contains("yolo = false"));
        assert!(grok.contains("default = \"grok-4\""));
        let claude = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(claude.contains("\"hooks\":{\"keep\":true}"));
        assert!(claude.contains("\"effortLevel\":\"medium\""));
        let local = fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        assert_eq!(local.matches("defaultMode").count(), 1);
        assert!(local.contains("bypassPermissions"));
        let codex = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("approval_policy = \"never\""));
        assert!(codex.contains("model = \"leave\""));
        assert!(codex.contains("model = \"old\""));
        apply_permit(&home, &dir, Permit::Plan);
        let local = fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        assert_eq!(local.matches("defaultMode").count(), 1);
        assert!(local.contains("\"plan\""));
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn model_and_effort_touch_only_the_selected_agent() {
        let home = scratch("mode");
        let dir = scratch("mode-proj");
        fs::create_dir_all(home.join(".grok")).unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(
            home.join(".grok/config.toml"),
            "[models]\ndefault = \"grok-4\"\ndefault_reasoning_effort = \"medium\"\n",
        )
        .unwrap();
        fs::write(
            home.join(".claude/settings.json"),
            "{\"hooks\":{\"keep\":true},\"effortLevel\":\"medium\"}\n",
        )
        .unwrap();
        fs::write(
            home.join(".codex/config.toml"),
            "model = \"old\"\nmodel_reasoning_effort = \"medium\"\n",
        )
        .unwrap();
        apply_model(&home, &dir, Some(AgentKind::Grok), "grok-4.7");
        apply_effort(&home, AgentKind::Claude, "max");
        let grok = fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(grok.contains("default = \"grok-4.7\""));
        assert!(grok.contains("default_reasoning_effort = \"medium\""));
        let claude = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(claude.contains("\"effortLevel\":\"max\""));
        assert!(claude.contains("\"hooks\":{\"keep\":true}"));
        let codex = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("model = \"old\""));
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn open_permission_and_session_usage_are_read() {
        let home = scratch("live");
        let cwd = "/tmp/desk";
        let id = "s1";
        let dir = session_path(&home, cwd, id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            home.join(".grok/active_sessions.json"),
            format!(
                "[{{\"session_id\":\"{id}\",\"pid\":{},\"cwd\":\"{cwd}\"}}]",
                std::process::id()
            ),
        )
        .unwrap();
        fs::write(
            dir.join("usage.json"),
            r#"{"session":{"inputTokens":10,"outputTokens":4,"modelUsage":{"m":{"inputTokens":1,"outputTokens":2}}},"turns":[]}"#,
        )
        .unwrap();
        fs::write(
            dir.join("signals.json"),
            r#"{"contextTokensUsed":30,"contextWindowTokens":100}"#,
        )
        .unwrap();
        fs::write(
            dir.join("plan_mode.json"),
            r#"{"awaiting_plan_approval":false}"#,
        )
        .unwrap();
        fs::write(
            dir.join("events.jsonl"),
            "{\"type\":\"permission_requested\",\"tool_name\":\"run_terminal_command\"}\n{\"type\":\"permission_resolved\",\"tool_name\":\"run_terminal_command\"}\n",
        )
        .unwrap();
        let usage = grok_usage(&home).unwrap();
        assert!(usage.contains("in 10 out 4"));
        assert!(usage.contains("context 30/100"));
        assert!(!usage.contains("out 2"));
        assert!(!usage.contains("left"));
        assert_eq!(questions(&home), "none");
        fs::write(
            dir.join("events.jsonl"),
            "{\"type\":\"permission_resolved\"}\n{\"type\":\"permission_requested\",\"tool_name\":\"run_terminal_command\"}\n",
        )
        .unwrap();
        assert_eq!(questions(&home), "permission run_terminal_command");
        fs::write(
            dir.join("plan_mode.json"),
            r#"{"awaiting_plan_approval":true}"#,
        )
        .unwrap();
        assert_eq!(questions(&home), "plan approval");
        let found = asks(&home);
        assert!(found.iter().any(|ask| ask.label == "plan approval"));
        assert!(
            found
                .iter()
                .any(|ask| ask.label == "permission run_terminal_command")
        );
        assert_eq!(context_line(&dir).as_deref(), Some("context 30%"));
        let _ = fs::remove_dir_all(home);
    }
}
