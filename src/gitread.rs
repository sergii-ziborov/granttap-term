use std::fs;
use std::path::{Path, PathBuf};

use weavatrix_git::{HistoryOptions, ObjectKind, Repository, RepositorySet};

pub struct Rev {
    pub id: String,
    pub when: String,
    pub subject: String,
}

pub fn recent(root: &Path, max: usize) -> Result<Vec<Rev>, String> {
    let repo = Repository::open(root).map_err(|err| err.to_string())?;
    let head = repo.resolve("HEAD").map_err(|err| err.to_string())?;
    let records = repo
        .history(
            head,
            HistoryOptions {
                max_commits: max.max(1),
                first_parent: true,
                ..HistoryOptions::default()
            },
        )
        .map_err(|err| err.to_string())?;
    Ok(records
        .into_iter()
        .map(|record| rev_from(&record.commit))
        .collect())
}

pub fn blob_text(
    root: &Path,
    revision: &str,
    path: &str,
    max_bytes: usize,
) -> Result<String, String> {
    let repo = Repository::open(root).map_err(|err| err.to_string())?;
    let snap = repo.snapshot(revision).map_err(|err| err.to_string())?;
    let want = normalize(path);
    let entry = snap
        .entries
        .iter()
        .find(|entry| entry.path == want.as_bytes())
        .ok_or_else(|| format!("{want} is not in {revision}"))?;
    let object = repo.object(entry.id).map_err(|err| err.to_string())?;
    if object.kind != ObjectKind::Blob {
        return Err(format!("{want} is a {}, not a blob", object.kind.as_str()));
    }
    let text = std::str::from_utf8(&object.data)
        .map_err(|_| format!("blob {} is not UTF-8 text", entry.id))?;
    let mut end = max_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(text[..end].to_string())
}

pub fn last_touch(root: &Path, path: &str) -> Result<Option<Rev>, String> {
    let repo = Repository::open(root).map_err(|err| err.to_string())?;
    let head = repo.resolve("HEAD").map_err(|err| err.to_string())?;
    let records = repo
        .history(
            head,
            HistoryOptions {
                max_commits: 30,
                first_parent: true,
                ..HistoryOptions::default()
            },
        )
        .map_err(|err| err.to_string())?;
    let want = normalize(path);
    for record in &records {
        let hit = match record.commit.parents.first() {
            Some(parent) => repo
                .diff_commits(*parent, record.commit.id)
                .map_err(|err| err.to_string())?
                .iter()
                .any(|change| change.path == want.as_bytes()),
            None => snap_has(&repo, &record.commit.id.to_string(), &want)?,
        };
        if hit {
            return Ok(Some(rev_from(&record.commit)));
        }
    }
    Ok(None)
}

pub fn dirty_paths(root: &Path) -> Result<Vec<String>, String> {
    let repo = Repository::open(root).map_err(|err| err.to_string())?;
    let status = repo.status().map_err(|err| err.to_string())?;
    Ok(status
        .into_iter()
        .map(|entry| String::from_utf8_lossy(&entry.path).replace('\\', "/"))
        .collect())
}

pub fn known_repos(home: &Path) -> Vec<(String, PathBuf)> {
    let path = home.join(".weavatrix/graphs/repositories.json");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut repos = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("\"repoPath\"") {
        rest = &rest[start + "\"repoPath\"".len()..];
        let Some(repo_path) = next_string(rest) else {
            break;
        };
        let window = &rest[..rest.len().min(800)];
        let label = next_key(window, "label").unwrap_or_else(|| {
            Path::new(&repo_path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| repo_path.clone())
        });
        repos.push((label, PathBuf::from(repo_path)));
        if repos.len() == 32 {
            break;
        }
    }
    repos
}

pub fn linked(repos: &[(String, PathBuf)]) -> Result<Vec<String>, String> {
    if repos.len() < 2 {
        return Ok(Vec::new());
    }
    let set = RepositorySet::open(
        repos
            .iter()
            .map(|(name, path)| (name.clone(), path.clone())),
    )
    .map_err(|err| err.to_string())?;
    let shared = set
        .shared_commits(HistoryOptions {
            max_commits: 40,
            first_parent: true,
            ..HistoryOptions::default()
        })
        .map_err(|err| err.to_string())?;
    let mut lines = Vec::new();
    for item in shared {
        if item.repositories.len() < 2 {
            continue;
        }
        let mut names = item
            .repositories
            .iter()
            .filter_map(|id| set.name(*id).map(str::to_string))
            .collect::<Vec<_>>();
        names.sort();
        let line = names.join(" ~ ");
        if !line.is_empty() && !lines.contains(&line) {
            lines.push(line);
        }
        if lines.len() == 8 {
            break;
        }
    }
    Ok(lines)
}

fn snap_has(repo: &Repository, revision: &str, path: &str) -> Result<bool, String> {
    let snap = repo.snapshot(revision).map_err(|err| err.to_string())?;
    Ok(snap
        .entries
        .iter()
        .any(|entry| entry.path == path.as_bytes()))
}

fn rev_from(commit: &weavatrix_git::Commit) -> Rev {
    let when = commit
        .committer
        .as_ref()
        .or(commit.author.as_ref())
        .map(|sign| ymd(sign.timestamp))
        .unwrap_or_else(|| "unknown".into());
    let subject = commit.summary_lossy().replace(['\n', '\r', '\t'], " ");
    Rev {
        id: commit.id.to_string().chars().take(12).collect(),
        when,
        subject,
    }
}

fn normalize(path: &str) -> String {
    path.trim().trim_start_matches("./").replace('\\', "/")
}

fn next_key(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    next_string(after)
}

fn next_string(text: &str) -> Option<String> {
    let after = text.split_once('"')?.1;
    let raw = after.split_once('"')?.0;
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

pub fn ymd(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe as i64 + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn unix_epoch_is_the_civil_day() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(86_400), "1970-01-02");
    }

    #[test]
    fn history_and_blob_come_from_weavatrix() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-git-{n}"));
        fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("GIT_AUTHOR_NAME", "dev")
                .env("GIT_AUTHOR_EMAIL", "dev@example.com")
                .env("GIT_COMMITTER_NAME", "dev")
                .env("GIT_COMMITTER_EMAIL", "dev@example.com")
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init"]);
        fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        git(&["add", "a.rs"]);
        git(&["commit", "-m", "start the file"]);
        let log = recent(&root, 5).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].subject.contains("start the file"));
        let text = blob_text(&root, "HEAD", "a.rs", 1000).unwrap();
        assert!(text.contains("fn main"));
        let touch = last_touch(&root, "a.rs").unwrap().unwrap();
        assert!(touch.subject.contains("start the file"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_paths_are_read_as_written() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-reg-{n}"));
        fs::create_dir_all(home.join(".weavatrix/graphs")).unwrap();
        fs::write(
            home.join(".weavatrix/graphs/repositories.json"),
            r#"{"repositories":[{"repoPath":"/tmp/one","label":"one"},{"repoPath":"/tmp/two","label":"two"}]}"#,
        )
        .unwrap();
        let repos = known_repos(&home);
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].0, "one");
        assert_eq!(repos[1].1, PathBuf::from("/tmp/two"));
        let _ = fs::remove_dir_all(home);
    }
}
