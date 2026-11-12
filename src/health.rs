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
    chars: usize,
    symbols: usize,
    entry: bool,
    text: String,
}

struct FileMeter {
    name: String,
    rel: String,
    module: String,
    lines: usize,
    chars: usize,
    symbols: usize,
    entry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
}

#[derive(Clone)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
    pub rel: String,
    pub lines: usize,
    pub chars: usize,
    pub broken: bool,
    pub entry: bool,
    /// Why this row is unhealthy. Empty when the import resolves.
    pub why: String,
}

pub struct Board {
    files: Vec<FileMeter>,
    roads: Option<Vec<String>>,
    deps: Option<Vec<String>>,
    broken: Vec<(String, String)>,
}

impl Board {
    pub fn empty() -> Self {
        Self {
            files: Vec::new(),
            roads: None,
            deps: None,
            broken: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn unhealthy_count(&self) -> usize {
        self.broken.len()
    }

    pub fn is_broken(&self, rel: &str) -> bool {
        self.broken.iter().any(|(item, _)| item == rel)
    }

    pub fn fault_of(&self, rel: &str) -> Option<&str> {
        self.broken
            .iter()
            .find(|(item, _)| item == rel)
            .map(|(_, why)| why.as_str())
    }

    pub fn faults(&self) -> impl Iterator<Item = (&str, &str)> {
        self.broken.iter().map(|(rel, why)| (rel.as_str(), why.as_str()))
    }

    /// Next path segment under `dir`. `dir` is empty at the repo root.
    /// A directory is unhealthy when any file under it has a broken local import.
    pub fn children(&self, dir: &str) -> Vec<Entry> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut dirs: Vec<(String, usize, usize, bool, String)> = Vec::new();
        let mut files = Vec::new();
        for file in &self.files {
            let rest = if dir.is_empty() {
                file.rel.as_str()
            } else if let Some(rest) = file.rel.strip_prefix(&prefix) {
                rest
            } else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            let broken = self.is_broken(&file.rel);
            let why = if broken {
                self.fault_of(&file.rel).unwrap_or("unhealthy").to_string()
            } else {
                String::new()
            };
            if let Some((name, _)) = rest.split_once('/') {
                if let Some(slot) = dirs.iter_mut().find(|(have, _, _, _, _)| have == name) {
                    slot.1 += file.lines;
                    slot.2 += file.chars;
                    slot.3 |= broken;
                    if slot.4.is_empty() {
                        slot.4 = why;
                    } else if !why.is_empty() && slot.4 != why {
                        slot.4 = "unhealthy".into();
                    }
                } else {
                    dirs.push((name.to_string(), file.lines, file.chars, broken, why));
                }
            } else {
                files.push(Entry {
                    kind: EntryKind::File,
                    name: file.name.clone(),
                    rel: file.rel.clone(),
                    lines: file.lines,
                    chars: file.chars,
                    broken,
                    entry: file.entry,
                    why,
                });
            }
        }
        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        files.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = Vec::new();
        for (name, lines, chars, broken, why) in dirs {
            let rel = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            out.push(Entry {
                kind: EntryKind::Dir,
                name,
                rel,
                lines,
                chars,
                broken,
                entry: false,
                why: if broken && why.is_empty() {
                    "unhealthy".into()
                } else {
                    why
                },
            });
        }
        out.extend(files);
        out
    }
}

pub fn ink_code(name: &str) -> u8 {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("rs" | "py" | "go" | "c" | "h" | "cpp" | "hpp" | "swift" | "java" | "kt") => 196,
        Some("js" | "ts" | "tsx" | "jsx" | "mjs" | "css" | "html" | "vue" | "svelte") => 39,
        Some("md" | "json" | "toml" | "yml" | "yaml" | "xml" | "sql" | "sh") => 67,
        _ => 153,
    }
}

#[cfg(test)]
pub fn picture(root: &Path, roads: bool, externals: bool, width: usize) -> Vec<String> {
    let board = scan(root, roads, externals);
    let mut lines = Vec::new();
    if board.files.is_empty() {
        lines.push("No towers.".into());
    }
    lines.extend(paint(&board, width));
    lines
}

