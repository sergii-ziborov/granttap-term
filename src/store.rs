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
        }
    }
}

pub fn path(home: &Path) -> PathBuf {
    home.join(".granttap-term/desk.txt")
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
                let Some(externals) = parts.next() else { continue };
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
            _ => {}
        }
    }
    state.actions.truncate(40);
    state.tasks.truncate(40);
    state.history.truncate(100);
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
    fs::write(path, body)
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
        save(&home, &state).unwrap();
        let loaded = load(&home);
        assert_eq!(loaded.mesh_accept, vec![("/repo".into(), AcceptLevel::Safe)]);
        assert_eq!(loaded.chat_accept, vec![("abc".into(), AcceptLevel::Full)]);
        assert!(loaded.paused);
        assert_eq!(loaded.permit, Permit::Plan);
        assert_eq!(loaded.mesh_health, vec![("/repo".into(), false, true)]);
        assert_eq!(loaded.actions, vec!["task write the desk".to_string()]);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].owner, AgentKind::Claude);
        assert_eq!(loaded.tasks[0].title, "write the desk");
        assert_eq!(loaded.history, vec!["mesh".to_string()]);
        let _ = fs::remove_dir_all(home);
    }
}
