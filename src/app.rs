use std::path::{Path, PathBuf};
use std::process::Command;

use crate::chats::{self, files};
use crate::draw;
use crate::lessons;
use crate::mesh;
use crate::model::{AcceptLevel, AgentKind, Permit, Section, Task};
use crate::term::Key;

pub enum Effect {
    None,
    Quit,
    Edit(PathBuf),
}

pub struct App {
    pub section: Section,
    pub cmd: String,
    pub cursor: usize,
    pub suggest: usize,
    pub note: String,
    pub vim: bool,
    pub accept: AcceptLevel,
    pub paused: bool,
    pub permit: Permit,
    pub model: String,
    pub effort: String,
    pub dir: PathBuf,
    pub desc_open: bool,
    home: PathBuf,
    history: Vec<String>,
    chats: Vec<crate::model::Chat>,
    tasks: Vec<Task>,
    images: Vec<crate::model::ImageMark>,
    preview: String,
    mode: Mode,
}

enum Mode {
    Desk,
    View { lines: Vec<String> },
    Split { lines: Vec<String> },
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "chats",
        "Show chats from Grok, Claude, and Codex on this computer.",
    ),
    ("tasks", "Show the task queue. A task has one owner."),
    (
        "mesh",
        "Show this repo and towers from the directories inside it.",
    ),
    ("files", "Show the file list for this directory."),
    ("browser", "Open the cell preview of the last pasted image."),
    (
        "lessons",
        "Open lessons for the desk, Claude, the terminal, and Kubernetes.",
    ),
    (
        "task",
        "Add a task owned by the selected chat's agent: task <title>.",
    ),
    ("move", "Send the selected chat to grok, claude, or codex."),
    (
        "accept",
        "Set auto-accept: ask, safe, except-push, except-destructive, full.",
    ),
    ("pause", "Pause or resume auto-accept."),
    ("permit", "Set permission mode: ask, edits, bypass, plan."),
    ("model", "Set the model name on the selected task."),
    ("effort", "Set model effort within that agent's catalog."),
    ("image", "Load an image path and show its cell preview."),
    ("open", "Open the selected chat in a split."),
    (
        "context",
        "Open the selected chat in a split with the transcript context.",
    ),
    ("desc", "Open the command description in the lower split."),
];

impl App {
    pub fn new(dir: PathBuf, home: PathBuf) -> Self {
        let chats = chats::load(&home);
        Self {
            section: Section::Chats,
            cmd: String::new(),
            cursor: 0,
            suggest: 0,
            note: String::new(),
            vim: false,
            accept: AcceptLevel::Ask,
            paused: false,
            permit: Permit::Ask,
            model: String::new(),
            effort: "medium".into(),
            dir,
            desc_open: false,
            home,
            history: Vec::new(),
            chats,
            tasks: Vec::new(),
            images: Vec::new(),
            preview: String::new(),
            mode: Mode::Desk,
        }
    }

    pub fn accept_label(&self) -> String {
        if self.paused {
            format!("{} paused", self.accept.label())
        } else {
            self.accept.label().into()
        }
    }

    pub fn mode_is_desk(&self) -> bool {
        matches!(self.mode, Mode::Desk) && !self.desc_open
    }

