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
    let mut out = String::new();
    let mut skip = 0u8;
    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::from("<");
            for next in chars.by_ref() {
                tag.push(next);
                if next == '>' || tag.len() > 500 {
                    break;
                }
            }
            let low = tag.to_ascii_lowercase();
            if low.starts_with("<script") {
                skip = 1;
            } else if low.starts_with("<style") {
                skip = 2;
            } else if low.starts_with("</script") || low.starts_with("</style") {
                skip = 0;
            } else if skip == 0 && low.starts_with("<img") {
                out.push_str(&image_mark(&tag));
                out.push('\n');
            } else if skip == 0 && breaks(&low) {
                out.push('\n');
            }
            continue;
        }
        if skip == 0 {
            out.push(ch);
        }
    }
    let mut lines = Vec::new();
    if !title.is_empty() {
        lines.push(format!("title  {title}"));
    }
    let mut blank = false;
    for line in decode(&out).lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            if !blank && !lines.is_empty() {
                lines.push(String::new());
            }
            blank = true;
            continue;
        }
        blank = false;
        lines.push(clip(&line, 200));
        if lines.len() == 180 {
            break;
        }
    }
    lines
}

fn breaks(tag: &str) -> bool {
    tag.starts_with("<br")
        || tag.starts_with("<p")
        || tag.starts_with("</p")
        || tag.starts_with("<div")
        || tag.starts_with("</div")
        || tag.starts_with("<h1")
        || tag.starts_with("<h2")
        || tag.starts_with("<h3")
        || tag.starts_with("<li")
        || tag.starts_with("<tr")
        || tag.starts_with("</h")
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
}
