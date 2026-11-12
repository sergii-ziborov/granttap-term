use std::path::Path;
use std::process::Command;

/// A web address, including a bare host such as `google.com`.
/// A path that exists on disk, or a lone filename, stays a file.
pub fn web_url(target: &str, dir: &Path) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    if target.starts_with("http://") || target.starts_with("https://") {
        return Some(target.to_string());
    }
    if let Some(rest) = target.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if looks_like_file(target, dir) || lone_filename(target) {
        return None;
    }
    let host = target
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(target);
    let host = host.rsplit('@').next().unwrap_or(host);
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    dotted_host(name).then(|| format!("https://{target}"))
}

pub struct PageDoc {
    pub lines: Vec<String>,
    pub bytes: u64,
}

pub fn load(target: &str, dir: &Path) -> Result<Vec<String>, String> {
    Ok(load_doc(target, dir)?.lines)
}

pub fn load_doc(target: &str, dir: &Path) -> Result<PageDoc, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("browse needs a path or an http(s) URL".into());
    }
    let web = web_url(target, dir);
    let bytes = if let Some(url) = &web {
        fetch(url, 12)?
    } else {
        let path = if Path::new(target).is_absolute() {
            Path::new(target).to_path_buf()
        } else {
            dir.join(target)
        };
        std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?
    };
    let full = bytes.len() as u64;
    let clipped = bytes.len() > 500_000;
    let bytes = if clipped {
        bytes[..500_000].to_vec()
    } else {
        bytes
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = render(&text);
    if lines.len() > 160 {
        lines.truncate(160);
    }
    if clipped {
        lines.push("page clipped".into());
    }
    let shots = page_shots(&text, web.as_deref(), dir);
    if let Some(url) = &web {
        let shown = short_href(url);
        let shown = if shown.is_empty() { url.clone() } else { shown };
        lines.insert(0, format!("url  <{shown}>"));
    }
    if !shots.is_empty() {
        let at = usize::from(web.is_some());
        let mut block = Vec::new();
        for shot in &shots {
            block.extend(shot.lines().filter(|line| !line.is_empty()).map(str::to_string));
        }
        for (offset, line) in block.into_iter().enumerate() {
            lines.insert((at + offset).min(lines.len()), line);
        }
    }
    if lines.is_empty() {
        lines.push("Empty page.".into());
    }
    Ok(PageDoc {
        lines,
        bytes: full.max(1),
    })
}

fn fetch(url: &str, seconds: u64) -> Result<Vec<u8>, String> {
    let timeout = seconds.to_string();
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--max-time",
        &timeout,
        "--silent",
        "--show-error",
        "--location",
        "--compressed",
        "--fail",
        "-A",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        url,
    ]);
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd
        .output()
        .map_err(|err| format!("curl: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("browse failed: {}", err.trim()));
    }
    Ok(output.stdout)
}

pub fn render(html: &str) -> Vec<String> {
    if !looks_html(html) {
        let mut lines: Vec<String> = html
            .lines()
            .map(|line| clip(line.trim_end(), 200))
            .filter(|line| !line.is_empty())
            .collect();
        if lines.is_empty() {
            lines.push("Empty page.".into());
        }
        lines.truncate(180);
        return lines;
    }
    let lower = html.to_ascii_lowercase();
    let mut title = String::new();
    if let Some(start) = lower.find("<title")
        && let Some(open) = lower[start..].find('>')
        && let Some(end) = lower[start + open + 1..].find("</title>")
    {
        let from = start + open + 1;
        title = decode(html[from..from + end].trim());
    }
    let mut build = Build::default();
    if !title.is_empty() {
        build.lines.push(format!("title  {title}"));
    }
    let mut chars = html.chars();
    while let Some(ch) = chars.next() {
        if build.lines.len() >= 180 {
            break;
        }
        if ch == '<' {
            let mut tag = String::from("<");
            let mut closed = false;
            for next in chars.by_ref() {
                if tag.len() < 4000 {
                    tag.push(next);
                }
                if next == '>' {
                    closed = true;
                    break;
                }
            }
            if !closed {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
            }
            build.on_tag(&tag);
            continue;
        }
        build.push_char(ch);
    }
    build.flush();
    build.lines.truncate(180);
    build.lines
}