    pub fn suggestions(&self) -> Vec<String> {
        let token = self.cmd.split_whitespace().next().unwrap_or("");
        if token.is_empty() {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (name, _) in COMMANDS {
            if name.starts_with(token) && *name != token {
                found.push((*name).to_string());
            }
        }
        if self.cmd.split_whitespace().count() >= 1 && self.cmd.ends_with(' ') {
            found.clear();
            let head = self.cmd.split_whitespace().next().unwrap_or("");
            let partial = "";
            push_second(&mut found, head, partial);
        } else if let Some((head, partial)) = self.cmd.split_once(' ') {
            found.clear();
            push_second(&mut found, head, partial.trim());
        }
        found.truncate(6);
        found
    }

    pub fn description_line(&self) -> String {
        let head = self.cmd.split_whitespace().next().unwrap_or("");
        if head.is_empty() {
            return "Description appears here. desc opens it.".into();
        }
        if let Some((_, text)) = COMMANDS.iter().find(|(name, _)| *name == head) {
            return (*text).to_string();
        }
        if let Some((_, text)) = COMMANDS.iter().find(|(name, _)| name.starts_with(head)) {
            return (*text).to_string();
        }
        format!("No description for {head}.")
    }

    pub fn description_body(&self) -> String {
        format!("Description  {}", self.description_line())
    }

    pub fn empty_line(&self) -> &'static str {
        match self.section {
            Section::Chats => "No chats.",
            Section::Tasks => "No tasks.",
            Section::Mesh => "No towers.",
            Section::Files => "No files.",
            Section::Browser => "No image.",
            Section::Lessons => "No lessons.",
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if let Mode::View { lines } | Mode::Split { lines } = &self.mode {
            return lines.clone();
        }
        if self.desc_open {
            return vec![self.description_body()];
        }
        match self.section {
            Section::Chats => self
                .chats
                .iter()
                .map(|chat| format!("{:<7} {}", chat.agent.label(), chat.title))
                .collect(),
            Section::Tasks => {
                let mut lines = vec![
                    format!(
                        "Usage  no data    Questions  no data    permit {}  effort {}",
                        self.permit.label(),
                        self.effort
                    ),
                    "A task has one owner.".into(),
                ];
                for (index, task) in self.tasks.iter().enumerate() {
                    lines.push(format!(
                        "{index}  {}  {}  {}  {}",
                        task.owner.label(),
                        task.model,
                        task.effort,
                        task.title
                    ));
                }
                if self.tasks.is_empty() {
                    lines.push("No tasks.".into());
                }
                lines
            }
            Section::Mesh => self.mesh_lines(),
            Section::Files => files(&self.dir)
                .into_iter()
                .map(|row| format!("{} {}", row.icon, row.name))
                .collect(),
            Section::Browser => {
                if self.preview.is_empty() {
                    vec!["Paste an image with Control+V, or: image <path>".into()]
                } else {
                    self.preview.lines().map(|line| line.to_string()).collect()
                }
            }
            Section::Lessons => lessons::lessons()
                .iter()
                .map(|lesson| lesson.title.to_string())
                .collect(),
        }
    }

    fn mesh_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        match mesh::git_root(&self.dir) {
            Some(root) => lines.push(format!("Repo  {}", root.display())),
            None => lines.push("Repo  none".into()),
        }
        lines.push("Linked repos  none".into());
        lines.push("Env  none    Skills  none".into());
        let root = mesh::git_root(&self.dir).unwrap_or_else(|| self.dir.clone());
        let towers = mesh::towers(&root);
        if towers.is_empty() {
            lines.push("No towers.".into());
        }
        for tower in towers {
            let bar = "█".repeat(tower.height.max(1));
            lines.push(format!("{:<16} {bar}  {}", tower.name, tower.files));
        }
        lines
    }

