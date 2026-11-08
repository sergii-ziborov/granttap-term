use std::fs;
use std::path::Path;

pub fn line(home: &Path) -> String {
    let path = home.join(".claude/stats-cache.json");
    let Ok(text) = fs::read_to_string(path) else {
        return "no data".into();
    };
    let Some(sessions) = number_after(&text, "totalSessions") else {
        return "no data".into();
    };
    let Some(messages) = number_after(&text, "totalMessages") else {
        return "no data".into();
    };
    let model = object_key_after(&text, "modelUsage").unwrap_or_else(|| "model".into());
    let window = text.split("\"modelUsage\"").nth(1).unwrap_or("");
    let input = number_after(window, "inputTokens").unwrap_or(0);
    let output = number_after(window, "outputTokens").unwrap_or(0);
    format!("Claude {sessions} sessions  {messages} messages  {model} in {input} out {output}")
}

fn number_after(text: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn object_key_after(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = text.split_once(&needle)?.1;
    let rest = after.trim_start().trim_start_matches(':').trim_start();
    let rest = rest.trim_start_matches('{').trim_start();
    let raw = rest.trim_start_matches('"').split('"').next()?;
    if raw.is_empty() { None } else { Some(raw.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_stats_are_read_without_inventing_a_remainder() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-usage-{n}"));
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(
            home.join(".claude/stats-cache.json"),
            r#"{"totalSessions":4,"totalMessages":9,"modelUsage":{"claude-sonnet":{"inputTokens":3,"outputTokens":5}}}"#,
        )
        .unwrap();
        let line = line(&home);
        assert!(line.contains("4 sessions"));
        assert!(line.contains("9 messages"));
        assert!(line.contains("claude-sonnet"));
        assert!(line.contains("in 3 out 5"));
        assert!(!line.contains("left"));
        let _ = fs::remove_dir_all(home);
    }
}
