use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub agent: &'static str,
    pub name: String,
}

pub fn load(home: &Path, root: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    collect(home.join(".claude/skills"), "claude", 0, &mut skills);
    collect(home.join(".codex/skills"), "codex", 0, &mut skills);
    collect(home.join(".agents/skills"), "shared", 0, &mut skills);
    collect(root.join(".claude/skills"), "claude", 0, &mut skills);
    collect(root.join(".codex/skills"), "codex", 0, &mut skills);
    collect(root.join(".agents/skills"), "shared", 0, &mut skills);
    skills.truncate(40);
    skills
}

fn collect(dir: std::path::PathBuf, agent: &'static str, depth: usize, out: &mut Vec<Skill>) {
    if depth > 5 || out.len() >= 40 || !dir.is_dir() {
        return;
    }
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= 40 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && name != ".system" {
            continue;
        }
        if path.is_dir() {
            if path.join("SKILL.md").is_file() {
                out.push(Skill {
                    agent,
                    name: name.trim_start_matches('.').to_string(),
                });
            }
            collect(path, agent, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn only_real_skill_files_are_listed() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-skills-{n}"));
        fs::create_dir_all(home.join(".claude/skills/ship")).unwrap();
        fs::create_dir_all(home.join(".codex/skills/review")).unwrap();
        fs::write(home.join(".claude/skills/ship/SKILL.md"), "# ship\n").unwrap();
        fs::write(home.join(".codex/skills/review/SKILL.md"), "# review\n").unwrap();
        fs::create_dir_all(home.join(".claude/skills/empty")).unwrap();
        let skills = load(&home, &home);
        assert!(skills.iter().any(|skill| skill.agent == "claude" && skill.name == "ship"));
        assert!(skills.iter().any(|skill| skill.agent == "codex" && skill.name == "review"));
        assert!(!skills.iter().any(|skill| skill.name == "empty"));
        let _ = fs::remove_dir_all(home);
    }
}
