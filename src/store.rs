use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{AcceptLevel, Permit};

#[derive(Clone, Debug)]
pub struct DeskState {
    pub mesh_accept: Vec<(String, AcceptLevel)>,
    pub chat_accept: Vec<(String, AcceptLevel)>,
    pub paused: bool,
    pub permit: Permit,
    pub roads: bool,
    pub externals: bool,
    pub actions: Vec<String>,
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
            actions: Vec::new(),
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
            _ => {}
        }
    }
    state.actions.truncate(40);
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
    body.push_str(&format!("T\t{}\n", state.permit.label()));
    for action in state.actions.iter().take(40) {
        let action = action.replace(['\n', '\r', '\t'], " ");
        body.push_str(&format!("A\t{action}\n"));
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
        state.actions.push("task write the desk".into());
        save(&home, &state).unwrap();
        let loaded = load(&home);
        assert_eq!(loaded.mesh_accept, vec![("/repo".into(), AcceptLevel::Safe)]);
        assert_eq!(loaded.chat_accept, vec![("abc".into(), AcceptLevel::Full)]);
        assert!(loaded.paused);
        assert_eq!(loaded.permit, Permit::Plan);
        assert_eq!(loaded.actions, vec!["task write the desk".to_string()]);
        let _ = fs::remove_dir_all(home);
    }
}
