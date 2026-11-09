use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agents;
use crate::chats::{self, files};
use crate::draw;
use crate::gitread;
use crate::health;
use crate::history::History;
use crate::hl;
use crate::inline;
use crate::lessons;
use crate::mesh;
use crate::model::{AcceptLevel, AgentKind, FileRow, Permit, Section, Task};
use crate::page;
use crate::skills::{self, Skill};
use crate::store;
use crate::term::Key;
use crate::usage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
}

pub struct App {
    pub section: Section,
    pub cmd: String,
    pub cursor: usize,
    pub scroll: usize,
    pub pane_from_end: usize,
    pub suggest: usize,
    pub note: String,
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
    questions_line: String,
    asks: Vec<agents::Ask>,
    mesh_health: Vec<(String, bool, bool)>,
    confirm: String,
    image_overlay: String,
    repos: Vec<(String, PathBuf)>,
    vim_path: Option<PathBuf>,
    pane_key: String,
}

enum Mode {
    Desk,
    View {
        lines: Vec<String>,
    },
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
        "Set auto-accept for this mesh and the selected chat. ask and safe repeat a write. except-push runs writes and still asks before push. except-destructive and full run push. Pause asks first.",
    ),
    ("pause", "Pause or resume auto-accept."),
    (
        "permit",
        "Set permission mode and write it into the agent config: ask, edits, bypass, plan.",
    ),
    (
        "model",
        "Set the model on the selected agent and its config.",
    ),
    (
        "effort",
        "Set model effort within that agent's catalog and write it into the config.",
    ),
    ("image", "Load an image path and show its cell preview."),
    ("open", "Open the selected chat in a split."),
    (
        "context",
        "Open the selected chat in a split with the transcript context.",
    ),
    ("desc", "Open the command description in the lower split."),
    ("browse", "Draw a local file or http(s) page in a split."),
    ("history", "Read recent commits through Weavatrix git."),
    (
        "blob",
        "Read a file at HEAD through Weavatrix git: blob <path>.",
    ),
    (
        "health",
        "Toggle mesh code-health: health roads, or health extern.",
    ),
    ("skill", "Open a shared skill file: skill <name>."),
    (
        "commit",
        "Commit with git commit. Stages nothing; only what is already staged is committed.",
    ),
    (
        "push",
        "Push with git push. except-destructive and full run it. Every other level asks once, then the same command repeats.",
    ),
    (
        "fill",
        "Expand the open split to the whole terminal, or collapse it.",
    ),
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
            pane_from_end: 0,
            suggest: 0,
            note: String::new(),
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
            history: History::seed(stored.history.clone()),
            chats,
            tasks: stored.tasks.clone(),
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
            questions_line: agents::questions(&home),
            asks: agents::asks(&home),
            mesh_health: stored.mesh_health,
            confirm: String::new(),
            image_overlay: String::new(),
            repos: Vec::new(),
            vim_path: None,
            pane_key: String::new(),
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

    pub fn composer_status(&self) -> (AgentKind, String) {
        let agent = self.selected_agent().unwrap_or(AgentKind::Grok);
        let live = agents::face(&self.home, &self.dir, agent, self.permit);
        let model = if live.model != "—" {
            live.model
        } else if !self.model.is_empty() {
            self.model.clone()
        } else {
            "—".into()
        };
        (
            agent,
            format!(
                "{model} ({}) · {} · {}",
                live.effort,
                live.permission,
                self.accept_label()
            ),
        )
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

    pub fn image_overlay(&self) -> &str {
        if self.section == Section::Browser {
            &self.image_overlay
        } else {
            ""
        }
    }

    pub fn mode_is_desk(&self) -> bool {
        matches!(self.mode, Mode::Desk) && !self.desc_open && !self.fill
    }

    pub fn suggestions(&self) -> Vec<String> {
        if !self.mode_is_desk() && !self.fill {
            return Vec::new();
        }
        let token = self.cmd.split_whitespace().next().unwrap_or("");
        if token.is_empty() {
            return self.section_verbs();
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
            if head == "skill" {
                push_skills(&mut found, &self.skills, "");
            }
        } else if let Some((head, partial)) = self.cmd.split_once(' ') {
            found.clear();
            push_second(&mut found, head, partial.trim());
            if head == "skill" {
                push_skills(&mut found, &self.skills, partial.trim());
            }
        }
        found.truncate(6);
        found
    }

    pub fn prompt_hint(&self) -> &'static str {
        if !self.mode_is_desk() {
            return "esc";
        }
        match self.section {
            Section::Chats => "open",
            Section::Tasks => "task",
            Section::Mesh => "health",
            Section::Files => "edit",
            Section::Browser => "browse",
            Section::Lessons => "open",
        }
    }

    pub fn section_title(&self) -> String {
        match self.section {
            Section::Chats => format!("Chats  {}", self.chats.len()),
            Section::Files => format!("Files  {}", self.repo_name()),
            Section::Tasks => format!("Tasks  {}", self.repo_name()),
            other => other.label().to_string(),
        }
    }

    pub fn description_line(&self) -> String {
        if !self.note.is_empty() {
            return self.note.clone();
        }
        if self.vim_active() {
            return "vim on in this split".into();
        }
        if self.fill && self.transcript_lines().is_some() {
            return "Wheel and arrows scroll this chat. Esc closes.".into();
        }
        if self.pane().is_some() {
            return "Wheel scrolls this chat. Arrows open the next one. Esc closes. F3 fills."
                .into();
        }
        let head = self.cmd.split_whitespace().next().unwrap_or("");
        if head.is_empty() {
            return match self.section {
                Section::Chats => {
                    "Grouped by agent, then mesh. Inside a mesh, newest first. Enter opens the chat."
                        .into()
                }
                Section::Tasks => {
                    "Each agent, then that mesh. Queue tasks sit under the agent. Newest first."
                        .into()
                }
                Section::Mesh => {
                    "Enter opens a skill. health and history stay on this repo.".into()
                }
                Section::Files => "Enter opens the file. F4 edits it in this split.".into(),
                Section::Browser => "Enter opens the page. browse loads a file or a url.".into(),
                Section::Lessons => "Enter opens the lesson.".into(),
            };
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

    pub fn pane(&self) -> Option<&[String]> {
        if self.fill || !matches!(self.section, Section::Chats | Section::Tasks) {
            return None;
        }
        self.transcript_lines()
    }

    pub fn transcript_lines(&self) -> Option<&[String]> {
        if !matches!(self.section, Section::Chats | Section::Tasks) {
            return None;
        }
        match &self.mode {
            Mode::Split { lines, .. } => Some(lines),
            _ => None,
        }
    }

    pub fn focus_index(&self) -> usize {
        if !matches!(self.section, Section::Chats | Section::Tasks) {
            return self.cursor;
        }
        let rows = self.section_rows();
        if rows.is_empty() {
            return 0;
        }
        focus_in(&rows, self.cursor)
    }

    pub fn rows_for(&self, width: usize) -> Vec<String> {
        match self.section {
            Section::Chats | Section::Tasks => self
                .section_rows()
                .iter()
                .map(|row| self.row_line(row, width))
                .collect(),
            _ => self.list_lines(),
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if let Mode::View { lines } = &self.mode {
            return paint_view(lines);
        }
        if let Mode::Split { lines, .. } = &self.mode {
            return lines.clone();
        }
        if self.desc_open {
            return vec![self.description_body()];
        }
        if matches!(self.section, Section::Chats | Section::Tasks) {
            self.rows_for(78)
        } else {
            self.list_lines()
        }
    }

    fn list_lines(&self) -> Vec<String> {
        match self.section {
            Section::Chats | Section::Tasks => self.rows_for(78),
            Section::Mesh => self.mesh_view.clone(),
            Section::Files => self
                .file_rows()
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

    pub fn history_band(&self) -> Vec<String> {
        if !self.mode_is_desk() || !matches!(self.section, Section::Chats | Section::Tasks) {
            return Vec::new();
        }
        let Some(chat) = self.band_chat() else {
            return vec!["History  none".into()];
        };
        let mut lines = vec![format!(
            "History  {}  {}",
            chat.agent.label(),
            repo_label(&chat.cwd)
        )];
        let tail = chats::transcript(&chat.path);
        let start = tail.len().saturating_sub(4);
        for line in tail.into_iter().skip(start) {
            lines.push(line.replace(['\n', '\r'], " "));
        }
        lines.truncate(5);
        lines
    }

    pub fn side_lines(&self) -> Vec<String> {
        if self.section != Section::Files || !self.mode_is_desk() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for (name, path) in self.linked_paths() {
            lines.push(clip_width(&format!("~ {name}"), 36));
            for row in files(&path) {
                if row.name == ".." {
                    continue;
                }
                lines.push(clip_width(&format!("{} {}", row.icon, row.name), 36));
            }
        }
        lines
    }

    pub fn ask_lines(&self) -> Vec<String> {
        if self.fill || !matches!(self.section, Section::Chats | Section::Tasks) {
            return Vec::new();
        }
        self.asks
            .iter()
            .take(3)
            .map(|ask| {
                format!(
                    "needs you  {}  {}  {}",
                    ask.agent.label(),
                    repo_label(&ask.cwd),
                    ask.label
                )
            })
            .collect()
    }

    pub fn on_key(&mut self, key: Key) -> Effect {
        self.refresh_asks();
        self.note.clear();
        if self.vim_active() {
            return self.on_vim_key(key);
        }
        match key {
            Key::Ignore => Effect::None,
            Key::CtrlC | Key::F(10) => Effect::Quit,
            Key::Esc => {
                if self.fill {
                    self.fill = false;
                    return Effect::None;
                }
                self.vim_path = None;
                self.mode = Mode::Desk;
                self.pane_key.clear();
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
                if self.fill && self.transcript_lines().is_some() {
                    self.scroll_transcript(-1);
                } else {
                    self.nudge(-1);
                }
                Effect::None
            }
            Key::Down => {
                if self.fill && self.transcript_lines().is_some() {
                    self.scroll_transcript(1);
                } else {
                    self.nudge(1);
                }
                Effect::None
            }
            Key::Wheel(delta) | Key::Page(delta) => {
                if self.transcript_lines().is_some() {
                    self.scroll_transcript(i32::from(delta));
                } else {
                    self.nudge(i32::from(delta));
                }
                Effect::None
            }
            Key::Left | Key::Right if self.cmd.is_empty() => {
                self.step_tab(if matches!(key, Key::Right) { 1 } else { -1 });
                Effect::None
            }
            Key::F(2) | Key::F(4) => self.edit_selected(),
            Key::Enter => self.activate(),
            Key::Backspace => {
                self.confirm.clear();
                self.cmd.pop();
                self.suggest = 0;
                Effect::None
            }
            Key::Char(ch) => {
                self.confirm.clear();
                self.cmd.push(ch);
                self.suggest = 0;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    pub fn press(&mut self, spot: crate::ui::Spot, times: u8) -> Effect {
        self.refresh_asks();
        match spot {
            crate::ui::Spot::Ask(index) => self.open_ask(index),
            crate::ui::Spot::Tab(section) => {
                self.enter(section);
                Effect::None
            }
            crate::ui::Spot::Desc => {
                self.desc_open = !self.desc_open;
                Effect::None
            }
            crate::ui::Spot::Cmd => Effect::None,
            crate::ui::Spot::Row(index) => {
                let index = if matches!(self.section, Section::Chats | Section::Tasks) {
                    let rows = self.section_rows();
                    if rows.is_empty() {
                        index
                    } else {
                        focus_in(&rows, index)
                    }
                } else {
                    index
                };
                if !self.mode_is_desk() {
                    return Effect::None;
                }
                let same = self.cursor == index;
                self.cursor = index;
                if self.pane().is_some() || same || times >= 2 {
                    self.open_selected()
                } else {
                    Effect::None
                }
            }
        }
    }

    pub fn apply_keys(
        &mut self,
        keys: &[crate::term::Key],
        mut spot_at: impl FnMut(usize, usize) -> Option<crate::ui::Spot>,
    ) -> Effect {
        use crate::term::Key;
        let mut quit = false;
        let mut index = 0;
        while index < keys.len() {
            if let Key::Click { x, y } = &keys[index] {
                let (x, y) = (*x, *y);
                let mut times = 1u8;
                let mut next = index + 1;
                while next < keys.len() {
                    match &keys[next] {
                        Key::Ignore => next += 1,
                        Key::Click { x: nx, y: ny } if *nx == x && *ny == y => {
                            times = times.saturating_add(1);
                            next += 1;
                        }
                        _ => break,
                    }
                }
                index = next;
                if x > 0
                    && y > 0
                    && let Some(spot) = spot_at(x - 1, y - 1)
                    && self.press(spot, times) == Effect::Quit
                {
                    quit = true;
                }
                continue;
            }
            let key = keys[index].clone();
            index += 1;
            if key != Key::Ignore && self.on_key(key) == Effect::Quit {
                quit = true;
            }
        }
        if quit { Effect::Quit } else { Effect::None }
    }

    fn enter(&mut self, section: Section) {
        self.section = section;
        self.cursor = 0;
        self.scroll = 0;
        self.vim_path = None;
        self.pane_key.clear();
        self.mode = Mode::Desk;
        self.fill = false;
        if section == Section::Tasks {
            self.usage_line = usage::line(&self.home);
            self.questions_line = agents::questions(&self.home);
        }
        if section == Section::Files {
            self.dir = self.repo_root();
        }
        if section == Section::Browser {
            self.ensure_page();
        }
    }

    fn shown_accept(&self) -> AcceptLevel {
        if let Some(id) = self.selected_chat_id()
            && let Some((_, level)) = self.chat_accept.iter().find(|(chat, _)| chat == &id)
        {
            return *level;
        }
        let key = self.mesh_key();
        self.mesh_accept
            .iter()
            .find(|(path, _)| path == &key)
            .map(|(_, level)| *level)
            .unwrap_or(AcceptLevel::Ask)
    }

    fn allows(&self) -> bool {
        if self.paused {
            return false;
        }
        matches!(
            self.shown_accept(),
            AcceptLevel::ExceptPush | AcceptLevel::ExceptDestructive | AcceptLevel::Full
        )
    }

    fn blocked(&mut self, key: &str) -> bool {
        self.hold(key, self.allows())
    }

    fn allows_push(&self) -> bool {
        if self.paused {
            return false;
        }
        matches!(
            self.shown_accept(),
            AcceptLevel::ExceptDestructive | AcceptLevel::Full
        )
    }

    fn hold(&mut self, key: &str, allow: bool) -> bool {
        if allow {
            self.confirm.clear();
            return false;
        }
        if self.confirm == key {
            self.confirm.clear();
            return false;
        }
        self.confirm = key.to_string();
        self.note = if self.paused {
            "paused  repeat to run".into()
        } else {
            "ask  repeat to run".into()
        };
        true
    }

    fn file_line(&self, row: &FileRow) -> String {
        let ink = health::ink_code(&row.name);
        let changed = mesh::git_root(&self.dir)
            .and_then(|root| health::rel(&root, &row.path))
            .is_some_and(|rel| self.dirty.iter().any(|item| item == &rel));
        let mark = if changed { '*' } else { ' ' };
        format!(
            "{mark}{} \x1b[38;5;{ink}m{}\x1b[0;48;5;0m",
            row.icon, row.name
        )
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
        self.repos = set.clone();
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
        let keys = health::env_example_keys(&root_path);
        if keys.is_empty() {
            lines.push("Env keys  none".into());
        } else {
            lines.push(format!("Env keys  {}", keys.join("  ")));
        }
        self.skills = skills::load(&self.home, &root_path);
        if self.skills.is_empty() {
            lines.push("Skills  none".into());
        } else {
            let claude = self
                .skills
                .iter()
                .filter(|skill| skill.agent == "claude")
                .count();
            let codex = self
                .skills
                .iter()
                .filter(|skill| skill.agent == "codex")
                .count();
            let shared = self
                .skills
                .iter()
                .filter(|skill| skill.agent == "shared")
                .count();
            lines.push(format!(
                "Skills  claude {claude}  codex {codex}  shared {shared}"
            ));
            for skill in self.skills.iter().take(8) {
                lines.push(format!("  {}  {}", skill.agent, skill.name));
            }
        }
        let (roads, externals) = self.health_flags();
        lines.push(format!(
            "Health  roads {}  extern {}  accept {}",
            on_off(roads),
            on_off(externals),
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
            let (roads, externals) = self.health_flags();
            lines.extend(health::picture(path, roads, externals));
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

    fn scroll_transcript(&mut self, delta: i32) {
        let step = if delta.abs() > 1 { 1 } else { 3 };
        let jump = step * delta.unsigned_abs() as usize;
        if delta < 0 {
            self.pane_from_end = self.pane_from_end.saturating_add(jump);
        } else {
            self.pane_from_end = self.pane_from_end.saturating_sub(jump);
        }
    }

    fn nudge(&mut self, delta: i32) {
        if self.cmd.split_whitespace().next().is_some() {
            let suggestions = self.suggestions();
            if !suggestions.is_empty() {
                let len = suggestions.len();
                self.suggest = clamp_index(self.suggest, delta, len);
                return;
            }
        }
        if self.pane().is_some() {
            if matches!(self.section, Section::Chats | Section::Tasks) {
                self.step_list(delta);
                self.follow_pane();
            }
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
        if matches!(self.section, Section::Chats | Section::Tasks) {
            self.step_list(delta);
            return;
        }
        let len = self.lines().len();
        if len == 0 {
            return;
        }
        self.cursor = clamp_index(self.cursor, delta, len);
    }

    fn step_tab(&mut self, delta: i32) {
        let index = Section::ALL
            .iter()
            .position(|section| *section == self.section)
            .unwrap_or(0) as i32;
        let next = (index + delta).rem_euclid(Section::ALL.len() as i32) as usize;
        self.enter(Section::ALL[next]);
    }

    fn toggle_fill(&mut self) {
        if matches!(self.mode, Mode::Desk) {
            return;
        }
        self.fill = !self.fill;
        if self.fill && !self.pane_key.is_empty() {
            self.scroll = usize::MAX;
        } else if self.fill {
            self.scroll = 0;
        }
    }

    fn activate(&mut self) -> Effect {
        if !self.cmd.is_empty() {
            let line = self.cmd.clone();
            if self.run(&line) {
                self.history.push(&line);
                self.persist();
                self.cmd.clear();
                self.confirm.clear();
            }
            return Effect::None;
        }
        self.open_selected()
    }

    fn open_selected(&mut self) -> Effect {
        match self.section {
            Section::Files => self.open_file(false),
            Section::Chats => self.open_chat(false),
            Section::Tasks => self.open_task(),
            Section::Mesh => self.open_mesh_row(),
            Section::Lessons => self.open_lesson(),
            Section::Browser => {
                if !self.page.is_empty() {
                    self.open_split(self.page.clone(), None);
                }
                Effect::None
            }
        }
    }

    fn run(&mut self, line: &str) -> bool {
        let mut parts = line.split_whitespace();
        let Some(head) = parts.next() else {
            return true;
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match head {
            "chats" | "tasks" | "mesh" | "files" | "browser" | "lessons" => {
                if let Some(section) = Section::parse(head) {
                    self.enter(section);
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
            "permit" => {
                if Permit::parse(&rest).is_none() {
                    self.note = "permit: ask, edits, bypass, plan".into();
                    return true;
                }
                if self.blocked(line) {
                    return false;
                }
                if let Some(permit) = Permit::parse(&rest) {
                    self.permit = permit;
                    self.note = agents::apply_permit(&self.home, &self.dir, permit);
                    self.record(&format!("permit {}", permit.label()));
                    self.persist();
                }
            }
            "model" => {
                if self.blocked(line) {
                    return false;
                }
                self.set_model(&rest);
            }
            "effort" => {
                if self.blocked(line) {
                    return false;
                }
                self.set_effort(&rest);
            }
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
            "skill" => self.open_skill_named(&rest),
            "commit" => {
                if self.blocked(line) {
                    return false;
                }
                self.commit_outside(&rest);
            }
            "push" => {
                if self.hold(line, self.allows_push()) {
                    return false;
                }
                self.push_outside();
            }
            "fill" => self.toggle_fill(),
            _ => self.note = format!("No command {head}."),
        }
        true
    }

    fn add_task(&mut self, title: &str) {
        if title.is_empty() {
            self.note = "task needs a title".into();
            return;
        }
        let owner = match self.row_at(self.cursor) {
            Some(DeskRow::Chat(index)) => self
                .chats
                .get(index)
                .map(|chat| chat.agent)
                .unwrap_or(AgentKind::Grok),
            Some(DeskRow::Task(index)) => self
                .tasks
                .get(index)
                .map(|task| task.owner)
                .unwrap_or(AgentKind::Grok),
            _ => AgentKind::Grok,
        };
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
        let Some(DeskRow::Chat(index)) = self.row_at(self.cursor) else {
            self.note = "No chat.".into();
            return;
        };
        let Some(chat) = self.chats.get(index).cloned() else {
            self.note = "No chat.".into();
            return;
        };
        if !chats::handoff(&mut self.chats, &chat.id, agent) {
            return;
        }
        let saved = chats::write_handoff(&self.home, &chat, agent);
        let mut body = vec![format!("Moved to {}", agent.label())];
        match &saved {
            Ok(path) => body.push(format!("handoff  {}", path.display())),
            Err(err) => body.push(format!("handoff  {err}")),
        }
        body.push("Context".into());
        body.extend(chats::transcript(&chat.path));
        self.note = format!("Moved to {}", agent.label());
        self.record(&format!("move {} to {}", chat.id, agent.label()));
        self.persist();
        self.refresh_mesh();
        self.open_split(dress(agent, &chat.title, body), Some(agent));
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
        if let Some(id) = self.selected_chat_id() {
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
        if let Some(DeskRow::Task(index)) = self.row_at(self.cursor)
            && let Some(task) = self.tasks.get_mut(index)
        {
            task.model = self.model.clone();
        }
        self.note = agents::apply_model(&self.home, &self.dir, self.selected_agent(), name);
        self.record(&format!("model {name}"));
        self.persist();
    }

    fn set_effort(&mut self, name: &str) {
        let agent = self.selected_agent().unwrap_or(AgentKind::Grok);
        if !agent.efforts().contains(&name) {
            self.note = format!("{} effort: {}", agent.label(), agent.efforts().join(", "));
            return;
        }
        self.effort = name.into();
        if let Some(task) = self.tasks.last_mut() {
            task.effort = self.effort.clone();
        }
        self.note = agents::apply_effort(&self.home, agent, name);
        self.record(&format!("effort {name}"));
        self.persist();
    }

    fn selected_agent(&self) -> Option<AgentKind> {
        match self.row_at(self.cursor) {
            Some(DeskRow::Task(index)) => self.tasks.get(index).map(|task| task.owner),
            Some(DeskRow::Chat(index)) => self.chats.get(index).map(|chat| chat.agent),
            _ => None,
        }
    }

    fn health_cmd(&mut self, name: &str) {
        let key = self.mesh_key();
        let (mut roads, mut externals) = self.health_flags();
        match name {
            "roads" => roads = !roads,
            "extern" => externals = !externals,
            _ => {
                self.note = "health roads, or health extern".into();
                return;
            }
        }
        if let Some(slot) = self
            .mesh_health
            .iter_mut()
            .find(|(path, _, _)| path == &key)
        {
            slot.1 = roads;
            slot.2 = externals;
        } else {
            self.mesh_health.push((key, roads, externals));
        }
        self.record(&format!("health {name}"));
        self.persist();
        self.refresh_mesh();
        self.section = Section::Mesh;
    }

    fn mesh_key(&self) -> String {
        mesh::git_root(&self.dir)
            .unwrap_or_else(|| self.dir.clone())
            .display()
            .to_string()
    }

    fn health_flags(&self) -> (bool, bool) {
        let key = self.mesh_key();
        self.mesh_health
            .iter()
            .find(|(path, _, _)| path == &key)
            .map(|(_, roads, externals)| (*roads, *externals))
            .unwrap_or((self.roads, self.externals))
    }

    fn commit_outside(&mut self, message: &str) {
        if message.is_empty() {
            self.note = "commit needs a message".into();
            return;
        }
        let Some(root) = mesh::git_root(&self.dir) else {
            self.note = "Repo  none".into();
            return;
        };
        match Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&root)
            .output()
        {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.trim().is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(err.trim());
                }
                if text.trim().is_empty() {
                    text = format!("git commit exited {}", output.status);
                }
                self.open_split(text.lines().map(str::to_string).collect(), None);
                if output.status.success() {
                    self.record(&format!("commit {message}"));
                    self.persist();
                    self.links_key.clear();
                    self.refresh_mesh();
                } else {
                    self.note = "commit refused".into();
                }
            }
            Err(err) => self.note = format!("git: {err}"),
        }
    }

    fn push_outside(&mut self) {
        let Some(root) = mesh::git_root(&self.dir) else {
            self.note = "Repo  none".into();
            return;
        };
        match Command::new("git")
            .args(["push"])
            .current_dir(&root)
            .output()
        {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.trim().is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(err.trim());
                }
                if text.trim().is_empty() {
                    text = format!("git push exited {}", output.status);
                }
                self.open_split(text.lines().map(str::to_string).collect(), None);
                if output.status.success() {
                    self.record("push");
                    self.persist();
                    self.links_key.clear();
                    self.refresh_mesh();
                } else {
                    self.note = "push refused".into();
                }
            }
            Err(err) => self.note = format!("git: {err}"),
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
                self.image_overlay = inline::choose(&preview.sixel, &preview.kitty, &preview.iterm);
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
        self.image_overlay.clear();
        if target.is_empty() {
            self.section = Section::Browser;
            self.mode = Mode::Desk;
            self.cursor = 0;
            self.ensure_page();
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
                let mut lines = vec![format!("where  {path}"), "what  blob at HEAD".into()];
                lines.extend(text.lines().take(200).map(str::to_string));
                self.vim_path = None;
                self.mode = Mode::View { lines };
                self.fill = false;
                self.scroll = 0;
            }
            Err(err) => self.note = err,
        }
    }

    fn open_chat(&mut self, context: bool) -> Effect {
        let Some(DeskRow::Chat(index)) = self.row_at(self.cursor) else {
            self.note = "No chat.".into();
            return Effect::None;
        };
        let Some(chat) = self.chats.get(index).cloned() else {
            self.note = "No chat.".into();
            return Effect::None;
        };
        self.show_chat(&chat, context);
        Effect::None
    }

    fn follow_pane(&mut self) {
        if self.pane().is_none() {
            return;
        }
        match self.section {
            Section::Chats => {
                let _ = self.open_chat(false);
            }
            Section::Tasks => {
                let _ = self.open_task();
            }
            _ => {}
        }
    }

    fn refresh_asks(&mut self) {
        self.asks = agents::asks(&self.home);
        self.questions_line = agents::questions(&self.home);
    }

    fn needs_you(&self, chat: &crate::model::Chat) -> bool {
        self.asks.iter().any(|ask| ask.session_id == chat.id)
    }

    fn push_chat_state(&self, body: &mut Vec<String>, chat: &crate::model::Chat) {
        if let Some(context) = agents::context_line(&chat.path) {
            body.push(context);
        }
        if let Some(ask) = self.asks.iter().find(|ask| ask.session_id == chat.id) {
            body.push(format!("needs you  {}", ask.label));
        }
    }

    fn open_ask(&mut self, index: usize) -> Effect {
        let Some(ask) = self.asks.get(index).cloned() else {
            return Effect::None;
        };
        let Some(position) = self.chats.iter().position(|chat| chat.id == ask.session_id) else {
            self.note = "That chat is not in the list.".into();
            return Effect::None;
        };
        self.section = Section::Chats;
        let rows = self.section_rows();
        if let Some(cursor) = rows
            .iter()
            .position(|row| matches!(row, DeskRow::Chat(slot) if *slot == position))
        {
            self.cursor = cursor;
        }
        let chat = self.chats[position].clone();
        self.show_chat(&chat, false);
        Effect::None
    }

    fn show_chat(&mut self, chat: &crate::model::Chat, context: bool) {
        let key = format!("{}:{context}", chat.path.display());
        if self.pane_key == key && self.pane().is_some() {
            return;
        }
        let mut body = Vec::new();
        if context {
            body.push("Context".into());
        }
        body.push(format!("repo  {}", repo_label(&chat.cwd)));
        self.push_chat_state(&mut body, chat);
        body.extend(chats::transcript_full(&chat.path));
        self.open_split(dress(chat.agent, &chat.title, body), Some(chat.agent));
        self.pane_key = key;
        self.scroll = usize::MAX;
        self.pane_from_end = 0;
        self.section = Section::Chats;
    }

    fn open_task(&mut self) -> Effect {
        match self.row_at(self.cursor) {
            Some(DeskRow::Chat(index)) => {
                if let Some(chat) = self.chats.get(index).cloned() {
                    self.show_chat(&chat, true);
                }
            }
            Some(DeskRow::Task(index)) => {
                let Some(task) = self.tasks.get(index).cloned() else {
                    return Effect::None;
                };
                let place = self.place_label();
                let key = format!("task:{index}:{}", task.title);
                if self.pane_key == key && self.pane().is_some() {
                    return Effect::None;
                }
                let mut body = vec![
                    format!("owner  {}", task.owner.label()),
                    format!("model  {}", task.model),
                    format!("effort  {}", task.effort),
                    format!("repo  {place}"),
                ];
                if let Some(chat) = self.chat_for_owner(task.owner) {
                    self.push_chat_state(&mut body, &chat);
                    body.extend(chats::transcript_full(&chat.path));
                } else {
                    body.push("No transcript.".into());
                }
                self.open_split(dress(task.owner, &task.title, body), Some(task.owner));
                self.pane_key = key;
                self.scroll = usize::MAX;
                self.pane_from_end = 0;
            }
            _ => {}
        }
        Effect::None
    }

    fn open_skill_named(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.open_mesh_row();
            if matches!(self.mode, Mode::Desk) && self.note.is_empty() {
                self.note = "skill needs a name".into();
            }
            return;
        }
        let Some(skill) = self.skills.iter().find(|skill| skill.name == name).cloned() else {
            self.note = format!("No skill {name}.");
            return;
        };
        self.show_skill(&skill);
    }

    fn open_mesh_row(&mut self) -> Effect {
        let Some(line) = self.mesh_view.get(self.cursor).cloned() else {
            return Effect::None;
        };
        let Some(rest) = line.strip_prefix("  ") else {
            return Effect::None;
        };
        let Some((agent, name)) = rest.split_once("  ") else {
            return Effect::None;
        };
        let Some(skill) = self
            .skills
            .iter()
            .find(|skill| skill.agent == agent && skill.name == name)
            .cloned()
        else {
            return Effect::None;
        };
        self.show_skill(&skill);
        Effect::None
    }

    fn show_skill(&mut self, skill: &crate::skills::Skill) {
        let Ok(text) = std::fs::read_to_string(&skill.path) else {
            self.note = "Skill file is missing.".into();
            return;
        };
        let mut lines = vec![format!("{}  {}", skill.agent, skill.name)];
        lines.extend(text.lines().take(120).map(str::to_string));
        self.open_split(lines, None);
    }

    fn open_lesson(&mut self) -> Effect {
        let Some(lesson) = lessons::lessons().get(self.cursor) else {
            return Effect::None;
        };
        self.vim_path = None;
        self.mode = Mode::View {
            lines: vec![lesson.title.into(), lesson.body.into()],
        };
        self.fill = false;
        self.scroll = 0;
        Effect::None
    }

    fn open_file(&mut self, edit: bool) -> Effect {
        let rows = self.file_rows();
        let Some(row) = rows.get(self.cursor) else {
            return Effect::None;
        };
        if let Some(path) = self.linked_repo(&row.name) {
            let mut lines = vec![format!("Repo  {}", path.display())];
            for item in files(&path) {
                if item.name == ".." {
                    continue;
                }
                lines.push(format!("{} {}", item.icon, item.name));
            }
            self.open_split(lines, None);
            return Effect::None;
        }
        if row.dir {
            let root = self.repo_root();
            if row.name == ".." && !row.path.starts_with(&root) {
                self.note = format!("Repo  {}", self.repo_name());
                return Effect::None;
            }
            self.dir = row.path.clone();
            self.cursor = 0;
            self.links_key.clear();
            self.refresh_mesh();
            return Effect::None;
        }
        let text = std::fs::read_to_string(&row.path).unwrap_or_default();
        if edit {
            let name = row.name.trim_end_matches('/');
            let mut lines = vec![format!("vim  {name}")];
            lines.push(format!("where  {}", self.file_rel(&row.path)));
            lines.extend(text.lines().take(200).map(str::to_string));
            let body_at = lines.iter().take_while(|line| is_header(line)).count();
            self.vim_path = Some(row.path.clone());
            self.mode = Mode::View { lines };
            self.fill = false;
            self.scroll = body_at;
            return Effect::None;
        }
        let (changed, body) = self.marked_lines(&row.path, &text);
        let mut lines = self.file_header(&row.path, changed);
        lines.extend(body);
        self.vim_path = None;
        self.mode = Mode::View { lines };
        self.fill = false;
        self.scroll = 0;
        Effect::None
    }

    fn file_header(&self, path: &Path, changed: usize) -> Vec<String> {
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
        if changed > 0 {
            lines.push(format!("changed  {changed} lines"));
        } else if self.dirty.iter().any(|item| item == &rel) {
            lines.push("changed  working tree".into());
        }
        lines
    }

    fn marked_lines(&self, path: &Path, text: &str) -> (usize, Vec<String>) {
        let previous = self.head_lines(path);
        let mut changed = 0usize;
        let mut lines = Vec::new();
        for (index, line) in text.lines().take(200).enumerate() {
            let touched = previous
                .as_ref()
                .is_some_and(|old| old.get(index).is_none_or(|item| item != line));
            if touched {
                changed += 1;
                lines.push(format!("* {line}"));
            } else {
                lines.push(line.to_string());
            }
        }
        (changed, lines)
    }

    fn head_lines(&self, path: &Path) -> Option<Vec<String>> {
        let root = mesh::git_root(&self.dir)?;
        let rel = health::rel(&root, path)?;
        match gitread::blob_text(&root, "HEAD", &rel, 64 * 1024) {
            Ok(text) => Some(text.lines().map(str::to_string).collect()),
            Err(_) => Some(Vec::new()),
        }
    }

    fn edit_selected(&mut self) -> Effect {
        if self.section != Section::Files || !self.mode_is_desk() {
            self.note = "Vim edits the file in its split.".into();
            return Effect::None;
        }
        let rows = self.file_rows();
        let Some(row) = rows.get(self.cursor) else {
            self.note = "No file.".into();
            return Effect::None;
        };
        if row.dir || self.linked_repo(&row.name).is_some() {
            self.note = "Not a file.".into();
            return Effect::None;
        }
        self.open_file(true)
    }

    fn open_split(&mut self, lines: Vec<String>, agent: Option<AgentKind>) {
        self.vim_path = None;
        self.pane_key.clear();
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

    fn ensure_page(&mut self) {
        if !self.page.is_empty() {
            return;
        }
        let root = self.repo_root();
        for name in ["README.md", "README", "readme.md", "Readme.md"] {
            let path = root.join(name);
            if !path.is_file() {
                continue;
            }
            match page::load(&path.display().to_string(), &root) {
                Ok(lines) => self.page = lines,
                Err(err) => self.page = vec![err],
            }
            return;
        }
        self.page = vec![format!("Repo  {}", root.display()), "No README.".into()];
    }

    fn file_rows(&self) -> Vec<crate::model::FileRow> {
        files(&self.dir)
    }

    fn linked_paths(&self) -> Vec<(String, PathBuf)> {
        let root = self.repo_root();
        let mut out = Vec::new();
        for line in &self.links {
            for name in line.split(" ~ ") {
                let Some((_, path)) = self.repos.iter().find(|(repo, _)| repo == name) else {
                    continue;
                };
                if path != &root && !out.iter().any(|(_, have)| have == path) {
                    out.push((name.to_string(), path.clone()));
                }
            }
        }
        out
    }

    fn linked_repo(&self, name: &str) -> Option<PathBuf> {
        let rest = name.strip_prefix("~ ")?;
        self.linked_paths()
            .into_iter()
            .find(|(repo, _)| repo == rest)
            .map(|(_, path)| path)
    }

    fn repo_root(&self) -> PathBuf {
        mesh::git_root(&self.dir).unwrap_or_else(|| self.dir.clone())
    }

    fn repo_name(&self) -> String {
        let root = self.repo_root();
        root.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && *name != "." && *name != "..")
            .map(str::to_string)
            .unwrap_or_else(|| "this repo".into())
    }

    fn place_label(&self) -> String {
        if let Some(link) = self.links.first() {
            let trimmed = link.trim();
            if !trimmed.is_empty() {
                return clip_width(trimmed, 36);
            }
        }
        self.repo_name()
    }

    fn file_rel(&self, path: &Path) -> String {
        mesh::git_root(&self.dir)
            .and_then(|root| health::rel(&root, path))
            .unwrap_or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file")
                    .to_string()
            })
    }

    fn vim_active(&self) -> bool {
        self.vim_path.is_some() && matches!(self.mode, Mode::View { .. })
    }

    fn on_vim_key(&mut self, key: Key) -> Effect {
        match key {
            Key::CtrlC | Key::F(10) => Effect::Quit,
            Key::Esc => {
                self.vim_path = None;
                self.mode = Mode::Desk;
                self.desc_open = false;
                self.fill = false;
                self.cmd.clear();
                self.scroll = 0;
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
            Key::Backspace => {
                self.confirm.clear();
                self.vim_backspace();
                Effect::None
            }
            Key::Char(ch) if !self.fill => {
                self.confirm.clear();
                self.vim_type(ch);
                Effect::None
            }
            Key::Enter => {
                self.confirm.clear();
                self.vim_break();
                Effect::None
            }
            Key::F(2) | Key::F(4) => self.write_vim(),
            _ => Effect::None,
        }
    }

    fn vim_type(&mut self, ch: char) {
        let header = self.vim_header_len();
        if self.scroll < header {
            self.scroll = header;
        }
        let Mode::View { lines } = &mut self.mode else {
            return;
        };
        if self.scroll >= lines.len() {
            lines.push(String::new());
            self.scroll = lines.len() - 1;
        }
        if let Some(line) = lines.get_mut(self.scroll) {
            line.push(ch);
        }
    }

    fn vim_backspace(&mut self) {
        let header = self.vim_header_len();
        if self.scroll < header {
            return;
        }
        let Mode::View { lines } = &mut self.mode else {
            return;
        };
        if let Some(line) = lines.get_mut(self.scroll) {
            line.pop();
        }
    }

    fn vim_break(&mut self) {
        let header = self.vim_header_len();
        if self.scroll < header {
            self.scroll = header;
        }
        let Mode::View { lines } = &mut self.mode else {
            return;
        };
        let at = self.scroll.min(lines.len());
        if at >= lines.len() {
            lines.push(String::new());
        } else {
            lines.insert(at + 1, String::new());
        }
        self.scroll = at + 1;
    }

    fn vim_header_len(&self) -> usize {
        let Mode::View { lines } = &self.mode else {
            return 0;
        };
        lines.iter().take_while(|line| is_header(line)).count()
    }

    fn write_vim(&mut self) -> Effect {
        let Some(path) = self.vim_path.clone() else {
            return Effect::None;
        };
        let key = format!("write {}", path.display());
        if self.hold(&key, self.allows()) {
            return Effect::None;
        }
        let body = match &self.mode {
            Mode::View { lines } => vim_body(lines),
            _ => return Effect::None,
        };
        match std::fs::write(&path, body) {
            Ok(()) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                self.note = format!("wrote {name}");
            }
            Err(err) => self.note = format!("write {err}"),
        }
        Effect::None
    }

    fn section_verbs(&self) -> Vec<String> {
        let verbs: &[&str] = match self.section {
            Section::Chats => &["open", "context", "move", "task"],
            Section::Tasks => &["task", "permit", "model", "effort"],
            Section::Mesh => &["health", "skill", "history", "blob"],
            Section::Files => &["commit", "push", "blob", "history"],
            Section::Browser => &["browse", "image"],
            Section::Lessons => &["lessons"],
        };
        verbs.iter().map(|verb| (*verb).to_string()).collect()
    }

    fn row_at(&self, cursor: usize) -> Option<DeskRow> {
        let rows = self.section_rows();
        if rows.is_empty() {
            return None;
        }
        match rows.get(focus_in(&rows, cursor))? {
            DeskRow::Chat(index) => Some(DeskRow::Chat(*index)),
            DeskRow::Task(index) => Some(DeskRow::Task(*index)),
            DeskRow::Agent(_) | DeskRow::Mesh(_) => None,
        }
    }

    fn selected_chat_id(&self) -> Option<String> {
        if self.section != Section::Chats {
            return None;
        }
        match self.row_at(self.cursor) {
            Some(DeskRow::Chat(index)) => self.chats.get(index).map(|chat| chat.id.clone()),
            _ => None,
        }
    }

    fn band_chat(&self) -> Option<crate::model::Chat> {
        match self.row_at(self.cursor)? {
            DeskRow::Chat(index) => self.chats.get(index).cloned(),
            DeskRow::Task(index) => {
                let owner = self.tasks.get(index)?.owner;
                self.chat_for_owner(owner)
            }
            DeskRow::Agent(_) | DeskRow::Mesh(_) => None,
        }
    }

    fn chat_for_owner(&self, owner: AgentKind) -> Option<crate::model::Chat> {
        self.chat_order()
            .into_iter()
            .find(|index| self.chats[*index].agent == owner)
            .map(|index| self.chats[index].clone())
    }

    fn section_rows(&self) -> Vec<DeskRow> {
        match self.section {
            Section::Chats => self.grouped(false),
            Section::Tasks => self.grouped(true),
            _ => Vec::new(),
        }
    }

    fn step_list(&mut self, delta: i32) {
        let rows = self.section_rows();
        let len = rows.len();
        if len == 0 {
            return;
        }
        let current = focus_in(&rows, self.cursor);
        let mut next = clamp_index(current, delta, len);
        let step = if delta < 0 { -1 } else { 1 };
        while next < len && !row_is_item(&rows[next]) {
            let candidate = next as i32 + step;
            if candidate < 0 || candidate >= len as i32 {
                return;
            }
            next = candidate as usize;
        }
        if row_is_item(&rows[next]) {
            self.cursor = next;
        }
    }

    fn row_line(&self, row: &DeskRow, width: usize) -> String {
        match row {
            DeskRow::Agent(agent) => agent_ink(*agent),
            DeskRow::Mesh(label) => format!("\x1b[38;5;131m  {label}"),
            DeskRow::Chat(index) => chat_line(
                &self.chats[*index],
                width,
                self.needs_you(&self.chats[*index]),
            ),
            DeskRow::Task(index) => self.task_line(&self.tasks[*index], width),
        }
    }

    fn grouped(&self, with_tasks: bool) -> Vec<DeskRow> {
        let root = self.repo_root();
        let repos = self.repos.clone();
        let links = self.links.clone();
        let order = self.chat_order();
        let mut rows = Vec::new();
        let mut seen = [false; 3];
        let mut current: Option<AgentKind> = None;
        let mut last_mesh: Option<String> = None;
        for index in &order {
            let chat = &self.chats[*index];
            if current != Some(chat.agent) {
                self.emit_task_agents(&mut rows, &mut seen, agent_rank(chat.agent), with_tasks);
                rows.push(DeskRow::Agent(chat.agent));
                seen[agent_rank(chat.agent) as usize] = true;
                if with_tasks {
                    self.push_tasks(&mut rows, chat.agent);
                }
                last_mesh = None;
                current = Some(chat.agent);
            }
            let label = mesh_group(&chat.cwd, &root, &repos, &links).1;
            if last_mesh.as_deref() != Some(label.as_str()) {
                rows.push(DeskRow::Mesh(label.clone()));
                last_mesh = Some(label);
            }
            rows.push(DeskRow::Chat(*index));
        }
        if with_tasks {
            self.emit_task_agents(&mut rows, &mut seen, 3, true);
        }
        rows
    }

    fn emit_task_agents(
        &self,
        rows: &mut Vec<DeskRow>,
        seen: &mut [bool; 3],
        before: u8,
        with_tasks: bool,
    ) {
        if !with_tasks {
            return;
        }
        for agent in [AgentKind::Grok, AgentKind::Claude, AgentKind::Codex] {
            let rank = agent_rank(agent);
            if rank >= before || seen[rank as usize] {
                continue;
            }
            if self.tasks.iter().any(|task| task.owner == agent) {
                rows.push(DeskRow::Agent(agent));
                self.push_tasks(rows, agent);
                seen[rank as usize] = true;
            }
        }
    }

    fn push_tasks(&self, rows: &mut Vec<DeskRow>, agent: AgentKind) {
        for (index, task) in self.tasks.iter().enumerate() {
            if task.owner == agent {
                rows.push(DeskRow::Task(index));
            }
        }
    }

    fn task_line(&self, task: &Task, width: usize) -> String {
        format!(
            "{}  {}  {}  {}",
            fit("—", 11),
            fit(&task.title, row_title_width(width)),
            fit(&self.place_label(), 14),
            fit("—", 6),
        )
    }

    fn chat_order(&self) -> Vec<usize> {
        let root = self.repo_root();
        let repos = self.repos.clone();
        let links = self.links.clone();
        let mut meta: Vec<(usize, u8, u8, String, i64)> = (0..self.chats.len())
            .map(|index| {
                let chat = &self.chats[index];
                let (rank, key) = mesh_group(&chat.cwd, &root, &repos, &links);
                (index, agent_rank(chat.agent), rank, key, date_key(chat.at))
            })
            .collect();
        let mut group_best: Vec<(u8, u8, String, i64)> = Vec::new();
        for (_, agent, rank, key, date) in &meta {
            if let Some(slot) = group_best
                .iter_mut()
                .find(|item| item.0 == *agent && item.1 == *rank && item.2 == *key)
            {
                slot.3 = slot.3.max(*date);
            } else {
                group_best.push((*agent, *rank, key.clone(), *date));
            }
        }
        let best = |agent: u8, rank: u8, key: &str| {
            group_best
                .iter()
                .find(|item| item.0 == agent && item.1 == rank && item.2 == key)
                .map(|item| item.3)
                .unwrap_or(i64::MIN)
        };
        meta.sort_by(|left, right| {
            left.1.cmp(&right.1).then(left.2.cmp(&right.2)).then(
                best(right.1, right.2, &right.3)
                    .cmp(&best(left.1, left.2, &left.3))
                    .then(left.3.cmp(&right.3))
                    .then(right.4.cmp(&left.4)),
            )
        });
        meta.into_iter().map(|item| item.0).collect()
    }

    fn persist(&self) {
        let state = store::DeskState {
            mesh_accept: self.mesh_accept.clone(),
            chat_accept: self.chat_accept.clone(),
            paused: self.paused,
            permit: self.permit,
            roads: self.roads,
            externals: self.externals,
            mesh_health: self.mesh_health.clone(),
            actions: self.actions.clone(),
            tasks: self.tasks.clone(),
            history: self.history.stored().to_vec(),
        };
        let _ = store::save(&self.home, &state);
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeskRow {
    Agent(AgentKind),
    Mesh(String),
    Chat(usize),
    Task(usize),
}

fn row_is_item(row: &DeskRow) -> bool {
    matches!(row, DeskRow::Chat(_) | DeskRow::Task(_))
}

fn focus_in(rows: &[DeskRow], cursor: usize) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let cursor = cursor.min(rows.len() - 1);
    if row_is_item(&rows[cursor]) {
        return cursor;
    }
    if let Some(offset) = rows.iter().skip(cursor).position(row_is_item) {
        return cursor + offset;
    }
    rows.iter()
        .take(cursor)
        .rposition(row_is_item)
        .unwrap_or(cursor)
}

fn row_title_width(width: usize) -> usize {
    let fixed = 11 + 2 + 14 + 2 + 6;
    width.saturating_sub(fixed).max(8)
}

fn chat_line(chat: &crate::model::Chat, width: usize, needs: bool) -> String {
    let mut title = visible_title(chat);
    if needs {
        title = format!("! {title}");
    }
    if chat.live {
        title = format!("* {title}");
    }
    format!(
        "{}  {}  {}  {}",
        fit(&stamp(chat.at), 11),
        fit(&title, row_title_width(width)),
        fit(&repo_label(&chat.cwd), 14),
        fit(&token_label(chat.tokens), 6),
    )
}

fn agent_rank(agent: AgentKind) -> u8 {
    match agent {
        AgentKind::Grok => 0,
        AgentKind::Claude => 1,
        AgentKind::Codex => 2,
    }
}

fn agent_ink(agent: AgentKind) -> String {
    let color = match agent {
        AgentKind::Grok => "196",
        AgentKind::Claude => "39",
        AgentKind::Codex => "51",
    };
    format!("\x1b[38;5;{color}m{}", agent.label())
}

fn date_key(at: i64) -> i64 {
    if at > 0 { at } else { i64::MIN }
}

fn stamp(at: i64) -> String {
    if at <= 0 {
        return "—".into();
    }
    let t = at as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let ptr = unsafe { libc::localtime_r(&t, tm.as_mut_ptr()) };
    if ptr.is_null() {
        return "—".into();
    }
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:02}-{:02} {:02}:{:02}",
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn token_label(tokens: Option<u64>) -> String {
    let Some(n) = tokens else {
        return "—".into();
    };
    if n >= 1_000_000 {
        return compact(n, 1_000_000, 'm');
    }
    if n >= 1000 {
        return compact(n, 1000, 'k');
    }
    n.to_string()
}

fn compact(n: u64, unit: u64, suffix: char) -> String {
    let tenths = n / (unit / 10);
    let whole = tenths / 10;
    let frac = tenths % 10;
    if frac == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{frac}{suffix}")
    }
}

fn visible_title(chat: &crate::model::Chat) -> String {
    let title = chat.title.trim();
    if title.is_empty() || title == chat.id || is_uuid(title) {
        let repo = repo_label(&chat.cwd);
        if repo != "—" {
            return repo;
        }
    }
    chat.title.clone()
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 32
        && bytes.iter().filter(|byte| **byte == b'-').count() >= 4
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
}

fn is_header(line: &str) -> bool {
    line.starts_with("vim ")
        || line.starts_with("where ")
        || line.starts_with("when ")
        || line.starts_with("what ")
        || line.starts_with("changed ")
}

fn vim_body(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines.iter().skip_while(|line| is_header(line)) {
        let raw = line.strip_prefix("* ").unwrap_or(line.as_str());
        out.push_str(raw);
        out.push('\n');
    }
    out
}

fn clamp_index(cursor: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let last = len as i32 - 1;
    (cursor as i32 + delta).clamp(0, last) as usize
}

fn mesh_group(
    cwd: &str,
    root: &Path,
    repos: &[(String, PathBuf)],
    links: &[String],
) -> (u8, String) {
    let window = repos
        .iter()
        .find(|(_, path)| path == root)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| repo_label(&root.display().to_string()));
    let window_line = links
        .iter()
        .find(|line| line.split(" ~ ").any(|name| name == window));
    let in_window = in_repo(cwd, root)
        || window_line.is_some_and(|line| line.split(" ~ ").any(|name| named_on(cwd, name, repos)));
    if in_window {
        return (0, window_line.cloned().unwrap_or(window));
    }
    if let Some(line) = links
        .iter()
        .find(|line| line.split(" ~ ").any(|name| named_on(cwd, name, repos)))
    {
        return (1, line.clone());
    }
    if let Some((name, _)) = repos.iter().find(|(_, path)| in_repo(cwd, path)) {
        return (2, name.clone());
    }
    (2, repo_label(cwd))
}

fn named_on(cwd: &str, name: &str, repos: &[(String, PathBuf)]) -> bool {
    repo_label(cwd) == name
        || repos
            .iter()
            .any(|(repo, path)| repo == name && in_repo(cwd, path))
}

fn in_repo(cwd: &str, root: &Path) -> bool {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return false;
    }
    let path = Path::new(cwd);
    if path == root || path.starts_with(root) {
        return true;
    }
    let enc = root.to_string_lossy().replace('/', "-");
    cwd == enc || cwd.starts_with(&format!("{enc}-"))
}

fn repo_label(cwd: &str) -> String {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return "—".into();
    }
    if cwd.contains('/')
        && let Some(name) = Path::new(cwd).file_name().and_then(|name| name.to_str())
        && !name.is_empty()
    {
        return name.to_string();
    }
    if let Some((head, tail)) = cwd.rsplit_once("-dev-")
        && !head.is_empty()
        && !tail.is_empty()
    {
        return tail.to_string();
    }
    clip_width(cwd.trim_start_matches('-'), 22)
}

fn fit(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    if text.chars().count() > width {
        out.pop();
        out.push('~');
    }
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn clip_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(width).collect();
    if text.chars().count() > width {
        out.pop();
        out.push('~');
    }
    out
}

fn paint_view(lines: &[String]) -> Vec<String> {
    let ext = view_ext(lines);
    lines.iter().map(|line| paint_line(line, ext)).collect()
}

fn view_ext(lines: &[String]) -> &str {
    for line in lines {
        if let Some(path) = line.strip_prefix("where  ") {
            return match path.rsplit_once('.') {
                Some((_, ext)) if !ext.is_empty() && !ext.contains('/') => ext,
                _ => "",
            };
        }
    }
    ""
}

fn paint_line(line: &str, ext: &str) -> String {
    if is_header(line) {
        line.to_string()
    } else {
        hl::paint_ext(line, ext)
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

fn push_skills(found: &mut Vec<String>, skills: &[Skill], partial: &str) {
    for skill in skills {
        if skill.name.starts_with(partial) && skill.name != partial {
            let line = format!("skill {}", skill.name);
            if !found.contains(&line) {
                found.push(line);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chats_sit_on_black_and_the_selected_row_is_gold() {
        let dir = std::env::temp_dir().join("granttap-ui-empty");
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(dir, PathBuf::from("/nowhere-home"));
        app.chats = vec![
            crate::model::Chat {
                agent: AgentKind::Codex,
                id: "one".into(),
                title: "Hold the line".into(),
                path: PathBuf::new(),
                cwd: String::new(),
                live: false,
                at: 0,
                tokens: None,
            },
            crate::model::Chat {
                agent: AgentKind::Codex,
                id: "two".into(),
                title: "Ship the desk".into(),
                path: PathBuf::new(),
                cwd: String::new(),
                live: false,
                at: 0,
                tokens: None,
            },
        ];
        app.cursor = app
            .lines()
            .iter()
            .position(|line| line.contains("Ship the desk"))
            .unwrap();
        let screen = crate::ui::Screen::paint(&app, 80, 24);
        assert!(screen.frame.contains("48;5;0"));
        assert!(screen.frame.contains("38;5;220"));
        assert!(screen.frame.contains("38;5;51"));
        assert!(screen.frame.contains("Hold the line"));
        assert!(screen.frame.contains("Ship the desk"));
        assert!(screen.frame.contains("\u{1b}[1;24r"));
        assert!(!screen.frame.contains('\u{2500}'));
        assert!(!screen.frame.contains("Vim"));
        let gold = screen.frame.split("38;5;220").nth(1).unwrap_or("");
        assert!(gold.contains("Ship the desk"), "{gold}");
        assert!(
            app.lines()
                .iter()
                .any(|line| line.contains("\u{1b}[38;5;51mCodex"))
        );
    }

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
    fn composer_shows_model_approval_and_a_suggestion_zone() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-face-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-face-dir-{n}"));
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            home.join(".grok/config.toml"),
            "[ui]\npermission_mode = \"always-approve\"\n\n[models]\ndefault = \"grok-4.7\"\ndefault_reasoning_effort = \"xhigh\"\n",
        )
        .unwrap();
        let mut app = App::new(dir, home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Grok,
            id: "s".into(),
            title: "Ship the line".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 1,
            tokens: None,
        });
        let key = app.mesh_key();
        app.mesh_accept.push((key, AcceptLevel::Full));
        let (agent, status) = app.composer_status();
        assert_eq!(agent, AgentKind::Grok);
        assert!(status.contains("grok-4.7"), "{status}");
        assert!(status.contains("xhigh"), "{status}");
        assert!(status.contains("always-approve"), "{status}");
        assert!(status.contains("full"), "{status}");
        let idle = crate::ui::Screen::paint(&app, 100, 24);
        assert!(idle.frame.contains("grok-4.7"), "{}", idle.frame);
        assert!(idle.frame.contains("always-approve"));
        assert!(idle.frame.contains("full"));
        assert!(idle.cursor.is_some());
        assert!(!idle.frame.contains(" context"), "{}", idle.frame);
        app.cmd = "mo".into();
        let typing = crate::ui::Screen::paint(&app, 100, 24);
        assert!(typing.frame.contains(" move"), "{}", typing.frame);
        assert!(typing.frame.contains(" model"), "{}", typing.frame);
        assert!(typing.cursor.is_some());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn task_has_one_owner_and_move_changes_the_chat() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
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
    fn task_opens_in_the_owner_split_and_survives_reload() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-queue-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-queue-dir-{n}"));
        std::fs::create_dir_all(home.join(".agents/skills/ship")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(home.join(".agents/skills/ship/SKILL.md"), "Keep the desk\n").unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
        });
        app.cursor = 0;
        app.cmd = "task write the desk".into();
        app.on_key(Key::Enter);
        assert_eq!(app.tasks[0].owner, AgentKind::Claude);
        let lines = app.lines();
        let header = lines
            .iter()
            .position(|line| line.ends_with("Claude"))
            .unwrap();
        app.cursor = lines
            .iter()
            .position(|line| line.contains("write the desk"))
            .unwrap();
        assert!(header < app.cursor, "{lines:?}");
        let row = lines[app.cursor].clone();
        assert!(!row.contains("Claude"), "{row}");
        assert!(row.contains("granttap"), "{row}");
        assert!(row.contains('—'), "{row}");
        app.cmd.clear();
        app.on_key(Key::Enter);
        let Mode::Split { lines, agent } = &app.mode else {
            panic!("task did not open");
        };
        assert_eq!(*agent, Some(AgentKind::Claude));
        let joined = lines.join("\n");
        assert!(joined.contains("write the desk"));
        assert!(joined.contains("owner  Claude"));
        app.cmd = "skill ship".into();
        app.on_key(Key::Enter);
        let joined = app.lines().join("\n");
        assert!(joined.contains("Keep the desk"));
        assert!(joined.contains("shared  ship"));
        let mut again = App::new(dir, home.clone());
        assert_eq!(again.tasks.len(), 1);
        assert_eq!(again.tasks[0].title, "write the desk");
        again.on_key(Key::CtrlP);
        assert_eq!(again.cmd, "skill ship");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn edit_opens_the_file_in_its_split() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-edit-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.txt"), "keep the line\n").unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.cmd = "files".into();
        app.on_key(Key::Enter);
        app.cursor = app
            .lines()
            .iter()
            .position(|line| line.contains("note.txt"))
            .unwrap();
        let effect = app.on_key(Key::F(4));
        assert!(matches!(effect, Effect::None));
        let joined = app.lines().join("\n");
        assert!(joined.contains("vim  note.txt"), "{joined}");
        assert!(joined.contains("keep the line"), "{joined}");
        assert!(!joined.contains("Vim off"));
        assert!(app.description_line().contains("vim on"));
        app.on_key(Key::Char('!'));
        app.on_key(Key::Up);
        app.on_key(Key::Wheel(-1));
        assert!(app.cmd.is_empty());
        app.on_key(Key::F(4));
        assert_eq!(app.note, "ask  repeat to run");
        app.on_key(Key::F(4));
        assert_eq!(app.note, "wrote note.txt");
        let saved = std::fs::read_to_string(dir.join("note.txt")).unwrap();
        assert!(saved.contains("keep the line!"), "{saved}");
        app.on_key(Key::Esc);
        assert!(app.mode_is_desk());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tasks_lead_with_each_agent_and_the_history_follows() {
        let mut app = App::new(PathBuf::from("/tmp/night"), PathBuf::from("/nowhere-home"));
        app.chats = vec![
            crate::model::Chat {
                agent: AgentKind::Codex,
                id: "c1".into(),
                title: "codex wall".into(),
                path: PathBuf::new(),
                cwd: "/tmp/other".into(),
                live: false,
                at: 0,
                tokens: None,
            },
            crate::model::Chat {
                agent: AgentKind::Codex,
                id: "c2".into(),
                title: "codex two".into(),
                path: PathBuf::new(),
                cwd: "/tmp/other".into(),
                live: false,
                at: 0,
                tokens: None,
            },
            crate::model::Chat {
                agent: AgentKind::Claude,
                id: "01234567-89ab-cdef-0123-456789abcdef".into(),
                title: "01234567-89ab-cdef-0123-456789abcdef".into(),
                path: PathBuf::new(),
                cwd: "-Users-serhiirihgt-dev-granttap-mcp".into(),
                live: false,
                at: 0,
                tokens: None,
            },
            crate::model::Chat {
                agent: AgentKind::Grok,
                id: "g1".into(),
                title: "desk towers".into(),
                path: PathBuf::new(),
                cwd: "/tmp/night".into(),
                live: true,
                at: 0,
                tokens: None,
            },
        ];
        app.section = Section::Chats;
        let lines = app.lines();
        let grok = lines
            .iter()
            .position(|line| line.ends_with("Grok"))
            .unwrap();
        let claude = lines
            .iter()
            .position(|line| line.ends_with("Claude"))
            .unwrap();
        let codex = lines
            .iter()
            .position(|line| line.ends_with("Codex"))
            .unwrap();
        assert!(grok < claude && claude < codex, "{lines:?}");
        let towers = lines
            .iter()
            .find(|line| line.contains("desk towers"))
            .unwrap();
        assert!(!towers.ends_with("Grok"), "{towers}");
        app.cursor = lines
            .iter()
            .position(|line| line.contains("granttap-mcp") && line.contains('—'))
            .unwrap();
        let band = app.history_band();
        assert!(band[0].contains("Claude"), "{band:?}");
        assert!(band[0].contains("granttap-mcp"), "{band:?}");
        app.section = Section::Tasks;
        app.cursor = 0;
        let tasks = app.lines();
        assert!(tasks[0].ends_with("Grok"), "{tasks:?}");
        assert!(
            tasks.iter().any(|line| line.ends_with("Claude")),
            "{tasks:?}"
        );
    }

    fn sample_chat(
        agent: AgentKind,
        title: &str,
        cwd: &str,
        at: i64,
        tokens: Option<u64>,
    ) -> crate::model::Chat {
        crate::model::Chat {
            agent,
            id: format!("id-{title}"),
            title: title.into(),
            path: PathBuf::new(),
            cwd: cwd.into(),
            live: false,
            at,
            tokens,
        }
    }

    #[test]
    fn list_groups_agent_then_mesh_then_date() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-window-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mesh = std::env::temp_dir().join(format!("aaa-mesh-{n}"));
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.repos = vec![
            ("here".into(), dir.clone()),
            ("mesh-b".into(), mesh.clone()),
        ];
        app.links = vec!["here ~ mesh-b".into()];
        let here = dir.display().to_string();
        let mesh = mesh.display().to_string();
        app.chats = vec![
            sample_chat(AgentKind::Codex, "codex-here", &here, 9_000, None),
            sample_chat(AgentKind::Grok, "grok-far", "/tmp/far", 9_000, None),
            sample_chat(AgentKind::Grok, "grok-mesh-old", &mesh, 1_000, None),
            sample_chat(AgentKind::Grok, "grok-mesh-new", &mesh, 8_000, None),
            sample_chat(AgentKind::Grok, "grok-here-old", &here, 2_000, None),
            sample_chat(AgentKind::Grok, "grok-here-new", &here, 3_000, None),
            sample_chat(AgentKind::Claude, "claude-z", "/tmp/zeta", 9_000, None),
            sample_chat(AgentKind::Claude, "claude-a", "/tmp/alpha", 1_000, None),
        ];
        let lines = app.lines();
        let at = |title: &str| {
            lines
                .iter()
                .position(|line| line.contains(title))
                .unwrap_or_else(|| panic!("{title} missing in {lines:?}"))
        };
        assert!(lines[at("Grok")].ends_with("Grok"), "{lines:?}");
        assert!(at("Grok") < at("grok-mesh-new"), "{lines:?}");
        assert!(at("here ~ mesh-b") < at("grok-mesh-new"), "{lines:?}");
        assert!(at("grok-mesh-new") < at("grok-here-new"), "{lines:?}");
        assert!(at("grok-here-new") < at("grok-here-old"), "{lines:?}");
        assert!(at("grok-here-old") < at("grok-mesh-old"), "{lines:?}");
        assert!(at("grok-mesh-old") < at("grok-far"), "{lines:?}");
        assert!(at("Claude") < at("claude-z"), "{lines:?}");
        assert!(at("claude-z") < at("claude-a"), "{lines:?}");
        assert!(at("Codex") < at("codex-here"), "{lines:?}");
        let chat = lines
            .iter()
            .find(|line| line.contains("grok-here-new"))
            .unwrap();
        assert!(!chat.ends_with("Grok"), "{chat}");
        app.tasks.push(Task {
            title: "claude job".into(),
            owner: AgentKind::Claude,
            model: String::new(),
            effort: String::new(),
        });
        app.tasks.push(Task {
            title: "grok job".into(),
            owner: AgentKind::Grok,
            model: String::new(),
            effort: String::new(),
        });
        app.section = Section::Tasks;
        let tasks = app.lines();
        let at = |title: &str| {
            tasks
                .iter()
                .position(|line| line.contains(title))
                .unwrap_or_else(|| panic!("{title} missing in {tasks:?}"))
        };
        assert!(tasks[at("grok job")].contains('—'), "{tasks:?}");
        assert!(at("Grok") < at("grok job"), "{tasks:?}");
        assert!(at("grok job") < at("grok-mesh-new"), "{tasks:?}");
        assert!(at("claude job") < at("claude-a"), "{tasks:?}");
    }

    #[test]
    fn row_shows_date_title_tokens_and_agent_color() {
        let mut app = App::new(PathBuf::from("/tmp/night"), PathBuf::from("/nowhere-home"));
        let id = "01234567-89ab-cdef-0123-456789abcdef";
        app.chats = vec![
            crate::model::Chat {
                agent: AgentKind::Codex,
                id: id.into(),
                title: id.into(),
                path: PathBuf::new(),
                cwd: "/tmp/night".into(),
                live: false,
                at: 86_400,
                tokens: Some(12_500),
            },
            sample_chat(AgentKind::Grok, "desk towers", "/tmp/other", 200_000, None),
            sample_chat(
                AgentKind::Claude,
                "Name the desk",
                "-Users-serhiirihgt-dev-granttap-mcp",
                0,
                Some(999),
            ),
        ];
        let lines = app.lines();
        let towers = lines
            .iter()
            .find(|line| line.contains("desk towers"))
            .unwrap();
        assert!(towers.contains('—'), "{towers}");
        assert!(!towers.ends_with("Grok"), "{towers}");
        let named = lines
            .iter()
            .find(|line| line.contains("Name the desk"))
            .unwrap();
        assert!(named.contains("999"), "{named}");
        assert!(!named.ends_with("Claude"), "{named}");
        let codex = lines.iter().find(|line| line.contains("12.5k")).unwrap();
        assert!(codex.contains(&stamp(86_400)), "{codex}");
        assert!(codex.contains("night"), "{codex}");
        assert!(!codex.contains(id), "{codex}");
        assert!(!codex.ends_with("Codex"), "{codex}");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("\u{1b}[38;5;196mGrok"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("\u{1b}[38;5;39mClaude"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("\u{1b}[38;5;51mCodex"))
        );
        app.cursor = lines
            .iter()
            .position(|line| line.contains("Name the desk"))
            .unwrap();
        let screen = crate::ui::Screen::paint(&app, 80, 24);
        assert!(screen.frame.contains("38;5;196"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;51"), "{}", screen.frame);
        let gold = screen.frame.split("38;5;220").nth(1).unwrap_or("");
        assert!(gold.contains("Name the desk"), "{gold}");
        app.cursor = 0;
        let screen = crate::ui::Screen::paint(&app, 80, 24);
        assert!(screen.frame.contains("38;5;39"), "{}", screen.frame);
    }

    #[test]
    fn a_double_click_opens_the_row_and_does_not_run_the_command() {
        let mut app = App::new(PathBuf::from("/tmp/night"), PathBuf::from("/nowhere-home"));
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "desk towers",
            "/tmp/night",
            10,
            None,
        )];
        app.section = Section::Chats;
        app.cursor = 0;
        let effect = app.apply_keys(
            &[
                Key::Paste("files".into()),
                Key::Click { x: 2, y: 4 },
                Key::Ignore,
                Key::Click { x: 2, y: 4 },
                Key::Ignore,
            ],
            |_, _| Some(crate::ui::Spot::Row(0)),
        );
        assert_eq!(effect, Effect::None);
        assert_eq!(app.section, Section::Chats);
        assert_eq!(app.cmd, "files");
        assert!(app.pane().is_some());
        app.on_key(Key::Enter);
        assert_eq!(app.section, Section::Files);
        assert!(app.cmd.is_empty());
    }

    #[test]
    fn the_open_chat_keeps_line_color() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-ink-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":\"first paragraph\\n\\nsecond paragraph\"}\n{\"type\":\"assistant\",\"content\":\"answer one\\nanswer two\"}\n",
        )
        .unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Claude,
            "ink chat",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.on_key(Key::Enter);
        let frame = crate::ui::Screen::paint(&app, 100, 24).frame;
        let user_at = frame.find("second paragraph").expect("second paragraph");
        assert!(
            frame[..user_at].ends_with("38;5;51m"),
            "user continuation lost its color"
        );
        let answer_at = frame.find("answer two").expect("answer two");
        assert!(
            frame[..answer_at].ends_with("38;5;203m"),
            "assistant continuation lost its color"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_code_fence_keeps_its_color() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-fence-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chat_history.jsonl"),
            "{\"type\":\"assistant\",\"content\":\"intro\\n\\n# Title\\n\\n```\\n    let desk = 1;\\n```\\n\"}\n",
        )
        .unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Claude,
            "fence chat",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.on_key(Key::Enter);
        let frame = crate::ui::Screen::paint(&app, 120, 30).frame;
        let title = frame.find("# Title").expect("heading");
        assert!(frame[..title].ends_with("38;5;220m"));
        let code = frame.find("    let desk = 1;").expect("code");
        assert!(frame[..code].ends_with("38;5;114m"));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn enter_opens_the_row_in_a_split_beside_the_list() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-pane-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(
            one.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"first desk\"}]}\n{\"type\":\"assistant\",\"content\":\"opened one\"}\n",
        )
        .unwrap();
        std::fs::write(
            two.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":\"second desk\"}\n{\"type\":\"assistant\",\"content\":\"opened two\"}\n",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.chats = vec![
            sample_chat(AgentKind::Grok, "first title", "/tmp/night", 2, None),
            sample_chat(AgentKind::Claude, "second title", "/tmp/other", 1, None),
        ];
        app.chats[0].path = one.clone();
        app.chats[1].path = two;
        app.section = Section::Chats;
        app.cursor = 0;
        app.on_key(Key::Enter);
        assert!(app.pane().is_some());
        let opened = app.lines().join("\n");
        assert!(opened.contains("first title"), "{opened}");
        assert!(opened.contains("first desk"), "{opened}");
        assert!(opened.contains("opened one"), "{opened}");
        assert!(app.suggestions().is_empty());
        let screen = crate::ui::Screen::paint(&app, 120, 24);
        assert!(screen.frame.contains("second title"), "{}", screen.frame);
        assert!(screen.frame.contains("first desk"));
        assert!(screen.frame.contains('|'));
        app.on_key(Key::Down);
        let opened = app.lines().join("\n");
        assert!(opened.contains("second title"), "{opened}");
        assert!(opened.contains("second desk"), "{opened}");
        app.on_key(Key::Esc);
        assert!(app.pane().is_none());
        assert!(app.lines().iter().any(|line| line.contains("first title")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn needs_you_sits_above_the_list() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-ask-{n}"));
        let cwd = "/tmp/night";
        let id = "ask-session";
        let dir = crate::agents::session_path(&home, cwd, id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            home.join(".grok/active_sessions.json"),
            format!(
                "[{{\"session_id\":\"{id}\",\"pid\":{},\"cwd\":\"{cwd}\"}}]",
                std::process::id()
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("events.jsonl"),
            "{\"type\":\"permission_requested\",\"tool_name\":\"run_terminal_command\"}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("signals.json"),
            r#"{"contextTokensUsed":88,"contextWindowTokens":100}"#,
        )
        .unwrap();
        let mut app = App::new(PathBuf::from("/tmp/night"), home.clone());
        app.chats = vec![sample_chat(AgentKind::Grok, "release audit", cwd, 10, None)];
        app.chats[0].id = id.into();
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.refresh_asks();
        let screen = crate::ui::Screen::paint(&app, 100, 24);
        assert!(screen.frame.contains("needs you"));
        assert!(screen.frame.contains("permission run_terminal_command"));
        assert!(screen.frame.contains("! release audit") || app.lines()[0].contains("! release"));
        app.on_key(Key::Enter);
        let opened = app.lines().join("\n");
        assert!(opened.contains("context 88%"), "{opened}");
        assert!(
            opened.contains("needs you  permission run_terminal_command"),
            "{opened}"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn chat_scroll_stops_at_the_ends() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.chats = vec![
            crate::model::Chat {
                agent: AgentKind::Claude,
                id: "one".into(),
                title: "first".into(),
                path: PathBuf::new(),
                cwd: String::new(),
                live: false,
                at: 0,
                tokens: None,
            },
            crate::model::Chat {
                agent: AgentKind::Grok,
                id: "two".into(),
                title: "last".into(),
                path: PathBuf::new(),
                cwd: String::new(),
                live: false,
                at: 0,
                tokens: None,
            },
        ];
        app.section = Section::Chats;
        app.cursor = 0;
        app.on_key(Key::Up);
        app.on_key(Key::Wheel(-1));
        assert_eq!(app.cursor, 0);
        let last = app.lines().len() - 1;
        app.cursor = last;
        app.on_key(Key::Down);
        app.on_key(Key::Wheel(1));
        assert_eq!(app.cursor, last);
    }

    #[test]
    fn the_open_chat_scrolls_by_paragraph() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-scroll-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = String::new();
        for index in 0..40 {
            body.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"paragraph {index:02}\"}}\n"
            ));
        }
        std::fs::write(dir.join("chat_history.jsonl"), body).unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Claude,
            "long chat",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir.clone();
        app.section = Section::Chats;
        app.on_key(Key::Enter);
        let tail = crate::ui::Screen::paint(&app, 100, 24);
        assert!(tail.frame.contains("paragraph 39"), "{}", tail.frame);
        assert!(!tail.frame.contains("paragraph 00"), "{}", tail.frame);
        let cursor = app.cursor;
        for _ in 0..30 {
            app.on_key(Key::Wheel(-1));
        }
        let older = crate::ui::Screen::paint(&app, 100, 24);
        assert!(older.frame.contains("paragraph 00"), "{}", older.frame);
        assert_eq!(app.cursor, cursor);
        let _ = std::fs::remove_dir_all(home);
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
        std::fs::write(
            dir.join("hi.html"),
            "<title>Башни</title><p>Hello</p><script>no</script>",
        )
        .unwrap();
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

    #[test]
    fn permit_writes_the_temp_home_and_commit_does_not_stage() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-desk-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-desk-repo-{n}"));
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            home.join(".grok/config.toml"),
            "[ui]\npermission_mode = \"ask\"\nyolo = false\n\n[models]\ndefault = \"grok-4\"\ndefault_reasoning_effort = \"medium\"\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".claude/settings.json"),
            "{\"hooks\":{\"keep\":true},\"effortLevel\":\"medium\"}\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            "model = \"old\"\nmodel_reasoning_effort = \"medium\"\napproval_policy = \"on-request\"\n\n[projects.demo]\nmodel = \"leave\"\n",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Grok,
            id: "s".into(),
            title: "s".into(),
            path: PathBuf::new(),
            cwd: dir.display().to_string(),
            live: true,
            at: 0,
            tokens: None,
        });
        app.cursor = 0;
        app.cmd = "accept full".into();
        app.on_key(Key::Enter);
        app.cmd = "permit bypass".into();
        app.on_key(Key::Enter);
        let grok = std::fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(grok.contains("permission_mode = \"always-approve\""));
        assert!(grok.contains("yolo = false"));
        let local = std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        assert!(local.contains("bypassPermissions"));
        let claude = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(claude.contains("\"hooks\":{\"keep\":true}"));
        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("approval_policy = \"never\""));
        assert!(codex.contains("model = \"leave\""));
        app.cmd = "model grok-4.7".into();
        app.on_key(Key::Enter);
        app.cmd = "effort high".into();
        app.on_key(Key::Enter);
        let grok = std::fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(grok.contains("default = \"grok-4.7\""));
        assert!(grok.contains("default_reasoning_effort = \"high\""));
        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("model = \"old\""));
        assert!(codex.contains("model_reasoning_effort = \"medium\""));
        let status = Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(dir.join("note.txt"), "unstaged\n").unwrap();
        app.cmd = "commit ship the desk".into();
        app.on_key(Key::Enter);
        assert_eq!(app.note, "commit refused");
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(!head.success());
        let porcelain = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let porcelain = String::from_utf8_lossy(&porcelain.stdout);
        assert!(porcelain.contains("note.txt"));
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ask_holds_a_write_until_the_same_command_repeats() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-ask-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.cmd = "commit ship".into();
        app.on_key(Key::Enter);
        assert_eq!(app.cmd, "commit ship");
        assert_eq!(app.note, "ask  repeat to run");
        app.on_key(Key::Enter);
        assert!(app.cmd.is_empty());
        assert_eq!(app.note, "Repo  none");
        app.cmd = "accept safe".into();
        app.on_key(Key::Enter);
        app.cmd = "commit ship".into();
        app.on_key(Key::Enter);
        assert_eq!(app.cmd, "commit ship");
        assert_eq!(app.note, "ask  repeat to run");
        app.cmd = "accept except-push".into();
        app.on_key(Key::Enter);
        app.cmd = "commit ship".into();
        app.on_key(Key::Enter);
        assert!(app.cmd.is_empty());
        assert_eq!(app.note, "Repo  none");
        app.paused = true;
        app.cmd = "commit ship".into();
        app.on_key(Key::Enter);
        assert_eq!(app.cmd, "commit ship");
        assert_eq!(app.note, "paused  repeat to run");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn except_push_asks_before_push_and_except_destructive_runs_it() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-push-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .env("GIT_AUTHOR_NAME", "dev")
                .env("GIT_AUTHOR_EMAIL", "dev@example.com")
                .env("GIT_COMMITTER_NAME", "dev")
                .env("GIT_COMMITTER_EMAIL", "dev@example.com")
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init"]);
        std::fs::write(dir.join("readme.txt"), "desk\n").unwrap();
        git(&["add", "readme.txt"]);
        git(&["commit", "-m", "start"]);
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.cmd = "accept except-push".into();
        app.on_key(Key::Enter);
        app.cmd = "push".into();
        app.on_key(Key::Enter);
        assert_eq!(app.cmd, "push");
        assert_eq!(app.note, "ask  repeat to run");
        assert!(matches!(app.mode, Mode::Desk));
        app.cmd = "accept except-destructive".into();
        app.on_key(Key::Enter);
        app.cmd = "push".into();
        app.on_key(Key::Enter);
        assert!(app.cmd.is_empty());
        assert_eq!(app.note, "push refused");
        assert!(matches!(app.mode, Mode::Split { .. }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn editor_marks_lines_that_differ_from_head() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-edit-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .env("GIT_AUTHOR_NAME", "dev")
                .env("GIT_AUTHOR_EMAIL", "dev@example.com")
                .env("GIT_COMMITTER_NAME", "dev")
                .env("GIT_COMMITTER_EMAIL", "dev@example.com")
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init"]);
        std::fs::write(dir.join("a.rs"), "fn old() {}\nlet x = 1;\n").unwrap();
        git(&["add", "a.rs"]);
        git(&["commit", "-m", "start the file"]);
        std::fs::write(dir.join("a.rs"), "fn new() {}\nlet x = 1;\n").unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.section = Section::Files;
        app.cursor = 1;
        app.on_key(Key::Enter);
        let Mode::View { lines } = &app.mode else {
            panic!("file did not open");
        };
        let joined = lines.join("\n");
        assert!(joined.contains("where  a.rs"));
        assert!(joined.contains("what  start the file"));
        assert!(joined.contains("changed  1 lines"));
        assert!(joined.contains("* fn new() {}"));
        assert!(joined.contains("let x = 1;"));
        assert!(!joined.contains("* let x"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