#[derive(Default)]
struct Build {
    lines: Vec<String>,
    buf: String,
    skip: u8,
    pre: bool,
    in_ol: bool,
    ol_n: usize,
    hrefs: Vec<String>,
}

impl Build {
    fn on_tag(&mut self, raw: &str) {
        let low = raw.to_ascii_lowercase();
        if tag_is(&low, "<script") {
            self.skip = 1;
            return;
        }
        if tag_is(&low, "<style") {
            self.skip = 2;
            return;
        }
        if tag_is(&low, "</script") || tag_is(&low, "</style") {
            self.skip = 0;
            return;
        }
        if self.skip != 0 || low.starts_with("<!--") {
            return;
        }
        if tag_is(&low, "<img") {
            self.flush();
            let alt = attr(raw, "alt").unwrap_or_default();
            if alt.trim().is_empty() {
                return;
            }
            self.lines.push(clip(&decode(&image_mark(raw)), 96));
            return;
        }
        if tag_is(&low, "<meta") {
            let name = attr(raw, "name").unwrap_or_default();
            if name.eq_ignore_ascii_case("description")
                && let Some(content) = attr(raw, "content")
            {
                self.flush();
                self.lines
                    .push(clip(&format!("meta  {}", decode(&content)), 200));
            }
            return;
        }
        if tag_is(&low, "<input") || tag_is(&low, "<textarea") {
            self.flush();
            let kind = attr(raw, "type").unwrap_or_default();
            if kind.eq_ignore_ascii_case("hidden")
                || kind.eq_ignore_ascii_case("file")
                || low.contains(" hidden")
            {
                return;
            }
            let name = attr(raw, "aria-label")
                .or_else(|| attr(raw, "placeholder"))
                .or_else(|| attr(raw, "value"))
                .or_else(|| attr(raw, "name"))
                .unwrap_or_else(|| "field".into());
            let mark = if kind.eq_ignore_ascii_case("submit") || kind.eq_ignore_ascii_case("button")
            {
                "button"
            } else {
                "field"
            };
            self.lines
                .push(clip(&format!("[{mark}] {}", decode(&name)), 200));
            return;
        }
        if tag_is(&low, "<pre") {
            self.flush();
            self.pre = true;
            return;
        }
        if tag_is(&low, "</pre") {
            self.flush();
            self.pre = false;
            return;
        }
        if tag_is(&low, "<ol") {
            self.flush();
            self.in_ol = true;
            self.ol_n = 0;
            return;
        }
        if tag_is(&low, "</ol") {
            self.flush();
            self.in_ol = false;
            return;
        }
        if tag_is(&low, "<ul") || tag_is(&low, "</ul") {
            self.flush();
            return;
        }
        if tag_is(&low, "<blockquote") {
            self.flush();
            self.buf = "> ".into();
            return;
        }
        if tag_is(&low, "<button") {
            self.flush();
            if let Some(label) = attr(raw, "aria-label").or_else(|| attr(raw, "value")) {
                self.lines
                    .push(clip(&format!("[button] {}", decode(&label)), 200));
                return;
            }
            self.buf = "[button] ".into();
            return;
        }
        if tag_is(&low, "<summary") {
            self.flush();
            self.buf = "summary  ".into();
            return;
        }
        if tag_is(&low, "<figcaption") {
            self.flush();
            self.buf = "caption  ".into();
            return;
        }
        if tag_is(&low, "<option") {
            self.flush();
            self.buf = "option ".into();
            return;
        }
        if tag_is(&low, "<code") && !self.pre {
            self.buf.push('`');
            return;
        }
        if tag_is(&low, "</code") && !self.pre {
            self.buf.push('`');
            return;
        }
        if tag_is(&low, "<li") {
            self.flush();
            if self.in_ol {
                self.ol_n += 1;
                self.buf = format!("{}. ", self.ol_n);
            } else {
                self.buf = "- ".into();
            }
            return;
        }
        if low.starts_with("<h")
            && low.as_bytes().get(2).is_some_and(u8::is_ascii_digit)
            && tag_is(&low, &format!("<h{}", low.as_bytes()[2] as char))
        {
            self.flush();
            let level = (low.as_bytes()[2] - b'0').clamp(1, 6);
            self.buf = "#".repeat(level as usize);
            self.buf.push(' ');
            return;
        }
        if tag_is(&low, "<a") {
            self.hrefs.push(attr(raw, "href").unwrap_or_default());
            return;
        }
        if tag_is(&low, "</a") {
            if let Some(href) = self.hrefs.pop() {
                let shown = short_href(&decode(&href));
                if !shown.is_empty() && !self.buf.contains(&shown) {
                    let addition = format!("<{shown}>");
                    let gap = usize::from(!self.buf.is_empty() && !self.buf.ends_with(' '));
                    if !self.buf.is_empty()
                        && self.buf.chars().count() + gap + addition.chars().count() > 96
                    {
                        self.flush();
                    }
                    if !self.buf.is_empty() && !self.buf.ends_with(' ') {
                        self.buf.push(' ');
                    }
                    self.buf.push_str(&addition);
                }
            }
            return;
        }
        if tag_is(&low, "<td") || tag_is(&low, "<th") {
            if !self.buf.trim().is_empty() {
                if !self.buf.ends_with(' ') {
                    self.buf.push(' ');
                }
                self.buf.push_str("| ");
            }
            return;
        }
        if tag_is(&low, "<tr")
            || tag_is(&low, "</tr")
            || tag_is(&low, "</h1")
            || tag_is(&low, "</h2")
            || tag_is(&low, "</h3")
            || tag_is(&low, "</h4")
            || tag_is(&low, "</h5")
            || tag_is(&low, "</h6")
            || tag_is(&low, "<p")
            || tag_is(&low, "</p")
            || tag_is(&low, "<div")
            || tag_is(&low, "</div")
            || tag_is(&low, "<br")
            || tag_is(&low, "</li")
            || tag_is(&low, "<table")
            || tag_is(&low, "</table")
            || tag_is(&low, "<hr")
            || tag_is(&low, "<blockquote")
            || tag_is(&low, "</blockquote")
            || tag_is(&low, "</button")
            || tag_is(&low, "</summary")
            || tag_is(&low, "</figcaption")
            || tag_is(&low, "<figure")
            || tag_is(&low, "</figure")
            || tag_is(&low, "<nav")
            || tag_is(&low, "</nav")
            || tag_is(&low, "<dt")
            || tag_is(&low, "<dd")
            || tag_is(&low, "</option")
            || tag_is(&low, "<label")
            || tag_is(&low, "</label")
            || tag_is(&low, "<select")
            || tag_is(&low, "</select")
            || tag_is(&low, "<header")
            || tag_is(&low, "</header")
            || tag_is(&low, "<footer")
            || tag_is(&low, "</footer")
            || tag_is(&low, "<section")
            || tag_is(&low, "</section")
            || tag_is(&low, "<article")
            || tag_is(&low, "</article")
            || tag_is(&low, "<main")
            || tag_is(&low, "</main")
            || tag_is(&low, "<aside")
            || tag_is(&low, "</aside")
        {
            self.flush();
            if tag_is(&low, "<hr") {
                self.lines.push("---".into());
            }
            if tag_is(&low, "<dd") {
                self.buf = "dd ".into();
            }
        }
    }

