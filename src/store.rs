use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{AcceptLevel, AgentKind, Permit, Task};

#[derive(Clone, Debug)]
pub struct DeskState {
    pub mesh_accept: Vec<(String, AcceptLevel)>,
    pub chat_accept: Vec<(String, AcceptLevel)>,
    pub paused: bool,
    pub permit: Permit,
    pub roads: bool,
    pub externals: bool,
    pub mesh_health: Vec<(String, bool, bool)>,
    pub actions: Vec<String>,
    pub tasks: Vec<Task>,
    pub history: Vec<String>,
    pub visits: Vec<Visit>,
    pub web_theme: String,
    pub web_lang: String,
    pub web_js: bool,
    pub web_live: bool,
    pub bookmarks: Vec<String>,
    pub look: String,
    pub font_face: String,
    pub font_size: u16,
    pub image_quality: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub url: String,
    pub count: u32,
    pub last: i64,
}

impl Default for DeskState {
    fn default() -> Self {
        Self {
            mesh_accept: Vec::new(),
            chat_accept: Vec::new(),
            paused: false,
            permit: Permit::Ask,
            roads: true,
            externals: true,
            mesh_health: Vec::new(),
            actions: Vec::new(),
            tasks: Vec::new(),
            history: Vec::new(),
            visits: Vec::new(),
            web_theme: "auto".into(),
            web_lang: "en".into(),
            web_js: true,
            web_live: true,
            bookmarks: Vec::new(),
            look: "cyberpunk".into(),
            font_face: String::new(),
            font_size: 16,
            image_quality: "high".into(),
        }
    }
}

pub fn path(home: &Path) -> PathBuf {
    home.join(".granttap-term/desk.txt")
}

pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

pub fn load(home: &Path) -> DeskState {
    let Ok(text) = fs::read_to_string(path(home)) else {
        return DeskState::default();
    };
    let mut state = DeskState::default();
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("M") => {
                let Some(root) = parts.next() else { continue };
                let Some(level) = parts.next().and_then(AcceptLevel::parse) else {
                    continue;
                };
                state.mesh_accept.push((root.to_string(), level));
            }
            Some("C") => {
                let Some(id) = parts.next() else { continue };
                let Some(level) = parts.next().and_then(AcceptLevel::parse) else {
                    continue;
                };
                state.chat_accept.push((id.to_string(), level));
            }
            Some("P") => state.paused = parts.next() == Some("1"),
            Some("R") => state.roads = parts.next() != Some("0"),
            Some("E") => state.externals = parts.next() != Some("0"),
            Some("H") => {
                let Some(root) = parts.next() else { continue };
                let Some(roads) = parts.next() else { continue };
                let Some(externals) = parts.next() else {
                    continue;
                };
                state
                    .mesh_health
                    .push((root.to_string(), roads != "0", externals != "0"));
            }
            Some("T") => {
                if let Some(permit) = parts.next().and_then(Permit::parse) {
                    state.permit = permit;
                }
            }
            Some("A") => {
                if let Some(action) = line.split_once('\t').map(|(_, text)| text.to_string()) {
                    state.actions.push(action);
                }
            }
            Some("Q") => {
                let Some(agent) = parts.next().and_then(AgentKind::parse) else {
                    continue;
                };
                let Some(model) = parts.next() else { continue };
                let Some(effort) = parts.next() else { continue };
                let Some(title) = parts.next() else { continue };
                if title.is_empty() {
                    continue;
                }
                state.tasks.push(Task {
                    title: title.to_string(),
                    owner: agent,
                    model: model.to_string(),
                    effort: effort.to_string(),
                });
            }
            Some("Y") => {
                if let Some(text) = line.split_once('\t').map(|(_, text)| text.to_string())
                    && !text.is_empty()
                {
                    state.history.push(text);
                }
            }
            Some("V") => {
                let Some(count) = parts.next().and_then(|text| text.parse().ok()) else {
                    continue;
                };
                let Some(last) = parts.next().and_then(|text| text.parse().ok()) else {
                    continue;
                };
                let url = parts.collect::<Vec<_>>().join("\t");
                if url.starts_with("http://") || url.starts_with("https://") {
                    state.visits.push(Visit { url, count, last });
                }
            }
            Some("W") => {
                if let Some(theme) = parts.next() {
                    state.web_theme = canon_theme(theme);
                }
                if let Some(lang) = parts.next() {
                    state.web_lang = canon_lang(lang);
                }
            }
            Some("J") => state.web_js = parts.next() != Some("0"),
            Some("L") => state.web_live = parts.next() != Some("0"),
            Some("B") => {
                let url = parts.collect::<Vec<_>>().join("\t");
                if url.starts_with("http://") || url.starts_with("https://") {
                    state.bookmarks.push(url);
                }
            }
            Some("D") => {
                if let Some(look) = parts.next() {
                    state.look = canon_look(look);
                }
            }
            Some("F") => {
                if let Some(face) = parts.next() {
                    state.font_face = canon_face(face);
                }
                if let Some(size) = parts.next() {
                    state.font_size = canon_size(size);
                }
            }
            Some("I") => {
                if let Some(quality) = parts.next() {
                    state.image_quality = canon_quality(quality);
                }
            }
            _ => {}
        }
    }
    state.actions.truncate(40);
    state.tasks.truncate(40);
    state.history.truncate(100);
    state.visits.truncate(30);
    state.bookmarks.truncate(30);
    state
}

