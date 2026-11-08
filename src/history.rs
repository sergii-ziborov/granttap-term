const CAP: usize = 100;

#[derive(Default)]
pub struct History {
    lines: Vec<String>,
    /// `None` is the live draft, past the newest stored line.
    pos: Option<usize>,
    draft: String,
}

impl History {
    pub fn seed(lines: Vec<String>) -> Self {
        let mut lines = lines;
        if lines.len() > CAP {
            let skip = lines.len() - CAP;
            lines.drain(0..skip);
        }
        Self {
            lines,
            pos: None,
            draft: String::new(),
        }
    }

    pub fn stored(&self) -> &[String] {
        &self.lines
    }

    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        self.pos = None;
        self.draft.clear();
        if self.lines.last().is_some_and(|last| last == line) {
            return;
        }
        if self.lines.len() == CAP {
            self.lines.remove(0);
        }
        self.lines.push(line.to_string());
    }

    pub fn previous(&mut self, current: &str) -> Option<String> {
        if self.lines.is_empty() {
            return None;
        }
        let index = match self.pos {
            Some(0) => return None,
            Some(index) => index - 1,
            None => {
                self.draft = current.to_string();
                self.lines.len() - 1
            }
        };
        self.pos = Some(index);
        Some(self.lines[index].clone())
    }

    pub fn next(&mut self) -> Option<String> {
        let index = self.pos?;
        if index + 1 >= self.lines.len() {
            self.pos = None;
            return Some(std::mem::take(&mut self.draft));
        }
        let index = index + 1;
        self.pos = Some(index);
        Some(self.lines[index].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_ignores_empty_and_duplicate_of_last() {
        let mut history = History::default();
        history.push("");
        history.push("ls");
        history.push("ls");
        history.push("");
        history.push("pwd");
        history.push("pwd");
        history.push("ls");

        let mut older = Vec::new();
        while let Some(line) = history.previous("") {
            older.push(line);
        }
        assert_eq!(
            older,
            vec!["ls".to_string(), "pwd".to_string(), "ls".to_string()]
        );
    }

    #[test]
    fn previous_and_next_walk_older_then_newer() {
        let mut history = History::default();
        assert_eq!(history.previous("draft"), None);
        assert_eq!(history.next(), None);

        history.push("one");
        history.push("two");
        history.push("three");

        assert_eq!(history.previous("draft"), Some("three".into()));
        assert_eq!(history.previous("three"), Some("two".into()));
        assert_eq!(history.previous("two"), Some("one".into()));
        assert_eq!(history.previous("one"), None);

        assert_eq!(history.next(), Some("two".into()));
        assert_eq!(history.next(), Some("three".into()));
        assert_eq!(history.next(), Some("draft".into()));
        assert_eq!(history.next(), None);

        assert_eq!(history.previous(""), Some("three".into()));
        assert_eq!(history.next(), Some("".into()));
        assert_eq!(history.next(), None);
    }

    #[test]
    fn stored_lines_cap_at_100() {
        let mut history = History::default();
        for n in 0..100 {
            history.push(&n.to_string());
        }
        assert_eq!(history.lines.len(), 100);
        history.push("0");
        history.push("0");
        history.push("");
        assert_eq!(history.lines.len(), 100);
        assert_eq!(history.lines[0], "1");
        assert_eq!(history.lines[98], "99");
        assert_eq!(history.lines[99], "0");

        let mut count = 0;
        while history.previous("").is_some() {
            count += 1;
        }
        assert_eq!(count, 100);
        assert_eq!(history.next(), Some("2".into()));
    }
}