    fn push_char(&mut self, ch: char) {
        if self.skip != 0 {
            return;
        }
        if self.pre {
            if ch == '\n' {
                self.flush();
            } else {
                self.buf.push(ch);
            }
            return;
        }
        if ch.is_whitespace() {
            if !self.buf.is_empty() && !self.buf.ends_with(' ') {
                self.buf.push(' ');
            }
            return;
        }
        self.buf.push(ch);
    }

    fn flush(&mut self) {
        if self.lines.len() >= 180 {
            self.buf.clear();
            return;
        }
        let raw = std::mem::take(&mut self.buf);
        let line = if self.pre {
            decode(&raw)
        } else {
            decode(&raw.split_whitespace().collect::<Vec<_>>().join(" "))
        };
        if line.is_empty() {
            return;
        }
        self.lines.push(clip(&line, 96));
    }
}

/// A clickable address that fits on one row. The query of a long URL is
/// dropped at `?` so the closing `>` stays on the same line.
fn short_href(href: &str) -> String {
    let href = href.trim();
    if href.is_empty() || href == "#" || href.to_ascii_lowercase().starts_with("javascript:") {
        return String::new();
    }
    let chars: Vec<char> = href.chars().collect();
    if chars.len() <= 64 {
        return href.to_string();
    }
    if let Some(query) = chars.iter().position(|ch| *ch == '?')
        && (12..64).contains(&query)
    {
        return chars.into_iter().take(query).collect();
    }
    let budget = 60.min(chars.len());
    let mut end = budget;
    for index in (24..budget).rev() {
        if matches!(chars[index], '&' | '/' | '=') {
            end = index;
            break;
        }
    }
    chars.into_iter().take(end).collect()
}