pub fn scan(root: &Path, roads: bool, externals: bool) -> Board {
    let mut towers = Vec::new();
    let mut dirs = 0usize;
    walk(root, root, 0, &mut dirs, &mut towers);
    towers.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then(b.lines.cmp(&a.lines))
            .then(a.rel.cmp(&b.rel))
    });
    let roads = if roads {
        Some(road_lines(&towers))
    } else {
        None
    };
    let deps = if externals { Some(deps_of(root)) } else { None };
    let broken = broken_rels(&towers);
    let files = towers
        .into_iter()
        .map(|file| {
            let name = file
                .rel
                .rsplit_once('/')
                .map(|(_, name)| name)
                .unwrap_or(file.rel.as_str())
                .to_string();
            FileMeter {
                name,
                rel: file.rel,
                module: file.module,
                lines: file.lines,
                chars: file.chars,
                symbols: file.symbols,
                entry: file.entry,
            }
        })
        .collect();
    Board {
        files,
        roads,
        deps,
        broken,
    }
}

/// Root totals and module meters. The file list is `children`, not this board.
pub fn overview(board: &Board, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if board.files.is_empty() {
        return lines;
    }
    let line_count: usize = board.files.iter().map(|file| file.lines).sum();
    let chars: usize = board.files.iter().map(|file| file.chars).sum();
    let syms: usize = board.files.iter().map(|file| file.symbols).sum();
    lines.push(format!(
        "\x1b[38;5;131mfiles\x1b[38;5;203m {}   \x1b[38;5;131mlines\x1b[38;5;51m {}   \x1b[38;5;131mchars\x1b[38;5;51m {}   \x1b[38;5;131msym\x1b[38;5;220m {}   \x1b[38;5;131munhealthy\x1b[38;5;196m {}",
        board.files.len(),
        compact_trim(line_count),
        compact_trim(chars),
        compact_trim(syms),
        board.broken.len()
    ));
    let (cols, name_w, track) = meter_layout(width);
    let modules = module_totals(&board.files);
    if modules.len() >= 2 {
        let total = modules.iter().map(|(_, n)| *n).sum::<usize>().max(1);
        let meters = modules
            .iter()
            .map(|(name, n)| meter(name, *n, total, name_w, track, false, None))
            .collect::<Vec<_>>();
        lines.extend(meter_rows(&meters, cols));
    }
    lines
}

/// Roads and external crates. Shown at the repo root when those flags are on.
pub fn extras(board: &Board) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(found) = &board.roads {
        if found.is_empty() {
            lines.push("Roads  none".into());
        } else {
            lines.push("Roads".into());
            lines.extend(found.iter().cloned());
        }
    }
    if let Some(deps) = &board.deps {
        if deps.is_empty() {
            lines.push("Extern  none".into());
        } else {
            lines.push("Extern".into());
            for dep in deps {
                lines.push(format!("\x1b[38;5;39m▄▄\x1b[38;5;203m  {dep}"));
            }
        }
    }
    lines
}

pub fn entry_line(entry: &Entry) -> String {
    let mark = if entry.broken {
        "\x1b[38;5;196m!\x1b[38;5;203m"
    } else {
        " "
    };
    let mut name = if entry.kind == EntryKind::Dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    if name.chars().count() > 18 {
        name = name.chars().take(17).collect();
        name.push('~');
    }
    let pad = " ".repeat(18 - name.chars().count());
    let hue = if entry.kind == EntryKind::Dir {
        220
    } else {
        ink_code(&entry.name)
    };
    let entry_mark = if entry.entry { "  entry" } else { "" };
    let why = if entry.why.is_empty() {
        String::new()
    } else {
        format!("  \x1b[38;5;131m{}", entry.why)
    };
    format!(
        "{mark} \x1b[38;5;{hue}m{name}\x1b[38;5;203m{pad} {} {:>6} ln  {:>6} ch{entry_mark}{why}",
        spark(entry.lines, entry.broken),
        compact_trim(entry.lines),
        compact_trim(entry.chars)
    )
}

fn spark(lines: usize, broken: bool) -> String {
    let n = match lines {
        0 => 0,
        1..=30 => 1,
        31..=120 => 2,
        121..=400 => 3,
        401..=1_200 => 4,
        1_201..=4_000 => 5,
        4_001..=12_000 => 6,
        _ => 8,
    };
    let ink = if broken { "38;5;196" } else { "38;5;46" };
    let fill = "█".repeat(n);
    let rest = "░".repeat(8 - n);
    format!("\x1b[{ink}m{fill}\x1b[38;5;240m{rest}\x1b[38;5;203m")
}

