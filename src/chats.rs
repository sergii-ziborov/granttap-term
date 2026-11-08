use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::model::{AgentKind, Chat};

const CAP: usize = 80;

pub fn load(home: &Path) -> Vec<Chat> {
    let mut chats = Vec::new();
    load_codex(&home.join(".codex/session_index.jsonl"), &mut chats);
    load_claude(&home.join(".claude/projects"), &mut chats);
    load_grok(&home.join(".grok/sessions"), &mut chats);
    mark_live(home, &mut chats);
    chats.truncate(CAP);
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
        let title = crate::agents::generated_title(&path).unwrap_or_else(|| session.id.clone());
        chats.insert(
            0,
            Chat {
                agent: AgentKind::Grok,
                id: session.id,
                title,
                path,
                live: true,
            },
        );
    }
}

fn load_codex(index: &Path, out: &mut Vec<Chat>) {
    let file = match File::open(index) {
        Ok(file) => file,
        Err(_) => return,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if out.len() >= CAP {
            break;
        }
        let Some(id) = json_string(&line, "id") else {
            continue;
        };
        let title = json_string(&line, "thread_name").unwrap_or_else(|| id.clone());
        out.push(Chat {
            agent: AgentKind::Codex,
            id,
            title,
            path: index.to_path_buf(),
            live: false,
        });
    }
}

fn load_claude(root: &Path, out: &mut Vec<Chat>) {
    let projects = match fs::read_dir(root) {
        Ok(projects) => projects,
        Err(_) => return,
    };
    for project in projects.flatten() {
        if out.len() >= CAP {
            break;
        }
        let files = match fs::read_dir(project.path()) {
            Ok(files) => files,
            Err(_) => continue,
        };
        for file in files.flatten() {
            if out.len() >= CAP {
                break;
            }
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let id = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "chat".into());
            out.push(Chat {
                agent: AgentKind::Claude,
                title: id.clone(),
                id,
                path,
                live: false,
            });
        }
    }
}

fn load_grok(root: &Path, out: &mut Vec<Chat>) {
    let dirs = match fs::read_dir(root) {
        Ok(dirs) => dirs,
        Err(_) => return,
    };
    for dir in dirs.flatten() {
        if out.len() >= CAP {
            break;
        }
        let path = dir.path();
        if !path.is_dir() {
            continue;
        }
        if grok_group(&path) {
            let nested = match fs::read_dir(&path) {
                Ok(nested) => nested,
                Err(_) => continue,
            };
            for child in nested.flatten() {
                if out.len() >= CAP {
                    break;
                }
                let child_path = child.path();
                if child_path.is_dir()
                    && !grok_group(&child_path)
                    && let Some(chat) = grok_session(&child_path)
                {
                    out.push(chat);
                }
            }
            continue;
        }
        if let Some(chat) = grok_session(&path) {
            out.push(chat);
        }
    }
}

fn grok_group(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    let mut saw_dir = false;
    for entry in entries.flatten() {
        if entry.path().is_file() {
            return false;
        }
        if entry.path().is_dir() {
            saw_dir = true;
        }
    }
    saw_dir
}

fn grok_session(path: &Path) -> Option<Chat> {
    let id = path.file_name()?.to_string_lossy().into_owned();
    if id.starts_with('.') {
        return None;
    }
    let title = crate::agents::generated_title(path).unwrap_or_else(|| id.clone());
    Some(Chat {
        agent: AgentKind::Grok,
        title,
        id,
        path: path.to_path_buf(),
        live: false,
    })
}

pub fn transcript(path: &Path) -> Vec<String> {
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
    let Ok(text) = fs::read_to_string(file) else {
        return vec!["No transcript.".into()];
    };
    if text.is_empty() {
        return vec!["No transcript.".into()];
    }
    text.lines().take(40).map(|line| clip(line, 200)).collect()
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
    let path = home.join(".granttap-term/handoff").join(format!("{safe}.txt"));
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
    let after = line.split_once(&needle)?.1;
    let value = after.split('"').nth(1)?;
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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
        Some("md") => 'M',
        Some("json") => 'J',
        Some("toml") => 'T',
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") => 'I',
        Some("sh") => '$',
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
    fn handoff_changes_owner() {
        let mut chats = vec![Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
            live: false,
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
        let chats = load(&home);
        let chat = chats.iter().find(|chat| chat.id == "one").unwrap();
        assert_eq!(chat.title, "Desk towers");
        assert!(chat.live);
        let path = write_handoff(&home, chat, AgentKind::Claude).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("from Grok"));
        assert!(text.contains("to Claude"));
        assert!(text.contains("hello from the chat"));
        let _ = fs::remove_dir_all(home);
    }
}