fn looks_html(text: &str) -> bool {
    let head: String = text.chars().take(2000).collect();
    let lower = head.to_ascii_lowercase();
    ["<html", "<body", "<p", "<div", "<title"]
        .iter()
        .any(|tag| lower.contains(tag))
}

fn tag_is(low: &str, name: &str) -> bool {
    let Some(rest) = low.strip_prefix(name) else {
        return false;
    };
    rest.is_empty() || rest.starts_with([' ', '>', '/']) || rest.starts_with('\t')
}

fn image_mark(tag: &str) -> String {
    let alt = attr(tag, "alt").unwrap_or_default();
    let src = attr(tag, "src").unwrap_or_default();
    let src = if src.starts_with("data:") {
        "picture".to_string()
    } else {
        src
    };
    format!("[img {alt}] {src}")
}

fn attr(tag: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let after = tag.split_once(&needle)?.1.trim_start();
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        after[1..].split(quote).next().map(str::to_string)
    } else {
        Some(after.split_whitespace().next()?.to_string())
    }
}

fn looks_like_file(target: &str, dir: &Path) -> bool {
    if target.contains('\\')
        || target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with(".\\")
        || target.starts_with("..\\")
    {
        return true;
    }
    if target.len() >= 3 && target.as_bytes()[1] == b':' {
        return true;
    }
    Path::new(target).is_absolute() || dir.join(target).is_file()
}

