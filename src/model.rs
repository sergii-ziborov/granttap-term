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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Chats,
    Tasks,
    Mesh,
    Files,
    Browser,
    Lessons,
}

impl Section {
    pub const ALL: [Self; 6] = [
        Self::Chats,
        Self::Tasks,
        Self::Mesh,
        Self::Files,
        Self::Browser,
        Self::Lessons,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Chats => "Chats",
            Self::Tasks => "Tasks",
            Self::Mesh => "Mesh",
            Self::Files => "Files",
            Self::Browser => "Browser",
            Self::Lessons => "Lessons",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tower {
    pub name: String,
    pub height: usize,
    pub files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub title: &'static str,
    pub body: &'static str,
}