    pub fn on_key(&mut self, key: Key) -> Effect {
        self.note.clear();
        match key {
            Key::Ignore => Effect::None,
            Key::CtrlC | Key::F(10) => Effect::Quit,
            Key::Esc => {
                self.mode = Mode::Desk;
                self.desc_open = false;
                self.cmd.clear();
                Effect::None
            }
            Key::CtrlV => Effect::None,
            Key::Paste(text) => {
                self.paste_image(&text);
                Effect::None
            }
            Key::Tab => {
                let suggestions = self.suggestions();
                if let Some(name) = suggestions.get(self.suggest) {
                    self.cmd = format!("{name} ");
                    self.suggest = 0;
                }
                Effect::None
            }
            Key::Up => {
                self.nudge(-1);
                Effect::None
            }
            Key::Down => {
                self.nudge(1);
                Effect::None
            }
            Key::Left | Key::Right if self.cmd.is_empty() => {
                self.step_tab(if matches!(key, Key::Right) { 1 } else { -1 });
                Effect::None
            }
            Key::F(2) => {
                self.vim = !self.vim;
                Effect::None
            }
            Key::F(4) => self.edit_selected(),
            Key::Enter => self.activate(),
            Key::Backspace => {
                self.cmd.pop();
                self.suggest = 0;
                Effect::None
            }
            Key::Char(ch) => {
                self.cmd.push(ch);
                self.suggest = 0;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    pub fn on_spot(&mut self, spot: crate::ui::Spot) -> Effect {
        match spot {
            crate::ui::Spot::Tab(section) => {
                self.section = section;
                self.cursor = 0;
                self.mode = Mode::Desk;
                Effect::None
            }
            crate::ui::Spot::Desc => {
                self.desc_open = !self.desc_open;
                Effect::None
            }
            crate::ui::Spot::Cmd => Effect::None,
            crate::ui::Spot::Row(index) => {
                if self.cursor == index {
                    self.activate()
                } else {
                    self.cursor = index;
                    Effect::None
                }
            }
        }
    }

    fn nudge(&mut self, delta: i32) {
        let suggestions = self.suggestions();
        if !suggestions.is_empty() {
            let len = suggestions.len() as i32;
            self.suggest = (self.suggest as i32 + delta).rem_euclid(len) as usize;
            return;
        }
        let len = self.lines().len() as i32;
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as i32 + delta).rem_euclid(len) as usize;
    }

    fn step_tab(&mut self, delta: i32) {
        let index = Section::ALL
            .iter()
            .position(|section| *section == self.section)
            .unwrap_or(0) as i32;
        let next = (index + delta).rem_euclid(Section::ALL.len() as i32) as usize;
        self.section = Section::ALL[next];
        self.cursor = 0;
        self.mode = Mode::Desk;
    }

    fn activate(&mut self) -> Effect {
        if !self.cmd.is_empty() {
            let line = self.cmd.clone();
            self.history.push(line.clone());
            self.run(&line);
            self.cmd.clear();
            return Effect::None;
        }
        match self.section {
            Section::Files => self.open_file(false),
            Section::Chats => self.open_chat(false),
            Section::Lessons => self.open_lesson(),
            Section::Browser => Effect::None,
            _ => Effect::None,
        }
    }

    fn run(&mut self, line: &str) {
        let mut parts = line.split_whitespace();
        let Some(head) = parts.next() else {
            return;
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match head {
            "chats" | "tasks" | "mesh" | "files" | "browser" | "lessons" => {
                if let Some(section) = Section::parse(head) {
                    self.section = section;
                    self.cursor = 0;
                }
            }
            "task" => self.add_task(&rest),
            "move" => self.move_chat(&rest),
            "accept" => match AcceptLevel::parse(&rest) {
                Some(level) => self.accept = level,
                None => {
                    self.note = "accept: ask, safe, except-push, except-destructive, full".into()
                }
            },
            "pause" => self.paused = !self.paused,
            "permit" => match Permit::parse(&rest) {
                Some(permit) => self.permit = permit,
                None => self.note = "permit: ask, edits, bypass, plan".into(),
            },
            "model" => self.set_model(&rest),
            "effort" => self.set_effort(&rest),
            "image" => self.paste_image(&rest),
            "open" => {
                let _ = self.open_chat(false);
            }
            "context" => {
                let _ = self.open_chat(true);
            }
            "desc" => self.desc_open = true,
            _ => self.note = format!("No command {head}."),
        }
    }

    fn add_task(&mut self, title: &str) {
        if title.is_empty() {
            self.note = "task needs a title".into();
            return;
        }
        let owner = self
            .chats
            .get(self.cursor)
            .map(|chat| chat.agent)
            .unwrap_or(AgentKind::Grok);
        self.tasks.push(Task {
            title: title.into(),
            owner,
            model: self.model.clone(),
            effort: self.effort.clone(),
        });
        self.section = Section::Tasks;
    }

    fn move_chat(&mut self, name: &str) {
        let Some(agent) = AgentKind::parse(name) else {
            self.note = "move grok, claude, or codex".into();
            return;
        };
        let Some(id) = self.chats.get(self.cursor).map(|chat| chat.id.clone()) else {
            self.note = "No chat.".into();
            return;
        };
        if chats::handoff(&mut self.chats, &id, agent) {
            self.note = format!("Moved to {}", agent.label());
        }
    }

    fn set_model(&mut self, name: &str) {
        if name.is_empty() {
            self.note = "model needs a name".into();
            return;
        }
        self.model = name.into();
        if let Some(task) = self.tasks.get_mut(self.cursor.saturating_sub(2)) {
            task.model = self.model.clone();
        }
    }

    fn set_effort(&mut self, name: &str) {
        let agent = self
            .chats
            .get(self.cursor)
            .map(|chat| chat.agent)
            .unwrap_or(AgentKind::Grok);
        if agent.efforts().contains(&name) {
            self.effort = name.into();
        } else {
            self.note = format!("{} effort: {}", agent.label(), agent.efforts().join(", "));
        }
    }

    fn paste_image(&mut self, text: &str) {
        let Some(path) = chats::image_path(text).or_else(|| chats::image_path(text.trim())) else {
            if !text.trim().is_empty() {
                self.cmd.push_str(text);
            }
            return;
        };
        let path = if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        };
        let n = self.images.len() + 1;
        match draw::inspect(&path, n, 40, 12) {
            Ok(preview) => {
                let token = preview.mark.token();
                self.preview = format!(
                    "{token}  {}  {}x{}  {} bytes  {}\n{}",
                    preview.mark.format,
                    preview.mark.width,
                    preview.mark.height,
                    preview.mark.bytes,
                    preview.mark.path.display(),
                    preview.picture
                );
                self.images.push(preview.mark);
                self.cmd = token;
                self.section = Section::Browser;
            }
            Err(err) => self.note = err,
        }
    }

    fn open_chat(&mut self, context: bool) -> Effect {
        let Some(chat) = self.chats.get(self.cursor).cloned() else {
            self.note = "No chat.".into();
            return Effect::None;
        };
        let mut lines = vec![format!("{}  {}", chat.agent.label(), chat.title)];
        if context {
            lines.push("Context".into());
            lines.extend(read_context(&chat.path));
        } else {
            lines.push("Opened.".into());
        }
        self.mode = Mode::Split { lines };
        self.section = Section::Chats;
        Effect::None
    }

    fn open_lesson(&mut self) -> Effect {
        let Some(lesson) = lessons::lessons().get(self.cursor) else {
            return Effect::None;
        };
        self.mode = Mode::View {
            lines: vec![lesson.title.into(), lesson.body.into()],
        };
        Effect::None
    }

    fn open_file(&mut self, edit: bool) -> Effect {
        let rows = files(&self.dir);
        let Some(row) = rows.get(self.cursor) else {
            return Effect::None;
        };
        if row.dir {
            self.dir = row.path.clone();
            self.cursor = 0;
            return Effect::None;
        }
        if edit {
            return self.edit_path(row.path.clone());
        }
        let text = std::fs::read_to_string(&row.path).unwrap_or_default();
        let lines = text
            .lines()
            .take(200)
            .map(|line| line.to_string())
            .collect();
        self.mode = Mode::View { lines };
        Effect::None
    }

    fn edit_selected(&mut self) -> Effect {
        if !self.vim {
            self.note = "Vim is off.".into();
            return Effect::None;
        }
        let rows = files(&self.dir);
        let Some(row) = rows.get(self.cursor) else {
            self.note = "No file.".into();
            return Effect::None;
        };
        if row.dir {
            self.note = "Not a file.".into();
            return Effect::None;
        }
        self.edit_path(row.path.clone())
    }

    fn edit_path(&mut self, path: PathBuf) -> Effect {
        if self.vim {
            Effect::Edit(path)
        } else {
            self.note = "Vim is off.".into();
            Effect::None
        }
    }

    pub fn reload(&mut self) {
        self.chats = chats::load(&self.home);
    }
}

fn push_second(found: &mut Vec<String>, head: &str, partial: &str) {
    let choices: &[&str] = match head {
        "move" => &["grok", "claude", "codex"],
        "accept" => &["ask", "safe", "except-push", "except-destructive", "full"],
        "permit" => &["ask", "edits", "bypass", "plan"],
        "effort" => &["low", "medium", "high", "xhigh", "max", "ultra"],
        _ => &[],
    };
    for choice in choices {
        if choice.starts_with(partial) && *choice != partial {
            found.push(format!("{head} {choice}"));
        }
    }
}

fn read_context(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.is_empty() {
        return vec!["No transcript.".into()];
    }
    text.lines().take(40).map(|line| clip(line, 200)).collect()
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

pub fn edit(path: &Path, dir: &Path) {
    let _ = Command::new("vim").arg(path).current_dir(dir).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_and_description_follow_the_command() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.cmd = "mo".into();
        assert!(app.suggestions().iter().any(|item| item == "move"));
        assert!(app.description_line().contains("chat"));
        app.on_key(Key::Tab);
        assert!(app.cmd.starts_with("move"));
    }

    #[test]
    fn task_has_one_owner_and_move_changes_the_chat() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
        });
        app.cmd = "task write the desk".into();
        app.on_key(Key::Enter);
        assert_eq!(app.tasks.len(), 1);
        assert_eq!(app.tasks[0].owner, AgentKind::Claude);
        app.section = Section::Chats;
        app.cursor = 0;
        app.cmd = "move grok".into();
        app.on_key(Key::Enter);
        assert_eq!(app.chats[0].agent, AgentKind::Grok);
    }

    #[test]
    fn vim_off_refuses_edit() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.section = Section::Files;
        let effect = app.on_key(Key::F(4));
        assert!(matches!(effect, Effect::None));
        assert_eq!(app.note, "Vim is off.");
    }
}