fn lone_filename(target: &str) -> bool {
    if target.contains('/') || target.contains('\\') {
        return false;
    }
    let lower = target.to_ascii_lowercase();
    [
        ".html", ".htm", ".md", ".txt", ".png", ".jpg", ".jpeg", ".gif", ".rs", ".js", ".css",
        ".json", ".toml", ".xml", ".svg", ".pdf", ".yml", ".yaml", ".lock", ".csv",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn dotted_host(name: &str) -> bool {
    if name.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() == 4
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.parse::<u8>().is_ok())
    {
        return true;
    }
    if parts.len() < 2 {
        return false;
    }
    if parts.iter().any(|label| {
        label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    }) {
        return false;
    }
    let last = parts.last().copied().unwrap_or("");
    last.len() >= 2 && last.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn page_shots(html: &str, page: Option<&str>, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut tried = 0usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    for src in image_srcs(html) {
        if out.len() >= 2 || tried >= 4 || std::time::Instant::now() >= deadline {
            break;
        }
        tried += 1;
        let Some(bytes) = image_bytes(&src, page, dir) else {
            continue;
        };
        if bytes.len() < 32 {
            continue;
        }
        let Ok(image) = image::load_from_memory(&bytes) else {
            continue;
        };
        if image.width() < 16 || image.height() < 16 {
            continue;
        }
        let (cols, rows) = crate::draw::header_box(image.width(), image.height());
        let picture = crate::draw::portrait_of(&image, cols, rows);
        if crate::draw::has_blocks(&picture) {
            out.push(picture);
        }
    }
    out
}

fn image_srcs(html: &str) -> Vec<String> {
    let mut srcs = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("<meta") {
        let start = from + rel;
        let end = lower[start..]
            .find('>')
            .map(|n| start + n + 1)
            .unwrap_or(html.len());
        let tag = &html[start..end.min(html.len())];
        from = end;
        let key = attr(tag, "itemprop")
            .or_else(|| attr(tag, "property"))
            .unwrap_or_default();
        if (key.eq_ignore_ascii_case("image") || key.eq_ignore_ascii_case("og:image"))
            && let Some(content) = attr(tag, "content")
            && raster_ref(&content)
        {
            srcs.push(content);
        }
        if from >= html.len() {
            break;
        }
    }
    from = 0;
    while let Some(rel) = lower[from..].find("<img") {
        let start = from + rel;
        let end = lower[start..]
            .find('>')
            .map(|n| start + n + 1)
            .unwrap_or(html.len());
        let tag = &html[start..end.min(html.len())];
        from = end;
        if let Some(set) = attr(tag, "srcset")
            && let Some(src) = smallest_raster("", &set)
        {
            srcs.push(src);
        } else if let Some(src) = attr(tag, "src")
            && raster_ref(&src)
        {
            srcs.push(src);
        }
        if from >= html.len() {
            break;
        }
    }
    srcs
}

/// The smallest png/jpeg/gif the tag offers. A `1x` or a narrow `w` beats the `src`.
pub(crate) fn smallest_raster(src: &str, srcset: &str) -> Option<String> {
    let mut best: Option<(String, u32)> = None;
    for cand in srcset.split(',') {
        let mut parts = cand.split_whitespace();
        let Some(url) = parts.next() else {
            continue;
        };
        if !raster_ref(url) {
            continue;
        }
        let mut score = 1000u32;
        for desc in parts {
            if let Some(width) = desc.strip_suffix('w').and_then(|text| text.parse().ok()) {
                score = width;
            } else if let Some(density) = desc.strip_suffix('x')
                && let Ok(value) = density.parse::<f32>()
            {
                score = (value * 1000.0).round() as u32;
            }
        }
        if best.as_ref().is_none_or(|(_, have)| score < *have) {
            best = Some((url.to_string(), score));
        }
    }
    if let Some((url, _)) = best {
        return Some(url);
    }
    raster_ref(src).then(|| src.trim().to_string())
}

pub(crate) fn image_choice(src: &str, srcset: &str, page: &str) -> Option<String> {
    let raw = smallest_raster(src, srcset)?;
    if raw.starts_with("data:image/") {
        return Some(raw);
    }
    absolute_url(&raw, Some(page))
}

pub(crate) fn load_raster_limited(url: &str, seconds: u64) -> Option<Vec<u8>> {
    if let Some(bytes) = data_image(url) {
        return (bytes.len() <= 400_000).then_some(bytes);
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let bytes = fetch_capped(url, seconds, 400_000).ok()?;
    (bytes.len() >= 32 && bytes.len() <= 400_000).then_some(bytes)
}

fn fetch_capped(url: &str, seconds: u64, max_bytes: usize) -> Result<Vec<u8>, String> {
    let timeout = seconds.to_string();
    let cap = max_bytes.to_string();
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--max-time",
        &timeout,
        "--max-filesize",
        &cap,
        "--silent",
        "--show-error",
        "--location",
        "--compressed",
        "--fail",
        "-A",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        url,
    ]);
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output().map_err(|err| format!("curl: {err}"))?;
    if !output.status.success() || output.stdout.len() > max_bytes {
        return Err("image skipped".into());
    }
    Ok(output.stdout)
}

fn raster_ref(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("data:image/png")
        || lower.starts_with("data:image/jpeg")
        || lower.starts_with("data:image/jpg")
        || lower.starts_with("data:image/gif")
    {
        return true;
    }
    let path = lower.split(['?', '#']).next().unwrap_or(&lower);
    path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".jpeg") || path.ends_with(".gif")
}

