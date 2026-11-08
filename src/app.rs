use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chats::{self, files};
use crate::draw;
use crate::gitread;
use crate::health;
use crate::history::History;
use crate::hl;
use crate::lessons;
use crate::mesh;
use crate::model::{AcceptLevel, AgentKind, FileRow, Permit, Section, Task};
use crate::page;
use crate::skills::{self, Skill};
use crate::store;
use crate::term::Key;
use crate::usage;

pub enum Effect {
    None,
    Quit,
    Edit(PathBuf),
}

pub struct App {
    pub section: Section,
    pub cmd: String,
    pub cursor: usize,
    pub scroll: usize,
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
    pub fill: bool,
    home: PathBuf,
    history: History,
    chats: Vec<crate::model::Chat>,
    tasks: Vec<Task>,
    images: Vec<crate::model::ImageMark>,
    preview: String,
    page: Vec<String>,
    mode: Mode,
    mesh_view: Vec<String>,
    usage_line: String,
    skills: Vec<Skill>,
    roads: bool,
    externals: bool,
    actions: Vec<String>,
    chat_accept: Vec<(String, AcceptLevel)>,
    mesh_accept: Vec<(String, AcceptLevel)>,
    links: Vec<String>,
    link_note: String,
    links_key: String,
    dirty: Vec<String>,
}

enum Mode {
    Desk,
    View { lines: Vec<String> },
    Split {
        lines: Vec<String>,
        agent: Option<AgentKind>,
    },
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "chats",
        "Show chats from Grok, Claude, and Codex on this computer.",
    ),
    ("tasks", "Show the task queue. A task has one owner."),
    (
        "mesh",
        "Show this repo, Weavatrix links, towers, skills, and envs.",
    ),
    ("files", "Show the file list for this directory."),
    (
        "browser",
        "Open the text browser, or the cell preview of the last image.",
    ),
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
        "Set auto-accept for this mesh and the selected chat.",
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
    ("browse", "Draw a local file or http(s) page in a split."),
    ("history", "Read recent commits through Weavatrix git."),
    ("blob", "Read a file at HEAD through Weavatrix git: blob <path>."),
    ("health", "Toggle mesh code-health: health roads, or health extern."),
    ("fill", "Expand the open split to the whole terminal, or collapse it."),
];

impl App {
    pub fn new(dir: PathBuf, home: PathBuf) -> Self {
        let stored = store::load(&home);
        let chats = chats::load(&home);
        let mut app = Self {
            section: Section::Chats,
            cmd: String::new(),
            cursor: 0,
            scroll: 0,
            suggest: 0,
            note: String::new(),
            vim: false,
            accept: stored
                .mesh_accept
                .first()
                .map(|(_, level)| *level)
                .unwrap_or(AcceptLevel::Ask),
            paused: stored.paused,
            permit: stored.permit,
            model: String::new(),
            effort: "medium".into(),
            dir,
            desc_open: false,
            fill: false,
            home: home.clone(),
            history: History::default(),
            chats,
            tasks: Vec::new(),
            images: Vec::new(),
            preview: String::new(),
            page: Vec::new(),
            mode: Mode::Desk,
            mesh_view: Vec::new(),
            usage_line: usage::line(&home),
            skills: Vec::new(),
            roads: stored.roads,
            externals: stored.externals,
            actions: stored.actions,
            chat_accept: stored.chat_accept,
            mesh_accept: stored.mesh_accept,
            links: Vec::new(),
            link_note: String::new(),
            links_key: String::new(),
            dirty: Vec::new(),
        };
        app.refresh_mesh();
        app
    }

    pub fn accept_label(&self) -> String {
        let level = self.shown_accept();
        if self.paused {
            format!("{} paused", level.label())
        } else {
            level.label().into()
        }
    }

    pub fn fill_bar(&self) -> String {
        let who = match &self.mode {
            Mode::Split {
                agent: Some(agent), ..
            } => agent.label(),
            Mode::Split { .. } | Mode::View { .. } => "view",
            Mode::Desk => "desk",
        };
        format!("{who}   filled   F3 collapse   Esc back")
    }