/// htop-style meters. Two columns when the row is wide enough.
/// The bracket fills by share of lines: green, then gold, then red.
/// The desk list uses `children` instead. `picture` keeps this layout for tests.
#[cfg_attr(not(test), allow(dead_code))]
pub fn paint(board: &Board, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if !board.files.is_empty() {
        let (cols, name_w, track) = meter_layout(width);
        lines.push(summary(&board.files));
        let modules = module_totals(&board.files);
        if modules.len() >= 2 {
            let total = modules.iter().map(|(_, n)| *n).sum::<usize>().max(1);
            let meters = modules
                .iter()
                .map(|(name, n)| meter(name, *n, total, name_w, track, false, None))
                .collect::<Vec<_>>();
            lines.extend(meter_rows(&meters, cols));
        }
        let max_lines = board
            .files
            .iter()
            .map(|file| file.lines)
            .max()
            .unwrap_or(1)
            .max(1);
        let mut module = String::new();
        let mut batch = Vec::new();
        let mut shown = 0usize;
        for file in &board.files {
            if shown == 48 {
                if !batch.is_empty() {
                    lines.push(batch.join("  "));
                    batch.clear();
                }
                lines.push(format!("… {} more files", board.files.len() - shown));
                break;
            }
            if file.module != module {
                if !batch.is_empty() {
                    lines.push(batch.join("  "));
                    batch.clear();
                }
                module.clone_from(&file.module);
                lines.push(format!("\x1b[38;5;131m{module}\x1b[38;5;203m"));
            }
            batch.push(meter(
                &file.name,
                file.lines,
                max_lines,
                name_w,
                track,
                file.entry,
                Some(file.symbols),
            ));
            if batch.len() == cols {
                lines.push(batch.join("  "));
                batch.clear();
            }
            shown += 1;
        }
        if !batch.is_empty() {
            lines.push(batch.join("  "));
        }
    }
    if let Some(found) = &board.roads {
        if found.is_empty() {
            lines.push("Roads  none".into());
        } else {
            lines.push("Roads".into());
            lines.extend(found.iter().cloned());
        }
    }
    if let Some(deps) = &board.deps {
        if deps.is_empty() {
            lines.push("Extern  none".into());
        } else {
            lines.push("Extern".into());
            for dep in deps {
                lines.push(format!("\x1b[38;5;39m▄▄\x1b[38;5;203m  {dep}"));
            }
        }
    }
    lines
}

#[cfg_attr(not(test), allow(dead_code))]
fn summary(files: &[FileMeter]) -> String {
    let lines: usize = files.iter().map(|file| file.lines).sum();
    let syms: usize = files.iter().map(|file| file.symbols).sum();
    format!(
        "\x1b[38;5;131mfiles\x1b[38;5;203m {}   \x1b[38;5;131mlines\x1b[38;5;51m {}   \x1b[38;5;131msym\x1b[38;5;220m {}",
        files.len(),
        compact_trim(lines),
        compact_trim(syms)
    )
}

fn module_totals(files: &[FileMeter]) -> Vec<(String, usize)> {
    let mut totals = Vec::new();
    for file in files {
        if let Some((_, total)) = totals.iter_mut().find(|(name, _)| name == &file.module) {
            *total += file.lines;
        } else {
            totals.push((file.module.clone(), file.lines));
        }
    }
    totals
}

fn meter_rows(meters: &[String], cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut batch = Vec::new();
    for meter in meters {
        batch.push(meter.clone());
        if batch.len() == cols {
            lines.push(batch.join("  "));
            batch.clear();
        }
    }
    if !batch.is_empty() {
        lines.push(batch.join("  "));
    }
    lines
}

fn meter_layout(width: usize) -> (usize, usize, usize) {
    let width = width.max(40);
    let cols: usize = if width >= 156 {
        3
    } else if width >= 84 {
        2
    } else {
        1
    };
    let col_w = (width - 2 * cols.saturating_sub(1)) / cols;
    let name_w = if col_w >= 44 { 14 } else { 10 };
    let track = col_w.saturating_sub(name_w + 13).clamp(6, 20);
    (cols, name_w, track)
}