fn image_bytes(src: &str, page: Option<&str>, dir: &Path) -> Option<Vec<u8>> {
    if let Some(bytes) = data_image(src) {
        return Some(bytes);
    }
    if let Some(url) = absolute_url(src, page) {
        return fetch_capped(&url, 3, 400_000).ok();
    }
    let path = if Path::new(src).is_absolute() {
        std::path::PathBuf::from(src)
    } else {
        dir.join(src)
    };
    std::fs::read(path).ok()
}

fn absolute_url(src: &str, page: Option<&str>) -> Option<String> {
    if src.starts_with("http://") || src.starts_with("https://") {
        return Some(src.to_string());
    }
    if let Some(rest) = src.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    let page = page?;
    let (scheme, rest) = page.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    if src.starts_with('/') {
        return Some(format!("{scheme}://{host}{src}"));
    }
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let prefix = match path.rfind('/') {
        Some(index) => format!("{scheme}://{host}{}", &path[..=index]),
        None => format!("{scheme}://{host}/"),
    };
    Some(format!("{prefix}{src}"))
}

fn data_image(src: &str) -> Option<Vec<u8>> {
    let rest = src.strip_prefix("data:image/")?;
    let (kind, data) = rest.split_once(',')?;
    if !kind.to_ascii_lowercase().contains("base64") {
        return None;
    }
    let kind = kind.split(';').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(kind.as_str(), "png" | "jpeg" | "jpg" | "gif") {
        return None;
    }
    decode_base64(data)
}

fn decode_base64(data: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = data
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if bytes.len() < 4 {
        return None;
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        let (a, b, c, d) = (
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        );
        let av = val(a)?;
        let bv = val(b)?;
        let cv = if c == b'=' { 0 } else { val(c)? };
        let dv = if d == b'=' { 0 } else { val(d)? };
        out.push((av << 2) | (bv >> 4));
        if c != b'=' {
            out.push((bv << 4) | (cv >> 2));
        }
        if d != b'=' {
            out.push((cv << 6) | dv);
        }
        index += 4;
    }
    Some(out)
}

