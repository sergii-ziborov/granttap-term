use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    Grok,
    Claude,
    Codex,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grok => "Grok",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "grok" => Some(Self::Grok),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn efforts(self) -> &'static [&'static str] {
        match self {
            Self::Grok => &["low", "medium", "high", "xhigh"],
            Self::Claude => &["low", "medium", "high", "xhigh", "max"],
            Self::Codex => &["low", "medium", "high", "xhigh", "max", "ultra"],
        }
    }

    pub fn models(self) -> &'static [&'static str] {
        match self {
            Self::Grok => &["grok-4.7", "grok-4.5", "grok-4"],
            Self::Claude => &["opus[1m]", "opus", "sonnet[1m]", "sonnet", "haiku", "fable"],
            Self::Codex => &[
                "gpt-6.1-sol",
                "gpt-6-astra",
                "gpt-6.1-terra",
                "gpt-6-sol",
                "gpt-5.6-sol",
            ],
        }
    }

    /// Each agent's own permission words. Claude does not use ask/safe/full.
    pub fn permits(self) -> &'static [&'static str] {
        match self {
            Self::Grok => &["ask", "acceptEdits", "plan", "always-approve"],
            Self::Claude => &["default", "acceptEdits", "plan", "bypassPermissions"],
            Self::Codex => &["on-request", "on-failure", "never"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Chats,
    Tasks,
    Mesh,
    Files,
    Browser,
    Lessons,
    Term,
}

impl Section {
    pub const ALL: [Self; 7] = [
        Self::Chats,
        Self::Tasks,
        Self::Mesh,
        Self::Files,
        Self::Browser,
        Self::Lessons,
        Self::Term,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Chats => "now",
            Self::Tasks => "Tasks",
            Self::Mesh => "Mesh",
            Self::Files => "Files",
            Self::Browser => "Browser",
            Self::Lessons => "Lessons",
            Self::Term => "Term",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.eq_ignore_ascii_case("chats") {
            return Some(Self::Chats);
        }
        Self::ALL
            .into_iter()
            .find(|section| section.label().eq_ignore_ascii_case(text))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptLevel {
    Ask,
    Safe,
    ExceptPush,
    ExceptDestructive,
    Full,
}

impl AcceptLevel {
    pub const ALL: [Self; 5] = [
        Self::Ask,
        Self::Safe,
        Self::ExceptPush,
        Self::ExceptDestructive,
        Self::Full,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Safe => "safe",
            Self::ExceptPush => "except-push",
            Self::ExceptDestructive => "except-destructive",
            Self::Full => "full",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.label() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permit {
    Ask,
    Edits,
    Bypass,
    Plan,
}

impl Permit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Edits => "edits",
            Self::Bypass => "bypass",
            Self::Plan => "plan",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ask" => Some(Self::Ask),
            "edits" => Some(Self::Edits),
            "bypass" => Some(Self::Bypass),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chat {
    pub agent: AgentKind,
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub cwd: String,
    pub live: bool,
    pub at: i64,
    pub tokens: Option<u64>,
    /// Context window from signals.json. Absent when the file does not say.
    pub window: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub title: String,
    pub owner: AgentKind,
    pub model: String,
    pub effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageMark {
    pub n: usize,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub path: PathBuf,
}

impl ImageMark {
    pub fn token(&self) -> String {
        format!("[Image #{}]", self.n)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub name: String,
    pub icon: char,
    pub dir: bool,
    pub path: PathBuf,
}