    pub fn mode_is_desk(&self) -> bool {
        matches!(self.mode, Mode::Desk) && !self.desc_open && !self.fill
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
            push_second(&mut found, head, "");
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
            Section::Browser => "No page.",
            Section::Lessons => "No lessons.",
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if let Mode::View { lines } = &self.mode {
            return lines.iter().map(|line| paint_line(line)).collect();
        }
        if let Mode::Split { lines, .. } = &self.mode {
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
            Section::Tasks => self.task_lines(),
            Section::Mesh => self.mesh_view.clone(),
            Section::Files => files(&self.dir)
                .iter()
                .map(|row| self.file_line(row))
                .collect(),
            Section::Browser => {
                if !self.page.is_empty() {
                    self.page.clone()
                } else if self.preview.is_empty() {
                    vec!["browse <file or url>, or paste an image with Control+V.".into()]
                } else {
                    self.preview.lines().map(str::to_string).collect()
                }
            }
            Section::Lessons => lessons::lessons()
                .iter()
                .map(|lesson| lesson.title.to_string())
                .collect(),
        }
    }

    pub fn on_key(&mut self, key: Key) -> Effect {
        self.note.clear();
        match key {
            Key::Ignore => Effect::None,
            Key::CtrlC | Key::F(10) => Effect::Quit,
            Key::Esc => {
                self.mode = Mode::Desk;
                self.desc_open = false;
                self.fill = false;
                self.cmd.clear();
                self.scroll = 0;
                Effect::None
            }
            Key::CtrlV => Effect::None,
            Key::F(3) => {
                self.toggle_fill();
                Effect::None
            }
            Key::CtrlP => {
                if let Some(line) = self.history.previous(&self.cmd) {
                    self.cmd = line;
                    self.suggest = 0;
                }
                Effect::None
            }
            Key::CtrlN => {
                if let Some(line) = self.history.next() {
                    self.cmd = line;
                    self.suggest = 0;
                }
                Effect::None
            }
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
            Key::Wheel(delta) => {
                self.nudge(i32::from(delta));
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
            Key::Char(_) if self.fill => Effect::None,
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
                self.scroll = 0;
                self.fill = false;
                self.mode = Mode::Desk;
                Effect::None
            }
            crate::ui::Spot::Desc => {
                self.desc_open = !self.desc_open;
                Effect::None
            }
            crate::ui::Spot::Cmd => Effect::None,
            crate::ui::Spot::Row(index) => {
                if !self.mode_is_desk() {
                    return Effect::None;
                }
                if self.cursor == index {
                    self.activate()
                } else {
                    self.cursor = index;
                    Effect::None
                }
            }
        }
    }

    pub fn reload(&mut self) {
        self.chats = chats::load(&self.home);
        self.links_key.clear();
        self.refresh_mesh();
    }

    fn shown_accept(&self) -> AcceptLevel {
        if self.section == Section::Chats
            && let Some(chat) = self.chats.get(self.cursor)
            && let Some((_, level)) = self.chat_accept.iter().find(|(id, _)| id == &chat.id)
        {
            return *level;
        }
        self.mesh_level().unwrap_or(AcceptLevel::Ask)
    }

    fn mesh_level(&self) -> Option<AcceptLevel> {
        let root = mesh::git_root(&self.dir)?;
        let key = root.display().to_string();
        self.mesh_accept
            .iter()
            .find(|(path, _)| path == &key)
            .map(|(_, level)| *level)
    }

    fn task_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Usage  {}", self.usage_line),
            "Questions  no data".into(),
            format!("permit {}  effort {}", self.permit.label(), self.effort),
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

    fn file_line(&self, row: &FileRow) -> String {
        let ink = health::ink_code(&row.name);
        let changed = mesh::git_root(&self.dir)
            .and_then(|root| health::rel(&root, &row.path))
            .is_some_and(|rel| self.dirty.iter().any(|item| item == &rel));
        let mark = if changed { '*' } else { ' ' };
        format!("{mark}{} \x1b[38;5;{ink}m{}\x1b[0m", row.icon, row.name)
    }