fn meter(
    name: &str,
    value: usize,
    max: usize,
    name_w: usize,
    track: usize,
    entry: bool,
    sym: Option<usize>,
) -> String {
    let label = clip_name(name, name_w);
    let ratio = if max == 0 {
        0.0
    } else {
        (value as f64 / max as f64).clamp(0.0, 1.0)
    };
    let fill = if value == 0 {
        0
    } else {
        ((track as f64) * ratio).round().clamp(1.0, track as f64) as usize
    };
    let color = if ratio < 0.35 {
        46
    } else if ratio < 0.70 {
        220
    } else {
        196
    };
    let blocks = "█".repeat(fill);
    let gap = " ".repeat(track - fill);
    let mut line = format!(
        "{label} [\x1b[38;5;{color}m{blocks}\x1b[38;5;203m{gap}{}]",
        compact(value)
    );
    if let Some(sym) = sym {
        line.push_str(&format!(" {sym:>4}"));
    }
    if entry {
        line.push_str(" entry");
    }
    line
}

fn clip_name(name: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out: String = name.chars().take(width).collect();
    if name.chars().count() > width {
        out.pop();
        out.push('~');
    }
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn compact(n: usize) -> String {
    let text = compact_trim(n);
    let mut text: String = text.chars().take(5).collect();
    while text.chars().count() < 5 {
        text.insert(0, ' ');
    }
    text
}

fn compact_trim(n: usize) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 100_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else if n < 1_000_000 {
        format!("{}k", n / 1000)
    } else {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    }
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
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
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
    let rel = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if rel.ends_with(".lock") || rel.ends_with(".png") || rel.ends_with(".jpg") {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 256 * 1024 || bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    let chars = text.chars().count();
    let symbols = text.lines().filter(|line| is_symbol(line)).count();
    let module = match rel.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').next().unwrap_or(dir).to_string(),
        None => "(root)".into(),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let entry = matches!(
        name,
        "main.rs" | "lib.rs" | "index.js" | "index.ts" | "index.tsx"
    ) || text
        .lines()
        .any(|line| line.trim() == "fn main()" || line.trim().starts_with("fn main("));
    Some(FileTower {
        rel,
        module,
        lines,
        chars,
        symbols,
        entry,
        text,
    })
}

fn is_symbol(line: &str) -> bool {
    let mut text = line.trim();
    for prefix in [
        "pub(crate) ",
        "pub ",
        "export default ",
        "export ",
        "async ",
        "default ",
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
        }
    }
    [
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "class ",
        "def ",
        "func ",
        "function ",
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
            if let Some(name) = rust_mod(text) {
                let sibling = join(dir, &format!("{name}.rs"));
                let nested = join(dir, &format!("{name}/mod.rs"));
                let _ = push_road(&mut roads, &file.rel, &sibling, files)
                    || push_road(&mut roads, &file.rel, &nested, files);
            }
            if let Some(spec) = rust_use(text) {
                for target in rust_use_files(dir, spec) {
                    if push_road(&mut roads, &file.rel, &target, files) {
                        break;
                    }
                }
            }
            if let Some(spec) = js_import(text)
                && let Some(target) = resolve_relative(dir, spec, files)
            {
                let _ = push_road(&mut roads, &file.rel, &target, files);
            }
            if roads.len() == 12 {
                return roads;
            }
        }
    }
    roads
}

fn push_road(roads: &mut Vec<String>, from: &str, target: &str, files: &[FileTower]) -> bool {
    if !files.iter().any(|file| file.rel == target) {
        return false;
    }
    let road = format!("{from} — {target}");
    if !roads.contains(&road) {
        roads.push(road);
    }
    true
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn rust_mod(text: &str) -> Option<&str> {
    let mut text = text.trim();
    for prefix in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
            break;
        }
    }
    let name = text
        .strip_prefix("mod ")?
        .trim()
        .trim_end_matches(';')
        .trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        None
    } else {
        Some(name)
    }
}

fn rust_use(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("use ")?.trim();
    let path = rest.split([' ', ';', '{']).next()?.trim();
    if path.is_empty() { None } else { Some(path) }
}

fn rust_use_files(dir: &str, spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rest) = spec.strip_prefix("crate::") {
        let parts: Vec<&str> = rest.split("::").filter(|part| !part.is_empty()).collect();
        for len in (1..=parts.len()).rev() {
            let joined = parts[..len].join("/");
            out.push(format!("src/{joined}.rs"));
            out.push(format!("src/{joined}/mod.rs"));
        }
        return out;
    }
    if let Some(rest) = spec.strip_prefix("super::") {
        let parent = dir.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
        let parts: Vec<&str> = rest.split("::").filter(|part| !part.is_empty()).collect();
        for len in (1..=parts.len()).rev() {
            out.push(join(parent, &format!("{}.rs", parts[..len].join("/"))));
        }
    }
    out
}

