use std::path::Path;
use std::process::Command;

pub fn load(target: &str, dir: &Path) -> Result<Vec<String>, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("browse needs a path or an http(s) URL".into());
    }
    let bytes = if target.starts_with("http://") || target.starts_with("https://") {
        fetch(target)?
    } else {
        let path = if Path::new(target).is_absolute() {
            Path::new(target).to_path_buf()
        } else {
            dir.join(target)
        };
        std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?
    };
    if bytes.len() > 200_000 {
        return Err("page is larger than 200000 bytes".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = render(&text);
    if let Some(src) = first_local_image(&text, dir)
        && let Ok(preview) = crate::draw::inspect(&src, 1, 24, 6)
    {
        lines.push(format!(
            "[Image #1]  {}  {}x{}",
            preview.mark.format, preview.mark.width, preview.mark.height
        ));
        lines.extend(preview.picture.lines().map(str::to_string));
    }
    if lines.is_empty() {
        lines.push("Empty page.".into());
    }
    lines.truncate(180);
    Ok(lines)
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("curl")
        .args([
            "--max-time",
            "8",
            "--silent",
            "--show-error",
            "--location",
            "--fail",
            url,
        ])
        .output()
        .map_err(|err| format!("curl: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("browse failed: {}", err.trim()));
    }
    Ok(output.stdout)
}

pub fn render(html: &str) -> Vec<String> {
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
            for next in chars.by_ref() {
                tag.push(next);
                if next == '>' || tag.len() > 500 {
                    break;
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
            self.lines.push(clip(&decode(&image_mark(raw)), 200));
            return;
        }
        if tag_is(&low, "<meta") {
            let name = attr(raw, "name").unwrap_or_default();
            if name.eq_ignore_ascii_case("description")
                && let Some(content) = attr(raw, "content")
            {
                self.flush();
                self.lines.push(clip(&format!("meta  {}", decode(&content)), 200));
            }
            return;
        }
        if tag_is(&low, "<input") || tag_is(&low, "<textarea") {
            self.flush();
            let name = attr(raw, "name")
                .or_else(|| attr(raw, "placeholder"))
                .unwrap_or_else(|| "field".into());
            self.lines.push(clip(&format!("[field] {}", decode(&name)), 200));
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
            if let Some(href) = self.hrefs.pop()
                && !href.is_empty()
            {
                if !self.buf.ends_with(' ') {
                    self.buf.push(' ');
                }
                self.buf.push('<');
                self.buf.push_str(&href);
                self.buf.push('>');
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
        self.lines.push(clip(&line, 200));
    }
}

fn tag_is(low: &str, name: &str) -> bool {
    let Some(rest) = low.strip_prefix(name) else {
        return false;
    };
    rest.is_empty()
        || rest.starts_with([' ', '>', '/'])
        || rest.starts_with('\t')
}

fn image_mark(tag: &str) -> String {
    let alt = attr(tag, "alt").unwrap_or_default();
    let src = attr(tag, "src").unwrap_or_default();
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

fn first_local_image(html: &str, dir: &Path) -> Option<std::path::PathBuf> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<img")?;
    let end = lower[start..].find('>').unwrap_or(0);
    let src = attr(&html[start..start + end], "src")?;
    if src.starts_with("http://") || src.starts_with("https://") {
        return None;
    }
    let path = if Path::new(&src).is_absolute() {
        std::path::PathBuf::from(src)
    } else {
        dir.join(src)
    };
    path.is_file().then_some(path)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