fn decode(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn clip(line: &str, max: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max {
        return line.to_string();
    }
    let mut end = max;
    let floor = max.saturating_sub(24).max(8);
    for index in (floor..max).rev() {
        let ch = chars[index];
        if ch.is_whitespace() {
            end = index;
            break;
        }
        if matches!(ch, '/' | '?' | '&' | '=' | '-' | ',' | '.') {
            end = (index + 1).min(max);
            break;
        }
    }
    if let Some(open) = chars[..end].iter().rposition(|ch| *ch == '<')
        && !chars[open..end].contains(&'>')
        && open > 0
    {
        end = open;
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
    }
    let mut out: String = chars.into_iter().take(end).collect();
    out.push('~');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_uses_the_small_file() {
        let src = smallest_raster(
            "/images/branding/googlelogo/2x/googlelogo_color_272x92dp.png",
            "/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png 1x, /images/branding/googlelogo/2x/googlelogo_color_272x92dp.png 2x",
        );
        assert_eq!(
            src.as_deref(),
            Some("/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png")
        );
        let full = absolute_url(
            src.as_deref().unwrap(),
            Some("https://www.google.com/?zx=1"),
        );
        assert_eq!(
            full.as_deref(),
            Some("https://www.google.com/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png")
        );
        let narrow = smallest_raster(
            "https://cdn.example/big.jpg",
            "https://cdn.example/small.jpg 320w, https://cdn.example/big.jpg 1280w",
        );
        assert_eq!(narrow.as_deref(), Some("https://cdn.example/small.jpg"));
    }

    #[test]
    fn markdown_keeps_angle_brackets() {
        let lines = render("# Desk\n\nUse `a < b` in the repo.\n\n- one\n");
        let joined = lines.join("\n");
        assert!(joined.contains("a < b"), "{joined}");
        assert!(joined.contains("# Desk"), "{joined}");
        assert!(joined.contains("- one"), "{joined}");
    }

    #[test]
    fn page_keeps_the_title_and_drops_script() {
        let lines = render(
            "<html><head><title>Desk &amp; mesh</title><script>secret()</script></head><body><h1>Towers</h1><p>Hello</p><img alt=\"map\" src=\"a.png\"></body></html>",
        );
        let joined = lines.join("\n");
        assert!(joined.contains("title  Desk & mesh"));
        assert!(joined.contains("Towers"));
        assert!(joined.contains("Hello"));
        assert!(joined.contains("[img map] a.png"));
        assert!(!joined.contains("secret"));
    }

    #[test]
    fn page_shows_lists_links_pre_and_tables() {
        let lines = render(
            "<style>hidden</style><h2>List</h2><ul><li>One</li><li>Two</li></ul><ol><li>First</li></ol><p><a href=\"https://example.com/a\">Go</a></p><pre>fn main() {\n}</pre><table><tr><th>Lang</th><th>Ink</th></tr><tr><td>rs</td><td>red</td></tr></table>",
        );
        let joined = lines.join("\n");
        assert!(joined.contains("## List"));
        assert!(joined.contains("- One"));
        assert!(joined.contains("- Two"));
        assert!(joined.contains("1. First"));
        assert!(joined.contains("Go <https://example.com/a>"));
        assert!(joined.contains("fn main() {"));
        assert!(joined.contains('}'));
        assert!(joined.contains("Lang | Ink"));
        assert!(joined.contains("rs | red"));
        assert!(!joined.contains("hidden"));
    }

    #[test]
    fn page_shows_fields_quotes_buttons_and_meta() {
        let lines = render(
            "<meta name=\"description\" content=\"Desk towers\"><blockquote>Keep</blockquote><p>Use <code>move</code></p><button>Send</button><form><input name=\"title\"></form><details><summary>More</summary><p>Body</p></details><select><option>Red</option><option>Blue</option></select><label>Name</label>",
        );
        let joined = lines.join("\n");
        assert!(joined.contains("meta  Desk towers"));
        assert!(joined.contains("> Keep"));
        assert!(joined.contains("`move`"));
        assert!(joined.contains("[button] Send"));
        assert!(joined.contains("[field] title"));
        assert!(joined.contains("summary More"));
        assert!(joined.contains("Body"));
        assert!(joined.contains("option Red"));
        assert!(joined.contains("option Blue"));
        assert!(joined.contains("Name"));
    }

    #[test]
    fn a_bare_host_is_a_site_and_a_filename_stays_a_file() {
        let dir = std::env::temp_dir();
        assert_eq!(
            web_url("google.com", &dir).as_deref(),
            Some("https://google.com")
        );
        assert_eq!(
            web_url("example.com/search?q=1", &dir).as_deref(),
            Some("https://example.com/search?q=1")
        );
        assert_eq!(
            web_url("https://example.com/a", &dir).as_deref(),
            Some("https://example.com/a")
        );
        assert!(web_url("README.md", &dir).is_none());
        assert!(web_url("notes.html", &dir).is_none());
        assert!(web_url("shot.png", &dir).is_none());
    }

    #[test]
    fn a_long_tag_does_not_spill_into_the_page() {
        let pad = "A".repeat(900);
        let html = format!(
            "<html><title>T</title><body><img alt=\"x\" src=\"data:image/png;base64,{pad}\"><p>Hello</p></body></html>"
        );
        let joined = render(&html).join("\n");
        assert!(joined.contains("Hello"), "{joined}");
        assert!(!joined.contains(&"A".repeat(40)), "{joined}");
    }

    #[test]
    fn a_local_picture_is_drawn_on_the_page() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-page-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut raw = vec![0u8; 40 * 20 * 3];
        for (index, pixel) in raw.chunks_mut(3).enumerate() {
            if index < 40 * 10 {
                pixel.copy_from_slice(&[220, 40, 40]);
            } else {
                pixel.copy_from_slice(&[30, 80, 220]);
            }
        }
        let png = dir.join("shot.png");
        image::RgbImage::from_raw(40, 20, raw)
            .unwrap()
            .save(&png)
            .unwrap();
        let html = dir.join("shot.html");
        std::fs::write(
            &html,
            "<title>Shot</title><p>See</p><img alt=\"map\" src=\"shot.png\">",
        )
        .unwrap();
        let lines = load(&html.display().to_string(), &dir).unwrap();
        let joined = lines.join("\n");
        assert!(joined.contains("Shot"), "{joined}");
        assert!(crate::draw::has_blocks(&joined), "{joined}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_logo_stays_a_header_and_the_page_keeps_its_size() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-logo-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let save = |name: &str, w: u32, h: u32, rgb: [u8; 3]| {
            let mut raw = vec![0u8; (w * h * 3) as usize];
            for pixel in raw.chunks_mut(3) {
                pixel.copy_from_slice(&rgb);
            }
            image::RgbImage::from_raw(w, h, raw)
                .unwrap()
                .save(dir.join(name))
                .unwrap();
        };
        save("g.png", 200, 200, [20, 80, 220]);
        save("word.png", 272, 92, [220, 40, 40]);
        let html = dir.join("img.html");
        std::fs::write(
            &html,
            "<title>Google Images</title><p>Search the web</p><img alt=\"G\" src=\"g.png\"><img alt=\"Google\" src=\"word.png\"><img alt=\"gone\" src=\"gone.png\"><form><input name=\"Search\"></form>",
        )
        .unwrap();
        let doc = load_doc(&html.display().to_string(), &dir).unwrap();
        assert!(doc.bytes > 0);
        let blocks = doc
            .lines
            .iter()
            .filter(|line| crate::draw::has_blocks(line))
            .count();
        assert!(blocks > 0 && blocks <= 8, "{blocks}");
        let title_at = doc
            .lines
            .iter()
            .position(|line| line.contains("Google Images"))
            .expect("title");
        assert!(title_at <= 9, "{title_at} {:?}", &doc.lines[..title_at.min(doc.lines.len())]);
        assert!(
            doc.lines.iter().any(|line| line.contains("Search")),
            "{:?}",
            doc.lines
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn google_opens_as_a_page() {
        let lines = load("google.com", &std::env::temp_dir()).expect("google.com");
        let joined = lines.join("\n");
        assert!(
            joined.contains("url  <https://google.com>"),
            "{joined}"
        );
        assert!(
            joined.to_ascii_lowercase().contains("google"),
            "{joined}"
        );
        assert!(
            joined.contains("[field]") || joined.contains("[button]") || joined.contains('▀'),
            "{joined}"
        );
    }

    #[test]
    fn a_long_link_stays_closed_and_an_empty_image_is_skipped() {
        let href = format!(
            "https://accounts.google.com/ServiceLogin?hl=ru&continue={}",
            "x".repeat(400)
        );
        let html = format!(
            "<title>T</title><img src=\"/tia/tia.png\"><img alt=\"map\" src=\"a.png\"><p><a href=\"{href}\">Войти</a> <a href=\"https://mail.google.com/mail/\">Почта</a></p>"
        );
        let lines = render(&html);
        let joined = lines.join("\n");
        assert!(
            joined.contains("Войти <https://accounts.google.com/ServiceLogin>"),
            "{joined}"
        );
        assert!(
            joined.contains("Почта <https://mail.google.com/mail/>"),
            "{joined}"
        );
        assert!(joined.contains("[img map] a.png"), "{joined}");
        assert!(!joined.contains("[img ]"), "{joined}");
        assert!(!joined.contains("tia.png"), "{joined}");
        assert!(!joined.contains(&"x".repeat(40)), "{joined}");
        for line in &lines {
            let chars: Vec<char> = line.chars().collect();
            if let Some(open) = chars.iter().position(|ch| *ch == '<') {
                assert!(chars[open..].contains(&'>'), "{line}");
            }
            assert!(chars.len() <= 100, "{line}");
        }
    }
}