pub fn save(home: &Path, state: &DeskState) -> std::io::Result<()> {
    let path = path(home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut body = String::new();
    for (root, level) in &state.mesh_accept {
        let root = root.replace(['\t', '\n', '\r'], " ");
        body.push_str(&format!("M\t{root}\t{}\n", level.label()));
    }
    for (id, level) in &state.chat_accept {
        let id = id.replace(['\t', '\n', '\r'], " ");
        body.push_str(&format!("C\t{id}\t{}\n", level.label()));
    }
    body.push_str(&format!("P\t{}\n", if state.paused { "1" } else { "0" }));
    body.push_str(&format!("R\t{}\n", if state.roads { "1" } else { "0" }));
    body.push_str(&format!("E\t{}\n", if state.externals { "1" } else { "0" }));
    for (root, roads, externals) in &state.mesh_health {
        let root = root.replace(['\t', '\n', '\r'], " ");
        body.push_str(&format!(
            "H\t{root}\t{}\t{}\n",
            if *roads { "1" } else { "0" },
            if *externals { "1" } else { "0" }
        ));
    }
    body.push_str(&format!("T\t{}\n", state.permit.label()));
    for action in state.actions.iter().take(40) {
        let action = action.replace(['\n', '\r', '\t'], " ");
        body.push_str(&format!("A\t{action}\n"));
    }
    for task in state.tasks.iter().take(40) {
        let model = task.model.replace(['\n', '\r', '\t'], " ");
        let effort = task.effort.replace(['\n', '\r', '\t'], " ");
        let title = task.title.replace(['\n', '\r', '\t'], " ");
        body.push_str(&format!(
            "Q\t{}\t{model}\t{effort}\t{title}\n",
            task.owner.label().to_ascii_lowercase()
        ));
    }
    for line in state.history.iter().take(100) {
        let line = line.replace(['\n', '\r', '\t'], " ");
        if !line.is_empty() {
            body.push_str(&format!("Y\t{line}\n"));
        }
    }
    for visit in state.visits.iter().take(30) {
        let url = visit.url.replace(['\n', '\r', '\t'], " ");
        if url.starts_with("http://") || url.starts_with("https://") {
            body.push_str(&format!("V\t{}\t{}\t{url}\n", visit.count, visit.last));
        }
    }
    let theme = canon_theme(&state.web_theme);
    let lang = canon_lang(&state.web_lang);
    body.push_str(&format!("W\t{theme}\t{lang}\n"));
    body.push_str(&format!("J\t{}\n", if state.web_js { "1" } else { "0" }));
    body.push_str(&format!("L\t{}\n", if state.web_live { "1" } else { "0" }));
    for url in state.bookmarks.iter().take(30) {
        let url = url.replace(['\n', '\r', '\t'], " ");
        if url.starts_with("http://") || url.starts_with("https://") {
            body.push_str(&format!("B\t{url}\n"));
        }
    }
    body.push_str(&format!("D\t{}\n", canon_look(&state.look)));
    if !state.font_face.is_empty() {
        let face = state.font_face.replace(['\n', '\r', '\t'], " ");
        body.push_str(&format!("F\t{face}\t{}\n", state.font_size));
    }
    body.push_str(&format!("I\t{}\n", canon_quality(&state.image_quality)));
    fs::write(path, body)
}

fn canon_theme(text: &str) -> String {
    match text {
        "dark" | "light" => text.to_string(),
        _ => "auto".into(),
    }
}

fn canon_lang(text: &str) -> String {
    match text {
        "auto" | "en" | "ru" | "uk" | "he" | "ar" | "de" | "fr" | "es" => text.to_string(),
        _ => "en".into(),
    }
}

fn canon_look(text: &str) -> String {
    match text {
        "cyberdeck" | "parchment" | "ink" | "dos" | "linux" => text.to_string(),
        _ => "cyberpunk".into(),
    }
}

fn canon_face(text: &str) -> String {
    match text {
        "Consolas" | "Cascadia Mono" | "Lucida Console" => text.to_string(),
        _ => String::new(),
    }
}

fn canon_size(text: &str) -> u16 {
    match text {
        "12" => 12,
        "20" => 20,
        "24" => 24,
        _ => 16,
    }
}

fn canon_quality(text: &str) -> String {
    match text {
        "low" | "medium" => text.to_string(),
        _ => "high".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn accept_pause_and_actions_round_trip() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-store-{n}"));
        let mut state = DeskState::default();
        state.mesh_accept.push(("/repo".into(), AcceptLevel::Safe));
        state.chat_accept.push(("abc".into(), AcceptLevel::Full));
        state.paused = true;
        state.permit = Permit::Plan;
        state.mesh_health.push(("/repo".into(), false, true));
        state.actions.push("task write the desk".into());
        state.tasks.push(Task {
            title: "write the desk".into(),
            owner: AgentKind::Claude,
            model: "claude".into(),
            effort: "high".into(),
        });
        state.history.push("mesh".into());
        state.visits.push(Visit {
            url: "https://example.com/docs".into(),
            count: 4,
            last: 20,
        });
        save(&home, &state).unwrap();
        let loaded = load(&home);
        assert_eq!(
            loaded.mesh_accept,
            vec![("/repo".into(), AcceptLevel::Safe)]
        );
        assert_eq!(loaded.chat_accept, vec![("abc".into(), AcceptLevel::Full)]);
        assert!(loaded.paused);
        assert_eq!(loaded.permit, Permit::Plan);
        assert_eq!(loaded.mesh_health, vec![("/repo".into(), false, true)]);
        assert_eq!(loaded.actions, vec!["task write the desk".to_string()]);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].owner, AgentKind::Claude);
        assert_eq!(loaded.tasks[0].title, "write the desk");
        assert_eq!(loaded.history, vec!["mesh".to_string()]);
        assert_eq!(loaded.visits.len(), 1);
        assert_eq!(loaded.visits[0].url, "https://example.com/docs");
        assert_eq!(loaded.visits[0].count, 4);
        assert_eq!(loaded.visits[0].last, 20);
        assert_eq!(loaded.web_theme, "auto");
        assert_eq!(loaded.web_lang, "en");
        assert!(loaded.web_js);
        assert!(loaded.web_live);
        assert!(loaded.bookmarks.is_empty());
        state.web_js = false;
        state.web_live = false;
        state.bookmarks.push("https://example.com/docs".into());
        state.bookmarks.push("file://local".into());
        save(&home, &state).unwrap();
        let flagged = load(&home);
        assert!(!flagged.web_js);
        assert!(!flagged.web_live);
        assert_eq!(flagged.bookmarks, vec!["https://example.com/docs".to_string()]);
        state.web_theme = "dark".into();
        state.web_lang = "ru".into();
        save(&home, &state).unwrap();
        let again = load(&home);
        assert_eq!(again.web_theme, "dark");
        assert_eq!(again.web_lang, "ru");
        state.web_theme = "nope".into();
        state.web_lang = "zz".into();
        save(&home, &state).unwrap();
        let cleaned = load(&home);
        assert_eq!(cleaned.web_theme, "auto");
        assert_eq!(cleaned.web_lang, "en");
        let _ = fs::remove_dir_all(home);
    }
}
