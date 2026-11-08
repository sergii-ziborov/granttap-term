use std::fs;
use std::path::{Path, PathBuf};

use crate::model::Tower;

pub fn git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn towers(root: &Path) -> Vec<Tower> {
    let mut towers = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return towers,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !path.is_dir() {
            continue;
        }
        let files = count_files(&path);
        if files == 0 {
            continue;
        }
        towers.push(Tower {
            height: files.min(12),
            files,
            name,
        });
    }
    towers.sort_by(|a, b| b.files.cmp(&a.files).then(a.name.cmp(&b.name)));
    towers.truncate(8);
    towers
}

fn count_files(dir: &Path) -> usize {
    let mut count = 0usize;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            count += 1;
        } else if path.is_dir()
            && let Ok(nested) = fs::read_dir(&path)
        {
            count += nested
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count();
        }
        if count > 999 {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn towers_count_real_files() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-towers-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();
        let towers = towers(&root);
        assert_eq!(towers.len(), 1);
        assert_eq!(towers[0].name, "src");
        assert_eq!(towers[0].files, 2);
        let _ = fs::remove_dir_all(root);
    }
}