fn js_import(text: &str) -> Option<&str> {
    if let Some(spec) = quoted_after(text, "from ") {
        return Some(spec);
    }
    if let Some(spec) = quoted_after(text, "import ") {
        return Some(spec);
    }
    quoted_after(text, "require(")
}

fn quoted_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text.split_once(key)?.1.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    rest[1..].split(quote).next()
}

fn resolve_relative(dir: &str, spec: &str, files: &[FileTower]) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for piece in spec.split('/') {
        if piece.is_empty() || piece == "." {
            continue;
        }
        if piece == ".." {
            parts.pop();
            continue;
        }
        parts.push(piece);
    }
    let joined = parts.join("/");
    let mut candidates = vec![joined.clone()];
    for ext in ["js", "jsx", "ts", "tsx", "mjs", "rs"] {
        candidates.push(format!("{joined}.{ext}"));
    }
    for index in ["index.js", "index.ts", "index.tsx", "mod.rs"] {
        candidates.push(format!("{joined}/{index}"));
    }
    candidates
        .into_iter()
        .find(|candidate| files.iter().any(|file| &file.rel == candidate))
}

fn broken_rels(files: &[FileTower]) -> Vec<(String, String)> {
    let mut broken = Vec::new();
    for file in files {
        if let Some(why) = import_missing(file, files) {
            broken.push((file.rel.clone(), why));
        }
    }
    broken
}

fn import_missing(file: &FileTower, files: &[FileTower]) -> Option<String> {
    let dir = file.rel.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let ext = file.rel.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    for line in file.text.lines() {
        let text = line.trim();
        if ext == "rs" {
            if let Some(name) = rust_mod(text) {
                let sibling = join(dir, &format!("{name}.rs"));
                let nested = join(dir, &format!("{name}/mod.rs"));
                if !has_file(files, &sibling) && !has_file(files, &nested) {
                    return Some(format!("missing {name}.rs"));
                }
            }
            if let Some(spec) = rust_use(text)
                && !spec.contains('*')
                && let Some(why) = crate_fault(spec, files)
            {
                return Some(why);
            }
        }
        if matches!(ext, "js" | "jsx" | "ts" | "tsx" | "mjs")
            && let Some(spec) = js_import(text)
            && spec.starts_with('.')
            && resolve_relative(dir, spec, files).is_none()
        {
            return Some(format!("missing {spec}"));
        }
    }
    None
}

/// `use crate::a::b::symbol` is unhealthy when a module segment has no file.
/// The last segment is a symbol. A file module (`src/a.rs`, not `src/a/mod.rs`)
/// owns the segments after it, so those are not missing files.
fn crate_fault(spec: &str, files: &[FileTower]) -> Option<String> {
    let rest = spec.strip_prefix("crate::")?;
    if rest.contains('*') || rest.contains('{') {
        return None;
    }
    let parts: Vec<&str> = rest.split("::").filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let mods = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &parts[..]
    };
    let mut path = String::from("src");
    for (index, name) in mods.iter().enumerate() {
        if *name == "self" || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return None;
        }
        let file = format!("{path}/{name}.rs");
        let nested = format!("{path}/{name}/mod.rs");
        if has_file(files, &file) || has_file(files, &nested) {
            path = format!("{path}/{name}");
            continue;
        }
        if index > 0 {
            let parent_file = format!("{path}.rs");
            let parent_mod = format!("{path}/mod.rs");
            if has_file(files, &parent_file) && !has_file(files, &parent_mod) {
                return None;
            }
        }
        return Some(format!("missing {path}/{name}.rs"));
    }
    None
}