    fn refresh_mesh(&mut self) {
        let root = mesh::git_root(&self.dir);
        let mut lines = Vec::new();
        match &root {
            Some(path) => lines.push(format!("Repo  {}", path.display())),
            None => lines.push("Repo  none".into()),
        }
        let known = gitread::known_repos(&self.home);
        if known.is_empty() {
            lines.push("Weavatrix  none".into());
        } else {
            let names = known
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("Weavatrix  {names}"));
        }
        let mut set = Vec::new();
        if let Some(path) = &root {
            set.push((health::root_name(path), path.clone()));
        }
        for (name, path) in known {
            if set.iter().any(|(_, have)| have == &path) {
                continue;
            }
            if path.join(".git").exists() {
                set.push((name, path));
            }
            if set.len() == 6 {
                break;
            }
        }
        self.ensure_links(&set);
        if !self.link_note.is_empty() {
            lines.push(format!("Linked repos  {}", self.link_note));
        } else if self.links.is_empty() {
            lines.push("Linked repos  none".into());
        } else {
            lines.push("Linked repos".into());
            lines.extend(self.links.iter().cloned());
        }
        let root_path = root.clone().unwrap_or_else(|| self.dir.clone());
        let envs = health::env_names(&root_path);
        if envs.is_empty() {
            lines.push("Env  none".into());
        } else {
            lines.push(format!("Env  {}", envs.join("  ")));
        }
        self.skills = skills::load(&self.home, &root_path);
        if self.skills.is_empty() {
            lines.push("Skills  none".into());
        } else {
            let claude = self.skills.iter().filter(|skill| skill.agent == "claude").count();
            let codex = self.skills.iter().filter(|skill| skill.agent == "codex").count();
            let shared = self.skills.iter().filter(|skill| skill.agent == "shared").count();
            lines.push(format!("Skills  claude {claude}  codex {codex}  shared {shared}"));
            for skill in self.skills.iter().take(8) {
                lines.push(format!("  {}  {}", skill.agent, skill.name));
            }
        }
        lines.push(format!(
            "Health  roads {}  extern {}  accept {}",
            on_off(self.roads),
            on_off(self.externals),
            self.shown_accept().label()
        ));
        if self.actions.is_empty() {
            lines.push("Actions  none".into());
        } else {
            lines.push("Actions".into());
            for action in self.actions.iter().take(8) {
                lines.push(format!("  {action}"));
            }
        }
        if let Some(path) = &root {
            lines.extend(health::picture(path, self.roads, self.externals));
            self.dirty = gitread::dirty_paths(path).unwrap_or_default();
        } else {
            lines.push("No towers.".into());
            self.dirty.clear();
        }
        self.mesh_view = lines;
    }

    fn ensure_links(&mut self, repos: &[(String, PathBuf)]) {
        let key = repos
            .iter()
            .map(|(name, path)| format!("{name}:{}", path.display()))
            .collect::<Vec<_>>()
            .join("|");
        if key == self.links_key {
            return;
        }
        self.links_key = key;
        match gitread::linked(repos) {
            Ok(links) => {
                self.links = links;
                self.link_note.clear();
            }
            Err(err) => {
                self.links.clear();
                self.link_note = err;
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
        if !self.mode_is_desk() {
            if delta < 0 {
                self.scroll = self.scroll.saturating_sub(delta.unsigned_abs() as usize);
            } else {
                let len = self.lines().len();
                self.scroll = (self.scroll + delta as usize).min(len.saturating_sub(1));
            }
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
        self.scroll = 0;
        self.mode = Mode::Desk;
        self.fill = false;
    }

    fn toggle_fill(&mut self) {
        if !matches!(self.mode, Mode::Desk) {
            self.fill = !self.fill;
            self.scroll = 0;
        }
    }

    fn activate(&mut self) -> Effect {
        if !self.cmd.is_empty() {
            let line = self.cmd.clone();
            self.history.push(&line);
            self.run(&line);
            self.cmd.clear();
            return Effect::None;
        }
        match self.section {
            Section::Files => self.open_file(false),
            Section::Chats => self.open_chat(false),
            Section::Lessons => self.open_lesson(),
            Section::Browser => {
                if !self.page.is_empty() {
                    self.open_split(self.page.clone(), None);
                }
                Effect::None
            }
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
                    self.mode = Mode::Desk;
                    self.fill = false;
                }
            }
            "task" => self.add_task(&rest),
            "move" => self.move_chat(&rest),
            "accept" => match AcceptLevel::parse(&rest) {
                Some(level) => self.set_accept(level),
                None => {
                    self.note = "accept: ask, safe, except-push, except-destructive, full".into();
                }
            },
            "pause" => {
                self.paused = !self.paused;
                self.record(if self.paused { "pause" } else { "resume" });
                self.persist();
            }
            "permit" => match Permit::parse(&rest) {
                Some(permit) => {
                    self.permit = permit;
                    self.record(&format!("permit {}", permit.label()));
                    self.persist();
                }
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
            "browse" => self.browse(&rest),
            "history" => self.show_history(),
            "blob" => self.show_blob(&rest),
            "health" => self.health_cmd(&rest),
            "fill" => self.toggle_fill(),
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
        self.record(&format!("task {title}"));
        self.persist();
        self.refresh_mesh();
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
            self.record(&format!("move {id} to {}", agent.label()));
            self.persist();
            self.refresh_mesh();
        }
    }

    fn set_accept(&mut self, level: AcceptLevel) {
        let key = mesh::git_root(&self.dir)
            .unwrap_or_else(|| self.dir.clone())
            .display()
            .to_string();
        if let Some(slot) = self.mesh_accept.iter_mut().find(|(path, _)| path == &key) {
            slot.1 = level;
        } else {
            self.mesh_accept.push((key, level));
        }
        if self.section == Section::Chats
            && let Some(id) = self.chats.get(self.cursor).map(|chat| chat.id.clone())
        {
            if let Some(slot) = self.chat_accept.iter_mut().find(|(chat, _)| chat == &id) {
                slot.1 = level;
            } else {
                self.chat_accept.push((id, level));
            }
        }
        self.accept = level;
        self.record(&format!("accept {}", level.label()));
        self.persist();
        self.refresh_mesh();
    }

    fn set_model(&mut self, name: &str) {
        if name.is_empty() {
            self.note = "model needs a name".into();
            return;
        }
        self.model = name.into();
        if self.cursor >= 4
            && let Some(task) = self.tasks.get_mut(self.cursor - 4)
        {
            task.model = self.model.clone();
        }
        self.record(&format!("model {name}"));
        self.persist();
    }

    fn set_effort(&mut self, name: &str) {
        let agent = self
            .chats
            .get(self.cursor)
            .map(|chat| chat.agent)
            .unwrap_or(AgentKind::Grok);
        if agent.efforts().contains(&name) {
            self.effort = name.into();
            if let Some(task) = self.tasks.last_mut() {
                task.effort = self.effort.clone();
            }
            self.record(&format!("effort {name}"));
            self.persist();
        } else {
            self.note = format!("{} effort: {}", agent.label(), agent.efforts().join(", "));
        }
    }

    fn health_cmd(&mut self, name: &str) {
        match name {
            "roads" => self.roads = !self.roads,
            "extern" => self.externals = !self.externals,
            _ => {
                self.note = "health roads, or health extern".into();
                return;
            }
        }
        self.record(&format!("health {name}"));
        self.persist();
        self.refresh_mesh();
        self.section = Section::Mesh;
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
                self.page.clear();
                self.section = Section::Browser;
                self.mode = Mode::Desk;
            }
            Err(err) => self.note = err,
        }
    }

    fn browse(&mut self, target: &str) {
        if target.is_empty() {
            self.section = Section::Browser;
            self.mode = Mode::Desk;
            return;
        }
        match page::load(target, &self.dir) {
            Ok(lines) => {
                self.page = lines.clone();
                self.open_split(lines, None);
                self.section = Section::Browser;
            }
            Err(err) => self.note = err,
        }
    }

    fn show_history(&mut self) {
        let Some(root) = mesh::git_root(&self.dir) else {
            self.note = "Repo  none".into();
            return;
        };
        match gitread::recent(&root, 20) {
            Ok(revs) => {
                let mut lines = vec![format!("history  {}", root.display())];
                if revs.is_empty() {
                    lines.push("No commits.".into());
                }
                for rev in revs {
                    lines.push(format!("{}  {}  {}", rev.id, rev.when, rev.subject));
                }
                self.open_split(lines, None);
            }
            Err(err) => self.note = err,
        }
    }

    fn show_blob(&mut self, path: &str) {
        if path.is_empty() {
            self.note = "blob needs a path".into();
            return;
        }
        let Some(root) = mesh::git_root(&self.dir) else {
            self.note = "Repo  none".into();
            return;
        };
        match gitread::blob_text(&root, "HEAD", path, 64 * 1024) {
            Ok(text) => {
                let mut lines = vec![
                    format!("where  {path}"),
                    "what  blob at HEAD".into(),
                ];
                lines.extend(text.lines().take(200).map(str::to_string));
                self.mode = Mode::View { lines };
                self.fill = false;
                self.scroll = 0;
            }
            Err(err) => self.note = err,
        }
    }

    fn open_chat(&mut self, context: bool) -> Effect {
        let Some(chat) = self.chats.get(self.cursor).cloned() else {
            self.note = "No chat.".into();
            return Effect::None;
        };
        let body = if context {
            let mut lines = vec!["Context".into()];
            lines.extend(read_context(&chat.path));
            lines
        } else {
            vec!["Opened.".into()]
        };
        let lines = dress(chat.agent, &chat.title, body);
        self.open_split(lines, Some(chat.agent));
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
        self.fill = false;
        self.scroll = 0;
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
            self.links_key.clear();
            self.refresh_mesh();
            return Effect::None;
        }
        if edit {
            return self.edit_path(row.path.clone());
        }
        let text = std::fs::read_to_string(&row.path).unwrap_or_default();
        let mut lines = self.file_header(&row.path);
        lines.extend(text.lines().take(200).map(str::to_string));
        self.mode = Mode::View { lines };
        self.fill = false;
        self.scroll = 0;
        Effect::None
    }

    fn file_header(&self, path: &Path) -> Vec<String> {
        let Some(root) = mesh::git_root(&self.dir) else {
            return vec![format!("where  {}", path.display())];
        };
        let rel = health::rel(&root, path).unwrap_or_else(|| path.display().to_string());
        let mut lines = vec![format!("where  {rel}")];
        match gitread::last_touch(&root, &rel) {
            Ok(Some(rev)) => {
                lines.push(format!("when  {}", rev.when));
                lines.push(format!("what  {}", rev.subject));
            }
            Ok(None) => lines.push("when  no commit for this path".into()),
            Err(err) => lines.push(format!("when  {err}")),
        }
        if self.dirty.iter().any(|item| item == &rel) {
            lines.push("changed  working tree".into());
        }
        lines
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
            self.record(&format!("edit {}", path.display()));
            self.persist();
            Effect::Edit(path)
        } else {
            self.note = "Vim is off.".into();
            Effect::None
        }
    }

    fn open_split(&mut self, lines: Vec<String>, agent: Option<AgentKind>) {
        self.mode = Mode::Split { lines, agent };
        self.fill = false;
        self.scroll = 0;
        self.desc_open = false;
    }

    fn record(&mut self, text: &str) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let day = secs % 86_400;
        self.actions.insert(
            0,
            format!("{:02}:{:02} {text}", day / 3600, (day % 3600) / 60),
        );
        self.actions.truncate(40);
    }

    fn persist(&self) {
        let state = store::DeskState {
            mesh_accept: self.mesh_accept.clone(),
            chat_accept: self.chat_accept.clone(),
            paused: self.paused,
            permit: self.permit,
            roads: self.roads,
            externals: self.externals,
            actions: self.actions.clone(),
        };
        let _ = store::save(&self.home, &state);
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn paint_line(line: &str) -> String {
    if line.starts_with("where ")
        || line.starts_with("when ")
        || line.starts_with("what ")
        || line.starts_with("changed ")
    {
        line.to_string()
    } else {
        hl::paint(line)
    }
}

fn dress(agent: AgentKind, title: &str, body: Vec<String>) -> Vec<String> {
    let head = match agent {
        AgentKind::Grok => format!("\x1b[38;5;196mGROK  {title}"),
        AgentKind::Claude => format!("\x1b[38;5;39m❯  {title}"),
        AgentKind::Codex => format!("\x1b[38;5;196mCODEX\x1b[38;5;39m  {title}"),
    };
    let mut lines = vec![head, String::new()];
    lines.extend(body);
    lines
}

fn push_second(found: &mut Vec<String>, head: &str, partial: &str) {
    let choices: &[&str] = match head {
        "move" => &["grok", "claude", "codex"],
        "accept" => &["ask", "safe", "except-push", "except-destructive", "full"],
        "permit" => &["ask", "edits", "bypass", "plan"],
        "effort" => &["low", "medium", "high", "xhigh", "max", "ultra"],
        "health" => &["roads", "extern"],
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
        if path.is_dir() {
            return vec!["No transcript file in this session.".into()];
        }
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

    #[test]
    fn control_p_recalls_the_last_command() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.cmd = "mesh".into();
        app.on_key(Key::Enter);
        app.on_key(Key::CtrlP);
        assert_eq!(app.cmd, "mesh");
        app.on_key(Key::CtrlN);
        assert!(app.cmd.is_empty());
    }

    #[test]
    fn local_page_opens_in_a_split() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-page-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hi.html"), "<title>Башни</title><p>Hello</p><script>no</script>").unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.cmd = "browse hi.html".into();
        app.on_key(Key::Enter);
        let joined = app.lines().join("\n");
        assert!(joined.contains("Башни"));
        assert!(joined.contains("Hello"));
        assert!(!joined.contains("no"));
        app.on_key(Key::F(3));
        assert!(app.fill);
        app.on_key(Key::F(3));
        assert!(!app.fill);
        let _ = std::fs::remove_dir_all(dir);
    }
}
