use std::fs;
use std::path::Path;

const SKIP: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    ".weavatrix",
    "graphify-out",
];

struct FileTower {
    rel: String,
    module: String,
    lines: usize,
    symbols: usize,
    height: usize,
    ink: u8,
    entry: bool,
    text: String,
}

pub fn ink_code(name: &str) -> u8 {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("rs" | "py" | "go" | "c" | "h" | "cpp" | "hpp" | "swift" | "java" | "kt") => 196,
        Some("js" | "ts" | "tsx" | "jsx" | "mjs" | "css" | "html" | "vue" | "svelte") => 39,
        Some("md" | "json" | "toml" | "yml" | "yaml" | "xml" | "sql" | "sh") => 67,
        _ => 153,
    }
}

pub fn picture(root: &Path, roads: bool, externals: bool) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = 0usize;
    walk(root, root, 0, &mut dirs, &mut files);
    let mut lines = Vec::new();
    if files.is_empty() {
        lines.push("No towers.".into());
    }
    files.sort_by(|a, b| a.module.cmp(&b.module).then(b.height.cmp(&a.height)).then(a.rel.cmp(&b.rel)));
    let mut module = String::new();
    for (shown, file) in files.iter().enumerate() {
        if shown == 36 {
            lines.push(format!("… {} more files", files.len() - shown));
            break;
        }
        if file.module != module {
            module.clone_from(&file.module);
            lines.push(module.clone());
        }
        let bar = "█".repeat(file.height);
        let entry = if file.entry { "  entry" } else { "" };
        let short = file
            .rel
            .rsplit_once('/')
            .map(|(_, name)| name)
            .unwrap_or(file.rel.as_str());
        lines.push(format!(
            "\x1b[38;5;{}m{bar}\x1b[38;5;153m  {short}  {} lines  {} sym{entry}",
            file.ink, file.lines, file.symbols
        ));
    }
    if roads {
        let found = road_lines(&files);
        if found.is_empty() {
            lines.push("Roads  none".into());
        } else {
            lines.push("Roads".into());
            lines.extend(found);
        }
    }
    if externals {
        let deps = deps_of(root);
        if deps.is_empty() {
            lines.push("Extern  none".into());
        } else {
            lines.push(format!("Extern  {}", deps.join("  ")));
        }
    }
    lines
}

pub fn env_names(root: &Path) -> Vec<String> {
    [".env", ".env.local", ".env.example", ".envrc"]
        .into_iter()
        .filter(|name| root.join(name).is_file())
        .map(str::to_string)
        .collect()
}

pub fn env_example_keys(root: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join(".env.example")) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            continue;
        }
        keys.push(key.to_string());
        if keys.len() == 12 {
            break;
        }
    }
    keys
}

fn walk(root: &Path, dir: &Path, depth: usize, dirs: &mut usize, out: &mut Vec<FileTower>) {
    if depth > 6 || out.len() >= 240 || *dirs > 80 {
        return;
    }
    *dirs += 1;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut nested = Vec::new();
    for entry in entries.flatten() {
        if out.len() >= 240 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            nested.push(path);
            continue;
        }
        if let Some(tower) = tower(root, &path) {
            out.push(tower);
        }
    }
    for path in nested {
        walk(root, &path, depth + 1, dirs, out);
    }
}

fn tower(root: &Path, path: &Path) -> Option<FileTower> {
    let rel = path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    if rel.ends_with(".lock") || rel.ends_with(".png") || rel.ends_with(".jpg") {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 256 * 1024 || bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    let symbols = text.lines().filter(|line| is_symbol(line)).count();
    let score = symbols + lines / 25;
    let height = score.clamp(1, 12);
    let module = match rel.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').next().unwrap_or(dir).to_string(),
        None => "(root)".into(),
    };
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let entry = matches!(name, "main.rs" | "lib.rs" | "index.js" | "index.ts" | "index.tsx")
        || text.lines().any(|line| line.trim() == "fn main()" || line.trim().starts_with("fn main("));
    Some(FileTower {
        ink: ink_code(name),
        rel,
        module,
        lines,
        symbols,
        height,
        entry,
        text,
    })
}