fn has_file(files: &[FileTower], rel: &str) -> bool {
    files.iter().any(|file| file.rel == rel)
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
        let picture = picture(&root, true, true, 96);
        let joined = picture.join("\n");
        assert!(joined.contains("main.rs"));
        assert!(joined.contains("entry"));
        assert!(joined.contains("src/main.rs — src/app.rs"));
        assert!(joined.contains("libc"));
        assert!(joined.contains("▄▄"));
        assert!(joined.contains("38;5;196"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn roads_follow_use_and_relative_import() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-roads-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::write(
            root.join("src/main.rs"),
            "use crate::app::run;\nfn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("src/app.rs"), "pub fn run() {}\n").unwrap();
        fs::write(
            root.join("web/index.js"),
            "import {a} from \"./util.js\";\n",
        )
        .unwrap();
        fs::write(root.join("web/util.js"), "export function a() {}\n").unwrap();
        let joined = picture(&root, true, false, 96).join("\n");
        assert!(joined.contains("src/main.rs — src/app.rs"));
        assert!(joined.contains("web/index.js — web/util.js"));
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

    #[test]
    fn meters_share_a_row_and_the_bar_follows_the_lines() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-meters-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        let big = "fn big() {}\n".repeat(80);
        fs::write(root.join("src/big.rs"), &big).unwrap();
        fs::write(root.join("src/tiny.rs"), "fn tiny() {}\n").unwrap();
        let picture = picture(&root, false, false, 100);
        let line = picture
            .iter()
            .find(|line| line.contains("big.rs") && line.contains("tiny.rs"))
            .expect("two meters on one row");
        let parts: Vec<&str> = line.split(']').collect();
        assert!(parts.len() >= 2, "{line}");
        let big_blocks = parts[0].matches('█').count();
        let tiny_blocks = parts[1].matches('█').count();
        assert!(big_blocks > tiny_blocks, "{line}");
        assert!(parts[0].contains("38;5;196"), "{line}");
        assert!(parts[1].contains("38;5;46"), "{line}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_opens_onto_files_and_a_missing_mod_is_unhealthy() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-drill-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::write(root.join("src/main.rs"), "mod gone;\nfn main() {}\n").unwrap();
        fs::write(root.join("src/app.rs"), "pub fn run() {}\n").unwrap();
        fs::write(root.join("web/index.js"), "import {a} from \"./missing.js\";\n").unwrap();
        fs::write(root.join("README.md"), "hello desk\n").unwrap();
        let board = scan(&root, false, false);
        let top = board.children("");
        let src = top.iter().find(|entry| entry.name == "src").expect("src");
        assert_eq!(src.kind, EntryKind::Dir);
        assert!(src.broken, "a missing mod marks the folder");
        assert!(src.lines >= 2);
        assert!(src.chars > src.lines);
        let src_line = entry_line(src);
        assert!(src_line.contains("src/"), "{src_line}");
        assert!(src_line.contains("ln"), "{src_line}");
        assert!(src_line.contains("ch"), "{src_line}");
        assert!(src_line.contains("38;5;196m!"), "{src_line}");
        assert!(src_line.contains('█'), "{src_line}");
        assert!(src_line.contains("missing gone.rs"), "{src_line}");
        let js = board
            .children("web")
            .into_iter()
            .find(|entry| entry.name == "index.js")
            .expect("index.js");
        assert!(js.why.contains("missing.js"), "{}", js.why);
        let inside = board.children("src");
        let main = inside
            .iter()
            .find(|entry| entry.name == "main.rs")
            .expect("main.rs");
        assert_eq!(main.kind, EntryKind::File);
        assert!(main.broken);
        assert!(main.entry);
        assert!(entry_line(main).contains("missing gone.rs"));
        let app = inside.iter().find(|entry| entry.name == "app.rs").unwrap();
        assert!(!app.broken);
        assert!(board.is_broken("web/index.js"));
        assert!(!board.is_broken("src/app.rs"));
        assert!(board.is_broken("src/main.rs"));
        let head = overview(&board, 96).join("\n");
        assert!(head.contains("chars"), "{head}");
        assert!(head.contains("unhealthy"), "{head}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_nested_crate_path_names_the_missing_file() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-nest-{n}"));
        fs::create_dir_all(root.join("src/foo")).unwrap();
        fs::write(root.join("src/foo/mod.rs"), "pub fn stay() {}\n").unwrap();
        fs::write(
            root.join("src/main.rs"),
            "use crate::foo::gone::run;\nuse crate::app::widget;\nfn main() {}\n",
        )
        .unwrap();
        fs::write(root.join("src/app.rs"), "pub fn widget() {}\n").unwrap();
        let board = scan(&root, false, false);
        assert_eq!(
            board.fault_of("src/main.rs"),
            Some("missing src/foo/gone.rs")
        );
        assert!(!board.is_broken("src/app.rs"));
        let _ = fs::remove_dir_all(root);
    }
}