fn is_symbol(line: &str) -> bool {
    let mut text = line.trim();
    for prefix in ["pub(crate) ", "pub ", "export default ", "export ", "async ", "default "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
        }
    }
    [
        "fn ", "struct ", "enum ", "trait ", "impl ", "class ", "def ", "func ", "function ",
        "type ",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

fn road_lines(files: &[FileTower]) -> Vec<String> {
    let mut roads = Vec::new();
    for file in files {
        let dir = file.rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        for line in file.text.lines() {
            let text = line.trim();
            let Some(rest) = text.strip_prefix("mod ") else {
                continue;
            };
            let name = rest.trim().trim_end_matches(';').trim();
            if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                continue;
            }
            let sibling = if dir.is_empty() {
                format!("{name}.rs")
            } else {
                format!("{dir}/{name}.rs")
            };
            let nested = if dir.is_empty() {
                format!("{name}/mod.rs")
            } else {
                format!("{dir}/{name}/mod.rs")
            };
            if files.iter().any(|other| other.rel == sibling || other.rel == nested) {
                let target = if files.iter().any(|other| other.rel == sibling) {
                    sibling
                } else {
                    nested
                };
                let road = format!("{} — {target}", file.rel);
                if !roads.contains(&road) {
                    roads.push(road);
                }
            }
            if roads.len() == 12 {
                return roads;
            }
        }
    }
    roads
}

fn deps_of(root: &Path) -> Vec<String> {
    let mut deps = Vec::new();
    if let Ok(text) = fs::read_to_string(root.join("Cargo.toml")) {
        let mut on = false;
        for line in text.lines() {
            let text = line.trim();
            if text.starts_with('[') {
                on = text.starts_with("[dependencies")
                    || text.starts_with("[dev-dependencies")
                    || text.starts_with("[build-dependencies");
                continue;
            }
            if !on || text.starts_with('#') {
                continue;
            }
            if let Some((name, _)) = text.split_once('=') {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() && !deps.contains(&name.to_string()) {
                    deps.push(name.to_string());
                }
            }
        }
    }
    if let Ok(text) = fs::read_to_string(root.join("package.json"))
        && let Some(body) = text.split("\"dependencies\"").nth(1)
    {
        let body = body.split('}').next().unwrap_or("");
        for part in body.split('"').skip(1).step_by(2) {
            if part != ":" && !part.contains('{') && !deps.contains(&part.to_string()) {
                deps.push(part.to_string());
            }
        }
    }
    deps.truncate(16);
    deps
}

pub fn rel(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

pub fn root_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn file_is_a_tower_with_a_road_and_an_external() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-health-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "mod app;\nfn main() {}\n").unwrap();
        fs::write(root.join("src/app.rs"), "pub fn run() {}\nstruct Desk {}\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[dependencies]\nlibc = \"0.2\"\n").unwrap();
        let picture = picture(&root, true, true);
        let joined = picture.join("\n");
        assert!(joined.contains("main.rs"));
        assert!(joined.contains("entry"));
        assert!(joined.contains("src/main.rs — src/app.rs"));
        assert!(joined.contains("libc"));
        assert!(joined.contains("38;5;196"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn env_example_lists_names_only() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-env-{n}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".env"), "SECRET=hidden\n").unwrap();
        fs::write(
            root.join(".env.example"),
            "# note\nAPI_URL=\nexport TOKEN=nope\n",
        )
        .unwrap();
        assert!(env_names(&root).iter().any(|name| name == ".env"));
        assert_eq!(
            env_example_keys(&root),
            vec!["API_URL".to_string(), "TOKEN".to_string()]
        );
        let _ = fs::remove_dir_all(root);
    }
}
