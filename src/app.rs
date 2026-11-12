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

pub struct ComposerFace {
    pub agent: AgentKind,
    pub model: String,
    pub effort: String,
    pub permit: String,
    pub lane: Option<&'static str>,
    pub ultra: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Say,
    Queue,
}

impl Lane {
    fn label(self) -> &'static str {
        match self {
            Self::Say => "say",
            Self::Queue => "queue",
        }
    }

    fn flip(self) -> Self {
        match self {
            Self::Say => Self::Queue,
            Self::Queue => Self::Say,
        }
    }
}

pub struct App {
    pub section: Section,
    pub cmd: String,
    pub cursor: usize,
    pub scroll: usize,
    pub pane_from_end: usize,
    /// Rows above the now list. `usize::MAX` keeps the selected row in view.
    pub list_from: usize,
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
    thumbs: Vec<(PathBuf, String)>,
    thumb_cols: usize,
    opens: Vec<OpenFile>,
    shot_seen: usize,
    preview: String,
    page: Vec<String>,
    mode: Mode,
    mesh_view: Vec<String>,
    mesh_board: health::Board,
    mesh_at: String,
    mesh_root: String,
    cols: usize,
    rows: usize,
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
    open_id: String,
    open_stamp: Option<SystemTime>,
    live_at: Option<u64>,
    live_reply: String,
    live_said: String,
    live_said_at: i64,
    busy_since: Option<i64>,
    last_pulse: i64,
    turn_tokens: Option<u64>,
    tokens_base: Option<u64>,
    marks: Vec<chats::TurnMark>,
    /// Byte offset of the oldest transcript line currently in the pane.
    /// Zero means the start of the file is already loaded.
    hist_from: u64,
    hist_path: PathBuf,
    hist_at: usize,
    held: Option<HeldChat>,
    shell: crate::shell::Shells,
    lane: Lane,
    paste_left: String,
    pub shown_image: Option<u16>,
    lesson_open: Option<usize>,
    outline: String,
    bound_cwd: String,
    page_base: PathBuf,
    page_url: String,
    page_hist: Vec<(String, Vec<String>)>,
    page_fwd: Vec<(String, Vec<String>)>,
    bookmarks: Vec<String>,
    visits: Vec<store::Visit>,
    engine: crate::engine::Engine,
    web_hits: Vec<crate::engine::WebHit>,
    web_on: bool,
    web_focus: Option<u16>,
    /// Characters typed into the focused field. The page paint can keep the
    /// placeholder, so the desk draws this over that row.
    web_draft: String,
    web_panel: bool,
    web_js: bool,
    web_live: bool,
    web_bytes: u64,
    web_traffic: u64,
    web_busy: bool,
    browse_hot: Option<u8>,
    web_hot: Option<u16>,
    web_job: Option<WebAct>,
    browser_pub: String,
    keys_open: bool,
    keys_query: String,
    keys_at: usize,
    keys_find: bool,
    keys_bits: u32,
    settings_open: bool,
    settings_query: String,
    settings_at: usize,
    settings_find: bool,
    settings_bits: u32,
    web_theme: String,
    web_lang: String,
    look: String,
    font_face: String,
    font_size: u16,
    image_quality: String,
    image_scroll: usize,
    queue_for: String,
    queue: Vec<String>,
    picked_effort: Option<(AgentKind, String)>,
    hover: Option<(usize, usize)>,
    meter_open: bool,
    meter_tab: u8,
    meter_scroll: usize,
    meter_loaded: String,
    meter_model: String,
    meter_turns: Option<u64>,
    meter_compactions: Option<u64>,
    meter_before: Option<u64>,
    meter_tools: Option<u64>,
    meter_split: chats::ContextSplit,
    open_folds: Vec<usize>,
    tree_shut: Vec<String>,
    doc_open: bool,
    doc_title: String,
    doc_lines: Vec<String>,
    doc_scroll: usize,
    pick: Option<crate::ui::Chip>,
    pick_at: usize,
    grip: Option<Grip>,
    sel: Option<(usize, usize, usize, usize)>,
    /// First Ctrl+C sets this. The next Ctrl+C quits. Another key clears it.
    ctrl_c_armed: bool,
    /// The armed Ctrl+C copied a drag, so the note says copied.
    ctrl_c_copied: bool,
    /// Pumps left that put the drag back on the clipboard.
    copy_push: u8,
    kept: Vec<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    copied: String,
}

struct Grip {
    x: usize,
    y: usize,
    spot: Option<crate::ui::Spot>,
    times: u8,
    moved: bool,
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

enum MeshHit {
    None,
    Up,
    Skill(String, String),
    Dir(String),
    File(String),
}

struct MeshRow {
    line: String,
    hit: MeshHit,
}

enum WebAct {
    Reload,
    Back,
    Forward,
    Open(String),
    Click(u16),
    Type(String),
    Key(String),
}

pub(crate) struct KeyRow {
    pub label: String,
    pub shortcut: String,
    pub group: bool,
    pub open: bool,
    pub detail: String,
    pub id: String,
    pub group_at: usize,
}

struct KeyItem {
    label: &'static str,
    shortcut: &'static str,
    detail: &'static str,
}

struct KeyGroup {
    name: &'static str,
    items: &'static [KeyItem],
}

const KEYS: &[KeyGroup] = &[
    KeyGroup {
        name: "Essentials",
        items: &[
            KeyItem { label: "Send or open", shortcut: "Enter", detail: "Enter sends the line, opens the row, or submits a field." },
            KeyItem { label: "Quit", shortcut: "F10 / Ctrl+c", detail: "F10 leaves the desk. Ctrl+c leaves on the second press." },
            KeyItem { label: "Keyboard shortcuts", shortcut: "? / [help]", detail: "Type ? on an empty line, or click [help], to open this list. Esc closes it." },
            KeyItem { label: "Settings", shortcut: ", / [settings]", detail: "Comma or [settings] opens Settings. Esc closes it. Look, font, and picture quality sit with the browser theme." },
            KeyItem { label: "Fill the pane", shortcut: "F3 / [full]", detail: "F3 or [full] expands the open chat. [fit], F3, or Esc returns." },
            KeyItem { label: "Refresh", shortcut: "F5", detail: "F5 rescans the mesh and reloads the open page." },
            KeyItem { label: "Say or queue", shortcut: "F6", detail: "F6 switches say and queue while a chat is open." },
            KeyItem { label: "Step back", shortcut: "Esc", detail: "Esc leaves a field, a folder, vim, a picture frame, a file frame, or this list. A picture frame closes before the chat." },
            KeyItem { label: "Older command", shortcut: "Ctrl+p", detail: "Ctrl+p walks back through typed lines. Ctrl+n walks forward." },
        ],
    },
    KeyGroup {
        name: "Browser",
        items: &[
            KeyItem { label: "Back", shortcut: "[<]", detail: "The back chip returns to the previous page." },
            KeyItem { label: "Forward", shortcut: "[>]", detail: "The forward chip walks into the page you left." },
            KeyItem { label: "Reload", shortcut: "[reload]", detail: "Reload fetches the open page again. The chip lights and the bar spins." },
            KeyItem { label: "Bookmark", shortcut: "[mark]", detail: "Mark saves the http page. Saved pages are listed under history." },
            KeyItem { label: "History", shortcut: "[hist]", detail: "History lists marks, then visits. Enter opens one. Esc returns." },
            KeyItem { label: "JavaScript", shortcut: "js on/off", detail: "The js chip turns scripts on or off and reloads the open page." },
            KeyItem { label: "Live network", shortcut: "live on/off", detail: "Live off freezes scripts and the network after the page has loaded." },
            KeyItem { label: "Theme", shortcut: ", / chip", detail: "Comma, [settings], or the theme chip opens Settings. Pick auto, dark, or light for the page." },
            KeyItem { label: "Language", shortcut: ", / chip", detail: "Themes sets auto, en, ru, uk, he, ar, de, fr, or es." },
            KeyItem { label: "Scroll the page", shortcut: "wheel / arrows", detail: "The wheel and the arrows scroll the page under the pointer." },
            KeyItem { label: "Field", shortcut: "click, type", detail: "Click a field and type. Enter submits. Esc leaves the field." },
        ],
    },
    KeyGroup {
        name: "Mesh",
        items: &[
            KeyItem { label: "Open", shortcut: "Enter", detail: "Enter opens a folder, then the file in vim." },
            KeyItem { label: "Step out", shortcut: "Esc / back", detail: "Esc or back leaves one folder." },
            KeyItem { label: "Unhealthy file", shortcut: "red !", detail: "A red ! is a missing mod, crate path, or relative import. The row names the file." },
            KeyItem { label: "Rescan", shortcut: "F5 / refresh", detail: "F5 or refresh scans the repo again." },
        ],
    },
    KeyGroup {
        name: "Input",
        items: &[
            KeyItem { label: "Paste", shortcut: "Ctrl+v / Alt+v", detail: "Ctrl+v or Alt+v puts text, a copied file, or a clipboard picture into the line. Alt+v still works when the terminal keeps Ctrl+v." },
            KeyItem { label: "Select", shortcut: "drag", detail: "Drag marks the cells under the pointer. Releasing copies those cells. A click without a drag still presses the spot." },
            KeyItem { label: "Complete", shortcut: "Tab", detail: "Tab inserts the gold suggestion and a space." },
            KeyItem { label: "History", shortcut: "Ctrl+p / Ctrl+n", detail: "Ctrl+p and Ctrl+n walk the lines you typed." },
        ],
    },
    KeyGroup {
        name: "Chat",
        items: &[
            KeyItem { label: "Switch chat", shortcut: "click a row", detail: "A click on a chat row shows that chat." },
            KeyItem { label: "Close the chat", shortcut: "[close]", detail: "[close] leaves the open chat. Esc does the same once a frame is shut." },
            KeyItem { label: "Open a picture", shortcut: "click", detail: "A click opens the picture in a frame. Esc, [X], or a click outside closes the frame. The chat stays." },
            KeyItem { label: "Open a file", shortcut: "click", detail: "A click on a file in the chat opens it in a frame. Up and down scroll. Esc closes the frame." },
            KeyItem { label: "Scroll the chat", shortcut: "wheel", detail: "The wheel over the list moves the list. The wheel over the talk moves the talk. Each bar shows its thumb. A hard top means the start. An arrow means older lines are still out." },
            KeyItem { label: "Queue a line", shortcut: "F6", detail: "A line sent while the chat is busy, or while F6 is on queue, waits above the prompt." },
            KeyItem { label: "Send now", shortcut: "click the chip", detail: "Send now, edit, and cancel sit on the queued line." },
        ],
    },
    KeyGroup {
        name: "Panels",
        items: &[
            KeyItem { label: "Tabs", shortcut: "click", detail: "The bottom row switches now, Tasks, Mesh, Files, Browser, Lessons, and Term." },
            KeyItem { label: "Description", shortcut: "click the hint", detail: "A click on the hint row opens the longer description." },
            KeyItem { label: "Vim", shortcut: "F2 / F4", detail: "F2 or F4 writes the file. Esc leaves vim." },
            KeyItem { label: "Lessons", shortcut: "Enter", detail: "Enter opens a lesson and its practice." },
        ],
    },
    KeyGroup {
        name: "Session",
        items: &[
            KeyItem { label: "Auto-accept", shortcut: "accept", detail: "Type accept ask, safe, except-push, except-destructive, or full. Pause asks first." },
            KeyItem { label: "Model and effort", shortcut: "click the chip", detail: "Model, effort, and approval chips open a box for the open agent. /effort and /model set them here, so the agent's menu stays out of the chat." },
            KeyItem { label: "Another shell", shortcut: "split", detail: "split opens another shell. shell moves to the next one." },
        ],
    },
];

struct HeldChat {
    lines: Vec<String>,
    agent: AgentKind,
    open_id: String,
    pane_key: String,
    pane_from_end: usize,
    hist_from: u64,
    hist_path: PathBuf,
    hist_at: usize,
}

struct OpenFile {
    n: u16,
    path: PathBuf,
    line: usize,
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "chats",
        "Show chats from Grok, Claude, and Codex on this computer.",
    ),
    ("tasks", "Show the task queue. A task has one owner."),
    (
        "mesh",
        "Show this repo. Enter opens a folder, then a file. Esc steps out. refresh rescans.",
    ),
    ("files", "Show the file list for this directory."),
    (
        "browser",
        "Open the browser. The bar has back, forward, reload, mark, history, size, js, and live.",
    ),
    (
        "lessons",
        "Open lessons for the desk, Claude, vim, the terminal, and Kubernetes.",
    ),
    (
        "term",
        "Open the shell tab. Enter runs a command there.",
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
    (
        "browse",
        "Open a local file or a web page. The bar reloads, marks, and can freeze the page.",
    ),
    ("back", "Step out of a mesh folder, or return to the previous browser page."),
    (
        "refresh",
        "Rescan the mesh. Reload the open page. F5 does the same.",
    ),
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
    (
        "edit",
        "Edit the selected file in vim in this split.",
    ),
    (
        "split",
        "Open another shell in the terminal tab.",
    ),
    (
        "shell",
        "Switch to the next shell in the terminal tab.",
    ),
];

impl App {
    pub fn new(dir: PathBuf, home: PathBuf) -> Self {
        let stored = store::load(&home);
        let chats = chats::load(&home);
        let page_base = dir.clone();
        let mut app = Self {
            section: Section::Chats,
            cmd: String::new(),
            cursor: 0,
            scroll: 0,
            pane_from_end: 0,
            list_from: usize::MAX,
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
            thumbs: Vec::new(),
            thumb_cols: 0,
            opens: Vec::new(),
            shot_seen: 0,
            preview: String::new(),
            page: Vec::new(),
            mode: Mode::Desk,
            mesh_view: Vec::new(),
            mesh_board: health::Board::empty(),
            mesh_at: String::new(),
            mesh_root: String::new(),
            cols: 96,
            rows: 28,
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
            open_id: String::new(),
            open_stamp: None,
            live_at: None,
            live_reply: String::new(),
            live_said: String::new(),
            live_said_at: 0,
            busy_since: None,
            last_pulse: 0,
            turn_tokens: None,
            tokens_base: None,
            marks: Vec::new(),
            hist_from: 0,
            hist_path: PathBuf::new(),
            hist_at: 0,
            held: None,
            shell: crate::shell::Shells::new(),
            lane: Lane::Say,
            paste_left: String::new(),
            shown_image: None,
            lesson_open: None,
            outline: String::new(),
            bound_cwd: String::new(),
            page_base,
            page_url: String::new(),
            page_hist: Vec::new(),
            page_fwd: Vec::new(),
            bookmarks: stored.bookmarks.clone(),
            visits: stored.visits.clone(),
            engine: crate::engine::Engine::default(),
            web_hits: Vec::new(),
            web_on: false,
            web_focus: None,
            web_draft: String::new(),
            web_panel: false,
            web_js: stored.web_js,
            web_live: stored.web_live,
            web_bytes: 0,
            web_traffic: 0,
            web_busy: false,
            browse_hot: None,
            web_hot: None,
            web_job: None,
            browser_pub: String::new(),
            keys_open: false,
            keys_query: String::new(),
            keys_at: 0,
            keys_find: false,
            keys_bits: 1,
            settings_open: false,
            settings_query: String::new(),
            settings_at: 0,
            settings_find: false,
            settings_bits: 24,
            web_theme: stored.web_theme.clone(),
            web_lang: stored.web_lang.clone(),
            look: stored.look.clone(),
            font_face: stored.font_face.clone(),
            font_size: stored.font_size,
            image_quality: stored.image_quality.clone(),
            image_scroll: 0,
            queue_for: String::new(),
            queue: Vec::new(),
            picked_effort: None,
            hover: None,
            meter_open: false,
            meter_tab: 0,
            meter_scroll: 0,
            meter_loaded: String::new(),
            meter_model: String::new(),
            meter_turns: None,
            meter_compactions: None,
            meter_before: None,
            meter_tools: None,
            meter_split: chats::ContextSplit::default(),
            open_folds: Vec::new(),
            tree_shut: Vec::new(),
            doc_open: false,
            doc_title: String::new(),
            doc_lines: Vec::new(),
            doc_scroll: 0,
            pick: None,
            pick_at: 0,
            grip: None,
            sel: None,
            ctrl_c_armed: false,
            ctrl_c_copied: false,
            copy_push: 0,
            kept: Vec::new(),
            copied: String::new(),
        };
        app.engine.set_policy(app.web_js, app.web_live);
        app.refresh_mesh();
        app
    }

    /// The phone helper and its monitor run on the Term tab. They do not open a console.
    pub fn host_mcp(&mut self) {
        let Some(home) = crate::store::user_home() else {
            return;
        };
        let node = home.join("scoop/apps/nvm/current/nodejs/nodejs/node.exe");
        if !node.is_file() {
            return;
        }
        let Some(script) = mcp_script(&home) else {
            return;
        };
        let Some(cwd) = script.parent().and_then(|path| path.parent()).map(|path| path.to_path_buf()) else {
            return;
        };
        let script = script.display().to_string();
        self.shell.host(
            "mcp serve  http://127.0.0.1:17342/mcp",
            &node,
            &[script.clone(), "internal".into(), "serve".into()],
            &cwd,
        );
        self.shell.host(
            "mcp monitor",
            &node,
            &[script, "internal".into(), "monitor".into()],
            &cwd,
        );
    }

    pub fn pump(&mut self) {
        if self.copy_push > 0 {
            self.copy_push -= 1;
            // The terminal can replace a drag with its own buffer copy. Put
            // the cells back once the mouse-up has finished.
            if !cfg!(test) && !self.copied.is_empty() {
                let _ = set_clipboard(&self.copied);
            }
        }
        self.refresh_open();
        self.settle_work();
        self.note_new_shots();
        let before = self.shell.lines().len();
        let follow = self.section == Section::Term && (before == 0 || self.cursor + 1 >= before);
        if self.shell.pump() && follow {
            let last = self.shell.lines().len();
            if last > 0 {
                self.cursor = last - 1;
            }
        }
        self.pull_browser_command();
        self.publish_browser();
    }

    pub fn browser_pending(&self) -> bool {
        self.web_job.is_some()
    }

    pub fn release_press(&mut self) {
        if self.web_job.is_none() {
            self.browse_hot = None;
            self.web_hot = None;
            self.web_busy = false;
        }
    }

    pub fn run_browser_pending(&mut self) {
        let Some(act) = self.web_job.take() else {
            return;
        };
        match act {
            WebAct::Reload => self.reload_page(),
            WebAct::Back => self.browse_back(),
            WebAct::Forward => self.browse_forward(),
            WebAct::Open(url) => self.browse(&url),
            WebAct::Click(index) => self.press_web(index),
            WebAct::Type(text) => {
                if let Err(err) = self.engine.insert(&text) {
                    self.note = err;
                } else {
                    self.redraw_web(false);
                    self.note = "typed".into();
                }
            }
            WebAct::Key(name) => {
                let code = match name.as_str() {
                    "Enter" => 13,
                    "Backspace" => 8,
                    "Tab" => 9,
                    _ => 0,
                };
                if code == 0 {
                    self.note = "no key".into();
                } else if let Err(err) = self.engine.key(&name, code) {
                    self.note = err;
                } else {
                    if name == "Enter" {
                        let _ = self.engine.wait();
                    }
                    self.redraw_web(name == "Enter");
                    self.note = name;
                }
            }
        }
        self.web_busy = false;
        self.browse_hot = None;
        self.web_hot = None;
        let failed = self.note.starts_with("no ")
            || self.note.starts_with("No ")
            || self.note.contains("error")
            || self.note.contains("failed");
        let status = if failed { "err" } else { "ok" };
        let note = self.note.clone();
        self.publish_browser();
        crate::mcp::write_browser_ack(&self.home, &format!("{status}\t{note}"));
    }

    pub fn theme_label(&self) -> &'static str {
        match self.web_theme.as_str() {
            "dark" => "dark",
            "light" => "light",
            _ => "auto",
        }
    }

    pub fn lang_label(&self) -> &'static str {
        match self.web_lang.as_str() {
            "auto" => "auto",
            "ru" => "ru",
            "uk" => "uk",
            "he" => "he",
            "ar" => "ar",
            "de" => "de",
            "fr" => "fr",
            "es" => "es",
            _ => "en",
        }
    }

    fn web_status(&self) -> String {
        format!("theme {} · lang {}", self.theme_label(), self.lang_label())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn accept_label(&self) -> String {
        let level = self.shown_accept();
        if self.paused {
            format!("{} paused", level.label())
        } else {
            level.label().into()
        }
    }

    pub fn composer_face(&self) -> ComposerFace {
        let agent = self.composer_agent();
        let live = agents::face(&self.home, &self.face_dir(), agent, self.permit);
        let model = if live.model != "—" {
            live.model
        } else if !self.model.is_empty() {
            self.model.clone()
        } else {
            "—".into()
        };
        let mut effort = live.effort;
        if let Some((kind, picked)) = &self.picked_effort
            && *kind == agent
        {
            effort = picked.clone();
        }
        ComposerFace {
            agent,
            model,
            effort,
            permit: live.permission,
            lane: self.transcript_lines().is_some().then(|| self.lane.label()),
            ultra: live.ultra,
        }
    }

    pub fn queue_items(&self) -> Vec<String> {
        if self.vim_active() || self.transcript_lines().is_none() || self.queue.is_empty() {
            return Vec::new();
        }
        if self.queue_for != self.open_id {
            return Vec::new();
        }
        self.queue.iter().take(3).cloned().collect()
    }

    fn face_dir(&self) -> PathBuf {
        self.live_cwd()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.dir.clone())
    }

    fn composer_agent(&self) -> AgentKind {
        self.open_agent()
            .or_else(|| self.selected_agent())
            .unwrap_or(AgentKind::Grok)
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

    pub fn image_outline(&self) -> Option<&str> {
        self.shown_image?;
        if self.outline.is_empty() {
            None
        } else {
            Some(self.outline.as_str())
        }
    }

    pub fn shot_number(&self, path: &Path) -> Option<u16> {
        self.images
            .iter()
            .find(|mark| mark.path == path)
            .and_then(|mark| u16::try_from(mark.n).ok())
            .filter(|n| *n > 0)
    }

    pub fn shot_thumb(&self, n: u16) -> Option<&str> {
        let path = self
            .images
            .iter()
            .find(|mark| mark.n == usize::from(n))?
            .path
            .as_path();
        self.thumbs
            .iter()
            .find(|(have, _)| have == path)
            .map(|(_, text)| text.as_str())
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

    pub fn shell_side(&self) -> Vec<String> {
        if self.section != Section::Term || self.pane().is_some() {
            return Vec::new();
        }
        self.shell.side_lines()
    }

    pub fn suggestions(&self) -> Vec<String> {
        if self.vim_active() {
            return Vec::new();
        }
        let token = self.cmd.split_whitespace().next().unwrap_or("");
        if self.chat_open()
            && (token.is_empty() || token.starts_with('/'))
            && self.section != Section::Term
            && let Some(agent) = self.open_agent()
        {
            return agent_suggestions(agent, &self.cmd);
        }
        if token.starts_with('/')
            && self.chat_open()
            && let Some(agent) = self.open_agent()
        {
            return agent_suggestions(agent, &self.cmd);
        }
        if self.section == Section::Term && !token.starts_with('/') {
            return self.term_suggestions();
        }
        if !self.mode_is_desk() && !self.fill && !(self.chat_open() && token.len() >= 4) {
            return Vec::new();
        }
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
        if self.section == Section::Browser && self.web_focus.is_some() {
            return "field";
        }
        if self.section == Section::Term {
            return "shell";
        }
        if self.transcript_lines().is_some() {
            return match self.lane {
                Lane::Say => "",
                Lane::Queue => "queue",
            };
        }
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
            Section::Term => "shell",
        }
    }

    pub fn section_title(&self) -> String {
        match self.section {
            Section::Chats => format!("now  {}", self.chats.len()),
            Section::Files => format!("Files  {}", self.repo_name()),
            Section::Tasks => format!("Tasks  {}", self.repo_name()),
            Section::Lessons => {
                if let Some(index) = self.lesson_open {
                    format!("Lessons  {}", lessons::lessons()[index].title)
                } else {
                    "Lessons".into()
                }
            }
            other => other.label().to_string(),
        }
    }

    pub fn description_line(&self) -> String {
        if !self.note.is_empty() {
            return self.note.clone();
        }
        if self.vim_active() {
            return "vim  type to edit. Enter breaks the line. F2 or F4 writes. Esc leaves vim.".into();
        }
        if self.section == Section::Files && self.cmd.split_whitespace().next().is_none() {
            return "Enter edits the file in vim. F2 or F4 writes it. Esc leaves vim. A typed line goes to the open chat.".into();
        }
        if self.section == Section::Term && self.cmd.split_whitespace().next().is_none() {
            return format!(
                "{}  Enter runs this shell. split opens another. A / line goes to the open chat.",
                self.shell.label()
            );
        }
        if self.transcript_lines().is_some() {
            let scroll = if self.fill {
                "Wheel and arrows scroll this chat. A click on the top bar returns."
            } else {
                "A list click switches the chat. A picture click, or the top bar, returns. Wheel over the list scrolls the list. Wheel over the talk scrolls the talk. F3 fills."
            };
            return format!(
                "{scroll} F6 switches say and queue. Commands sit above the prompt. Tab takes one. Enter sends."
            );
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
                    "Enter opens a folder, then a file in vim. Esc or back steps out. refresh or F5 rescans. A red ! names the missing import. ? lists keys.".into()
                }
                Section::Files => "Enter edits the file in vim. F2 or F4 writes it. Esc leaves vim.".into(),
                Section::Browser => {
                    if self.web_panel {
                        "Marks, then visits. Enter opens one. Esc or back returns to the page.".into()
                    } else if self.web_on {
                        "The bar lights on a click and spins while the page loads. Back, forward, reload, mark, history, size, js, and live. A slab covers each control. ? lists keys.".into()
                    } else if self.page.is_empty() {
                        "Enter opens a site. Most visited first, then the latest. The bar marks a page and can freeze it.".into()
                    } else {
                        "Enter opens a link. The bar steps back and forward.".into()
                    }
                }
                Section::Lessons => {
                    if self.lesson_open.is_some() {
                        "Enter starts the practice. Esc returns to the sections.".into()
                    } else {
                        "Enter opens the lesson. A lesson has sections and a practice.".into()
                    }
                }
                Section::Term => {
                    "Enter runs a Linux command. split opens another shell. After kubectl, git, or another tool, its commands join the suggestions."
                        .into()
                }
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
            Section::Chats => "Empty.",
            Section::Tasks => "No tasks.",
            Section::Mesh => "No towers.",
            Section::Files => "No files.",
            Section::Browser => "No sites.",
            Section::Lessons => "No lessons.",
            Section::Term => "No output.",
        }
    }

    pub fn pane(&self) -> Option<&[String]> {
        if self.fill {
            return None;
        }
        match &self.mode {
            Mode::Split { lines, agent: Some(_) } => Some(lines),
            Mode::Split { lines, .. }
                if matches!(
                    self.section,
                    Section::Chats | Section::Tasks | Section::Mesh
                ) =>
            {
                Some(lines)
            }
            _ => None,
        }
    }

    pub fn transcript_lines(&self) -> Option<&[String]> {
        match &self.mode {
            Mode::Split { lines, agent: Some(_) } => Some(lines),
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

    pub fn set_size(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(40);
        let rows = rows.max(12);
        let changed = cols != self.cols || rows != self.rows;
        self.cols = cols;
        self.rows = rows;
        let width = self.picture_box().0;
        if self.thumb_cols != 0 && self.thumb_cols != width {
            self.thumbs.clear();
            self.shot_seen = 0;
        }
        self.thumb_cols = width;
        if changed && self.web_on {
            self.redraw_web(true);
        }
        if self.section == Section::Mesh {
            let len = self.mesh_lines().len();
            if len > 0 && self.cursor >= len {
                self.cursor = len - 1;
            }
        }
    }

    pub fn rows_for(&self, width: usize) -> Vec<String> {
        match self.section {
            Section::Chats | Section::Tasks => self
                .section_rows()
                .iter()
                .map(|row| self.row_line(row, width))
                .collect(),
            Section::Mesh => self.mesh_lines(),
            _ => self.list_lines(),
        }
    }

    pub fn lines(&self) -> Vec<String> {
        if self.web_panel && self.section == Section::Browser {
            return self.panel_lines();
        }
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
            Section::Mesh => self.mesh_lines(),
            Section::Files => self
                .file_rows()
                .iter()
                .map(|row| self.file_line(row))
                .collect(),
            Section::Browser => {
                if self.web_panel {
                    return self.panel_lines();
                }
                if !self.page.is_empty() {
                    self.page.clone()
                } else if !self.preview.is_empty() {
                    self.preview.lines().map(str::to_string).collect()
                } else {
                    self.site_lines()
                }
            }
            Section::Lessons => lessons::rows(self.lesson_open)
                .iter()
                .map(|row| row.text())
                .collect(),
            Section::Term => {
                let mut lines = self.shell.lines().to_vec();
                if self.pane().is_some() {
                    let side = self.shell.side_lines();
                    if !side.is_empty() {
                        lines.push(String::new());
                        lines.extend(side);
                    }
                }
                lines
            }
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
        // Copy the drag before this key clears it. The first Ctrl+C never quits.
        let copying = matches!(key, Key::CtrlC) && self.sel.is_some();
        if copying {
            self.copy_selection();
        }
        self.ctrl_c_copied = copying;
        if !matches!(key, Key::CtrlC | Key::Ignore) {
            self.ctrl_c_armed = false;
        }
        self.sel = None;
        self.refresh_asks();
        if self.eat_paste(&key) {
            return Effect::None;
        }
        self.note.clear();
        if self.vim_active() {
            return self.on_vim_key(key);
        }
        if self.doc_open {
            return self.on_doc_key(key);
        }
        if self.shown_image.is_some() {
            return self.on_picture_key(key);
        }
        if self.settings_open {
            return self.on_settings_key(key);
        }
        if self.keys_open {
            return self.on_keys_key(key);
        }
        if self.pick.is_some() {
            return self.on_pick_key(key);
        }
        if self.meter_open {
            return self.on_meter_key(key);
        }
        match key {
            Key::Ignore => Effect::None,
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                if self.section == Section::Browser && self.web_focus.take().is_some() {
                    self.web_draft.clear();
                    self.note = "field left".into();
                    return Effect::None;
                }
                if self.section == Section::Browser && self.web_panel {
                    self.web_panel = false;
                    self.note = "page".into();
                    return Effect::None;
                }
                if self.section == Section::Mesh && !self.mesh_at.is_empty() && self.mode_is_desk()
                {
                    self.mesh_up();
                    return Effect::None;
                }
                if self.fill {
                    self.fill = false;
                    return Effect::None;
                }
                if self.section == Section::Lessons && self.lesson_open.is_some() {
                    let index = self.lesson_open.take().unwrap_or(0);
                    self.vim_path = None;
                    self.mode = Mode::Desk;
                    self.fill = false;
                    self.scroll = 0;
                    self.cursor = lessons::rows(None)
                        .iter()
                        .position(|row| matches!(row, lessons::Row::Lesson(n) if *n == index))
                        .unwrap_or(0);
                    return Effect::None;
                }
                if self.held.is_some() && !self.chat_open() {
                    self.restore_held();
                    self.desc_open = false;
                    self.cmd.clear();
                    self.scroll = 0;
                    return Effect::None;
                }
                self.leave_chat();
                Effect::None
            }
            Key::CtrlV => {
                self.paste_clipboard();
                Effect::None
            }
            Key::F(3) => {
                self.toggle_fill();
                Effect::None
            }
            Key::F(5) => {
                self.refresh_desk();
                Effect::None
            }
            Key::F(6) => {
                if self.transcript_lines().is_some() {
                    self.lane = self.lane.flip();
                    self.note = match self.lane {
                        Lane::Say => "say  this line stays in the chat".into(),
                        Lane::Queue => {
                            "queue  the line waits in the chat. Send now, edit, or cancel.".into()
                        }
                    };
                }
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
                self.accept_paste(&text);
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
                if self.web_scroll_keys() {
                    self.scroll_web(-1);
                    return Effect::None;
                }
                if self.fill && self.transcript_lines().is_some() {
                    self.scroll_transcript(-1);
                } else {
                    self.nudge(-1);
                }
                Effect::None
            }
            Key::Down => {
                if self.web_scroll_keys() {
                    self.scroll_web(1);
                    return Effect::None;
                }
                if self.fill && self.transcript_lines().is_some() {
                    self.scroll_transcript(1);
                } else {
                    self.nudge(1);
                }
                Effect::None
            }
            Key::Wheel(delta) | Key::Page(delta) => {
                if self.web_on
                    && !self.web_panel
                    && self.section == Section::Browser
                    && self.transcript_lines().is_none()
                {
                    self.scroll_web(i32::from(delta));
                    return Effect::None;
                }
                if self.transcript_lines().is_some() {
                    if matches!(key, Key::Wheel(_)) && self.pointer_on_list() {
                        self.scroll_list(i32::from(delta));
                    } else {
                        self.scroll_transcript(i32::from(delta));
                    }
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
            Key::Enter => {
                if self.field_keys() {
                    self.submit_web();
                    return Effect::None;
                }
                self.activate()
            }
            Key::Backspace => {
                if self.field_keys() {
                    self.web_key("Backspace", 8);
                    return Effect::None;
                }
                self.confirm.clear();
                self.cmd.pop();
                self.suggest = 0;
                Effect::None
            }
            Key::Char(ch) => {
                if ch == '?' && self.cmd.is_empty() && self.web_focus.is_none() {
                    self.toggle_keys();
                    return Effect::None;
                }
                if ch == ',' && self.cmd.is_empty() && self.web_focus.is_none() {
                    self.toggle_settings();
                    return Effect::None;
                }
                if self.field_keys() {
                    self.type_web(ch);
                    return Effect::None;
                }
                self.confirm.clear();
                self.cmd.push(ch);
                self.suggest = 0;
                self.take_image_cmd();
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
            crate::ui::Spot::Chip(chip) => {
                self.open_pick(chip);
                Effect::None
            }
            crate::ui::Spot::PickClose => {
                self.pick = None;
                Effect::None
            }
            crate::ui::Spot::Pick(index) => {
                self.pick_at = usize::from(index);
                self.apply_picked();
                Effect::None
            }
            crate::ui::Spot::Theme | crate::ui::Spot::Lang => {
                self.open_settings();
                Effect::None
            }
            crate::ui::Spot::Browse(kind) => {
                self.browse_hot = Some(kind);
                self.arm_browse(kind);
                Effect::None
            }
            crate::ui::Spot::KeysClose => {
                self.keys_open = false;
                self.keys_find = false;
                Effect::None
            }
            crate::ui::Spot::Keys(index) => {
                self.keys_at = usize::from(index);
                self.keys_activate();
                Effect::None
            }
            crate::ui::Spot::MeterClose => {
                self.meter_open = false;
                Effect::None
            }
            crate::ui::Spot::MeterTab(tab) => {
                self.meter_tab = tab;
                self.meter_scroll = 0;
                Effect::None
            }
            crate::ui::Spot::Meter | crate::ui::Spot::Fold(_) => {
                if matches!(spot, crate::ui::Spot::Meter) {
                    if !self.meter_open {
                        self.open_meter();
                    }
                } else if let crate::ui::Spot::Fold(index) = spot {
                    self.toggle_fold(usize::from(index));
                }
                Effect::None
            }
            crate::ui::Spot::Tree(index) => {
                self.toggle_tree(usize::from(index));
                Effect::None
            }
            crate::ui::Spot::Doc(index) => {
                self.open_doc(index);
                Effect::None
            }
            crate::ui::Spot::DocClose => {
                self.doc_open = false;
                Effect::None
            }
            crate::ui::Spot::DocBox => Effect::None,
            crate::ui::Spot::Compact => {
                self.compact_context();
                Effect::None
            }
            crate::ui::Spot::SettingClose => {
                self.settings_open = false;
                self.settings_find = false;
                Effect::None
            }
            crate::ui::Spot::Setting(index) => {
                self.settings_at = usize::from(index);
                self.settings_activate();
                Effect::None
            }
            crate::ui::Spot::Steer(code) => {
                let index = usize::from(code >> 2);
                self.steer_queue(index, code & 3);
                Effect::None
            }
            crate::ui::Spot::Image(n) => {
                if self.shown_image == Some(n) {
                    self.clear_picture();
                } else {
                    self.toggle_outline(n);
                }
                Effect::None
            }
            crate::ui::Spot::PicClose => {
                self.clear_picture();
                Effect::None
            }
            crate::ui::Spot::PicBox => Effect::None,
            crate::ui::Spot::Gear => {
                self.toggle_settings();
                Effect::None
            }
            crate::ui::Spot::Help => {
                self.toggle_keys();
                Effect::None
            }
            crate::ui::Spot::ChatClose => {
                self.clear_picture();
                self.doc_open = false;
                self.leave_chat();
                Effect::None
            }
            crate::ui::Spot::ChatFill => {
                self.toggle_fill();
                Effect::None
            }
            crate::ui::Spot::Back => self.on_key(crate::term::Key::Esc),
            crate::ui::Spot::Web(index) => {
                let index = self.field_under(index);
                let kind = self
                    .web_hits
                    .get(usize::from(index))
                    .map(|hit| hit.kind);
                if kind == Some(crate::engine::KIND_FIELD) {
                    self.press_web(index);
                } else {
                    self.web_hot = Some(index);
                    self.note = "loading".into();
                    self.queue_web(WebAct::Click(index));
                }
                Effect::None
            }
            crate::ui::Spot::Suggest(index) => {
                if let Some(name) = self.suggestions().get(usize::from(index)) {
                    self.cmd = format!("{name} ");
                    self.suggest = 0;
                }
                Effect::None
            }
            crate::ui::Spot::Open(n) => {
                self.open_cited(n);
                Effect::None
            }
            crate::ui::Spot::Row(index) if self.section == Section::Browser => {
                self.cursor = index;
                self.activate_browser();
                Effect::None
            }
            crate::ui::Spot::Row(index) => {
                let index = if matches!(self.section, Section::Chats | Section::Tasks) {
                    // A click on a row that is already on screen must not
                    // slide the list. Freeze the window, then move the cursor.
                    self.pin_list_window();
                    let rows = self.section_rows();
                    if rows.is_empty() {
                        index
                    } else {
                        focus_in(&rows, index)
                    }
                } else {
                    index
                };
                let same = self.cursor == index;
                self.cursor = index;
                if matches!(self.section, Section::Chats | Section::Tasks) {
                    self.reveal_list_cursor();
                }
                if self.section == Section::Tasks
                    && matches!(self.row_at(index), Some(DeskRow::NewTask))
                {
                    self.begin_task();
                    return Effect::None;
                }
                let open = if matches!(self.section, Section::Chats | Section::Tasks) {
                    self.pane().is_some() || same || times >= 2
                } else {
                    same || times >= 2
                };
                if open {
                    self.clear_picture();
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
        if self.paste_left.is_empty()
            && looks_like_burst(keys)
            && let Some(text) = clipboard_text()
            && burst_is_paste(keys, &text)
        {
            self.paste_left = text.clone();
            let trimmed = text.trim();
            if chats::image_path(trimmed).is_some() {
                self.paste_image(trimmed);
            } else {
                self.keep_paste_file(&text);
            }
        }
        let mut quit = false;
        let mut index = 0;
        while index < keys.len() {
            if let Key::Move { x, y } = &keys[index] {
                if *x > 0 && *y > 0 {
                    self.hover = Some((x - 1, y - 1));
                }
                index += 1;
                continue;
            }
            if let Key::MouseDown { x, y } = keys[index] {
                index += 1;
                self.ctrl_c_armed = false;
                if let Some((x, y)) = mouse_cell(x, y) {
                    let (x, y) = self.clamp_cell(x, y);
                    self.grip = Some(Grip {
                        x,
                        y,
                        spot: spot_at(x, y),
                        times: 1,
                        moved: false,
                    });
                    self.sel = None;
                }
                continue;
            }
            if let Key::Drag { x, y } = keys[index] {
                index += 1;
                if let Some((x, y)) = mouse_cell(x, y) {
                    let (x, y) = self.clamp_cell(x, y);
                    self.track_drag(x, y);
                }
                continue;
            }
            if let Key::Release { x, y } = keys[index] {
                index += 1;
                if let Some((x, y)) = mouse_cell(x, y) {
                    let (x, y) = self.clamp_cell(x, y);
                    if self.release_at(x, y) == Effect::Quit {
                        quit = true;
                    }
                }
                continue;
            }
            if let Key::Click { x, y } = &keys[index] {
                self.ctrl_c_armed = false;
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

    fn clamp_cell(&self, x: usize, y: usize) -> (usize, usize) {
        (
            x.min(self.cols.saturating_sub(1)),
            y.min(self.rows.saturating_sub(1)),
        )
    }

    fn arm_ctrl_c(&mut self) -> Effect {
        if self.ctrl_c_armed {
            return Effect::Quit;
        }
        self.ctrl_c_armed = true;
        self.note = if self.ctrl_c_copied {
            "copied".into()
        } else {
            "ctrl+c  repeat to quit".into()
        };
        Effect::None
    }

    fn track_drag(&mut self, x: usize, y: usize) {
        let Some(grip) = self.grip.as_mut() else {
            return;
        };
        if grip.x.abs_diff(x) > 1 || grip.y != y {
            grip.moved = true;
        }
        if !grip.moved {
            return;
        }
        let ax = grip.x;
        let ay = grip.y;
        self.sel = Some((ax, ay, x, y));
    }

    fn release_at(&mut self, x: usize, y: usize) -> Effect {
        let Some(grip) = self.grip.take() else {
            return Effect::None;
        };
        let moved = grip.moved || grip.x.abs_diff(x) > 1 || grip.y != y;
        if moved {
            self.sel = Some((grip.x, grip.y, x, y));
            self.copy_selection();
            return Effect::None;
        }
        self.sel = None;
        match grip.spot {
            Some(spot) => self.press(spot, grip.times),
            None => Effect::None,
        }
    }

    fn copy_selection(&mut self) {
        let Some(sel) = self.sel else {
            return;
        };
        let (x0, mut y0, x1, mut y1) = ordered_sel(sel);
        // Stay on the painted rows. A coordinate past the window used to walk
        // every kept line and copy the whole desk.
        let Some(last_row) = self.kept.len().checked_sub(1) else {
            return;
        };
        y0 = y0.min(last_row);
        y1 = y1.min(last_row);
        let mut lines = Vec::new();
        for y in y0..=y1 {
            let row = self.kept.get(y).map(String::as_str).unwrap_or("");
            let chars: Vec<char> = row.chars().collect();
            let text = if y0 == y1 {
                slice_chars(&chars, x0, x1.saturating_add(1))
            } else if y == y0 {
                slice_chars(&chars, x0, chars.len())
            } else if y == y1 {
                slice_chars(&chars, 0, x1.saturating_add(1))
            } else {
                row.to_string()
            };
            lines.push(text.trim_end().to_string());
        }
        let text = lines.join("\n");
        if text.trim().is_empty() {
            return;
        }
        self.copied = text.clone();
        self.copy_push = 2;
        if !cfg!(test) && set_clipboard(&text) {
            self.note = "copied".into();
        }
    }

    fn enter(&mut self, section: Section) {
        self.pick = None;
        self.doc_open = false;
        if self.held.is_some() && !self.chat_open() {
            self.restore_held();
        }
        self.remember_bound();
        let mesh = self.bound_mesh();
        let keep = self.chat_open();
        self.section = section;
        self.cursor = 0;
        self.scroll = 0;
        self.fill = false;
        self.vim_path = None;
        if !keep {
            self.pane_key.clear();
            self.open_id.clear();
            self.open_stamp = None;
            self.held = None;
            self.mode = Mode::Desk;
        }
        if section == Section::Tasks {
            self.usage_line = usage::line(&self.home);
            self.questions_line = agents::questions(&self.home);
        }
        if section == Section::Files {
            self.dir = mesh.clone();
            self.links_key.clear();
            self.refresh_mesh();
        }
        if section == Section::Browser {
            if self.web_on && !self.page.is_empty() {
                self.stash_chat();
                self.vim_path = None;
                self.mode = Mode::Desk;
                self.fill = false;
            } else if self.page.is_empty() {
                self.page_base = mesh;
            }
        }
        if section == Section::Lessons {
            self.lesson_open = None;
            self.cursor = lessons::rows(None)
                .iter()
                .position(|row| row.picks())
                .unwrap_or(0);
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
        let root_key = root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if root_key != self.mesh_root {
            self.mesh_root = root_key;
            self.mesh_at.clear();
        }
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
            self.mesh_board = health::scan(path, roads, externals);
            self.dirty = gitread::dirty_paths(path).unwrap_or_default();
        } else {
            self.mesh_board = health::Board::empty();
            lines.push("No towers.".into());
            self.dirty.clear();
        }
        if !self.mesh_board.is_empty() {
            let bad = self.mesh_board.unhealthy_count();
            let shown = if bad == 0 {
                "none".to_string()
            } else {
                bad.to_string()
            };
            lines.push(format!("Unhealthy  {shown}"));
        }
        if !self.mesh_at.is_empty() && self.mesh_board.children(&self.mesh_at).is_empty() {
            self.mesh_at.clear();
        }
        self.mesh_view = lines;
    }

    fn refresh_desk(&mut self) {
        self.links_key.clear();
        self.refresh_mesh();
        if self.web_on {
            self.browse_hot = Some(2);
            self.note = format!("refreshing  {}", self.web_status());
            self.queue_web(WebAct::Reload);
            return;
        }
        self.note = "refreshed".into();
    }

    fn list_width(&self) -> usize {
        let cols = self.cols.max(40);
        if self.pane().is_some() && !self.fill {
            let left_max = cols.saturating_sub(40);
            let left = if cols < 72 || left_max < 36 {
                cols
            } else {
                (cols / 2).clamp(36, left_max)
            };
            left.saturating_sub(3).max(36)
        } else {
            cols.saturating_sub(3).max(36)
        }
    }

    fn mesh_lines(&self) -> Vec<String> {
        self.mesh_rows().into_iter().map(|row| row.line).collect()
    }

    fn mesh_rows(&self) -> Vec<MeshRow> {
        let mut rows = Vec::new();
        for line in &self.mesh_view {
            let hit = skill_hit(line)
                .map(|(agent, name)| MeshHit::Skill(agent, name))
                .unwrap_or(MeshHit::None);
            rows.push(MeshRow {
                line: line.clone(),
                hit,
            });
        }
        if self.mesh_at.is_empty() {
            for line in health::overview(&self.mesh_board, self.list_width()) {
                rows.push(MeshRow {
                    line,
                    hit: MeshHit::None,
                });
            }
        } else {
            rows.push(MeshRow {
                line: format!("\x1b[38;5;220m{}\x1b[38;5;203m", self.mesh_at),
                hit: MeshHit::None,
            });
            rows.push(MeshRow {
                line: "  ..".into(),
                hit: MeshHit::Up,
            });
        }
        for entry in self.mesh_board.children(&self.mesh_at) {
            let hit = match entry.kind {
                health::EntryKind::Dir => MeshHit::Dir(entry.rel.clone()),
                health::EntryKind::File => MeshHit::File(entry.rel.clone()),
            };
            rows.push(MeshRow {
                line: health::entry_line(&entry),
                hit,
            });
        }
        if self.mesh_at.is_empty() {
            for line in health::extras(&self.mesh_board) {
                rows.push(MeshRow {
                    line,
                    hit: MeshHit::None,
                });
            }
        }
        if rows.is_empty() {
            rows.push(MeshRow {
                line: "No towers.".into(),
                hit: MeshHit::None,
            });
        }
        rows
    }

    fn mesh_up(&mut self) {
        self.mesh_at = match self.mesh_at.rsplit_once('/') {
            Some((parent, _)) => parent.to_string(),
            None => String::new(),
        };
        self.cursor = self
            .mesh_rows()
            .iter()
            .position(|row| matches!(row.hit, MeshHit::Dir(_) | MeshHit::File(_)))
            .unwrap_or(0);
        self.note = if self.mesh_at.is_empty() {
            "repo".into()
        } else {
            self.mesh_at.clone()
        };
    }

    fn mesh_down(&mut self, rel: &str) {
        self.mesh_at = rel.to_string();
        self.cursor = self
            .mesh_rows()
            .iter()
            .position(|row| matches!(row.hit, MeshHit::Dir(_) | MeshHit::File(_)))
            .unwrap_or(0);
        self.note = rel.to_string();
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

    /// The wheel sits on the now list, left of the talk.
    fn pointer_on_list(&self) -> bool {
        if self.fill || !matches!(self.section, Section::Chats | Section::Tasks) {
            return false;
        }
        let Some((x, y)) = self.hover else {
            return false;
        };
        let cols = self.cols.max(1);
        let left = crate::ui::Screen::list_column(cols);
        let top = 2usize;
        let bottom = self.rows.saturating_sub(3);
        x < left.saturating_sub(1) && y >= top && y < bottom
    }

    /// Rows the now list can show, matching the chrome around the body.
    fn list_room(&self) -> usize {
        let rows = self.rows;
        if rows < 8 {
            return 1;
        }
        let desc_y = rows - 3;
        let menu = !self.vim_active()
            && (!self.cmd.is_empty()
                || self.transcript_lines().is_some()
                || self.section == Section::Term);
        let show_suggest = menu
            && !self.suggestions().is_empty()
            && self.note.is_empty()
            && !self.desc_open;
        let show_band = self.desc_open
            || !self.note.is_empty()
            || show_suggest
            || self.transcript_lines().is_none();
        let rule_y = if show_band { desc_y - 1 } else { desc_y };
        let list_top = if self.fill {
            1
        } else if self.section == Section::Browser {
            3
        } else {
            2
        };
        let ask_n = if self.fill {
            0
        } else {
            self.ask_lines()
                .len()
                .min(3)
                .min(rule_y.saturating_sub(list_top))
        };
        let queue_n = if self.fill {
            0
        } else {
            let cap = rule_y.saturating_sub(list_top + ask_n);
            self.queue_items().len().min(cap)
        };
        let hist_n = if self.fill {
            0
        } else {
            let hist_room = rule_y
                .saturating_sub(list_top + ask_n + queue_n)
                .saturating_sub(1);
            self.history_band().len().min(2).min(hist_room)
        };
        let body_end = rule_y.saturating_sub(hist_n + queue_n);
        body_end.saturating_sub(list_top + ask_n).max(1)
    }

    fn scroll_list(&mut self, delta: i32) {
        let len = self.section_rows().len();
        if len == 0 {
            return;
        }
        let room = self.list_room().max(1);
        let max_start = len.saturating_sub(room);
        let mut start = if self.list_from == usize::MAX {
            self.focus_index()
                .saturating_sub(room.saturating_sub(1))
                .min(max_start)
        } else {
            self.list_from.min(max_start)
        };
        let jump = if delta.abs() > 1 {
            delta.unsigned_abs() as usize
        } else {
            3
        };
        if delta < 0 {
            start = start.saturating_sub(jump);
        } else {
            start = (start + jump).min(max_start);
        }
        self.list_from = start;
    }

    /// Keep the rows on screen. Follow mode becomes a fixed window.
    fn pin_list_window(&mut self) {
        if self.list_from != usize::MAX {
            return;
        }
        if !matches!(self.section, Section::Chats | Section::Tasks) {
            return;
        }
        let len = self.section_rows().len();
        if len == 0 {
            return;
        }
        let room = self.list_room().max(1);
        let max_start = len.saturating_sub(room);
        self.list_from = self
            .focus_index()
            .saturating_sub(room.saturating_sub(1))
            .min(max_start);
    }

    fn reveal_list_cursor(&mut self) {
        if self.list_from == usize::MAX {
            return;
        }
        let len = self.section_rows().len();
        if len == 0 {
            return;
        }
        let room = self.list_room().max(1);
        let at = self.focus_index().min(len - 1);
        let max_start = len.saturating_sub(room);
        let start = self.list_from.min(max_start);
        if at < start {
            self.list_from = at;
        } else if at >= start + room {
            self.list_from = at.saturating_sub(room - 1).min(max_start);
        }
    }

    fn scroll_transcript(&mut self, delta: i32) {
        let step = if delta.abs() > 1 { 1 } else { 3 };
        let jump = step * delta.unsigned_abs() as usize;
        if delta < 0 {
            self.pane_from_end = self.pane_from_end.saturating_add(jump);
            self.grow_older();
        } else {
            self.pane_from_end = self.pane_from_end.saturating_sub(jump);
        }
    }

    /// Scrolling toward the start of a long chat reads the previous page.
    /// Lines already on screen stay put: the offset counts from the end.
    fn grow_older(&mut self) {
        if self.hist_from == 0 || self.hist_path.as_os_str().is_empty() || self.vim_active() {
            return;
        }
        if !self.near_transcript_top() {
            return;
        }
        let page = chats::transcript_before(&self.hist_path, self.hist_from);
        let blank = page.lines.iter().all(|line| line == "No transcript.");
        if blank || page.from >= self.hist_from {
            self.hist_from = 0;
            return;
        }
        let lines = stamp_turns(page.lines, &self.marks);
        if lines.is_empty() {
            self.hist_from = 0;
            return;
        }
        let at = self.hist_at;
        let width = self.transcript_pane_width();
        let Mode::Split { lines: pane, agent: Some(_) } = &mut self.mode else {
            return;
        };
        let at = at.min(pane.len());
        // The open diff sits above this insert. Counting from the end would
        // slide the viewport off that list and onto the page that just arrived.
        let tail = block_rows(&pane[at..], width);
        let added = block_rows(&lines, width);
        let hold = tail <= self.pane_from_end;
        for (index, line) in lines.into_iter().enumerate() {
            pane.insert(at + index, line);
        }
        if hold {
            self.pane_from_end = self.pane_from_end.saturating_add(added);
        }
        self.hist_from = page.from;
        self.shot_seen = 0;
    }

    /// Same width the transcript pane wraps with.
    fn transcript_pane_width(&self) -> usize {
        let cols = self.cols.max(1);
        let left = crate::ui::Screen::list_column(cols);
        if left >= cols.saturating_sub(8) {
            return cols.max(8);
        }
        cols.saturating_sub(left.saturating_add(4)).max(8)
    }

    /// The scroll offset counts folded rows, the same rows the pane paints.
    /// Tool and think lines collapse into one row, and a shut folder hides
    /// its files, so the previous page loads when that painted top is near.
    fn near_transcript_top(&self) -> bool {
        let Some(lines) = self.transcript_lines() else {
            return false;
        };
        let width = self.transcript_pane_width();
        let folded = fold_lines(lines, &self.open_folds, &self.tree_shut);
        let need = self
            .pane_from_end
            .saturating_add(self.rows)
            .saturating_add(24);
        let mut rows = 0usize;
        for row in folded.iter().rev() {
            rows = rows.saturating_add(row.text.chars().count() / width + 1);
            if rows > need {
                return false;
            }
        }
        true
    }

    fn nudge(&mut self, delta: i32) {
        if self.vim_active() {
            let len = self.lines().len();
            if len == 0 {
                return;
            }
            if delta < 0 {
                self.scroll = self.scroll.saturating_sub(delta.unsigned_abs() as usize);
            } else {
                self.scroll = (self.scroll + delta as usize).min(len - 1);
            }
            self.cursor = self.scroll;
            return;
        }
        if self.cmd.split_whitespace().next().is_some() {
            let suggestions = self.suggestions();
            if !suggestions.is_empty() {
                let len = suggestions.len();
                self.suggest = clamp_index(self.suggest, delta, len);
                return;
            }
        }
        if self.pane().is_some() && matches!(self.section, Section::Chats | Section::Tasks) {
            self.step_list(delta);
            self.follow_pane();
            return;
        }
        if self.chat_open() && !matches!(self.section, Section::Chats | Section::Tasks) {
            let len = self.list_lines().len();
            if len > 0 {
                self.cursor = clamp_index(self.cursor, delta, len);
            }
            return;
        }
        if !self.mode_is_desk() {
            if self.section == Section::Browser {
                let len = self.lines().len();
                if len > 0 {
                    self.cursor = clamp_index(self.cursor, delta, len);
                }
                return;
            }
            if self.section == Section::Lessons {
                self.step_lessons(delta);
                return;
            }
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
        if self.section == Section::Lessons {
            self.step_lessons(delta);
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

    fn leave_chat(&mut self) {
        self.vim_path = None;
        self.held = None;
        self.mode = Mode::Desk;
        self.pane_key.clear();
        self.open_id.clear();
        self.open_stamp = None;
        self.live_at = None;
        self.live_reply.clear();
        self.live_said.clear();
        self.live_said_at = 0;
        self.busy_since = None;
        self.last_pulse = 0;
        self.turn_tokens = None;
        self.marks.clear();
        self.desc_open = false;
        self.fill = false;
        self.cmd.clear();
        self.scroll = 0;
        self.list_from = usize::MAX;
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
            Section::Files => self.open_file(true),
            Section::Chats => self.open_chat(false),
            Section::Tasks => self.open_task(),
            Section::Mesh => self.open_mesh_row(),
            Section::Lessons => self.open_lesson(),
            Section::Browser => {
                self.activate_browser();
                Effect::None
            }
            Section::Term => Effect::None,
        }
    }

    fn run(&mut self, line: &str) -> bool {
        let mut parts = line.split_whitespace();
        let Some(head) = parts.next() else {
            return true;
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match head {
            "chats" | "tasks" | "mesh" | "files" | "browser" | "lessons" | "term" => {
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
            "back" => {
                if self.section == Section::Mesh && !self.mesh_at.is_empty() {
                    self.mesh_up();
                } else {
                    self.browse_back();
                }
            }
            "refresh" => self.refresh_desk(),
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
            "edit" => {
                let _ = self.edit_selected();
            }
            "split" => {
                if self.section != Section::Term {
                    self.enter(Section::Term);
                }
                self.shell.split();
                let last = self.shell.lines().len();
                if last > 0 {
                    self.cursor = last - 1;
                }
                self.note = self.shell.label();
            }
            "shell" => {
                if self.section != Section::Term {
                    self.enter(Section::Term);
                }
                self.shell.cycle();
                let last = self.shell.lines().len();
                if last > 0 {
                    self.cursor = last - 1;
                }
                self.note = self.shell.label();
            }
            _ => {
                let to_agent = line.trim_start().starts_with('/') && self.transcript_lines().is_some();
                if self.section == Section::Term && !to_agent {
                    self.shell.submit(&self.dir, line);
                    let last = self.shell.lines().len();
                    if last > 0 {
                        self.cursor = last - 1;
                    }
                } else if self.transcript_lines().is_some() {
                    if self.hold_for_queue() {
                        self.enqueue(line);
                    } else {
                        self.post_say(line);
                    }
                } else if self.section == Section::Browser
                    && crate::page::web_url(line.trim(), &self.page_base).is_some()
                {
                    self.browse(line.trim());
                } else {
                    self.note = format!("No command {head}.");
                }
            }
        }
        true
    }

    fn hold_for_queue(&self) -> bool {
        self.lane == Lane::Queue
            || self.open_busy()
            || (self.queue_for == self.open_id && !self.queue.is_empty())
    }

    fn open_busy(&self) -> bool {
        self.busy_since.is_some()
            || self
                .chats
                .iter()
                .any(|chat| chat.id == self.open_id && chats::updates_fresh(&chat.path))
    }

    fn enqueue(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.queue_for != self.open_id {
            self.queue.clear();
            self.queue_for = self.open_id.clone();
        }
        if self.queue.len() >= 8 {
            self.note = "queue is full".into();
            return;
        }
        self.queue.push(text.to_string());
        self.note = "queued".into();
    }

    fn steer_queue(&mut self, index: usize, kind: u16) {
        if self.queue_for != self.open_id || index >= self.queue.len() {
            return;
        }
        match kind {
            0 => {
                let text = self.queue.remove(index);
                self.post_say(&text);
            }
            1 => {
                self.cmd = self.queue.remove(index);
                self.suggest = 0;
                self.note = "edit".into();
            }
            2 => {
                self.queue.remove(index);
                self.note = "cancelled".into();
            }
            _ => {}
        }
    }

    fn post_say(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        // /effort and /model open a full-screen picker in the agent. Set them
        // here so that picker is not typed into the live session or the chat.
        if self.local_slash(text) {
            return;
        }
        let agent = self.open_agent().unwrap_or(AgentKind::Grok);
        let id = self.open_id.clone();
        let Mode::Split { lines, .. } = &mut self.mode else {
            return;
        };
        let now = unix_now();
        lines.push(format!("user  {}  {text}", stamp(now)));
        self.pane_from_end = 0;
        if id.is_empty() {
            self.note = "not live".into();
            return;
        }
        self.tokens_base = self.turn_tokens.or(self.tokens_base);
        self.turn_tokens = None;
        self.last_pulse = 0;
        self.busy_since = Some(now);
        let history = self
            .chats
            .iter()
            .find(|chat| chat.id == id)
            .map(|chat| chat.path.clone());
        let saved = chats::append_outbox(&self.home, &id, history.as_deref(), text);
        let sent = crate::deliver::send(&self.home, agent, &id, text);
        self.note = match saved {
            Ok(()) => sent.note.clone(),
            Err(err) => format!("{}  {err}", sent.note),
        };
        let screen = screen_reply(lines, &sent.screen);
        if !screen.is_empty() {
            if !self.live_reply.is_empty() {
                self.live_reply.push('\n');
            }
            self.live_reply.push_str(&screen);
            for (index, line) in screen.lines().enumerate() {
                if index == 0 && lines.last().is_some_and(|row| !row.is_empty()) {
                    lines.push(String::new());
                }
                if index == 0 {
                    lines.push(stamp_assistant_line(&format!("assistant  {line}"), now));
                } else {
                    lines.push(line.to_string());
                }
            }
        }
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

    fn local_slash(&mut self, line: &str) -> bool {
        let Some(body) = line.trim().strip_prefix('/') else {
            return false;
        };
        let mut parts = body.split_whitespace();
        let Some(head) = parts.next() else {
            return false;
        };
        let arg = parts.next().unwrap_or("");
        if parts.next().is_some() {
            return false;
        }
        let agent = self.composer_agent();
        match head {
            "effort" => {
                if arg.is_empty() {
                    self.note = format!("{} effort: {}", agent.label(), agent.efforts().join(", "));
                } else {
                    self.set_effort(arg);
                }
                true
            }
            "model" => {
                if arg.is_empty() {
                    self.note = format!("{} model: {}", agent.label(), agent.models().join(", "));
                } else {
                    self.set_model(arg);
                }
                true
            }
            _ => false,
        }
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
        let agent = self.open_agent().or_else(|| self.selected_agent());
        self.note = agents::apply_model(&self.home, &self.face_dir(), agent, name);
        if agent == Some(AgentKind::Claude) {
            self.nudge_claude(&format!("/model {name}"));
        }
        self.record(&format!("model {name}"));
        self.persist();
    }

    fn nudge_claude(&mut self, command: &str) {
        if self.composer_agent() != AgentKind::Claude || self.open_id.is_empty() {
            return;
        }
        let sent = crate::deliver::send(&self.home, AgentKind::Claude, &self.open_id, command);
        if sent.note.is_empty() || sent.note == "not live" {
            return;
        }
        if self.note.is_empty() {
            self.note = sent.note;
        } else {
            self.note = format!("{}  {}", self.note, sent.note);
        }
    }

    fn arm_web_look(&mut self) {
        let dark = scheme_is_dark(&self.web_theme, system_dark());
        let locale = locale_tag(&self.web_lang, &system_locale());
        self.engine.set_look(dark, &locale);
    }

    fn push_web_look(&mut self, reload: bool) {
        self.arm_web_look();
        self.note = self.web_status();
        self.persist();
        if reload && self.web_on {
            self.browse_hot = Some(2);
            self.queue_web(WebAct::Reload);
        }
    }

    fn open_pick(&mut self, chip: crate::ui::Chip) {
        if self.pick == Some(chip) {
            self.pick = None;
            return;
        }
        self.meter_open = false;
        self.keys_open = false;
        self.keys_find = false;
        self.settings_open = false;
        self.settings_find = false;
        self.doc_open = false;
        self.pick = Some(chip);
        let current = self.pick_current();
        self.pick_at = self
            .pick_choices()
            .iter()
            .position(|item| *item == current)
            .unwrap_or(0);
    }

    fn apply_picked(&mut self) {
        let Some(chip) = self.pick else {
            return;
        };
        let choices = self.pick_choices();
        let Some(name) = choices.get(self.pick_at).copied() else {
            self.pick = None;
            return;
        };
        self.pick = None;
        match chip {
            crate::ui::Chip::Model => self.set_model(name),
            crate::ui::Chip::Effort => self.set_effort(name),
            crate::ui::Chip::Permit => self.set_permit_name(name),
        }
    }

    fn set_permit_name(&mut self, name: &str) {
        let agent = self.composer_agent();
        if let Some(permit) = permit_from(agent, name) {
            self.permit = permit;
        }
        self.note = agents::apply_agent_permit(&self.home, &self.face_dir(), agent, name);
        self.record(&format!("permit {name}"));
        self.persist();
    }

    fn on_pick_key(&mut self, key: Key) -> Effect {
        let n = self.pick_choices().len();
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                self.pick = None;
                Effect::None
            }
            Key::Up => {
                self.pick_at = self.pick_at.saturating_sub(1);
                Effect::None
            }
            Key::Down => {
                if n > 0 {
                    self.pick_at = (self.pick_at + 1).min(n - 1);
                }
                Effect::None
            }
            Key::Enter => {
                self.apply_picked();
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn set_effort(&mut self, name: &str) {
        let agent = self.composer_agent();
        if !agent.efforts().contains(&name) {
            self.note = format!("{} effort: {}", agent.label(), agent.efforts().join(", "));
            return;
        }
        self.effort = name.into();
        if let Some(task) = self.tasks.last_mut() {
            task.effort = self.effort.clone();
        }
        if agent == AgentKind::Claude && name == "max" {
            self.picked_effort = Some((agent, name.into()));
            self.note = "Claude effort max  this session".into();
            self.nudge_claude("/effort max");
            self.record("effort max");
            self.persist();
            return;
        }
        if self
            .picked_effort
            .as_ref()
            .is_some_and(|(kind, _)| *kind == agent)
        {
            self.picked_effort = None;
        }
        let model = self.composer_face().model;
        self.note = agents::apply_effort(&self.home, agent, name, &model);
        if agent == AgentKind::Claude {
            self.nudge_claude(&format!("/effort {name}"));
        }
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

    fn paste_clipboard(&mut self) {
        let files: Vec<_> = clipboard_files()
            .into_iter()
            .filter(|path| path.exists())
            .collect();
        let in_field = self.field_keys();
        if in_field {
            if !files.is_empty() {
                self.paste_into_field(&join_quoted(&files));
                return;
            }
            if let Some(path) = clipboard_image(&self.home) {
                self.paste_into_field(&quote_paste(&path));
                return;
            }
            if let Some(text) = clipboard_text() {
                let flat = text.replace(['\r', '\n'], " ");
                let flat = flat.trim();
                if !flat.is_empty() {
                    self.paste_into_field(flat);
                    return;
                }
            }
            self.note = "clipboard empty".into();
            return;
        }
        let image = if files.is_empty() {
            clipboard_image(&self.home)
        } else {
            None
        };
        let text = if files.is_empty() && image.is_none() {
            clipboard_text()
        } else {
            None
        };
        self.place_clip(files, image, text);
    }

    fn paste_into_field(&mut self, text: &str) {
        let flat = text.replace(['\r', '\n'], " ");
        let flat = flat.trim();
        if flat.is_empty() {
            self.note = "clipboard empty".into();
            return;
        }
        self.web_draft.push_str(flat);
        if let Err(err) = self.engine.insert(flat) {
            self.note = err;
        }
        self.redraw_web(false);
    }

    fn place_clip(&mut self, files: Vec<PathBuf>, image: Option<PathBuf>, text: Option<String>) {
        let files: Vec<_> = files.into_iter().filter(|path| path.exists()).collect();
        if !files.is_empty() {
            self.insert_paths(&files);
            return;
        }
        if let Some(path) = image.filter(|path| path.is_file()) {
            self.insert_paths(&[path]);
            return;
        }
        if let Some(text) = text {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                self.paste_left = text.clone();
                if chats::image_path(trimmed).is_some() {
                    self.paste_image(trimmed);
                } else {
                    self.keep_paste_file(&text);
                }
                return;
            }
        }
        self.note = "clipboard empty".into();
    }

    fn insert_paths(&mut self, paths: &[PathBuf]) {
        let piece = join_quoted(paths);
        if piece.is_empty() {
            self.note = "clipboard empty".into();
            return;
        }
        if !self.cmd.is_empty() && !self.cmd.ends_with(' ') {
            self.cmd.push(' ');
        }
        self.cmd.push_str(&piece);
        self.suggest = 0;
        let label = if paths.len() == 1 {
            paths[0]
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into())
        } else {
            format!("{} files", paths.len())
        };
        self.note = format!("pasted  {label}");
    }

    fn eat_paste(&mut self, key: &crate::term::Key) -> bool {
        if self.paste_left.is_empty() {
            return false;
        }
        if matches!(key, crate::term::Key::CtrlV) {
            return false;
        }
        let mut rest = self.paste_left.chars().peekable();
        while rest.next_if(|ch| *ch == '\r').is_some() {}
        let Some(next) = rest.peek().copied() else {
            self.paste_left.clear();
            return false;
        };
        let hit = match key {
            crate::term::Key::Enter => next == '\n',
            crate::term::Key::Char(ch) => *ch == next,
            _ => false,
        };
        if !hit {
            self.paste_left.clear();
            return false;
        }
        rest.next();
        self.paste_left = rest.collect();
        true
    }

    fn accept_paste(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if chats::image_path(trimmed).is_some() {
            self.paste_image(trimmed);
            return;
        }
        if !text.contains('\n') && !text.contains('\r') {
            self.cmd.push_str(text);
            return;
        }
        self.keep_paste_file(text);
    }

    fn keep_paste_file(&mut self, text: &str) {
        match write_paste_md(&self.home, text) {
            Ok(path) => {
                let shown = path.display().to_string();
                if self.transcript_lines().is_some() {
                    self.post_say(&shown);
                    self.cmd.clear();
                } else {
                    self.cmd = shown.clone();
                }
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(shown);
                self.note = format!("pasted  {name}");
            }
            Err(err) => self.note = format!("paste  {err}"),
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
        self.load_image(path);
    }

    fn take_image_cmd(&mut self) {
        let trimmed = self.cmd.trim().trim_matches('"').trim();
        let Some(path) = chats::image_path(trimmed) else {
            return;
        };
        let path = if path.is_absolute() {
            path
        } else {
            self.dir.join(path)
        };
        if !path.is_file() {
            return;
        }
        self.cmd.clear();
        self.suggest = 0;
        self.load_image(path);
    }

    fn remember_thumb(&mut self, path: &Path) {
        let cols = self.picture_box().0;
        if self.thumb_cols != cols {
            self.thumbs.clear();
            self.thumb_cols = cols;
        }
        if self.thumbs.iter().any(|(have, _)| have == path) {
            return;
        }
        if self.thumbs.len() >= 12 {
            self.thumbs.remove(0);
        }
        if let Ok(thumb) = draw::portrait(path, cols, 14) {
            self.thumbs.push((path.to_path_buf(), thumb));
        }
    }

    fn load_image(&mut self, path: PathBuf) {
        let n = self.images.len() + 1;
        match draw::inspect(&path, n, 40, 12) {
            Ok(preview) => {
                self.remember_thumb(&path);
                let token = preview.mark.token();
                let line = image_line(&preview.mark);
                self.preview = format!("{line}\n{}", preview.picture);
                self.image_overlay = inline::choose(&preview.sixel, &preview.kitty, &preview.iterm);
                self.images.push(preview.mark);
                if self.transcript_lines().is_some() {
                    self.post_say(&line);
                    self.cmd.clear();
                    self.note = format!("pasted  {token}");
                } else {
                    self.cmd = line;
                    self.page.clear();
                    self.page_url.clear();
                    self.clear_web();
                    self.section = Section::Browser;
                    self.mode = Mode::Desk;
                }
            }
            Err(err) => self.note = err,
        }
    }

    fn clear_picture(&mut self) {
        self.shown_image = None;
        self.outline.clear();
        self.image_scroll = 0;
    }

    fn toggle_outline(&mut self, n: u16) {
        if self.shown_image == Some(n) {
            self.clear_picture();
            return;
        }
        let Some(path) = self
            .images
            .iter()
            .find(|mark| mark.n == usize::from(n))
            .map(|mark| mark.path.clone())
        else {
            self.note = "image missing".into();
            return;
        };
        let (cols, rows) = self.picture_budget();
        match draw::portrait(&path, cols, rows) {
            Ok(picture) => {
                self.doc_open = false;
                self.keys_open = false;
                self.settings_open = false;
                self.shown_image = Some(n);
                self.outline = picture;
                self.image_scroll = 0;
                self.note = format!("image  {n}");
            }
            Err(err) => self.note = err,
        }
    }

    pub(crate) fn picture_budget(&self) -> (usize, usize) {
        let cols = self.cols.saturating_sub(8).clamp(24, 220);
        let rows = self.rows.saturating_sub(10).clamp(8, 64);
        match self.image_quality.as_str() {
            "low" => (cols.min(64), rows.min(18)),
            "medium" => (cols.min(140), rows.min(40)),
            _ => (cols.min(220), rows.min(64)),
        }
    }

    pub(crate) fn image_scroll(&self) -> usize {
        self.image_scroll
    }

    pub(crate) fn more_above(&self) -> bool {
        self.hist_from > 0
    }

    pub(crate) fn look_name(&self) -> &str {
        &self.look
    }

    pub(crate) fn filled(&self) -> bool {
        self.fill
    }

    pub fn apply_font(&self) {
        crate::term::apply_font(&self.font_face, self.font_size);
    }

    fn picture_box(&self) -> (usize, usize) {
        let cols = self.cols.max(40);
        let rows = self.rows.max(16);
        let left_max = cols.saturating_sub(40);
        let left = if cols < 72 || left_max < 36 {
            0
        } else {
            (cols / 2).clamp(36, left_max)
        };
        let width = cols.saturating_sub(left).saturating_sub(3).clamp(16, 160);
        let height = rows.saturating_sub(10).clamp(8, 44);
        (width, height)
    }

    fn note_new_shots(&mut self) {
        // A live turn appends lines. Scanning the whole thread on each one
        // stats every path again and stalls the click.
        let (len, restart, view) = {
            let Some(lines) = self.transcript_lines() else {
                return;
            };
            let len = lines.len();
            if len == self.shot_seen {
                return;
            }
            let restart = len < self.shot_seen;
            let from = if restart { 0 } else { self.shot_seen };
            let view = if lines[from..]
                .iter()
                .any(|line| line.contains("<oai-mem-citation>"))
            {
                crate::ui::present_pane(&lines[from..])
            } else {
                lines[from..].to_vec()
            };
            (len, restart, view)
        };
        if restart {
            self.opens.clear();
        }
        self.shot_seen = len;
        self.note_pictures(&view);
        if self.opens.len() < 24 {
            self.note_opens(&view);
        }
    }

    fn note_opens(&mut self, lines: &[String]) {
        let roots = self.file_roots();
        for line in lines {
            if self.opens.len() >= 24 {
                break;
            }
            for (_, _, cited) in crate::ui::file_spans(line, &roots) {
                if self
                    .opens
                    .iter()
                    .any(|open| open.path == cited.path && open.line == cited.line)
                {
                    continue;
                }
                if self.opens.len() >= 24 {
                    break;
                }
                let n = u16::try_from(self.opens.len() + 1).unwrap_or(u16::MAX);
                self.opens.push(OpenFile {
                    n,
                    path: cited.path,
                    line: cited.line,
                });
            }
        }
    }

    pub(crate) fn open_number(&self, path: &Path, line: usize) -> u16 {
        self.opens
            .iter()
            .find(|open| open.path == path && open.line == line)
            .map(|open| open.n)
            .unwrap_or(0)
    }

    pub(crate) fn file_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(cwd) = self.live_cwd() {
            let path = PathBuf::from(cwd);
            if path.is_dir() {
                roots.push(path);
            }
        }
        if !roots.iter().any(|have| have == &self.dir) {
            roots.push(self.dir.clone());
        }
        roots
    }

    fn open_cited(&mut self, n: u16) {
        let Some((path, line_no)) = self
            .opens
            .iter()
            .find(|open| open.n == n)
            .map(|open| (open.path.clone(), open.line))
        else {
            self.note = "file missing".into();
            return;
        };
        if !path.is_file() {
            self.note = "file missing".into();
            return;
        }
        self.open_doc_at(path);
        if self.doc_open && line_no > 0 {
            let at = line_no.saturating_sub(1);
            self.doc_scroll = at.min(self.doc_lines.len().saturating_sub(1));
        }
    }

    fn note_pictures(&mut self, lines: &[String]) {
        for line in lines {
            for (_, _, path) in crate::ui::picture_paths(line) {
                let known = self.images.iter().any(|mark| mark.path == path);
                if !known {
                    if self.images.len() >= 12 {
                        self.remember_thumb(&path);
                        continue;
                    }
                    let n = self.images.len() + 1;
                    let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    let format = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("image")
                        .to_ascii_lowercase();
                    self.images.push(crate::model::ImageMark {
                        n,
                        format,
                        width: 0,
                        height: 0,
                        bytes,
                        path: path.clone(),
                    });
                }
                self.remember_thumb(&path);
            }
        }
    }

    pub(crate) fn web_on(&self) -> bool {
        self.web_on
    }

    pub(crate) fn web_hits(&self) -> &[crate::engine::WebHit] {
        &self.web_hits
    }

    pub(crate) fn web_lines(&self) -> &[String] {
        &self.page
    }

    pub(crate) fn web_focus(&self) -> Option<u16> {
        self.web_focus
    }

    pub(crate) fn web_draft(&self) -> &str {
        &self.web_draft
    }

    /// Dark desk slabs belong on a dark page. Light follows the chip, and auto
    /// follows the system, so a light Google page keeps its own paper.
    pub(crate) fn web_scheme_dark(&self) -> bool {
        scheme_is_dark(&self.web_theme, system_dark())
    }

    fn field_keys(&self) -> bool {
        self.web_focus.is_some() && self.section == Section::Browser
    }

    fn clear_web_focus(&mut self) {
        self.web_focus = None;
        self.web_draft.clear();
    }

    /// A button drawn over the search box still belongs to the field under it.
    fn field_under(&self, index: u16) -> u16 {
        let Some(hit) = self.web_hits.get(usize::from(index)) else {
            return index;
        };
        if hit.kind == crate::engine::KIND_FIELD {
            return index;
        }
        let cx = hit.x + hit.w / 2;
        let cy = hit.y + hit.h / 2;
        self.web_hits
            .iter()
            .enumerate()
            .find(|(_, other)| {
                other.kind == crate::engine::KIND_FIELD
                    && cx >= other.x
                    && cy >= other.y
                    && cx < other.x.saturating_add(other.w)
                    && cy < other.y.saturating_add(other.h)
            })
            .map(|(at, _)| at as u16)
            .unwrap_or(index)
    }

    pub(crate) fn web_panel(&self) -> bool {
        self.web_panel
    }

    pub(crate) fn web_js_on(&self) -> bool {
        self.web_js
    }

    pub(crate) fn web_live_on(&self) -> bool {
        self.web_live
    }

    pub(crate) fn page_marked(&self) -> bool {
        let url = self.page_url.as_str();
        (url.starts_with("http://") || url.starts_with("https://"))
            && self.bookmarks.iter().any(|item| item == url)
    }

    pub(crate) fn web_size_label(&self) -> String {
        compact_bytes(self.web_bytes)
    }

    pub(crate) fn web_net_label(&self) -> String {
        compact_bytes(self.web_traffic)
    }

    pub(crate) fn web_busy(&self) -> bool {
        self.web_busy
    }

    pub(crate) fn browse_hot(&self) -> Option<u8> {
        self.browse_hot
    }

    pub(crate) fn web_hot(&self) -> Option<u16> {
        self.web_hot
    }

    pub(crate) fn keys_open(&self) -> bool {
        self.keys_open
    }

    pub(crate) fn keys_query(&self) -> &str {
        &self.keys_query
    }

    pub(crate) fn keys_finding(&self) -> bool {
        self.keys_find
    }

    pub(crate) fn keys_at(&self) -> usize {
        self.keys_at
    }

    pub(crate) fn settings_open(&self) -> bool {
        self.settings_open
    }

    pub(crate) fn settings_query(&self) -> &str {
        &self.settings_query
    }

    pub(crate) fn settings_finding(&self) -> bool {
        self.settings_find
    }

    pub(crate) fn settings_at(&self) -> usize {
        self.settings_at
    }

    pub(crate) fn key_rows(&self) -> Vec<KeyRow> {
        let query = self.keys_query.to_lowercase();
        let mut rows = Vec::new();
        for (index, group) in KEYS.iter().enumerate() {
            let name_hit = query.is_empty() || group.name.to_lowercase().contains(&query);
            let items: Vec<&KeyItem> = group
                .items
                .iter()
                .filter(|item| {
                    query.is_empty()
                        || name_hit
                        || item.label.to_lowercase().contains(&query)
                        || item.shortcut.to_lowercase().contains(&query)
                })
                .collect();
            if items.is_empty() && !name_hit {
                continue;
            }
            let open = !query.is_empty() || self.keys_bits & (1 << index) != 0;
            rows.push(KeyRow {
                label: if open {
                    group.name.into()
                } else {
                    format!("{} ({})", group.name, group.items.len())
                },
                shortcut: String::new(),
                group: true,
                open,
                detail: String::new(),
                id: String::new(),
                group_at: index,
            });
            if open {
                for item in items {
                    rows.push(KeyRow {
                        label: format!("  {}", item.label),
                        shortcut: item.shortcut.into(),
                        group: false,
                        open: false,
                        detail: item.detail.into(),
                        id: String::new(),
                        group_at: index,
                    });
                }
            }
        }
        rows
    }

    pub(crate) fn setting_rows(&self) -> Vec<KeyRow> {
        let query = self.settings_query.to_lowercase();
        let theme = self.theme_label();
        let lang = self.lang_label();
        let accept = self.shown_accept().label();
        let choices = |prefix: &str, names: &[&str], current: &str| -> Vec<(String, String, String)> {
            names
                .iter()
                .map(|name| {
                    let mark = if *name == current { "on" } else { "" };
                    ((*name).to_string(), format!("{prefix}{name}"), mark.to_string())
                })
                .collect()
        };
        let look = self.look.as_str();
        let face = self.font_face.as_str();
        let size = self.font_size.to_string();
        let quality = self.image_quality.as_str();
        let catalog = [
            (
                "Look",
                choices(
                    "look:",
                    &["cyberpunk", "cyberdeck", "parchment", "ink", "dos", "linux"],
                    look,
                ),
            ),
            (
                "Font",
                {
                    let mut rows = choices(
                        "face:",
                        &["Consolas", "Cascadia Mono", "Lucida Console"],
                        face,
                    );
                    rows.extend(choices("size:", &["12", "16", "20", "24"], &size));
                    rows
                },
            ),
            (
                "Picture",
                choices("quality:", &["low", "medium", "high"], quality),
            ),
            (
                "Theme",
                choices("theme:", &["auto", "dark", "light"], theme),
            ),
            (
                "Language",
                choices(
                    "lang:",
                    &["auto", "en", "ru", "uk", "he", "ar", "de", "fr", "es"],
                    lang,
                ),
            ),
            (
                "Browser",
                vec![
                    (
                        "JavaScript".into(),
                        "js".into(),
                        if self.web_js { "on" } else { "off" }.into(),
                    ),
                    (
                        "Live".into(),
                        "live".into(),
                        if self.web_live { "on" } else { "off" }.into(),
                    ),
                ],
            ),
            (
                "Desk",
                choices(
                    "accept:",
                    &["ask", "safe", "except-push", "except-destructive", "full"],
                    accept,
                ),
            ),
        ];
        let mut rows = Vec::new();
        for (index, (name, items)) in catalog.into_iter().enumerate() {
            let name_hit = query.is_empty() || name.to_lowercase().contains(&query);
            let shown: Vec<_> = items
                .into_iter()
                .filter(|(label, id, shortcut)| {
                    query.is_empty()
                        || name_hit
                        || label.to_lowercase().contains(&query)
                        || id.to_lowercase().contains(&query)
                        || shortcut.to_lowercase().contains(&query)
                })
                .collect();
            if shown.is_empty() && !name_hit {
                continue;
            }
            let open = !query.is_empty() || self.settings_bits & (1 << index) != 0;
            rows.push(KeyRow {
                label: if open {
                    name.into()
                } else {
                    format!("{name} ({})", shown.len())
                },
                shortcut: String::new(),
                group: true,
                open,
                detail: String::new(),
                id: String::new(),
                group_at: index,
            });
            if open {
                for (label, id, shortcut) in shown {
                    rows.push(KeyRow {
                        label: format!("  {label}"),
                        shortcut,
                        group: false,
                        open: false,
                        detail: String::new(),
                        id,
                        group_at: index,
                    });
                }
            }
        }
        rows
    }

    fn page_rows(&self) -> usize {
        let hist = self.history_band().len().min(2);
        let asks = self.ask_lines().len().min(3);
        self.rows.saturating_sub(7 + hist + asks).max(8)
    }

    fn web_scroll_keys(&self) -> bool {
        self.web_on
            && !self.web_panel
            && self.section == Section::Browser
            && self.cmd.is_empty()
            && !self.fill
            && self.transcript_lines().is_none()
    }

    fn open_web(&mut self, url: &str, push_hist: bool) -> bool {
        self.arm_web_look();
        self.engine.set_policy(self.web_js, self.web_live);
        match self.engine.show(url, self.cols, self.page_rows()) {
            Ok(view) => {
                if push_hist {
                    self.remember_page();
                }
                self.install_web(view);
                true
            }
            Err(err) => {
                self.note = err;
                false
            }
        }
    }

    fn install_web(&mut self, view: crate::engine::WebView) {
        self.stash_chat();
        self.vim_path = None;
        self.mode = Mode::Desk;
        self.section = Section::Browser;
        self.fill = false;
        self.cursor = 0;
        self.scroll = 0;
        self.desc_open = false;
        self.clear_web_focus();
        self.web_on = true;
        self.web_hits = view.hits;
        self.page = view.lines;
        self.web_panel = false;
        if !view.url.is_empty() {
            self.page_url = view.url.clone();
        }
        self.take_traffic();
        self.note = self.web_status();
        let url = self.page_url.clone();
        self.note_visit(&url);
    }

    fn take_traffic(&mut self) {
        match self.engine.traffic() {
            Ok((page, net)) => {
                if page == 0 && net == 0 {
                    if let Ok(bytes) = self.engine.document_bytes()
                        && bytes > 0
                    {
                        self.web_bytes = bytes;
                        self.web_traffic = bytes;
                        return;
                    }
                }
                self.web_bytes = page;
                self.web_traffic = net;
            }
            Err(_) => {
                if let Ok(bytes) = self.engine.document_bytes()
                    && bytes > 0
                {
                    self.web_bytes = bytes;
                    self.web_traffic = bytes;
                    return;
                }
                self.web_bytes = 0;
                self.web_traffic = 0;
            }
        }
    }

    fn remember_page(&mut self) {
        self.push_back();
        self.page_fwd.clear();
    }

    fn push_back(&mut self) {
        if self.page.is_empty() && self.page_url.is_empty() {
            return;
        }
        self.page_hist
            .push((self.page_url.clone(), self.page.clone()));
        if self.page_hist.len() > 8 {
            self.page_hist.remove(0);
        }
    }

    fn push_forward(&mut self) {
        if self.page.is_empty() && self.page_url.is_empty() {
            return;
        }
        self.page_fwd
            .push((self.page_url.clone(), self.page.clone()));
        if self.page_fwd.len() > 8 {
            self.page_fwd.remove(0);
        }
    }

    fn note_nav(&mut self, url: String, lines: Vec<String>) {
        if url.is_empty() && lines.is_empty() {
            return;
        }
        self.page_hist.push((url, lines));
        if self.page_hist.len() > 8 {
            self.page_hist.remove(0);
        }
        self.page_fwd.clear();
    }

    fn redraw_web(&mut self, capture: bool) {
        if !self.web_on {
            return;
        }
        let focus = self.web_focus.and_then(|index| {
            self.web_hits.get(usize::from(index)).map(|hit| {
                (hit.px, hit.py, hit.label.clone())
            })
        });
        let cols = self.cols;
        let rows = self.page_rows();
        let previous = self.page_url.clone();
        match self.engine.refresh(cols, rows, capture) {
            Ok(view) => {
                self.page = view.lines;
                self.web_hits = view.hits;
                if !view.url.is_empty() {
                    self.page_url = view.url;
                }
                if capture {
                    let url = self.page_url.clone();
                    if url != previous {
                        self.note_visit(&url);
                    }
                    self.take_traffic();
                    self.note = self.web_status();
                }
                self.web_focus = focus.and_then(|(px, py, label)| {
                    let mut best: Option<(usize, i32)> = None;
                    for (index, hit) in self.web_hits.iter().enumerate() {
                        if hit.kind != crate::engine::KIND_FIELD {
                            continue;
                        }
                        let dist = (hit.px - px).abs() + (hit.py - py).abs();
                        if dist < 160 && best.is_none_or(|(_, have)| dist < have) {
                            best = Some((index, dist));
                        }
                    }
                    if let Some((index, _)) = best {
                        return Some(index as u16);
                    }
                    if label.is_empty() {
                        return None;
                    }
                    let matches: Vec<usize> = self
                        .web_hits
                        .iter()
                        .enumerate()
                        .filter(|(_, hit)| {
                            hit.kind == crate::engine::KIND_FIELD && hit.label == label
                        })
                        .map(|(index, _)| index)
                        .collect();
                    if matches.len() == 1 {
                        Some(matches[0] as u16)
                    } else {
                        None
                    }
                });
                if self.web_focus.is_none() {
                    self.web_draft.clear();
                }
            }
            Err(err) => self.note = err,
        }
    }

    fn press_web(&mut self, index: u16) {
        let Some(hit) = self.web_hits.get(usize::from(index)).cloned() else {
            return;
        };
        if hit.kind == crate::engine::KIND_FIELD {
            self.web_focus = Some(index);
            self.web_draft.clear();
            if let Err(err) = self.engine.focus_at(hit.px, hit.py) {
                self.note = err;
                return;
            }
            self.note = if hit.label.is_empty() {
                "field".into()
            } else {
                hit.label
            };
            return;
        }
        if !self.web_live && hit.kind == crate::engine::KIND_LINK {
            self.note = "live off".into();
            return;
        }
        let before_url = self.page_url.clone();
        let before_lines = self.page.clone();
        if let Err(err) = self.engine.click(hit.px, hit.py) {
            self.note = err;
            return;
        }
        if self.web_live {
            let _ = self.engine.wait();
        }
        self.clear_web_focus();
        self.redraw_web(true);
        if !before_url.is_empty() && self.page_url != before_url {
            self.note_nav(before_url, before_lines);
        }
    }

    fn type_web(&mut self, ch: char) {
        self.web_draft.push(ch);
        if let Err(err) = self.engine.insert(&ch.to_string()) {
            self.note = err;
        }
        self.redraw_web(false);
    }

    fn web_key(&mut self, key: &str, code: i32) {
        if key == "Backspace" {
            self.web_draft.pop();
        }
        if let Err(err) = self.engine.key(key, code) {
            self.note = err;
            return;
        }
        self.redraw_web(false);
    }

    fn submit_web(&mut self) {
        if !self.web_live {
            self.note = "live off".into();
            return;
        }
        let before_url = self.page_url.clone();
        let before_lines = self.page.clone();
        if let Err(err) = self.engine.key("Enter", 13) {
            self.note = err;
            return;
        }
        let _ = self.engine.wait();
        self.clear_web_focus();
        self.redraw_web(true);
        if !before_url.is_empty() && self.page_url != before_url {
            self.note_nav(before_url, before_lines);
        }
    }

    fn scroll_web(&mut self, delta: i32) {
        if let Err(err) = self.engine.wheel(delta) {
            self.note = err;
            return;
        }
        self.redraw_web(false);
    }

    fn clear_web(&mut self) {
        self.web_on = false;
        self.clear_web_focus();
        self.web_hits.clear();
        self.web_panel = false;
        self.web_bytes = 0;
        self.web_traffic = 0;
    }

    fn browse(&mut self, target: &str) {
        self.image_overlay.clear();
        let target = target.trim();
        if target.is_empty() {
            self.stash_chat();
            self.section = Section::Browser;
            self.mode = Mode::Desk;
            self.cursor = 0;
            return;
        }
        let resolved = if let Some(url) = page::web_url(target, &self.page_base) {
            url
        } else {
            self.resolve_href(target)
        };
        let web = resolved.starts_with("http://") || resolved.starts_with("https://");
        if web && self.open_web(&resolved, true) {
            return;
        }
        let edge = if web {
            let note = self.note.clone();
            self.engine.close();
            self.clear_web();
            note
        } else {
            self.clear_web();
            String::new()
        };
        match page::load_doc(&resolved, &self.page_base) {
            Ok(doc) => {
                if !self.page.is_empty() {
                    let previous = std::mem::take(&mut self.page);
                    self.note_nav(self.page_url.clone(), previous);
                }
                let mut lines = doc.lines;
                if resolved.starts_with("http://") || resolved.starts_with("https://") {
                    self.page_url = resolved.clone();
                    self.note = resolved.clone();
                    self.note_visit(&resolved);
                } else {
                    let path = PathBuf::from(&resolved);
                    if let Some(parent) = path.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        self.page_base = parent.to_path_buf();
                    }
                    if let Some(banner) = self.unhealthy_banner(&path) {
                        self.note = "unhealthy".into();
                        lines.insert(0, banner);
                    } else {
                        self.note.clear();
                    }
                    self.page_url.clear();
                }
                self.web_bytes = doc.bytes;
                self.web_traffic = doc.bytes;
                self.page = lines.clone();
                self.open_split(lines, None);
                self.section = Section::Browser;
                self.cursor = 0;
                if !edge.is_empty() {
                    self.note = format!("{edge}  {}", compact_bytes(doc.bytes));
                }
            }
            Err(err) => {
                self.note = if edge.is_empty() {
                    err
                } else {
                    format!("{edge}  {err}")
                };
            }
        }
    }

    fn unhealthy_banner(&self, path: &Path) -> Option<String> {
        let root = mesh::git_root(&self.dir)?;
        let rel = health::rel(&root, path)?;
        if self.mesh_board.is_broken(&rel) {
            Some(format!("\x1b[38;5;196munhealthy\x1b[38;5;203m  {rel}"))
        } else {
            None
        }
    }

    fn browse_back(&mut self) {
        if self.web_panel {
            self.web_panel = false;
            self.note = "page".into();
            return;
        }
        let Some((url, lines)) = self.page_hist.pop() else {
            self.note = "No previous page.".into();
            return;
        };
        self.push_forward();
        self.show_stacked(&url, lines);
    }

    fn browse_forward(&mut self) {
        self.web_panel = false;
        let Some((url, lines)) = self.page_fwd.pop() else {
            self.note = "No next page.".into();
            return;
        };
        self.push_back();
        self.show_stacked(&url, lines);
    }

    fn show_stacked(&mut self, url: &str, lines: Vec<String>) {
        if (url.starts_with("http://") || url.starts_with("https://")) && self.open_web(url, false) {
            return;
        }
        self.clear_web();
        self.page_url = url.to_string();
        self.page = lines.clone();
        self.open_split(lines, None);
        self.section = Section::Browser;
        self.cursor = 0;
        self.note = if url.is_empty() {
            "back".into()
        } else {
            url.to_string()
        };
    }

    fn browse_reload(&mut self) {
        if !self.web_on && self.page_url.is_empty() {
            self.note = "no page".into();
            return;
        }
        self.web_panel = false;
        self.note = "loading".into();
        self.queue_web(WebAct::Reload);
    }

    fn reload_page(&mut self) {
        self.web_panel = false;
        if self.web_on {
            self.engine.set_policy(self.web_js, self.web_live);
            match self.engine.reload() {
                Ok(()) => {
                    self.redraw_web(true);
                    self.note = format!("reloaded  {}", self.web_size_label());
                }
                Err(err) => {
                    let url = self.page_url.clone();
                    if url.is_empty() {
                        self.note = err;
                        return;
                    }
                    self.engine.close();
                    self.web_on = false;
                    self.web_hits.clear();
                    self.clear_web_focus();
                    self.show_text_page(&url);
                    if self.note.starts_with("reloaded") {
                        self.note = format!("{err}  {}", self.web_size_label());
                    }
                }
            }
            return;
        }
        let url = self.page_url.clone();
        if url.is_empty() {
            self.note = "no page".into();
            return;
        }
        self.show_text_page(&url);
    }

    fn show_text_page(&mut self, target: &str) {
        match page::load_doc(target, &self.page_base) {
            Ok(doc) => {
                self.web_bytes = doc.bytes;
                self.web_traffic = doc.bytes;
                self.page = doc.lines.clone();
                if target.starts_with("http://") || target.starts_with("https://") {
                    self.page_url = target.to_string();
                }
                self.open_split(doc.lines, None);
                self.section = Section::Browser;
                self.cursor = 0;
                self.note = format!("reloaded  {}", compact_bytes(doc.bytes));
            }
            Err(err) => self.note = err,
        }
    }

    fn arm_browse(&mut self, kind: u8) {
        match kind {
            0 => {
                if self.web_panel {
                    self.web_panel = false;
                    self.note = "page".into();
                    return;
                }
                if self.page_hist.is_empty() {
                    self.note = "No previous page.".into();
                    return;
                }
                self.note = "loading".into();
                self.queue_web(WebAct::Back);
            }
            1 => {
                if self.page_fwd.is_empty() {
                    self.note = "No next page.".into();
                    return;
                }
                self.note = "loading".into();
                self.queue_web(WebAct::Forward);
            }
            2 => self.browse_reload(),
            3 => self.toggle_mark(),
            4 => self.toggle_panel(),
            5 => self.toggle_js(),
            6 => self.toggle_live(),
            _ => {}
        }
    }

    fn queue_web(&mut self, act: WebAct) {
        self.web_job = Some(act);
        self.web_busy = true;
    }

    fn toggle_keys(&mut self) {
        self.settings_open = false;
        self.settings_find = false;
        self.clear_picture();
        self.keys_open = !self.keys_open;
        self.keys_find = false;
        self.keys_query.clear();
        self.keys_at = 0;
        if self.keys_open {
            self.pick = None;
            self.doc_open = false;
            self.note = "shortcuts".into();
        }
    }

    fn open_settings(&mut self) {
        self.keys_open = false;
        self.keys_find = false;
        self.pick = None;
        self.doc_open = false;
        self.clear_picture();
        self.settings_open = true;
        self.settings_find = false;
        self.settings_query.clear();
        self.settings_at = 0;
        self.note = "settings".into();
    }

    fn toggle_settings(&mut self) {
        if self.settings_open {
            self.settings_open = false;
            self.settings_find = false;
            return;
        }
        self.open_settings();
    }

    fn on_keys_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                if self.keys_find || !self.keys_query.is_empty() {
                    self.keys_find = false;
                    self.keys_query.clear();
                    self.keys_at = 0;
                    self.note = "shortcuts".into();
                    return Effect::None;
                }
                self.keys_open = false;
                Effect::None
            }
            Key::Up => {
                self.keys_move(-1);
                Effect::None
            }
            Key::Down => {
                self.keys_move(1);
                Effect::None
            }
            Key::Left => {
                self.keys_collapse();
                Effect::None
            }
            Key::Right => {
                self.keys_expand();
                Effect::None
            }
            Key::Enter => {
                self.keys_activate();
                Effect::None
            }
            Key::Backspace if self.keys_find => {
                self.keys_query.pop();
                self.keys_at = 0;
                Effect::None
            }
            Key::Char('?') if !self.keys_find => {
                self.keys_open = false;
                Effect::None
            }
            Key::Char(',') if !self.keys_find => {
                self.keys_open = false;
                self.open_settings();
                Effect::None
            }
            Key::Char('/') | Key::Char('f') if !self.keys_find => {
                self.keys_find = true;
                self.note = "filter".into();
                Effect::None
            }
            Key::Char('e') | Key::Char(' ') if !self.keys_find => {
                self.keys_expand();
                Effect::None
            }
            Key::Char(ch) if self.keys_find => {
                self.keys_query.push(ch);
                self.keys_at = 0;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn keys_move(&mut self, delta: i32) {
        let len = self.key_rows().len();
        if len == 0 {
            self.keys_at = 0;
            return;
        }
        let next = self.keys_at as i32 + delta;
        self.keys_at = next.clamp(0, len as i32 - 1) as usize;
    }

    fn keys_current(&self) -> Option<KeyRow> {
        self.key_rows().into_iter().nth(self.keys_at)
    }

    fn keys_expand(&mut self) {
        if let Some(row) = self.keys_current() {
            self.keys_bits |= 1 << row.group_at;
        }
    }

    fn keys_collapse(&mut self) {
        if let Some(row) = self.keys_current() {
            self.keys_bits &= !(1 << row.group_at);
            self.keys_at = self
                .key_rows()
                .iter()
                .position(|item| item.group && item.group_at == row.group_at)
                .unwrap_or(0);
        }
    }

    fn keys_activate(&mut self) {
        let Some(row) = self.keys_current() else {
            return;
        };
        if row.group {
            self.keys_bits ^= 1 << row.group_at;
            return;
        }
        self.note = row.detail;
    }

    pub(crate) fn meter_open(&self) -> bool {
        self.meter_open
    }

    pub(crate) fn pick(&self) -> Option<crate::ui::Chip> {
        self.pick
    }

    pub(crate) fn pick_at(&self) -> usize {
        self.pick_at
    }

    pub(crate) fn pick_title(&self) -> &'static str {
        match self.pick {
            Some(crate::ui::Chip::Model) => "Model",
            Some(crate::ui::Chip::Effort) => "Effort",
            Some(crate::ui::Chip::Permit) => "Approval",
            None => "",
        }
    }

    pub(crate) fn pick_choices(&self) -> Vec<&'static str> {
        let agent = self.composer_agent();
        let list = match self.pick {
            Some(crate::ui::Chip::Model) => agent.models(),
            Some(crate::ui::Chip::Effort) => agent.efforts(),
            Some(crate::ui::Chip::Permit) => agent.permits(),
            None => return Vec::new(),
        };
        list.to_vec()
    }

    pub(crate) fn pick_current(&self) -> String {
        let face = self.composer_face();
        match self.pick {
            Some(crate::ui::Chip::Model) => face.model,
            Some(crate::ui::Chip::Effort) => face.effort,
            Some(crate::ui::Chip::Permit) => face.permit,
            None => String::new(),
        }
    }

    pub(crate) fn selection(&self) -> Option<(usize, usize, usize, usize)> {
        self.sel
    }

    pub fn keep_rows(&mut self, rows: Vec<String>) {
        self.kept = rows;
    }

    pub(crate) fn meter_tab(&self) -> u8 {
        self.meter_tab
    }

    pub(crate) fn meter_scroll(&self) -> usize {
        self.meter_scroll
    }

    pub(crate) fn open_folds(&self) -> &[usize] {
        &self.open_folds
    }

    pub(crate) fn tree_shut(&self) -> &[String] {
        &self.tree_shut
    }

    pub(crate) fn doc_open(&self) -> bool {
        self.doc_open
    }

    pub(crate) fn doc_title(&self) -> &str {
        &self.doc_title
    }

    pub(crate) fn doc_lines(&self) -> &[String] {
        &self.doc_lines
    }

    pub(crate) fn doc_scroll(&self) -> usize {
        self.doc_scroll
    }

    pub(crate) fn context_chip(&self) -> Option<String> {
        if self.section == Section::Browser {
            return None;
        }
        let chat = self.meter_chat()?;
        let used = chat.tokens?;
        let label = token_label(Some(used));
        match chat.window.filter(|n| *n > 0) {
            Some(window) => Some(format!("{label}/{}", token_label(Some(window)))),
            None => Some(label),
        }
    }

    pub(crate) fn meter_box(&self) -> Option<(usize, usize, usize)> {
        if self.cols < 40 || self.rows < 8 || self.section == Section::Browser {
            return None;
        }
        let label = self.context_chip()?;
        let w = label.chars().count() + 2;
        let face = self.composer_face();
        let lane = match face.lane {
            Some(name) if name != "say" => format!(" · {name}"),
            _ => String::new(),
        };
        let effort = if face.ultra {
            format!("{} · ultra", face.effort)
        } else {
            face.effort.clone()
        };
        let status = format!("{} · {effort} · {}{lane}", face.model, face.permit);
        let status_w = face.agent.label().chars().count() + 1 + status.chars().count();
        let x = self.cols.saturating_sub(status_w + w + 2);
        if x < 8 {
            return None;
        }
        Some((x, self.rows.saturating_sub(2), w))
    }

    pub(crate) fn hover_cell(&self) -> Option<(usize, usize)> {
        self.hover
    }

    pub(crate) fn tip_line(&self) -> Option<String> {
        if self.meter_open || self.pick.is_some() || self.doc_open {
            return None;
        }
        let (x, y, w) = self.meter_box()?;
        let (hx, hy) = self.hover?;
        let chat = self.meter_chat()?;
        let used = chat.tokens?;
        let text = match chat.window.filter(|n| *n > 0) {
            Some(window) => context_tip_line(used, window),
            None => format!(
                "{}   окно не записано   dashboard",
                token_label(Some(used))
            ),
        };
        let on_chip = hy == y && hx >= x && hx < x + w;
        let tip_w = text.chars().count();
        let tip_x = self.cols.saturating_sub(tip_w + 1);
        let on_tip = y > 0 && hy + 1 == y && hx >= tip_x && hx < tip_x + tip_w;
        if on_chip || on_tip { Some(text) } else { None }
    }

    #[allow(dead_code)]
    pub(crate) fn meter_head(&self) -> String {
        let tabs = ["Context usage", "Usage limit", "Session info"];
        let mut out = String::new();
        for (index, name) in tabs.iter().enumerate() {
            if index > 0 {
                out.push_str("  ");
            }
            if index == usize::from(self.meter_tab) {
                out.push_str(name);
            } else {
                out.push_str(name);
            }
        }
        out
    }

    pub(crate) fn meter_body(&self) -> Vec<String> {
        match self.meter_tab {
            1 => self.limit_lines(),
            2 => self.session_lines(),
            _ => self.context_lines(),
        }
    }

    fn meter_chat(&self) -> Option<&crate::model::Chat> {
        if !self.open_id.is_empty()
            && let Some(chat) = self.chats.iter().find(|chat| chat.id == self.open_id)
        {
            return Some(chat);
        }
        match self.row_at(self.cursor) {
            Some(DeskRow::Chat(index)) => self.chats.get(index),
            _ => None,
        }
    }

    fn open_meter(&mut self) {
        self.pick = None;
        self.doc_open = false;
        self.meter_open = true;
        self.meter_tab = 0;
        self.meter_scroll = 0;
        self.keys_open = false;
        self.settings_open = false;
        self.load_meter();
    }

    fn load_meter(&mut self) {
        let Some(chat) = self.meter_chat().cloned() else {
            return;
        };
        if self.meter_loaded == chat.id {
            return;
        }
        let facts = chats::context_facts(&chat.path);
        if let Some(slot) = self.chats.iter_mut().find(|item| item.id == chat.id) {
            if facts.used.is_some() {
                slot.tokens = facts.used;
            }
            if facts.window.is_some() {
                slot.window = facts.window;
            }
        }
        self.meter_model = facts.model;
        self.meter_turns = facts.turns;
        self.meter_compactions = facts.compactions;
        self.meter_before = facts.before;
        self.meter_tools = facts.tools;
        let used = facts.used.or(chat.tokens).unwrap_or(0);
        self.meter_split = chats::context_split(&chat.path, used);
        self.meter_loaded = chat.id;
    }

    fn context_lines(&self) -> Vec<String> {
        let Some(chat) = self.meter_chat() else {
            return vec!["No open chat".into()];
        };
        let used = chat.tokens;
        let window = chat.window.filter(|n| *n > 0);
        let mut lines = Vec::new();
        lines.push("Context".into());
        match (used, window) {
            (Some(used), Some(window)) => {
                let pct = used.saturating_mul(10_000) / window;
                lines.push(format!(
                    "{} / {} tokens ({}.{:02}%)",
                    chats::grok_token_face(used),
                    chats::grok_token_face(window),
                    pct / 100,
                    pct % 100
                ));
            }
            (Some(used), None) => {
                lines.push(format!("{} tokens", chats::grok_token_face(used)));
                lines.push("Context window is not in the session files.".into());
            }
            _ => lines.push("Context is not in the session files.".into()),
        }
        let model = if self.meter_model.is_empty() {
            self.model.clone()
        } else {
            self.meter_model.clone()
        };
        if !model.is_empty() {
            lines.push(model);
        }
        if let Some(used_n) = used {
            let split = &self.meter_split;
            let measured = split
                .system
                .saturating_add(split.messages)
                .saturating_add(split.reasoning);
            let system = split.system;
            let messages = if measured == 0 { used_n } else { split.messages };
            let reasoning = split.reasoning;
            let free = window.map(|size| size.saturating_sub(used_n));
            let whole = window.unwrap_or(used_n.max(1));
            lines.push(String::new());
            let parts: Vec<(u64, &str, char)> = if let Some(free) = free {
                vec![
                    (system, "38;5;81", '◆'),
                    (messages, "38;5;220", '◆'),
                    (reasoning, "38;5;141", '◆'),
                    (free, "38;5;240", '◇'),
                ]
            } else {
                vec![
                    (system, "38;5;81", '◆'),
                    (messages, "38;5;220", '◆'),
                    (reasoning, "38;5;141", '◆'),
                ]
            };
            lines.extend(diamond_rows(&parts));
            lines.push(String::new());
            if system > 0 {
                lines.push(bucket_row("38;5;81", '◆', "System prompt", system, whole, ""));
            }
            if messages > 0 {
                lines.push(bucket_row("38;5;220", '◆', "Messages", messages, whole, ""));
            }
            if reasoning > 0 {
                lines.push(bucket_row(
                    "38;5;141",
                    '◆',
                    "Reasoning/overhead",
                    reasoning,
                    whole,
                    "",
                ));
            }
            if let Some(free) = free {
                lines.push(bucket_row("38;5;240", '◇', "Free", free, whole, ""));
            }
            let mut subsets = Vec::new();
            if split.tools > 0 {
                subsets.push(bucket_row(
                    "38;5;81",
                    '◆',
                    "Tool definitions",
                    split.tools_tokens,
                    whole,
                    &count_phrase(split.tools, "tool", "tools"),
                ));
            }
            if split.skills > 0 {
                subsets.push(bucket_row(
                    "38;5;81",
                    '◆',
                    "Skills",
                    split.skills_tokens,
                    whole,
                    &count_phrase(split.skills, "skill", "skills"),
                ));
            }
            if split.flows > 0 {
                subsets.push(bucket_row(
                    "38;5;81",
                    '◆',
                    "Workflows",
                    split.flows_tokens,
                    whole,
                    &count_phrase(split.flows, "workflow", "workflows"),
                ));
            }
            if split.mcps > 0 {
                subsets.push(bucket_row(
                    "38;5;81",
                    '◆',
                    "MCP servers",
                    split.mcp_tokens,
                    whole,
                    &count_phrase(split.mcps, "server", "servers"),
                ));
            }
            if !subsets.is_empty() {
                lines.push(String::new());
                lines.push(" \x1b[38;5;245mAlready counted above".into());
                lines.extend(subsets);
            }
            if let Some(size) = window {
                let cap = size.saturating_mul(80) / 100;
                let left = cap.saturating_sub(used_n);
                lines.push(String::new());
                lines.push(format!(
                    "Auto-compact at 80% · ~{} tokens remaining",
                    chats::grok_token_face(left)
                ));
            }
        }
        lines.push(String::new());
        lines.push("[compact context]".into());
        lines
    }

    fn limit_lines(&self) -> Vec<String> {
        let mut lines = vec![
            "Usage limit".into(),
            "Weekly allowance is not in the session files.".into(),
        ];
        if let Some(n) = self.meter_turns {
            lines.push(format!("Turns  {n}"));
        }
        if let Some(n) = self.meter_compactions {
            lines.push(format!("Compactions  {n}"));
        }
        if let Some(n) = self.meter_before {
            lines.push(format!(
                "Tokens before compaction  {}",
                chats::grok_token_face(n)
            ));
        }
        if let Some(n) = self.meter_tools {
            lines.push(format!("Tool calls  {n}"));
        }
        lines
    }

    fn session_lines(&self) -> Vec<String> {
        let Some(chat) = self.meter_chat() else {
            return vec!["No session".into()];
        };
        let mut lines = vec![
            "Session info".into(),
            format!("id  {}", chat.id),
            format!("agent  {}", chat.agent.label()),
        ];
        let model = if self.meter_model.is_empty() {
            self.model.clone()
        } else {
            self.meter_model.clone()
        };
        if !model.is_empty() {
            lines.push(format!("model  {model}"));
        }
        if !chat.cwd.is_empty() {
            lines.push(format!("cwd  {}", chat.cwd));
        }
        lines
    }

    fn on_meter_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                self.meter_open = false;
                Effect::None
            }
            Key::Tab => {
                self.meter_tab = (self.meter_tab + 1) % 3;
                self.meter_scroll = 0;
                Effect::None
            }
            Key::Up => {
                self.meter_scroll = self.meter_scroll.saturating_sub(1);
                Effect::None
            }
            Key::Down => {
                self.meter_scroll = self.meter_scroll.saturating_add(1);
                Effect::None
            }
            Key::Char('c') | Key::Char('C') => {
                self.copy_session();
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn copy_session(&mut self) {
        let id = self
            .meter_chat()
            .map(|chat| chat.id.clone())
            .unwrap_or_default();
        if id.is_empty() {
            self.note = "no session".into();
            return;
        }
        if set_clipboard(&id) {
            self.note = "session copied".into();
        } else {
            self.note = id;
        }
    }

    fn compact_context(&mut self) {
        let Some(chat) = self.meter_chat().cloned() else {
            self.note = "no chat".into();
            return;
        };
        if chat.id.is_empty() {
            self.note = "not live".into();
            return;
        }
        if self.open_id == chat.id && self.chat_open() {
            self.post_say("/compact");
            return;
        }
        let sent = crate::deliver::send(&self.home, chat.agent, &chat.id, "/compact");
        self.note = if sent.note.is_empty() {
            "sent".into()
        } else {
            sent.note
        };
    }

    fn toggle_fold(&mut self, index: usize) {
        if let Some(at) = self.open_folds.iter().position(|item| *item == index) {
            self.open_folds.remove(at);
        } else {
            self.open_folds.push(index);
        }
    }

    fn on_doc_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                self.doc_open = false;
                Effect::None
            }
            Key::Up => {
                self.doc_scroll = self.doc_scroll.saturating_sub(1);
                Effect::None
            }
            Key::Down => {
                self.doc_scroll = self.doc_scroll.saturating_add(1);
                Effect::None
            }
            Key::Wheel(delta) | Key::Page(delta) => {
                if delta < 0 {
                    self.doc_scroll = self.doc_scroll.saturating_sub(delta.unsigned_abs() as usize);
                } else {
                    self.doc_scroll = self.doc_scroll.saturating_add(delta as usize);
                }
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn on_picture_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                self.clear_picture();
                Effect::None
            }
            Key::Up => {
                self.image_scroll = self.image_scroll.saturating_sub(1);
                Effect::None
            }
            Key::Down => {
                self.image_scroll = self.image_scroll.saturating_add(1);
                Effect::None
            }
            Key::Wheel(delta) | Key::Page(delta) => {
                if delta < 0 {
                    self.image_scroll = self
                        .image_scroll
                        .saturating_sub(delta.unsigned_abs() as usize);
                } else {
                    self.image_scroll = self.image_scroll.saturating_add(delta as usize);
                }
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn toggle_tree(&mut self, index: usize) {
        let width = self.transcript_pane_width();
        let before = self.folded_now();
        if index == usize::from(TREE_ALL) {
            if let Some(at) = self.tree_shut.iter().position(|item| item == "*") {
                self.tree_shut.remove(at);
            } else {
                self.tree_shut.push("*".into());
            }
            let after = self.folded_now();
            self.keep_opened_row(&before, &after, index, width);
            return;
        }
        let dir = self.transcript_lines().and_then(|lines| {
            let mut seen = 0usize;
            let mut index_at = 0usize;
            while index_at < lines.len() {
                let Some(dir) = lines[index_at].strip_prefix("folder  ") else {
                    index_at += 1;
                    continue;
                };
                if !folder_visible(&lines[index_at + 1..]) {
                    index_at += 1;
                    while index_at < lines.len() && lines[index_at].starts_with("stat  ") {
                        index_at += 1;
                    }
                    continue;
                }
                if seen == index {
                    return Some(dir.to_string());
                }
                seen += 1;
                index_at += 1;
            }
            None
        });
        let Some(dir) = dir else {
            return;
        };
        if let Some(at) = self.tree_shut.iter().position(|item| item == &dir) {
            self.tree_shut.remove(at);
        } else {
            self.tree_shut.push(dir);
        }
        let after = self.folded_now();
        self.keep_opened_row(&before, &after, index, width);
    }

    fn folded_now(&self) -> Vec<Folded> {
        match self.transcript_lines() {
            Some(lines) => fold_lines(lines, &self.open_folds, &self.tree_shut),
            None => Vec::new(),
        }
    }

    /// The row that was clicked stays at the top of the pane when the opened
    /// list is taller than the screen. A list above the viewport stays there.
    fn keep_opened_row(&mut self, before: &[Folded], after: &[Folded], index: usize, width: usize) {
        let Ok(mark) = u16::try_from(index) else {
            return;
        };
        let Some(v0) = visual_at(before, mark, width) else {
            return;
        };
        if visual_at(after, mark, width).is_none() {
            return;
        }
        let h0 = rows_high(before, width);
        let h1 = rows_high(after, width);
        let room = self.rows.saturating_sub(7).max(1);
        let from = self.pane_from_end;
        let view_end = h0.saturating_sub(from);
        let view_start = view_end.saturating_sub(room);
        if v0 < view_start || v0 >= view_end {
            return;
        }
        let next_start = h1.saturating_sub(from).saturating_sub(room);
        if v0 < next_start {
            self.pane_from_end = h1.saturating_sub(room).saturating_sub(v0);
        }
    }

    fn open_doc(&mut self, index: u16) {
        let Some(path) = self.doc_path_at(index) else {
            self.note = "file missing".into();
            return;
        };
        self.open_doc_at(path);
    }

    fn open_doc_at(&mut self, path: PathBuf) {
        self.pick = None;
        self.meter_open = false;
        self.keys_open = false;
        self.keys_find = false;
        self.settings_open = false;
        self.settings_find = false;
        self.clear_picture();
        if !path.is_file() {
            self.note = "file missing".into();
            self.doc_open = false;
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.note = "file is not text".into();
            self.doc_open = false;
            return;
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        let mut lines = Vec::new();
        for line in text.lines().take(4000) {
            let mut owned: String = line.chars().filter(|ch| *ch != '\u{1b}').take(400).collect();
            if line.chars().filter(|ch| *ch != '\u{1b}').count() > 400 {
                owned.push('…');
            }
            lines.push(owned);
        }
        if text.lines().nth(4000).is_some() {
            lines.push("…".into());
        }
        self.doc_title = name;
        self.doc_lines = lines;
        self.doc_scroll = 0;
        self.doc_open = true;
        self.note.clear();
    }

    fn doc_path_at(&self, index: u16) -> Option<PathBuf> {
        let lines = self.transcript_lines()?;
        let mut dir = String::new();
        let mut seen = 0u16;
        let mut hiding = false;
        for line in lines {
            if let Some(folder) = line.strip_prefix("folder  ") {
                dir = folder.to_string();
                hiding = self.tree_shut.iter().any(|item| item == folder);
                continue;
            }
            if hiding && line.starts_with("stat  ") {
                continue;
            }
            hiding = false;
            let Some(rest) = line.strip_prefix("stat  ") else {
                continue;
            };
            if matches!(stat_nums(rest), Some((0, 0))) {
                continue;
            }
            let name = rest.split("  ").next().unwrap_or("");
            if !name
                .rsplit_once('.')
                .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("md"))
            {
                continue;
            }
            if seen == index {
                return Some(self.resolve_doc(&dir, name));
            }
            seen = seen.saturating_add(1);
        }
        None
    }

    fn resolve_doc(&self, dir: &str, name: &str) -> PathBuf {
        let rel = if dir.is_empty() || dir == "." {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        };
        let roots = self.doc_roots();
        for root in &roots {
            let path = root.join(&rel);
            if path.is_file() {
                return path;
            }
        }
        if let Some((_, rest)) = rel.split_once('/') {
            for root in &roots {
                let path = root.join(rest);
                if path.is_file() {
                    return path;
                }
            }
        }
        roots
            .first()
            .map(|root| root.join(&rel))
            .unwrap_or_else(|| PathBuf::from(rel))
    }

    fn doc_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(cwd) = self.live_cwd() {
            if let Some(root) = mesh::git_root(Path::new(&cwd)) {
                roots.push(root);
            }
            let path = PathBuf::from(&cwd);
            if path.is_dir() && !roots.iter().any(|have| have == &path) {
                roots.push(path);
            }
        }
        if let Some(root) = mesh::git_root(&self.dir) {
            if !roots.iter().any(|have| have == &root) {
                roots.push(root);
            }
        }
        if !roots.iter().any(|have| have == &self.dir) {
            roots.push(self.dir.clone());
        }
        roots
    }

    fn on_settings_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                if self.settings_find || !self.settings_query.is_empty() {
                    self.settings_find = false;
                    self.settings_query.clear();
                    self.settings_at = 0;
                    self.note = "themes".into();
                    return Effect::None;
                }
                self.settings_open = false;
                Effect::None
            }
            Key::Up => {
                self.settings_move(-1);
                Effect::None
            }
            Key::Down => {
                self.settings_move(1);
                Effect::None
            }
            Key::Left => {
                self.settings_collapse();
                Effect::None
            }
            Key::Right => {
                self.settings_expand();
                Effect::None
            }
            Key::Enter => {
                self.settings_activate();
                Effect::None
            }
            Key::Backspace if self.settings_find => {
                self.settings_query.pop();
                self.settings_at = 0;
                Effect::None
            }
            Key::Char('?') if !self.settings_find => {
                self.settings_open = false;
                self.settings_find = false;
                self.keys_open = true;
                self.keys_find = false;
                self.keys_query.clear();
                self.keys_at = 0;
                self.note = "shortcuts".into();
                Effect::None
            }
            Key::Char(',') if !self.settings_find => {
                self.settings_open = false;
                self.settings_find = false;
                Effect::None
            }
            Key::Char('/') | Key::Char('f') if !self.settings_find => {
                self.settings_find = true;
                self.note = "filter".into();
                Effect::None
            }
            Key::Char('e') | Key::Char(' ') if !self.settings_find => {
                self.settings_expand();
                Effect::None
            }
            Key::Char(ch) if self.settings_find => {
                self.settings_query.push(ch);
                self.settings_at = 0;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn settings_move(&mut self, delta: i32) {
        let len = self.setting_rows().len();
        if len == 0 {
            self.settings_at = 0;
            return;
        }
        let next = self.settings_at as i32 + delta;
        self.settings_at = next.clamp(0, len as i32 - 1) as usize;
    }

    fn settings_current(&self) -> Option<KeyRow> {
        self.setting_rows().into_iter().nth(self.settings_at)
    }

    fn settings_expand(&mut self) {
        if let Some(row) = self.settings_current() {
            self.settings_bits |= 1 << row.group_at;
        }
    }

    fn settings_collapse(&mut self) {
        if let Some(row) = self.settings_current() {
            self.settings_bits &= !(1 << row.group_at);
            self.settings_at = self
                .setting_rows()
                .iter()
                .position(|item| item.group && item.group_at == row.group_at)
                .unwrap_or(0);
        }
    }

    fn settings_activate(&mut self) {
        let Some(row) = self.settings_current() else {
            return;
        };
        if row.group {
            self.settings_bits ^= 1 << row.group_at;
            return;
        }
        if let Some(name) = row.id.strip_prefix("theme:") {
            self.web_theme = name.to_string();
            self.push_web_look(true);
            return;
        }
        if let Some(name) = row.id.strip_prefix("look:") {
            self.look = name.to_string();
            self.note = format!("look {name}");
            self.persist();
            return;
        }
        if let Some(name) = row.id.strip_prefix("face:") {
            self.font_face = name.to_string();
            self.note = format!("font {name}");
            self.apply_font();
            self.persist();
            return;
        }
        if let Some(name) = row.id.strip_prefix("size:") {
            if let Ok(size) = name.parse::<u16>() {
                self.font_size = size;
                self.note = format!("font {size}");
                self.apply_font();
                self.persist();
            }
            return;
        }
        if let Some(name) = row.id.strip_prefix("quality:") {
            self.image_quality = name.to_string();
            self.note = format!("picture {name}");
            if let Some(n) = self.shown_image {
                self.shown_image = None;
                self.outline.clear();
                self.toggle_outline(n);
            }
            self.persist();
            return;
        }
        if let Some(name) = row.id.strip_prefix("lang:") {
            self.web_lang = name.to_string();
            self.push_web_look(true);
            return;
        }
        match row.id.as_str() {
            "js" => self.toggle_js(),
            "live" => self.toggle_live(),
            other => {
                if let Some(name) = other.strip_prefix("accept:")
                    && let Some(level) = AcceptLevel::parse(name)
                {
                    self.set_accept(level);
                    self.note = format!("accept {}", level.label());
                }
            }
        }
    }

    fn pull_browser_command(&mut self) {
        if self.web_job.is_some() {
            return;
        }
        let Some(line) = crate::mcp::take_browser_command(&self.home) else {
            return;
        };
        let mut parts = line.splitn(2, '\t');
        let cmd = parts.next().unwrap_or("").trim();
        let arg = parts.next().unwrap_or("").trim().to_string();
        self.section = Section::Browser;
        match cmd {
            "open" => {
                if arg.is_empty() {
                    crate::mcp::write_browser_ack(&self.home, "err\turl is required");
                    return;
                }
                self.note = "loading".into();
                self.queue_web(WebAct::Open(arg));
            }
            "back" => {
                self.browse_hot = Some(0);
                self.arm_browse(0);
            }
            "forward" => {
                self.browse_hot = Some(1);
                self.arm_browse(1);
            }
            "reload" => {
                self.browse_hot = Some(2);
                self.arm_browse(2);
            }
            "click" => match arg.parse::<u16>() {
                Ok(index) => {
                    self.web_hot = Some(index);
                    self.note = "loading".into();
                    self.queue_web(WebAct::Click(index));
                }
                Err(_) => crate::mcp::write_browser_ack(&self.home, "err\tclick needs a number"),
            },
            "type" => {
                if arg.is_empty() {
                    crate::mcp::write_browser_ack(&self.home, "err\ttext is required");
                    return;
                }
                self.note = "loading".into();
                self.queue_web(WebAct::Type(arg));
            }
            "key" => {
                self.note = "loading".into();
                self.queue_web(WebAct::Key(arg));
            }
            "js" => {
                self.browse_hot = Some(5);
                self.arm_browse(5);
            }
            "live" => {
                self.browse_hot = Some(6);
                self.arm_browse(6);
            }
            "mark" => {
                self.browse_hot = Some(3);
                self.toggle_mark();
                let note = self.note.clone();
                crate::mcp::write_browser_ack(&self.home, &format!("ok\t{note}"));
            }
            _ => crate::mcp::write_browser_ack(&self.home, "err\tunknown browser action"),
        }
        if self.web_job.is_none()
            && matches!(cmd, "back" | "forward" | "reload" | "js" | "live")
        {
            let note = if self.note.is_empty() {
                "ok".into()
            } else {
                self.note.clone()
            };
            let status = if note.starts_with("No ") || note.starts_with("no ") {
                "err"
            } else {
                "ok"
            };
            crate::mcp::write_browser_ack(&self.home, &format!("{status}\t{note}"));
        }
    }

    fn publish_browser(&mut self) {
        let body = self.browser_snapshot();
        if body == self.browser_pub {
            return;
        }
        self.browser_pub = body.clone();
        crate::mcp::write_browser_snapshot(&self.home, &body);
    }

    fn browser_snapshot(&self) -> String {
        let mut out = String::new();
        let stamp = unix_now();
        out.push_str(&format!("stamp\t{stamp}\n"));
        out.push_str(&format!("url\t{}\n", self.page_url));
        out.push_str(&format!("js\t{}\n", u8::from(self.web_js)));
        out.push_str(&format!("live\t{}\n", u8::from(self.web_live)));
        out.push_str(&format!("bytes\t{}\n", self.web_bytes));
        out.push_str(&format!("net\t{}\n", self.web_traffic));
        out.push_str(&format!("busy\t{}\n", u8::from(self.web_busy)));
        out.push_str(&format!("open\t{}\n", u8::from(self.web_on)));
        out.push_str("hits\n");
        for (index, hit) in self.web_hits.iter().enumerate() {
            let kind = match hit.kind {
                crate::engine::KIND_LINK => "link",
                crate::engine::KIND_FIELD => "field",
                crate::engine::KIND_BUTTON => "button",
                _ => "hit",
            };
            let label = one_line(&hit.label);
            let href = one_line(&hit.href);
            out.push_str(&format!(
                "{index}\t{kind}\t{}\t{}\t{}\t{}\t{label}\t{href}\n",
                hit.x, hit.y, hit.w, hit.h
            ));
        }
        out.push_str("text\n");
        for line in self.page.iter().take(80) {
            out.push_str(&one_line(&strip_sgr(line)));
            out.push('\n');
        }
        out
    }

    fn toggle_mark(&mut self) {
        let url = self.page_url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            self.note = "no page".into();
            return;
        }
        let url = url.to_string();
        if let Some(index) = self.bookmarks.iter().position(|item| item == &url) {
            self.bookmarks.remove(index);
            self.note = "mark removed".into();
        } else {
            self.bookmarks.insert(0, url);
            self.bookmarks.truncate(30);
            self.note = "marked".into();
        }
        self.persist();
    }

    fn toggle_panel(&mut self) {
        if self.web_panel {
            self.web_panel = false;
            self.note = "page".into();
            return;
        }
        self.clear_web_focus();
        self.web_panel = true;
        self.cursor = 0;
        self.scroll = 0;
        self.note = "history".into();
    }

    fn toggle_js(&mut self) {
        self.web_js = !self.web_js;
        self.engine.set_policy(self.web_js, self.web_live);
        self.persist();
        if self.web_on || !self.page_url.is_empty() {
            self.note = "loading".into();
            self.queue_web(WebAct::Reload);
        } else {
            self.note = format!("js {}", on_off(self.web_js));
        }
    }

    fn toggle_live(&mut self) {
        self.web_live = !self.web_live;
        self.engine.set_policy(self.web_js, self.web_live);
        self.persist();
        if self.web_on || !self.page_url.is_empty() {
            self.note = "loading".into();
            self.queue_web(WebAct::Reload);
        } else {
            self.note = format!("live {}", on_off(self.web_live));
        }
    }

    fn panel_entries(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for url in &self.bookmarks {
            let host = site_host(url).unwrap_or_else(|| url.clone());
            rows.push((format!("mark  {host}"), url.clone()));
        }
        for visit in self.ranked_visits() {
            let host = site_host(&visit.url).unwrap_or_else(|| visit.url.clone());
            rows.push((format!("{:>3}  {host}", visit.count), visit.url.clone()));
        }
        rows
    }

    fn panel_lines(&self) -> Vec<String> {
        let rows = self.panel_entries();
        if rows.is_empty() {
            vec!["No marks.".into()]
        } else {
            rows.into_iter().map(|(line, _)| line).collect()
        }
    }

    fn panel_url(&self, index: usize) -> Option<String> {
        self.panel_entries().into_iter().nth(index).map(|(_, url)| url)
    }

    fn note_visit(&mut self, url: &str) {
        let Some(host) = site_host(url) else {
            return;
        };
        let now = unix_now();
        if let Some(found) = self
            .visits
            .iter_mut()
            .find(|visit| site_host(&visit.url).as_deref() == Some(host.as_str()))
        {
            found.count = found.count.saturating_add(1);
            found.last = now;
            found.url = url.to_string();
        } else {
            self.visits.push(store::Visit {
                url: url.to_string(),
                count: 1,
                last: now,
            });
        }
        self.visits
            .sort_by(|left, right| right.count.cmp(&left.count).then(right.last.cmp(&left.last)));
        self.visits.truncate(30);
        self.persist();
    }

    fn ranked_visits(&self) -> Vec<&store::Visit> {
        let mut rows: Vec<&store::Visit> = self
            .visits
            .iter()
            .filter(|visit| site_host(&visit.url).is_some())
            .collect();
        rows.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then(right.last.cmp(&left.last))
                .then(left.url.cmp(&right.url))
        });
        rows
    }

    fn site_lines(&self) -> Vec<String> {
        let rows = self.ranked_visits();
        if rows.is_empty() {
            return vec!["browse a site".into()];
        }
        rows.into_iter()
            .map(|visit| {
                let host = site_host(&visit.url).unwrap_or_else(|| visit.url.clone());
                format!("{:>3}  {host}", visit.count)
            })
            .collect()
    }

    fn visit_url(&self, index: usize) -> Option<String> {
        self.ranked_visits()
            .get(index)
            .map(|visit| visit.url.clone())
    }

    fn home_sites(&self) -> bool {
        self.section == Section::Browser
            && !self.web_on
            && self.page.is_empty()
            && self.preview.is_empty()
    }

    fn activate_browser(&mut self) {
        if self.web_panel {
            if let Some(url) = self.panel_url(self.cursor) {
                self.web_panel = false;
                self.browse(&url);
            }
            return;
        }
        if self.home_sites() {
            if let Some(url) = self.visit_url(self.cursor) {
                self.browse(&url);
            }
            return;
        }
        let line = self.browser_line();
        if let Some(href) = line.as_deref().and_then(link_target) {
            self.browse(href);
            return;
        }
        if !self.page.is_empty() && self.mode_is_desk() {
            self.open_split(self.page.clone(), None);
        }
    }

    fn browser_line(&self) -> Option<String> {
        let lines = match &self.mode {
            Mode::Split { lines, .. } if self.section == Section::Browser => lines.clone(),
            _ => self.page.clone(),
        };
        lines.get(self.cursor).cloned()
    }

    fn resolve_href(&self, href: &str) -> String {
        let href = href.trim();
        if href.starts_with("http://")
            || href.starts_with("https://")
            || Path::new(href).is_absolute()
        {
            return href.to_string();
        }
        if !self.page_url.is_empty() {
            return join_url(&self.page_url, href);
        }
        self.page_base.join(href).display().to_string()
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
                self.stash_chat();
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
            self.reveal_list_cursor();
        }
        let chat = self.chats[position].clone();
        self.show_chat(&chat, false);
        Effect::None
    }

    fn show_chat(&mut self, chat: &crate::model::Chat, context: bool) {
        if self.open_id != chat.id {
            self.live_said.clear();
            self.live_said_at = 0;
            // chat_body pastes this buffer. It is the previous chat's stream.
            self.live_reply.clear();
            self.live_at = None;
            // The diff opens as one total. A click expands the files.
            self.tree_shut.clear();
            self.tree_shut.push("*".into());
            self.doc_open = false;
        }
        self.open_id = chat.id.clone();
        self.held = None;
        self.sync_clock(&chat.path);
        let key = format!("{}:{context}", chat.path.display());
        if self.pane_key == key && self.pane().is_some() {
            return;
        }
        let body = self.chat_body(chat, context);
        self.open_split(dress(chat.agent, &chat.title, body), Some(chat.agent));
        self.shot_seen = 0;
        self.pane_key = key;
        self.scroll = usize::MAX;
        self.pane_from_end = 0;
        self.open_stamp = chats::transcript_stamp(&chat.path);
        self.prime_live(&chat.path);
        self.section = Section::Chats;
    }

    fn chat_body(&mut self, chat: &crate::model::Chat, context: bool) -> Vec<String> {
        let mut body = Vec::new();
        if context {
            body.push("Context".into());
        }
        body.push(format!("repo  {}", repo_label(&chat.cwd)));
        if let Some(span) = chats::worked(&chat.path) {
            body.push(format!("worked  {span}"));
        }
        self.push_chat_state(&mut body, chat);
        // The pane opens on its last lines. Keep the talk there.
        // A dirty tree can be taller than the window and would hide it.
        body.extend(repo_changes(self, &chat.cwd));
        let page = chats::transcript_tail(&chat.path);
        self.hist_path = page.path;
        self.hist_from = page.from;
        self.hist_at = body.len() + 2;
        let transcript = stamp_turns(page.lines, &self.marks);
        if said_landed(&transcript, self.live_said.trim()) {
            self.live_said.clear();
        }
        body.extend(transcript.clone());
        body.extend(fresh_outbox(
            &transcript,
            chats::pending_outbox(&self.home, &chat.id, &chat.path),
        ));
        self.push_live_said(&mut body);
        self.push_live_reply(&mut body, &transcript);
        body
    }

    pub fn window_title(&self) -> String {
        "GrantTap Term".into()
    }

    pub fn work_footer(&self) -> Option<String> {
        if !self.chat_open() {
            return None;
        }
        let since = self.busy_since?;
        let secs = unix_now().saturating_sub(since);
        Some(format!("{} работает  {secs}s", spinner()))
    }

    fn chat_working(&self, chat: &crate::model::Chat) -> bool {
        chats::updates_fresh(&chat.path)
            || (self.busy_since.is_some() && chat.id == self.open_id)
    }

    fn sync_clock(&mut self, path: &Path) {
        self.marks = chats::turn_marks(path);
        let Some(mark) = self.marks.last().cloned() else {
            return;
        };
        if mark.done == 0 {
            self.busy_since = Some(mark.at.max(1));
            self.turn_tokens = mark.tokens.or(self.turn_tokens);
            let prev = self.marks.len().saturating_sub(2);
            self.tokens_base = self.marks.get(prev).and_then(|item| item.tokens);
            if self.marks.len() < 2 {
                self.tokens_base = None;
            }
            self.last_pulse = unix_now();
        } else if self.busy_since.is_some_and(|since| since <= mark.done) {
            self.busy_since = None;
            self.tokens_base = mark.tokens;
        }
    }

    fn note_user(&mut self, at: i64) {
        let at = if at > 0 { at } else { unix_now() };
        let fresh = self
            .busy_since
            .is_none_or(|since| at.saturating_sub(since) > 3);
        if fresh {
            self.tokens_base = self.turn_tokens.or(self.tokens_base);
            self.turn_tokens = None;
            self.last_pulse = 0;
            self.busy_since = Some(at);
        }
    }

    fn note_pulse(&mut self, at: i64, tokens: Option<u64>) {
        if at > 0 {
            self.last_pulse = at;
        }
        if let Some(n) = tokens {
            self.turn_tokens = Some(n);
            let id = self.open_id.clone();
            if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == id) {
                let (context, window) = chats::context_pair(&chat.path);
                if let Some(context) = context {
                    chat.tokens = Some(context);
                } else {
                    chat.tokens = Some(chat.tokens.map(|had| had.max(n)).unwrap_or(n));
                }
                if window.is_some() {
                    chat.window = window;
                }
            }
        }
        if self.busy_since.is_none() {
            self.busy_since = Some(if at > 0 { at } else { unix_now() });
        }
    }

    fn settle_work(&mut self) {
        let running = self
            .chats
            .iter()
            .find(|chat| chat.id == self.open_id)
            .is_some_and(|chat| chats::turn_running(&chat.path));
        if running && self.busy_since.is_none() && self.chat_open() {
            let now = unix_now();
            self.busy_since = Some(now);
            self.last_pulse = now;
        }
        let Some(since) = self.busy_since else {
            return;
        };
        let now = unix_now();
        if running {
            self.last_pulse = now;
            return;
        }
        let saw = self.last_pulse >= since;
        if !saw {
            if now.saturating_sub(since) >= 12 {
                self.busy_since = None;
            }
            return;
        }
        if now.saturating_sub(self.last_pulse) < 3 {
            return;
        }
        let done = self.last_pulse;
        let tokens = self.turn_tokens;
        self.busy_since = None;
        self.append_sum(done, tokens);
    }

    fn append_sum(&mut self, done: i64, tokens: Option<u64>) {
        let spent = match (self.tokens_base, tokens) {
            (Some(base), Some(now)) if now >= base => Some(now - base),
            (_, Some(now)) => Some(now),
            _ => None,
        };
        let label = spent.map(|n| token_label(Some(n))).unwrap_or_else(|| "—".into());
        let row = format!("итог  {}  {label}", stamp(done));
        let Mode::Split { lines, agent: Some(_) } = &mut self.mode else {
            return;
        };
        if lines.iter().rev().take(8).any(|line| line == &row) {
            return;
        }
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(row);
        self.pane_from_end = 0;
    }

    fn prime_live(&mut self, path: &Path) {
        let file = path.join("updates.jsonl");
        self.live_at = Some(std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0));
        self.live_reply.clear();
        self.live_said.clear();
        self.live_said_at = 0;
    }

    fn push_live_said(&self, body: &mut Vec<String>) {
        let text = self.live_said.trim();
        if text.is_empty() || said_landed(body, text) {
            return;
        }
        if body.last().is_some_and(|line| !line.is_empty()) {
            body.push(String::new());
        }
        body.extend(said_rows(self.live_said_at, text));
    }

    fn pane_has_said(&self, text: &str) -> bool {
        match &self.mode {
            Mode::Split { lines, agent: Some(_) } => said_landed(lines, text),
            _ => false,
        }
    }

    fn note_said(&mut self, at: i64, text: &str, added: &mut Vec<String>) {
        let text = text.trim();
        if text.is_empty() || self.live_said.trim() == text || self.pane_has_said(text) {
            return;
        }
        let at = if at > 0 { at } else { unix_now() };
        let rows = said_rows(at, text);
        self.live_said = text.to_string();
        self.live_said_at = at;
        added.extend(rows);
    }

    fn push_live_reply(&self, body: &mut Vec<String>, transcript: &[String]) {
        let reply = self.live_reply.trim();
        if reply.is_empty() {
            return;
        }
        let tail: String = reply.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
        let tail = tail.trim();
        if tail.chars().count() > 12 && transcript.iter().any(|line| line.contains(tail)) {
            return;
        }
        if body.last().is_some_and(|line| !line.is_empty()) {
            body.push(String::new());
        }
        let mut first = true;
        for line in self.live_reply.split('\n') {
            if line.trim().is_empty() {
                continue;
            }
            if line.starts_with("tool  ") {
                body.push(line.to_string());
                first = false;
                continue;
            }
            if first {
                body.push(stamp_assistant_line(
                    &format!("assistant  {line}"),
                    unix_now(),
                ));
                first = false;
            } else {
                body.push(line.to_string());
            }
        }
    }

    fn pull_live(&mut self) -> bool {
        if !self.chat_open() {
            return false;
        }
        let Some(chat) = self
            .chats
            .iter()
            .find(|chat| chat.id == self.open_id)
            .cloned()
        else {
            return false;
        };
        if chat.agent != AgentKind::Grok {
            return false;
        }
        let file = chat.path.join("updates.jsonl");
        let offset = match self.live_at {
            Some(offset) => offset,
            None => {
                self.live_at = Some(std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0));
                return false;
            }
        };
        let (next, events) = chats::read_updates(&file, offset);
        self.live_at = Some(next);
        if events.is_empty() {
            return false;
        }
        let mut added = Vec::new();
        let mut replace_assistant: Option<String> = None;
        for event in events {
            match event {
                chats::LiveUpdate::User(at, text) => {
                    self.note_user(at);
                    self.note_said(at, &text, &mut added);
                }
                chats::LiveUpdate::Pulse(at, tokens) => {
                    self.note_pulse(at, tokens);
                }
                chats::LiveUpdate::Agent(text) => {
                    if self.live_reply.ends_with(text.trim()) {
                        continue;
                    }
                    // A cumulative chunk repeats the reply so far. Keep one line.
                    let extended = !self.live_reply.is_empty()
                        && text.starts_with(self.live_reply.trim());
                    if extended {
                        self.live_reply = text.clone();
                        replace_assistant = Some(stamp_assistant_line(
                            &format!("assistant  {text}"),
                            unix_now(),
                        ));
                    } else {
                        if !self.live_reply.is_empty() {
                            self.live_reply.push('\n');
                        }
                        self.live_reply.push_str(&text);
                        added.push(stamp_assistant_line(
                            &format!("assistant  {text}"),
                            unix_now(),
                        ));
                    }
                }
                chats::LiveUpdate::Tool(title) => {
                    let row = format!("tool  {title}");
                    if self.live_reply.contains(&row) {
                        continue;
                    }
                    if !self.live_reply.is_empty() {
                        self.live_reply.push('\n');
                    }
                    self.live_reply.push_str(&row);
                    added.push(row);
                }
            }
        }
        if added.is_empty() && replace_assistant.is_none() {
            return false;
        }
        if self.live_reply.chars().count() > 12_000 {
            let tail: String = self.live_reply.chars().rev().take(8_000).collect::<Vec<_>>().into_iter().rev().collect();
            self.live_reply = tail;
        }
        if let Mode::Split { lines, agent: Some(_) } = &mut self.mode {
            if let Some(line) = replace_assistant {
                if let Some(slot) = lines.iter_mut().rev().find(|row| row.starts_with("assistant  "))
                {
                    *slot = line;
                } else {
                    if lines.last().is_some_and(|row| !row.is_empty()) {
                        lines.push(String::new());
                    }
                    lines.push(line);
                }
            }
            if !added.is_empty() {
                if lines.last().is_some_and(|line| !line.is_empty()) {
                    lines.push(String::new());
                }
                lines.extend(added);
            }
        }
        self.pane_from_end = 0;
        true
    }

    fn refresh_open(&mut self) {
        if self.open_id.is_empty() {
            return;
        }
        let Some(chat) = self.chats.iter().find(|chat| chat.id == self.open_id).cloned() else {
            return;
        };
        let stamp = chats::transcript_stamp(&chat.path);
        let first = self.open_stamp.is_none();
        let stamp_changed = self.open_stamp != stamp;
        if first {
            self.open_stamp = stamp;
        }
        let _live = self.pull_live();
        if first || !stamp_changed {
            return;
        }
        // A failed tail used to replace the open talk with "No transcript."
        // and pin the old desk lines under it. Keep the conversation.
        let page = chats::transcript_tail(&chat.path);
        if page.lines.iter().all(|line| line == "No transcript.") {
            let keep = match &self.mode {
                Mode::Split { lines, agent: Some(_) } => lines.iter().any(|line| {
                    line.starts_with("user  ")
                        || line.starts_with("assistant  ")
                        || line.starts_with("tool  ")
                }),
                _ => false,
            };
            if keep {
                self.open_stamp = stamp;
                return;
            }
        }
        self.open_stamp = stamp;
        self.sync_clock(&chat.path);
        let lines = dress(chat.agent, &chat.title, self.chat_body(&chat, false));
        if let Mode::Split { lines: slot, agent: Some(_) } = &mut self.mode {
            *slot = lines;
            self.pane_from_end = 0;
            self.shot_seen = 0;
            return;
        }
        if let Some(held) = &mut self.held
            && held.open_id == chat.id
        {
            held.lines = lines;
        }
    }

    fn open_task(&mut self) -> Effect {
        match self.row_at(self.cursor) {
            Some(DeskRow::Chat(index)) => {
                if let Some(chat) = self.chats.get(index).cloned() {
                    self.show_chat(&chat, true);
                }
            }
            Some(DeskRow::NewTask) => self.begin_task(),
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
                    self.open_id = chat.id.clone();
                    self.open_stamp = chats::transcript_stamp(&chat.path);
                    if let Some(span) = chats::worked(&chat.path) {
                        body.push(format!("worked  {span}"));
                    }
                    self.push_chat_state(&mut body, &chat);
                    body.extend(repo_changes(self, &chat.cwd));
                    let page = chats::transcript_tail(&chat.path);
                    self.hist_path = page.path;
                    self.hist_from = page.from;
                    self.hist_at = body.len() + 2;
                    let transcript = page.lines;
                    let pending = fresh_outbox(
                        &transcript,
                        chats::pending_outbox(&self.home, &chat.id, &chat.path),
                    );
                    body.extend(transcript);
                    body.extend(pending);
                } else {
                    self.open_id.clear();
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
        let Some(hit) = self.mesh_rows().get(self.cursor).map(|row| match &row.hit {
            MeshHit::None => MeshHit::None,
            MeshHit::Up => MeshHit::Up,
            MeshHit::Skill(agent, name) => MeshHit::Skill(agent.clone(), name.clone()),
            MeshHit::Dir(rel) => MeshHit::Dir(rel.clone()),
            MeshHit::File(rel) => MeshHit::File(rel.clone()),
        }) else {
            return Effect::None;
        };
        match hit {
            MeshHit::Up => self.mesh_up(),
            MeshHit::Dir(rel) => self.mesh_down(&rel),
            MeshHit::File(rel) => return self.open_mesh_file(&rel),
            MeshHit::Skill(agent, name) => {
                if let Some(skill) = self
                    .skills
                    .iter()
                    .find(|skill| skill.agent == agent && skill.name == name)
                    .cloned()
                {
                    self.show_skill(&skill);
                }
            }
            MeshHit::None => {}
        }
        Effect::None
    }

    fn open_mesh_file(&mut self, rel: &str) -> Effect {
        let mut path = mesh::git_root(&self.dir).unwrap_or_else(|| self.dir.clone());
        for part in rel.split('/') {
            if !part.is_empty() {
                path.push(part);
            }
        }
        if !path.is_file() {
            self.note = format!("missing {rel}");
            return Effect::None;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        self.stash_chat();
        let (changed, _) = self.marked_lines(&path, &text);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(rel);
        let mut lines = vec![format!("vim  {name}")];
        lines.push(format!("where  {rel}"));
        if self.mesh_board.is_broken(rel) {
            lines.push(format!("unhealthy  {rel}"));
        }
        if changed > 0 {
            lines.push(format!("changed  {changed} lines"));
        }
        lines.extend(text.lines().take(200).map(str::to_string));
        let body_at = lines.iter().take_while(|line| is_header(line)).count();
        self.vim_path = Some(path);
        self.mode = Mode::View { lines };
        self.fill = false;
        self.scroll = body_at;
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
        let rows = lessons::rows(self.lesson_open);
        match rows.get(self.cursor).copied() {
            Some(lessons::Row::Lesson(index)) => self.show_lesson(index),
            Some(lessons::Row::Drill(lesson, drill)) => self.run_drill(lesson, drill),
            _ => {}
        }
        Effect::None
    }

    fn show_lesson(&mut self, index: usize) {
        if lessons::lessons().get(index).is_none() {
            return;
        }
        self.lesson_open = Some(index);
        self.stash_chat();
        self.vim_path = None;
        let rows = lessons::rows(Some(index));
        let cursor = rows.iter().position(|row| row.picks()).unwrap_or(0);
        let lines = rows.iter().map(|row| row.text()).collect();
        self.mode = Mode::View { lines };
        self.cursor = cursor;
        self.scroll = 0;
        self.fill = false;
    }

    fn step_lessons(&mut self, delta: i32) {
        let rows = lessons::rows(self.lesson_open);
        let picks: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.picks())
            .map(|(index, _)| index)
            .collect();
        if picks.is_empty() {
            return;
        }
        let next = if let Some(pos) = picks.iter().position(|index| *index == self.cursor) {
            clamp_index(pos, delta, picks.len())
        } else if delta < 0 {
            picks
                .iter()
                .rposition(|index| *index < self.cursor)
                .unwrap_or(0)
        } else {
            picks
                .iter()
                .position(|index| *index > self.cursor)
                .unwrap_or(picks.len() - 1)
        };
        self.cursor = picks[next];
    }

    fn run_drill(&mut self, lesson: usize, drill: usize) {
        let Some(kind) = lessons::lessons()
            .get(lesson)
            .and_then(|item| item.drills.get(drill))
            .map(|item| item.kind)
        else {
            return;
        };
        match kind {
            lessons::DrillKind::Vim => self.start_scratch(),
            lessons::DrillKind::Split => {
                self.enter(Section::Term);
                self.shell.split();
                self.note = "shell split. Type echo hello and press Enter.".into();
            }
            lessons::DrillKind::Kubectl => {
                self.enter(Section::Term);
                self.shell.remember("kubectl");
                self.cmd = "kubectl ".into();
                self.note = "kubectl suggestions are open. Tab takes one.".into();
            }
            lessons::DrillKind::Files => {
                self.enter(Section::Files);
                self.note = "Enter opens a file in vim.".into();
            }
            lessons::DrillKind::Say => {
                if self.chat_open() || self.held.is_some() {
                    self.note = "Type a line and press Enter. It goes to the open chat.".into();
                } else {
                    self.note = "Open a chat first. A typed line goes to that chat.".into();
                }
            }
        }
    }

    fn start_scratch(&mut self) {
        let dir = self.home.join(".granttap-term/practice");
        if std::fs::create_dir_all(&dir).is_err() {
            self.note = "practice folder is missing".into();
            return;
        }
        let path = dir.join("scratch.txt");
        let body = "practice\nchange this line\nthen press F2\n";
        if std::fs::write(&path, body).is_err() {
            self.note = "practice file was not written".into();
            return;
        }
        self.vim_path = Some(path.clone());
        let mut lines = vec![
            "vim  scratch.txt".into(),
            format!("where  {}", path.display()),
            "row  2".into(),
        ];
        lines.extend(body.lines().map(str::to_string));
        self.scroll = 4;
        self.cursor = 4;
        self.mode = Mode::View { lines };
        self.fill = false;
        self.note = "vim  scratch.txt:2".into();
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
            self.stash_chat();
            let (changed, _) = self.marked_lines(&row.path, &text);
            let name = row.name.trim_end_matches('/');
            let mut lines = vec![format!("vim  {name}")];
            lines.push(format!("where  {}", self.file_rel(&row.path)));
            if changed > 0 {
                lines.push(format!("changed  {changed} lines"));
            }
            lines.extend(text.lines().take(200).map(str::to_string));
            let body_at = lines.iter().take_while(|line| is_header(line)).count();
            self.vim_path = Some(row.path.clone());
            self.mode = Mode::View { lines };
            self.fill = false;
            self.scroll = body_at;
            return Effect::None;
        }
        self.stash_chat();
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
        if self.vim_active() {
            return self.write_vim();
        }
        if self.section != Section::Files {
            self.note = "Vim edits a file on Files.".into();
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
        if agent.is_none() {
            self.stash_chat();
        } else {
            self.held = None;
        }
        self.vim_path = None;
        self.pane_key.clear();
        self.mode = Mode::Split { lines, agent };
        self.fill = false;
        self.scroll = 0;
        self.desc_open = false;
    }

    fn chat_open(&self) -> bool {
        !self.open_id.is_empty() && matches!(self.mode, Mode::Split { agent: Some(_), .. })
    }

    fn open_agent(&self) -> Option<AgentKind> {
        match &self.mode {
            Mode::Split { agent: Some(agent), .. } if !self.open_id.is_empty() => Some(*agent),
            _ => self.held.as_ref().map(|held| held.agent),
        }
    }

    fn stash_chat(&mut self) {
        let Mode::Split { lines, agent: Some(agent) } = &self.mode else {
            return;
        };
        if self.open_id.is_empty() {
            return;
        }
        self.held = Some(HeldChat {
            lines: lines.clone(),
            agent: *agent,
            open_id: self.open_id.clone(),
            pane_key: self.pane_key.clone(),
            pane_from_end: self.pane_from_end,
            hist_from: self.hist_from,
            hist_path: self.hist_path.clone(),
            hist_at: self.hist_at,
        });
    }

    fn restore_held(&mut self) -> bool {
        let Some(held) = self.held.take() else {
            return false;
        };
        self.open_id = held.open_id;
        self.pane_key = held.pane_key;
        self.pane_from_end = held.pane_from_end;
        self.hist_from = held.hist_from;
        self.hist_path = held.hist_path;
        self.hist_at = held.hist_at;
        self.vim_path = None;
        self.mode = Mode::Split {
            lines: held.lines,
            agent: Some(held.agent),
        };
        true
    }

    fn term_suggestions(&self) -> Vec<String> {
        let mut parts = self.cmd.split_whitespace();
        let head = parts.next().unwrap_or("");
        let second = parts.next();
        let third = parts.next();
        let trailing = self.cmd.ends_with(' ');
        let mut found = Vec::new();
        if head.is_empty() {
            found.push("split".into());
            found.push("shell".into());
            for tool in self.shell.opened() {
                if !found.iter().any(|have| have == tool) {
                    found.push(tool.clone());
                }
            }
        } else if second.is_none() && !trailing {
            for name in [
                "split", "shell", "kubectl", "git", "docker", "cargo", "npm", "gh", "helm", "az",
                "terraform",
            ] {
                if name.starts_with(head) && name != head && !found.iter().any(|have| have == name) {
                    found.push(name.into());
                }
            }
            for tool in self.shell.opened() {
                if tool.starts_with(head) && tool != head && !found.iter().any(|have| have == tool) {
                    found.push(tool.clone());
                }
            }
        } else if third.is_none() && second.is_none() && trailing {
            for sub in self.shell.subs(head) {
                found.push(format!("{head} {sub}"));
            }
        } else if third.is_none() && !trailing {
            let partial = second.unwrap_or("");
            for sub in self.shell.subs(head) {
                if sub.starts_with(partial) && sub != partial {
                    found.push(format!("{head} {sub}"));
                }
            }
        }
        found.truncate(6);
        found
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

    fn remember_bound(&mut self) {
        let Some(cwd) = self.live_cwd() else {
            return;
        };
        if !cwd.is_empty() {
            self.bound_cwd = cwd;
        }
    }

    fn live_cwd(&self) -> Option<String> {
        let raw = if !self.open_id.is_empty() {
            self.chats
                .iter()
                .find(|chat| chat.id == self.open_id)
                .map(|chat| chat.cwd.clone())
        } else if matches!(self.section, Section::Chats | Section::Tasks) {
            self.band_chat().map(|chat| chat.cwd)
        } else if let Some(held) = &self.held {
            self.chats
                .iter()
                .find(|chat| chat.id == held.open_id)
                .map(|chat| chat.cwd.clone())
        } else {
            None
        }?;
        chats::resolve_cwd(&raw)
            .map(|path| path.display().to_string())
            .or_else(|| usable_cwd(&raw).then(|| raw))
    }

    fn bound_mesh(&self) -> PathBuf {
        if self.bound_cwd.is_empty() {
            return self.repo_root();
        }
        let cwd = PathBuf::from(&self.bound_cwd);
        mesh::git_root(&cwd).unwrap_or(cwd)
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

    pub(crate) fn vim_active(&self) -> bool {
        self.vim_path.is_some() && matches!(self.mode, Mode::View { .. })
    }

    fn on_vim_key(&mut self, key: Key) -> Effect {
        match key {
            Key::F(10) => Effect::Quit,
            Key::CtrlC => self.arm_ctrl_c(),
            Key::Esc => {
                self.vim_path = None;
                if let Some(index) = self.lesson_open
                    && self.section == Section::Lessons
                {
                    self.show_lesson(index);
                    return Effect::None;
                }
                if !self.restore_held() {
                    self.mode = Mode::Desk;
                    self.scroll = 0;
                }
                self.desc_open = false;
                self.fill = false;
                self.cmd.clear();
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
            Section::Mesh => &["refresh", "health", "skill", "history", "blob"],
            Section::Files => &["edit", "commit", "push", "blob", "history"],
            Section::Browser => &["refresh", "browse", "back"],
            Section::Lessons => &["lessons"],
            Section::Term => &["split", "shell"],
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
            DeskRow::NewTask => Some(DeskRow::NewTask),
            DeskRow::Agent(_) | DeskRow::Mesh(_) | DeskRow::Band(_) => None,
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
            DeskRow::Agent(_) | DeskRow::Mesh(_) | DeskRow::NewTask | DeskRow::Band(_) => None,
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
            Section::Tasks => self.task_board(),
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
            self.reveal_list_cursor();
        }
    }

    fn row_line(&self, row: &DeskRow, width: usize) -> String {
        match row {
            DeskRow::Agent(agent) => agent_ink(*agent),
            DeskRow::Mesh(label) => format!("\x1b[38;5;131m  {label}"),
            DeskRow::NewTask => "\x1b[38;5;40m+ новая задача".into(),
            DeskRow::Band(label) => format!("\x1b[38;5;131m▼ {label}  {}", "-".repeat(18)),
            DeskRow::Chat(index) if self.section == Section::Tasks => {
                self.task_chat_line(&self.chats[*index], width)
            }
            DeskRow::Chat(index) => chat_line(
                &self.chats[*index],
                width,
                self.needs_you(&self.chats[*index]),
                self.chat_working(&self.chats[*index]),
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
        if self.section == Section::Tasks {
            let title = fit(&task.title, width.saturating_sub(20).max(8));
            return format!(
                "\x1b[38;5;203m◇  {title}  \x1b[38;5;131m{}",
                task.owner.label()
            );
        }
        format!(
            "{}  {}  {}  {}",
            fit("—", 11),
            fit(&task.title, row_title_width(width)),
            fit(&self.place_label(), 14),
            fit("—", 6),
        )
    }

    fn begin_task(&mut self) {
        self.cmd = "task ".into();
        self.note = "название задачи".into();
        self.suggest = 0;
    }

    fn task_board(&self) -> Vec<DeskRow> {
        let mut order: Vec<usize> = (0..self.chats.len()).collect();
        order.sort_by(|left, right| {
            date_key(self.chats[*right].at).cmp(&date_key(self.chats[*left].at))
        });
        let mut working = Vec::new();
        let mut idle = Vec::new();
        for index in order {
            if self.chat_working(&self.chats[index]) {
                working.push(DeskRow::Chat(index));
            } else {
                idle.push(DeskRow::Chat(index));
            }
        }
        for index in 0..self.tasks.len() {
            idle.push(DeskRow::Task(index));
        }
        let mut rows = vec![DeskRow::NewTask];
        if !working.is_empty() {
            rows.push(DeskRow::Band(format!("работает  {}", working.len())));
            rows.extend(working);
        }
        if !idle.is_empty() {
            rows.push(DeskRow::Band(format!("ждут  {}", idle.len())));
            rows.extend(idle);
        }
        rows
    }

    fn task_chat_line(&self, chat: &crate::model::Chat, width: usize) -> String {
        let working = self.chat_working(chat);
        let mark = if working { "::" } else { "◇" };
        let mut title = visible_title(chat);
        if self.needs_you(chat) {
            title = format!("! {title}");
        }
        let repo = repo_label(&chat.cwd);
        let age = age_label(chat.at, unix_now());
        let status = if working {
            "думает"
        } else if chat.live {
            "живой"
        } else {
            ""
        };
        let left = format!("{mark}  {title}  ·  {repo}");
        let right = if status.is_empty() {
            age
        } else {
            format!("{status}  {age}")
        };
        let gap = width.saturating_sub(left.chars().count() + right.chars().count() + 2);
        format!("\x1b[38;5;203m{left}{}{right}", " ".repeat(gap))
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
            visits: self.visits.clone(),
            web_theme: self.web_theme.clone(),
            web_lang: self.web_lang.clone(),
            web_js: self.web_js,
            web_live: self.web_live,
            bookmarks: self.bookmarks.clone(),
            look: self.look.clone(),
            font_face: self.font_face.clone(),
            font_size: self.font_size,
            image_quality: self.image_quality.clone(),
        };
        let _ = store::save(&self.home, &state);
    }
}

fn mcp_script(home: &Path) -> Option<PathBuf> {
    let root = home.join("scoop/persist/nvm/nodejs");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path().join("node_modules/granttap-mcp/bin/granttap-mcp.mjs");
        if path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    found.pop()
}

fn screen_reply(have: &[String], screen: &str) -> String {
    let mut out = Vec::new();
    for line in screen.lines() {
        let line = line.trim();
        if line.chars().count() < 2 || !line.chars().any(|ch| ch.is_alphanumeric()) {
            continue;
        }
        if chrome_line(line) {
            continue;
        }
        if have.iter().any(|row| row.contains(line)) {
            continue;
        }
        out.push(line.to_string());
        if out.len() == 12 {
            break;
        }
    }
    out.join("\n")
}

fn chrome_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("enter:send")
        || lower.contains("shift+tab")
        || lower.contains("ctrl+x:")
        || lower.contains("<level>")
        || lower.contains("<model>")
        || lower.contains("(active)")
        || lower.contains("maximum reasoning")
        || lower.contains("thorough reasoning")
        || lower.contains("faster turnaround")
        || lower.contains("fastest responses")
    {
        return true;
    }
    let compact = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact == "always-approve" {
        return true;
    }
    if compact.starts_with("grok ") && compact.contains('(') && compact.chars().count() < 48 {
        return true;
    }
    let frames = line.chars().filter(|ch| frame_char(*ch)).count();
    let letters = line.chars().filter(|ch| ch.is_alphanumeric()).count();
    frames > 0 && frames * 2 >= letters.max(1)
}

fn frame_char(ch: char) -> bool {
    matches!(
        ch,
        '─' | '│'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '═'
            | '║'
            | '╭'
            | '╮'
            | '╯'
            | '╰'
            | '━'
            | '┃'
            | '█'
            | '▀'
            | '▄'
    )
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn compact_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n}b")
    } else if n < 1024 * 1024 {
        format!("{}kb", n / 1024)
    } else {
        let tenths = n.saturating_mul(10) / (1024 * 1024);
        format!("{}.{}mb", tenths / 10, tenths % 10)
    }
}

fn scheme_is_dark(theme: &str, system_dark: bool) -> bool {
    match theme {
        "light" => false,
        "dark" => true,
        _ => system_dark,
    }
}

fn locale_tag(lang: &str, system: &str) -> String {
    match lang {
        "en" => "en-US".into(),
        "ru" => "ru-RU".into(),
        "uk" => "uk-UA".into(),
        "he" => "he-IL".into(),
        "ar" => "ar".into(),
        "de" => "de-DE".into(),
        "fr" => "fr-FR".into(),
        "es" => "es-ES".into(),
        "auto" => {
            let system = system.replace('_', "-");
            let ok = system.len() >= 2
                && system
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic());
            if ok { system } else { "en-US".into() }
        }
        _ => "en-US".into(),
    }
}

fn system_dark() -> bool {
    #[cfg(windows)]
    {
        return windows_apps_dark().unwrap_or(true);
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn system_locale() -> String {
    #[cfg(windows)]
    {
        if let Some(name) = windows_locale() {
            return name;
        }
    }
    "en-US".into()
}

#[cfg(windows)]
fn windows_apps_dark() -> Option<bool> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: isize,
            sub: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut u8,
            len: *mut u32,
        ) -> i32;
    }
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut data = 1u32;
    let mut len = 4u32;
    let rc = unsafe {
        RegGetValueW(
            0x8000_0001u32 as i32 as isize,
            sub.as_ptr(),
            name.as_ptr(),
            0x10,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut u8,
            &mut len,
        )
    };
    if rc != 0 { None } else { Some(data == 0) }
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(buf: *mut u16, size: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 1 {
        return None;
    }
    String::from_utf16(&buf[..(n as usize) - 1]).ok()
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeskRow {
    Agent(AgentKind),
    Mesh(String),
    Chat(usize),
    Task(usize),
    NewTask,
    Band(String),
}

fn row_is_item(row: &DeskRow) -> bool {
    matches!(row, DeskRow::Chat(_) | DeskRow::Task(_) | DeskRow::NewTask)
}

pub(crate) struct Folded {
    pub text: String,
    pub fold: Option<usize>,
    pub tree: Option<u16>,
    pub doc: Option<u16>,
}

/// The one row that stands for the whole diff. Folder rows stay 0, 1, 2, …
pub(crate) const TREE_ALL: u16 = 60_000;

fn line_rows(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut rows = 1usize;
    let mut col = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            rows += 1;
            col = 0;
            continue;
        }
        col += 1;
        if col >= width {
            rows += 1;
            col = 0;
        }
    }
    rows
}

fn rows_high(rows: &[Folded], width: usize) -> usize {
    rows.iter().map(|row| line_rows(&row.text, width)).sum()
}

fn visual_at(rows: &[Folded], mark: u16, width: usize) -> Option<usize> {
    let mut at = 0usize;
    for row in rows {
        if row.tree == Some(mark) {
            return Some(at);
        }
        at += line_rows(&row.text, width);
    }
    None
}

fn block_rows(lines: &[String], width: usize) -> usize {
    rows_high(&fold_lines(lines, &[], &[]), width)
}

pub(crate) fn fold_lines(lines: &[String], open: &[usize], shut: &[String]) -> Vec<Folded> {
    let mut out = Vec::new();
    let mut index = 0usize;
    let mut tree_n = 0u16;
    let mut doc_n = 0u16;
    let mut hiding = false;
    let mut summarized = false;
    while index < lines.len() {
        let line = &lines[index];
        if !is_tree_line(line) {
            summarized = false;
        } else if line.starts_with("folder  ") && !summarized {
            let (plus, minus, files, span) = tree_span(&lines[index..]);
            summarized = true;
            if files > 0 {
                let mark = if shut.iter().any(|item| item == "*") {
                    "▶"
                } else {
                    "▼"
                };
                let text = if plus > 0 || minus > 0 {
                    format!("tree  {mark}  *  {plus}  {minus}")
                } else {
                    format!("tree  {mark}  *")
                };
                out.push(Folded {
                    text,
                    fold: None,
                    tree: Some(TREE_ALL),
                    doc: None,
                });
            }
            if shut.iter().any(|item| item == "*") {
                index += span.max(1);
                summarized = false;
                continue;
            }
        }
        if let Some(dir) = line.strip_prefix("folder  ") {
            if !folder_visible(&lines[index + 1..]) {
                index += 1;
                while index < lines.len() && lines[index].starts_with("stat  ") {
                    index += 1;
                }
                continue;
            }
            hiding = shut.iter().any(|item| item == dir);
            let mark = if hiding { "▶" } else { "▼" };
            out.push(Folded {
                text: format!("tree  {mark}  {dir}"),
                fold: None,
                tree: Some(tree_n),
                doc: None,
            });
            tree_n = tree_n.saturating_add(1);
            index += 1;
            continue;
        }
        if hiding && line.starts_with("stat  ") {
            index += 1;
            continue;
        }
        hiding = false;
        if let Some(rest) = line.strip_prefix("stat  ") {
            if matches!(stat_nums(rest), Some((0, 0))) {
                index += 1;
                continue;
            }
        }
        if line.starts_with("stat  ") {
            let doc = md_stat_line(line).map(|_| {
                let id = doc_n;
                doc_n = doc_n.saturating_add(1);
                id
            });
            out.push(Folded {
                text: line.clone(),
                fold: None,
                tree: None,
                doc,
            });
            index += 1;
            continue;
        }
        if !is_work_line(line) {
            out.push(Folded {
                text: line.clone(),
                fold: None,
                tree: None,
                doc: None,
            });
            index += 1;
            continue;
        }
        let start = index;
        let mut kids = Vec::new();
        while index < lines.len() && is_work_line(&lines[index]) {
            kids.push(lines[index].clone());
            index += 1;
            while index < lines.len() && lines[index].is_empty() {
                let rest = index + 1;
                if rest < lines.len() && is_work_line(&lines[rest]) {
                    index = rest;
                } else {
                    break;
                }
            }
        }
        let expanded = open.contains(&start);
        let mark = if expanded { "v" } else { "◆" };
        out.push(Folded {
            text: format!("{mark}  {}", work_summary(&kids)),
            fold: Some(start),
            tree: None,
            doc: None,
        });
        if expanded {
            for kid in kids {
                out.push(Folded {
                    text: work_child(&kid),
                    fold: None,
                    tree: None,
                    doc: None,
                });
            }
        }
    }
    out
}

fn is_tree_line(line: &str) -> bool {
    line.starts_with("folder  ")
        || line.starts_with("stat  ")
        || (line.starts_with("changed  ") && line.ends_with(" more"))
}

fn stat_nums(rest: &str) -> Option<(u64, u64)> {
    let mut parts = rest.split("  ");
    let _name = parts.next()?;
    let plus = parts.next()?.parse().ok()?;
    let minus = parts.next()?.parse().ok()?;
    Some((plus, minus))
}

fn folder_visible(lines: &[String]) -> bool {
    for line in lines {
        if let Some(rest) = line.strip_prefix("stat  ") {
            if !matches!(stat_nums(rest), Some((0, 0))) {
                return true;
            }
            continue;
        }
        return false;
    }
    false
}

/// Plus, minus, visible files, and how many source lines belong to this diff.
fn tree_span(lines: &[String]) -> (u64, u64, u64, usize) {
    let mut plus = 0u64;
    let mut minus = 0u64;
    let mut files = 0u64;
    let mut n = 0usize;
    for line in lines {
        if !is_tree_line(line) {
            break;
        }
        n += 1;
        if let Some(rest) = line.strip_prefix("stat  ") {
            match stat_nums(rest) {
                Some((0, 0)) => {}
                Some((add, del)) => {
                    plus += add;
                    minus += del;
                    files += 1;
                }
                None => files += 1,
            }
        }
    }
    (plus, minus, files, n)
}

fn md_stat_line(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("stat  ")?;
    let name = rest.split("  ").next().unwrap_or("");
    name.rsplit_once('.')
        .filter(|(_, ext)| ext.eq_ignore_ascii_case("md"))
        .map(|_| name)
}

fn is_work_line(line: &str) -> bool {
    line.starts_with("tool  ") || line == "think" || line.starts_with("think  ")
}

fn work_summary(kids: &[String]) -> String {
    let mut edits = 0u64;
    let mut files = 0u64;
    let mut searches = 0u64;
    let mut thinks = 0u64;
    let mut other = 0u64;
    let mut add = 0u64;
    let mut del = 0u64;
    for line in kids {
        if line == "think" || line.starts_with("think  ") {
            thinks += 1;
            continue;
        }
        let rest = line.strip_prefix("tool  ").unwrap_or(line);
        let name = rest.split_whitespace().next().unwrap_or("");
        match work_kind(name) {
            Work::Edit => edits += 1,
            Work::Read => files += 1,
            Work::Search => searches += 1,
            Work::Other => other += 1,
        }
        if let Some((plus, minus)) = work_delta(rest) {
            add += plus;
            del += minus;
        }
    }
    if thinks > 0 && edits == 0 && files == 0 && searches == 0 && other == 0 {
        return "Thinking...".into();
    }
    let mut parts = Vec::new();
    if edits > 0 {
        let word = if edits == 1 { "file" } else { "files" };
        parts.push(format!("Edited {edits} {word}"));
    }
    if files > 0 {
        let word = if files == 1 { "file" } else { "files" };
        if edits > 0 {
            parts.push(format!("explored {files} {word}"));
        } else {
            parts.push(format!("Explored {files} {word}"));
        }
    }
    if searches > 0 {
        let word = if searches == 1 { "search" } else { "searches" };
        parts.push(format!("{searches} {word}"));
    }
    if other > 0 {
        parts.push(format!("{other} tools"));
    }
    if parts.is_empty() {
        parts.push("Thinking...".into());
    }
    let mut text = parts.join(", ");
    if add > 0 || del > 0 {
        text.push_str(&format!("  +{add}  -{del}"));
    }
    text
}

fn work_child(line: &str) -> String {
    if line == "think" || line.starts_with("think  ") {
        return "◆  Thinking...".into();
    }
    let rest = line.strip_prefix("tool  ").unwrap_or(line);
    let mut parts = rest.split_whitespace();
    let name = parts.next().unwrap_or("tool");
    let path = parts.next().unwrap_or("");
    let kind = match work_kind(name) {
        Work::Edit => "Edit",
        Work::Read => "Read",
        Work::Search => "Search",
        Work::Other => name,
    };
    let delta = work_delta(rest)
        .map(|(add, del)| format!("  +{add}/-{del}"))
        .unwrap_or_default();
    if path.is_empty() || path.starts_with('+') || path.starts_with('-') {
        format!("◆  {kind}{delta}")
    } else {
        format!("◆  {kind}  {path}{delta}")
    }
}

fn work_delta(rest: &str) -> Option<(u64, u64)> {
    let mut plus = None;
    let mut minus = None;
    for word in rest.split_whitespace() {
        if let Some(n) = word.strip_prefix('+').and_then(|n| n.parse().ok()) {
            plus = Some(n);
        }
        if let Some(n) = word.strip_prefix('-').and_then(|n| n.parse().ok()) {
            minus = Some(n);
        }
    }
    match (plus, minus) {
        (Some(add), Some(del)) => Some((add, del)),
        _ => None,
    }
}

enum Work {
    Edit,
    Read,
    Search,
    Other,
}

fn work_kind(name: &str) -> Work {
    let name = name.to_ascii_lowercase();
    if name.contains("edit")
        || name.contains("write")
        || name.contains("replace")
        || name.contains("patch")
        || name == "filechange"
    {
        Work::Edit
    } else if name.contains("search") || name.contains("grep") || name.contains("glob") {
        Work::Search
    } else if name.contains("read") || name.contains("list") || name == "cat" {
        Work::Read
    } else {
        Work::Other
    }
}

fn context_tip_line(used: u64, window: u64) -> String {
    let pct = used.saturating_mul(1000) / window.max(1);
    format!(
        "{}  {}.{}%   {} / {}   dashboard",
        meter_bar(used, window, 8),
        pct / 10,
        pct % 10,
        token_label(Some(used)),
        token_label(Some(window))
    )
}

fn window_pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "(0%)".into();
    }
    let mut tenths = part.saturating_mul(1000) / whole;
    if part > 0 && tenths == 0 {
        tenths = 1;
    }
    if tenths < 100 {
        format!("({}.{}%)", tenths / 10, tenths % 10)
    } else {
        format!("({}%)", tenths.saturating_add(5) / 10)
    }
}

fn count_phrase(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn bucket_row(color: &str, mark: char, label: &str, tokens: u64, whole: u64, extra: &str) -> String {
    let face = format!("{:>6}", chats::grok_token_face(tokens));
    let pct = format!("{:>7}", window_pct(tokens, whole));
    let name = format!("{label:<22}");
    let tail = if extra.is_empty() {
        format!("{face} tokens  {pct}")
    } else {
        format!("{face} tokens  {pct} · {extra}")
    };
    format!(" \x1b[{color}m{mark}\x1b[38;5;223m {name}{tail}")
}

/// Five rows of 26 diamonds. Cells follow system, messages, reasoning, then free.
fn diamond_rows(parts: &[(u64, &str, char)]) -> Vec<String> {
    const COLS: usize = 26;
    const ROWS: usize = 5;
    let counts = share_cells(&parts.iter().map(|part| part.0).collect::<Vec<_>>(), COLS * ROWS);
    let mut cells = Vec::with_capacity(COLS * ROWS);
    for (index, count) in counts.iter().enumerate() {
        for _ in 0..*count {
            cells.push((parts[index].1, parts[index].2));
        }
    }
    let mut rows = Vec::new();
    for row in 0..ROWS {
        let mut line = String::from(" ");
        let mut color = "";
        for col in 0..COLS {
            let (next, ch) = cells
                .get(row * COLS + col)
                .copied()
                .unwrap_or(("38;5;240", '◇'));
            if next != color {
                line.push_str(&format!("\x1b[{next}m"));
                color = next;
            }
            line.push(ch);
        }
        rows.push(line);
    }
    rows
}

fn share_cells(parts: &[u64], cells: usize) -> Vec<usize> {
    let sum: u64 = parts.iter().copied().sum();
    let mut out = vec![0usize; parts.len()];
    if sum == 0 || cells == 0 {
        return out;
    }
    let mut ranked = Vec::with_capacity(parts.len());
    let mut used = 0usize;
    for (index, n) in parts.iter().enumerate() {
        let prod = (*n as u128).saturating_mul(cells as u128);
        let base = (prod / sum as u128) as usize;
        out[index] = base;
        used += base;
        ranked.push((index, prod % sum as u128));
    }
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut left = cells.saturating_sub(used);
    for (index, _) in ranked {
        if left == 0 {
            break;
        }
        out[index] += 1;
        left -= 1;
    }
    out
}

fn meter_bar(used: u64, window: u64, cells: usize) -> String {
    let cells = cells.max(1);
    let filled = if window == 0 {
        0
    } else {
        ((used as u128).saturating_mul(cells as u128) / window as u128) as usize
    };
    let filled = filled.min(cells);
    let mut bar = String::new();
    for index in 0..cells {
        bar.push(if index < filled { '▓' } else { '░' });
    }
    bar
}

fn age_label(at: i64, now: i64) -> String {
    if at <= 0 || now < at {
        return "—".into();
    }
    let secs = (now - at) as u64;
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 48 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 60 {
        return format!("{days}d");
    }
    format!("{}mo", days / 30)
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

fn chat_line(chat: &crate::model::Chat, width: usize, needs: bool, working: bool) -> String {
    let mut title = visible_title(chat);
    if needs {
        title = format!("! {title}");
    }
    if working {
        title = format!("{} {title}", spinner());
    } else if chat.live {
        title = format!("* {title}");
    }
    format!(
        "\x1b[38;5;131m{}\x1b[38;5;203m  {}  \x1b[38;5;51m{}\x1b[38;5;203m  \x1b[38;5;220m{}",
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
    #[cfg(unix)]
    {
        let t = at as libc::time_t;
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        let ptr = unsafe { libc::localtime_r(&t, tm.as_mut_ptr()) };
        if ptr.is_null() {
            return "—".into();
        }
        let tm = unsafe { tm.assume_init() };
        return format!(
            "{:02}-{:02} {:02}:{:02}",
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        );
    }
    #[cfg(windows)]
    {
        windows_stamp(at)
    }
}

#[cfg(windows)]
fn windows_stamp(at: i64) -> String {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToSystemTime(file: *const FileTime, system: *mut SystemTime) -> i32;
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const core::ffi::c_void,
            utc: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
    }
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    const EPOCH_DIFF: u64 = 11_644_473_600;
    let Some(seconds) = u64::try_from(at).ok() else {
        return "—".into();
    };
    let ticks = seconds.saturating_add(EPOCH_DIFF).saturating_mul(10_000_000);
    let file = FileTime {
        low: ticks as u32,
        high: (ticks >> 32) as u32,
    };
    let mut utc = SystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    let mut local = utc;
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(core::ptr::null(), &utc, &mut local) == 0
        {
            return "—".into();
        }
    }
    format!(
        "{:02}-{:02} {:02}:{:02}",
        local.month, local.day, local.hour, local.minute
    )
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn busy_glyph() -> char {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let frames = ['|', '/', '-', '\\'];
    frames[((ms / 120) as usize) % frames.len()]
}

fn one_line(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn strip_sgr(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn spinner() -> char {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    frames[((ms / 120) as usize) % frames.len()]
}

fn clock_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 13
        && bytes[2] == b'-'
        && bytes[5] == b' '
        && bytes[8] == b':'
        && bytes[11] == b' '
        && bytes[12] == b' '
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7].is_ascii_digit()
        && bytes[9].is_ascii_digit()
        && bytes[10].is_ascii_digit()
}

fn said_rows(at: i64, text: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut labeled = false;
    for para in text.split('\n') {
        let para = para.trim_end();
        if para.trim().is_empty() {
            continue;
        }
        if labeled {
            rows.push(para.to_string());
        } else {
            rows.push(format!("user  {}  {para}", stamp(at)));
            labeled = true;
        }
    }
    rows
}

/// A short user block already carries this typed line. A long summary that
/// merely quotes the words does not.
fn said_landed(lines: &[String], text: &str) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    let need = needle.chars().count();
    let mut index = 0usize;
    while index < lines.len() {
        if !lines[index].starts_with("user  ") {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < lines.len() && !turn_break(&lines[index]) {
            index += 1;
        }
        let block = lines[start..index].join("\n");
        let body = block.chars().count();
        if block.contains(needle) && body < need.saturating_mul(6).max(need + 160) {
            return true;
        }
    }
    false
}

fn stamp_turns(lines: Vec<String>, marks: &[chats::TurnMark]) -> Vec<String> {
    if marks.is_empty() {
        return lines;
    }
    let mut out = Vec::with_capacity(lines.len() + marks.len());
    let mut mark_at = 0usize;
    let mut index = 0usize;
    let mut pending: Option<String> = None;
    let mut assist_at: Option<i64> = None;
    while index < lines.len() {
        if !lines[index].starts_with("user  ") {
            if lines[index].starts_with("assistant  ")
                && let Some(at) = assist_at.take()
            {
                out.push(stamp_assistant_line(&lines[index], at));
            } else {
                out.push(lines[index].clone());
            }
            index += 1;
            continue;
        }
        if let Some(row) = pending.take() {
            out.push(row);
        }
        let start = index;
        index += 1;
        while index < lines.len() && !turn_break(&lines[index]) {
            index += 1;
        }
        let block = lines[start..index].join("\n");
        let found = marks[mark_at..]
            .iter()
            .position(|mark| mark_fits(&block, &mark.head));
        if let Some(offset) = found {
            mark_at += offset;
            let mark = marks[mark_at].clone();
            out.extend(stamp_block(&lines[start..index], mark.at));
            assist_at = Some(if mark.done > 0 { mark.done } else { mark.at });
            if mark.done > 0 {
                let prev = if mark_at == 0 {
                    None
                } else {
                    marks[mark_at - 1].tokens
                };
                let spent = match (prev, mark.tokens) {
                    (Some(base), Some(now)) if now >= base => Some(now - base),
                    (_, Some(now)) => Some(now),
                    _ => None,
                };
                let label = spent
                    .map(|n| token_label(Some(n)))
                    .unwrap_or_else(|| "—".into());
                pending = Some(format!("итог  {}  {label}", stamp(mark.done)));
            }
            mark_at += 1;
        } else {
            assist_at = None;
            out.extend(lines[start..index].iter().cloned());
        }
    }
    if let Some(row) = pending {
        out.push(row);
    }
    out
}

fn turn_break(line: &str) -> bool {
    line.starts_with("user  ")
        || line.starts_with("assistant  ")
        || line.starts_with("tool  ")
        || line == "think"
        || line.starts_with("think  ")
        || line.starts_with("итог  ")
}

fn mark_fits(block: &str, head: &str) -> bool {
    let head = head.trim();
    if head.chars().count() < 8 {
        return false;
    }
    let head: String = head.chars().take(40).collect();
    block.contains(&head)
}

fn stamp_block(lines: &[String], at: i64) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut placed = false;
    for line in lines {
        let body = line.strip_prefix("user  ").unwrap_or(line.as_str()).trim();
        let tag = body.starts_with('<') && body.ends_with('>');
        if !placed && !tag && !body.is_empty() {
            if line.starts_with("user  ") {
                out.push(stamp_user_line(line, at));
            } else if clock_prefix(line) {
                out.push(line.clone());
            } else {
                out.push(format!("{}  {line}", stamp(at)));
            }
            placed = true;
            continue;
        }
        out.push(line.clone());
    }
    if !placed && !out.is_empty() {
        out[0] = stamp_user_line(&out[0], at);
    }
    out
}

fn stamp_user_line(line: &str, at: i64) -> String {
    stamp_speaker_line(line, "user  ", at)
}

fn stamp_assistant_line(line: &str, at: i64) -> String {
    stamp_speaker_line(line, "assistant  ", at)
}

fn stamp_speaker_line(line: &str, prefix: &str, at: i64) -> String {
    let Some(rest) = line.strip_prefix(prefix) else {
        return line.to_string();
    };
    if clock_prefix(rest) {
        return line.to_string();
    }
    format!("{prefix}{}  {rest}", stamp(at))
}

pub(crate) fn clock_face(stamp: &str) -> String {
    let bytes = stamp.as_bytes();
    if bytes.len() < 11 {
        return stamp.to_string();
    }
    let date = &stamp[..5];
    let hour: u16 = stamp[6..8].parse().unwrap_or(0);
    let minute = &stamp[9..11];
    let (shown, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour.saturating_sub(12), "PM"),
    };
    format!("{date}  {shown}:{minute} {half}")
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
        || line.starts_with("unhealthy ")
        || line.starts_with("row ")
}

fn skill_hit(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("  ")?;
    let (agent, name) = rest.split_once("  ")?;
    if agent.is_empty() || name.is_empty() || agent.contains(' ') {
        return None;
    }
    Some((agent.to_string(), name.to_string()))
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
    if let Some(path) = chats::resolve_cwd(cwd) {
        return path == root || path.starts_with(root);
    }
    let path = Path::new(cwd);
    if path == root || path.starts_with(root) {
        return true;
    }
    let cwd_slash = cwd.replace('\\', "/");
    let root_shown = root.to_string_lossy();
    let root_slash = root_shown.replace('\\', "/");
    if cwd_slash != cwd || root_slash != root_shown {
        let cwd_path = Path::new(&cwd_slash);
        let root_path = Path::new(&root_slash);
        if cwd_path == root_path || cwd_path.starts_with(root_path) {
            return true;
        }
    }
    let enc = root_shown.replace(['/', '\\'], "-");
    cwd == enc || cwd.starts_with(&format!("{enc}-"))
}

fn repo_label(cwd: &str) -> String {
    if let Some(path) = chats::resolve_cwd(cwd) {
        return repo_label_of(&path.display().to_string());
    }
    repo_label_of(cwd)
}

fn repo_label_of(cwd: &str) -> String {
    let cwd = cwd.trim().trim_end_matches(['/', '\\']);
    if cwd.is_empty() {
        return "—".into();
    }
    if let Some(name) = worktree_repo(cwd) {
        return name;
    }
    if cwd.contains('/') || cwd.contains('\\') {
        let name = cwd.rsplit(['/', '\\']).next().unwrap_or(cwd);
        if !name.is_empty() {
            if scratch_id(name) {
                return "—".into();
            }
            return name.to_string();
        }
    }
    if let Some((head, tail)) = cwd.rsplit_once("-dev-")
        && !head.is_empty()
        && !tail.is_empty()
    {
        return tail.to_string();
    }
    clip_width(cwd.trim_start_matches('-'), 22)
}

fn scratch_id(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('m') else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits >= 10 && (rest.len() == digits || rest.as_bytes().get(digits) == Some(&b'-'))
}

fn worktree_repo(cwd: &str) -> Option<String> {
    if !cwd.contains('/') && !cwd.contains('\\') {
        return None;
    }
    let parts: Vec<&str> = cwd
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let idx = parts
        .iter()
        .rposition(|part| *part == "worktrees" || *part == ".worktrees")?;
    let repo = parts[..idx].iter().rev().copied().find(|part| {
        *part != ".claude" && *part != ".git"
    })?;
    if repo.is_empty() {
        None
    } else {
        Some(repo.to_string())
    }
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

fn repo_changes(app: &App, cwd: &str) -> Vec<String> {
    let resolved = chats::resolve_cwd(cwd);
    let shown = resolved
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let cwd = if shown.is_empty() { cwd.trim() } else { shown.as_str() };
    if !cwd.is_empty() && mesh::git_root(Path::new(cwd)).is_some() {
        return changed_lines(cwd);
    }
    let name = app
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    changed_lines(&app.dir.display().to_string())
        .into_iter()
        .map(|line| prefix_repo(&name, line))
        .collect()
}

fn prefix_repo(name: &str, line: String) -> String {
    if name.is_empty() {
        return line;
    }
    if let Some(rest) = line.strip_prefix("folder  ") {
        if rest == "." {
            return format!("folder  {name}");
        }
        return format!("folder  {name}/{rest}");
    }
    let Some(rest) = line.strip_prefix("changed  ") else {
        return line;
    };
    if rest == "clean" || rest == "…" || rest.ends_with(" more") {
        format!("changed  {name}  {rest}")
    } else {
        line
    }
}

fn usable_cwd(cwd: &str) -> bool {
    let path = Path::new(cwd);
    path.is_absolute() && path.is_dir()
}

fn image_line(mark: &crate::model::ImageMark) -> String {
    format!(
        "{}  {}  {}x{}  {} bytes  {}",
        mark.token(),
        mark.format,
        mark.width,
        mark.height,
        mark.bytes,
        mark.path.display()
    )
}

fn site_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    if rest.is_empty() {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = match host.rsplit_once(':') {
        Some((name, port))
            if !name.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            name
        }
        _ => host,
    };
    let host = host.strip_prefix("www.").unwrap_or(host).trim();
    if host.is_empty() {
        return None;
    }
    let ok = host.eq_ignore_ascii_case("localhost") || host.contains('.') || host.contains(':');
    ok.then(|| host.to_ascii_lowercase())
}

fn link_target(line: &str) -> Option<&str> {
    let start = line.rfind('<')?;
    let rest = &line[start + 1..];
    let end = rest.find('>')?;
    let href = &rest[..end];
    if href.is_empty() || href.contains(' ') {
        return None;
    }
    let linked = href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("./")
        || href.starts_with("../")
        || href.starts_with('/')
        || href.contains('.');
    linked.then_some(href)
}

fn mouse_cell(x: usize, y: usize) -> Option<(usize, usize)> {
    (x > 0 && y > 0).then_some((x - 1, y - 1))
}

fn ordered_sel(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (ax, ay, bx, by) = sel;
    if (ay, ax) <= (by, bx) {
        (ax, ay, bx, by)
    } else {
        (bx, by, ax, ay)
    }
}

fn slice_chars(chars: &[char], start: usize, end: usize) -> String {
    let start = start.min(chars.len());
    let end = end.min(chars.len()).max(start);
    chars[start..end].iter().collect()
}

fn permit_from(agent: AgentKind, mode: &str) -> Option<Permit> {
    let mapped = match agent {
        AgentKind::Grok => match mode {
            "ask" => Permit::Ask,
            "acceptEdits" => Permit::Edits,
            "plan" => Permit::Plan,
            "always-approve" => Permit::Bypass,
            _ => return None,
        },
        AgentKind::Claude => match mode {
            "default" => Permit::Ask,
            "acceptEdits" => Permit::Edits,
            "plan" => Permit::Plan,
            "bypassPermissions" => Permit::Bypass,
            _ => return None,
        },
        AgentKind::Codex => match mode {
            "on-request" => Permit::Ask,
            "on-failure" => Permit::Edits,
            "never" => Permit::Bypass,
            _ => return None,
        },
    };
    Some(mapped)
}

fn join_url(base: &str, href: &str) -> String {
    let Some((scheme, rest)) = base.split_once("://") else {
        return href.to_string();
    };
    let host = rest.split('/').next().unwrap_or("");
    if href.starts_with('/') {
        return format!("{scheme}://{host}{href}");
    }
    let prefix = if let Some(slash) = rest.find('/') {
        format!("{scheme}://{host}{}", &rest[..slash])
    } else {
        format!("{scheme}://{host}")
    };
    format!("{prefix}/{href}")
}

fn looks_like_burst(keys: &[crate::term::Key]) -> bool {
    let mut chars = 0usize;
    let mut enter_inside = false;
    for (index, key) in keys.iter().enumerate() {
        match key {
            crate::term::Key::Ignore => continue,
            crate::term::Key::Char(_) => chars += 1,
            crate::term::Key::Enter if index + 1 < keys.len() => enter_inside = true,
            crate::term::Key::Enter => {}
            _ => return false,
        }
    }
    chars >= 16 || enter_inside
}

fn burst_is_paste(keys: &[crate::term::Key], clip: &str) -> bool {
    let mut clip = clip.chars().peekable();
    let mut chars = 0usize;
    let mut enter_inside = false;
    for (index, key) in keys.iter().enumerate() {
        while clip.next_if(|ch| *ch == '\r').is_some() {}
        match key {
            crate::term::Key::Ignore => continue,
            crate::term::Key::Char(ch) => {
                if clip.next() != Some(*ch) {
                    return false;
                }
                chars += 1;
            }
            crate::term::Key::Enter => {
                if clip.next() != Some('\n') {
                    return false;
                }
                if index + 1 < keys.len() {
                    enter_inside = true;
                }
            }
            _ => return false,
        }
    }
    chars >= 16 || enter_inside
}

fn write_paste_md(home: &Path, text: &str) -> std::io::Result<PathBuf> {
    let dir = home.join(".granttap-term/paste");
    std::fs::create_dir_all(&dir)?;
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("paste-{n}.md"));
    let mut body = text.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    std::fs::write(&path, body)?;
    Ok(path)
}

fn quote_paste(path: &Path) -> String {
    let text = path.display().to_string();
    if text.chars().any(|ch| ch.is_whitespace()) {
        format!("\"{text}\"")
    } else {
        text
    }
}

fn join_quoted(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| quote_paste(path))
        .collect::<Vec<_>>()
        .join(" ")
}

fn clipboard_files() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        return windows_clipboard_files();
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn parse_hdrop(bytes: &[u8]) -> Vec<PathBuf> {
    if bytes.len() < 20 {
        return Vec::new();
    }
    let offset = u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4])) as usize;
    if offset >= bytes.len() || offset > bytes.len().saturating_sub(1) {
        return Vec::new();
    }
    let wide = u32::from_le_bytes(bytes[16..20].try_into().unwrap_or([0; 4])) != 0;
    let mut paths = Vec::new();
    if wide {
        if offset % 2 != 0 {
            return Vec::new();
        }
        let mut units = Vec::new();
        let mut index = offset;
        while index + 1 < bytes.len() && paths.len() < 64 {
            let unit = u16::from_le_bytes([bytes[index], bytes[index + 1]]);
            index += 2;
            if unit == 0 {
                if units.is_empty() {
                    break;
                }
                let text = String::from_utf16_lossy(&units);
                units.clear();
                if !text.is_empty() {
                    paths.push(PathBuf::from(text));
                }
                continue;
            }
            if units.len() < 32_768 {
                units.push(unit);
            }
        }
        if !units.is_empty() {
            let text = String::from_utf16_lossy(&units);
            if !text.is_empty() {
                paths.push(PathBuf::from(text));
            }
        }
    } else {
        let mut name = Vec::new();
        for byte in &bytes[offset..] {
            if paths.len() >= 64 {
                break;
            }
            if *byte == 0 {
                if name.is_empty() {
                    break;
                }
                let text = String::from_utf8_lossy(&name).into_owned();
                name.clear();
                if !text.is_empty() {
                    paths.push(PathBuf::from(text));
                }
                continue;
            }
            if name.len() < 32_768 {
                name.push(*byte);
            }
        }
        if !name.is_empty() {
            let text = String::from_utf8_lossy(&name).into_owned();
            if !text.is_empty() {
                paths.push(PathBuf::from(text));
            }
        }
    }
    paths
}

#[cfg(windows)]
fn windows_clipboard_files() -> Vec<PathBuf> {
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut core::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut core::ffi::c_void;
        fn GlobalLock(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GlobalUnlock(mem: *mut core::ffi::c_void) -> i32;
        fn GlobalSize(mem: *mut core::ffi::c_void) -> usize;
        fn EnumClipboardFormats(format: u32) -> u32;
        fn GetClipboardFormatNameW(format: u32, name: *mut u16, cch: i32) -> i32;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(
            hdrop: *mut core::ffi::c_void,
            index: u32,
            file: *mut u16,
            cch: u32,
        ) -> u32;
    }
    const CF_HDROP: u32 = 15;
    const MAX: usize = 1024 * 1024;

    struct CloseClip;
    impl Drop for CloseClip {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    fn copy_format(format: u32) -> Option<Vec<u8>> {
        unsafe {
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let size = GlobalSize(handle);
            if size == 0 || size > MAX {
                return None;
            }
            let locked = GlobalLock(handle);
            if locked.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(locked as *const u8, size).to_vec();
            GlobalUnlock(handle);
            Some(bytes)
        }
    }

    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return Vec::new();
        }
        let _close = CloseClip;
        let hdrop = GetClipboardData(CF_HDROP);
        if !hdrop.is_null() {
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, core::ptr::null_mut(), 0);
            if count > 0 && count < 10_000 {
                let mut paths = Vec::new();
                for index in 0..count.min(64) {
                    let need = DragQueryFileW(hdrop, index, core::ptr::null_mut(), 0);
                    if need == 0 || need > 32_768 {
                        continue;
                    }
                    let mut buf = vec![0u16; need as usize + 1];
                    let wrote = DragQueryFileW(hdrop, index, buf.as_mut_ptr(), need + 1);
                    if wrote == 0 {
                        continue;
                    }
                    let text = String::from_utf16_lossy(&buf[..wrote as usize]);
                    if !text.is_empty() {
                        paths.push(PathBuf::from(text));
                    }
                }
                if !paths.is_empty() {
                    return paths;
                }
            }
            if let Some(bytes) = copy_format(CF_HDROP) {
                let paths = parse_hdrop(&bytes);
                if !paths.is_empty() {
                    return paths;
                }
            }
        }
        let mut format = 0u32;
        let mut buf = [0u16; 128];
        let mut file_name = 0u32;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            let n = GetClipboardFormatNameW(format, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 {
                continue;
            }
            let name = String::from_utf16_lossy(&buf[..n as usize]);
            if name.eq_ignore_ascii_case("FileNameW") {
                file_name = format;
                break;
            }
        }
        if file_name == 0 {
            return Vec::new();
        }
        let Some(bytes) = copy_format(file_name) else {
            return Vec::new();
        };
        let mut units = Vec::new();
        let mut index = 0usize;
        while index + 1 < bytes.len() && units.len() < 32_768 {
            let unit = u16::from_le_bytes([bytes[index], bytes[index + 1]]);
            if unit == 0 {
                break;
            }
            units.push(unit);
            index += 2;
        }
        let text = String::from_utf16_lossy(&units);
        if text.is_empty() {
            Vec::new()
        } else {
            vec![PathBuf::from(text)]
        }
    }
}

fn clipboard_image(home: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return windows_clipboard_image(home);
    }
    #[cfg(not(windows))]
    {
        let _ = home;
        None
    }
}

#[cfg(windows)]
fn windows_clipboard_image(home: &Path) -> Option<PathBuf> {
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut core::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut core::ffi::c_void;
        fn GlobalLock(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GlobalUnlock(mem: *mut core::ffi::c_void) -> i32;
        fn GlobalSize(mem: *mut core::ffi::c_void) -> usize;
        fn EnumClipboardFormats(format: u32) -> u32;
        fn GetClipboardFormatNameW(format: u32, name: *mut u16, cch: i32) -> i32;
    }

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const MAX: usize = 20 * 1024 * 1024;

    struct CloseClip;
    impl Drop for CloseClip {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    fn copy_format(format: u32) -> Option<Vec<u8>> {
        unsafe {
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let size = GlobalSize(handle);
            if size == 0 || size > MAX {
                return None;
            }
            let locked = GlobalLock(handle);
            if locked.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(locked as *const u8, size).to_vec();
            GlobalUnlock(handle);
            Some(bytes)
        }
    }

    fn named_format(want: &[&str]) -> Option<u32> {
        unsafe {
            let mut format = 0u32;
            let mut buf = [0u16; 128];
            loop {
                format = EnumClipboardFormats(format);
                if format == 0 {
                    return None;
                }
                let n = GetClipboardFormatNameW(format, buf.as_mut_ptr(), buf.len() as i32);
                if n <= 0 {
                    continue;
                }
                let name = String::from_utf16_lossy(&buf[..n as usize]);
                if want.iter().any(|item| name.eq_ignore_ascii_case(item)) {
                    return Some(format);
                }
            }
        }
    }

    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return None;
        }
        let _close = CloseClip;
        let png = named_format(&["PNG", "image/png"]).and_then(copy_format);
        let dib = copy_format(CF_DIB).or_else(|| copy_format(CF_DIBV5));
        drop(_close);
        if let Some(bytes) = png.filter(|bytes| bytes.starts_with(b"\x89PNG")) {
            let bytes = trim_png(&bytes);
            return write_png_bytes(home, bytes);
        }
        let image = dib.as_deref().and_then(decode_dib)?;
        let path = image_paste_path(home).ok()?;
        image.save(&path).ok()?;
        Some(path)
    }
}

fn trim_png(bytes: &[u8]) -> &[u8] {
    let Some(pos) = bytes.windows(4).position(|window| window == b"IEND") else {
        return bytes;
    };
    let end = pos + 8;
    if end <= bytes.len() { &bytes[..end] } else { bytes }
}

fn image_paste_path(home: &Path) -> std::io::Result<PathBuf> {
    let dir = home.join(".granttap-term/paste");
    std::fs::create_dir_all(&dir)?;
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(dir.join(format!("image-{n}.png")))
}

fn write_png_bytes(home: &Path, bytes: &[u8]) -> Option<PathBuf> {
    let path = image_paste_path(home).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn decode_dib(bytes: &[u8]) -> Option<image::RgbImage> {
    if bytes.len() < 40 {
        return None;
    }
    let bi_size = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    if !(40..=1024).contains(&bi_size) || bi_size > bytes.len() {
        return None;
    }
    let width = i32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height_raw = i32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let planes = u16::from_le_bytes(bytes[12..14].try_into().ok()?);
    let bit_count = u16::from_le_bytes(bytes[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    if planes != 1 || width <= 0 || width > 8000 {
        return None;
    }
    if bit_count != 24 && bit_count != 32 {
        return None;
    }
    if compression != 0 && compression != 3 {
        return None;
    }
    let height_u = height_raw.unsigned_abs();
    if height_u == 0 || height_u > 8000 {
        return None;
    }
    let width = width as usize;
    let height = height_u as usize;
    let top_down = height_raw < 0;
    let stride = ((width * usize::from(bit_count) + 31) / 32) * 4;
    let masks = dib_masks(bytes, compression)?;
    let first = if compression == 3 && bi_size == 40 {
        52
    } else {
        bi_size
    };
    let second = (compression == 3 && bi_size == 40).then_some(56);
    let offset = dib_offset(bytes.len(), stride, height, first, second)?;
    let mut rgb = vec![0u8; width * height * 3];
    let bpp = usize::from(bit_count / 8);
    for row in 0..height {
        let src_row = if top_down { row } else { height - 1 - row };
        let src = offset + src_row * stride;
        for x in 0..width {
            let at = src + x * bpp;
            let pixel = read_pixel(bytes, at, bpp)?;
            let dst = (row * width + x) * 3;
            rgb[dst] = mask_channel(pixel, masks.0);
            rgb[dst + 1] = mask_channel(pixel, masks.1);
            rgb[dst + 2] = mask_channel(pixel, masks.2);
        }
    }
    image::RgbImage::from_raw(width as u32, height as u32, rgb)
}

fn dib_masks(bytes: &[u8], compression: u32) -> Option<(u32, u32, u32)> {
    if compression == 0 {
        return Some((0x00FF_0000, 0x0000_FF00, 0x0000_00FF));
    }
    if bytes.len() < 52 {
        return None;
    }
    let red = u32::from_le_bytes(bytes[40..44].try_into().ok()?);
    let green = u32::from_le_bytes(bytes[44..48].try_into().ok()?);
    let blue = u32::from_le_bytes(bytes[48..52].try_into().ok()?);
    if red == 0 || green == 0 || blue == 0 {
        Some((0x00FF_0000, 0x0000_FF00, 0x0000_00FF))
    } else {
        Some((red, green, blue))
    }
}

fn dib_offset(
    len: usize,
    stride: usize,
    height: usize,
    first: usize,
    second: Option<usize>,
) -> Option<usize> {
    let fits = |offset: usize| -> bool {
        match stride
            .checked_mul(height)
            .and_then(|rows| offset.checked_add(rows))
        {
            Some(need) => need <= len,
            None => false,
        }
    };
    if fits(first) {
        return Some(first);
    }
    second.filter(|offset| fits(*offset))
}

fn read_pixel(bytes: &[u8], at: usize, bpp: usize) -> Option<u32> {
    if at + bpp > bytes.len() {
        return None;
    }
    if bpp == 4 {
        Some(u32::from_le_bytes(bytes[at..at + 4].try_into().ok()?))
    } else if bpp == 3 {
        Some(
            u32::from(bytes[at])
                | (u32::from(bytes[at + 1]) << 8)
                | (u32::from(bytes[at + 2]) << 16),
        )
    } else {
        None
    }
}

fn mask_channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).trailing_ones();
    if bits == 0 || bits > 32 {
        return 0;
    }
    let value = (pixel & mask) >> shift;
    if bits >= 8 {
        (value >> (bits - 8)) as u8
    } else {
        ((value * 255) / ((1u32 << bits) - 1)) as u8
    }
}

fn set_clipboard(text: &str) -> bool {
    #[cfg(windows)]
    {
        return windows_set_clipboard(text);
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        false
    }
}

#[cfg(windows)]
fn windows_set_clipboard(text: &str) -> bool {
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut core::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut core::ffi::c_void;
        fn GlobalLock(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GlobalUnlock(mem: *mut core::ffi::c_void) -> i32;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len().saturating_mul(2));
        if mem.is_null() {
            CloseClipboard();
            return false;
        }
        let locked = GlobalLock(mem) as *mut u16;
        if locked.is_null() {
            CloseClipboard();
            return false;
        }
        core::ptr::copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        GlobalUnlock(mem);
        let ok = !SetClipboardData(CF_UNICODETEXT, mem).is_null();
        CloseClipboard();
        ok
    }
}

fn clipboard_text() -> Option<String> {
    #[cfg(windows)]
    {
        return windows_clipboard();
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_clipboard() -> Option<String> {

    const CF_UNICODETEXT: u32 = 13;
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut core::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut core::ffi::c_void;
        fn GlobalLock(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        fn GlobalUnlock(mem: *mut core::ffi::c_void) -> i32;
    }
    unsafe {
        if OpenClipboard(core::ptr::null_mut()) == 0 {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            CloseClipboard();
            return None;
        }
        let locked = GlobalLock(handle) as *const u16;
        if locked.is_null() {
            CloseClipboard();
            return None;
        }
        let mut units = Vec::new();
        let mut index = 0isize;
        while index < 65_536 {
            let unit = *locked.offset(index);
            if unit == 0 {
                break;
            }
            units.push(unit);
            index += 1;
        }
        GlobalUnlock(handle);
        CloseClipboard();
        let text = String::from_utf16_lossy(&units);
        if text.is_empty() { None } else { Some(text) }
    }
}

fn changed_lines(cwd: &str) -> Vec<String> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Vec::new();
    }
    let Some(root) = mesh::git_root(Path::new(cwd)) else {
        return Vec::new();
    };
    match gitread::change_view(&root) {
        Ok(lines) if lines.is_empty() => vec!["changed  clean".into()],
        Ok(lines) => lines,
        Err(_) => Vec::new(),
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

fn agent_commands(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::Grok => &[
            "/model",
            "/effort",
            "/always-approve",
            "/auto",
            "/compact",
            "/context",
            "/new",
            "/clear",
            "/resume",
            "/rewind",
            "/undo",
            "/fork",
            "/rename",
            "/session-info",
            "/status",
            "/info",
            "/plan",
            "/memory",
            "/remember",
            "/vim-mode",
            "/multiline",
            "/ml",
            "/m",
            "/history",
            "/dashboard",
            "/home",
            "/quit",
            "/exit",
            "/copy",
            "/export",
            "/delete",
            "/context-window",
            "/skills",
            "/hooks",
            "/plugins",
            "/imagine",
            "/goal",
            "/workflow",
            "/doctor",
            "/feedback",
            "/usage",
            "/settings",
            "/theme",
            "/docs",
        ],
        AgentKind::Claude => &[
            "/model",
            "/effort",
            "/compact",
            "/clear",
            "/plan",
            "/context",
            "/usage",
            "/cost",
            "/memory",
            "/init",
            "/doctor",
            "/permissions",
            "/mcp",
            "/plugin",
            "/resume",
            "/rewind",
            "/rename",
            "/diff",
            "/status",
            "/config",
            "/help",
            "/exit",
            "/login",
            "/logout",
            "/hooks",
            "/sandbox",
            "/fast",
            "/skills",
            "/code-review",
            "/review",
            "/add-dir",
            "/export",
            "/bug",
            "/ide",
            "/output-style",
            "/loop",
            "/goal",
            "/security-review",
        ],
        AgentKind::Codex => &[
            "/model",
            "/approvals",
            "/permissions",
            "/status",
            "/review",
            "/diff",
            "/init",
            "/compact",
            "/clear",
            "/new",
            "/plan",
            "/resume",
            "/fork",
            "/quit",
            "/exit",
            "/copy",
            "/skills",
            "/mcp",
            "/fast",
            "/goal",
            "/rename",
            "/usage",
            "/mention",
            "/debug-config",
        ],
    }
}

fn agent_suggestions(agent: AgentKind, cmd: &str) -> Vec<String> {
    let commands = agent_commands(agent);
    let trimmed = cmd.trim_start();
    let mut parts = trimmed.split_whitespace();
    let head = parts.next().unwrap_or("");
    let second = parts.next();
    let third = parts.next();
    let trailing = trimmed.ends_with(' ');
    let mut found = Vec::new();
    if head.is_empty() {
        found.extend(commands.iter().take(6).map(|name| (*name).to_string()));
        return found;
    }
    if head == "/effort" && third.is_none() && (trailing || second.is_some()) {
        let partial = if trailing && second.is_none() {
            ""
        } else {
            second.unwrap_or("")
        };
        for effort in agent.efforts() {
            if effort.starts_with(partial) && *effort != partial {
                found.push(format!("/effort {effort}"));
            }
        }
        found.truncate(6);
        return found;
    }
    if second.is_none() && !trailing {
        for name in commands {
            if name.starts_with(head) && *name != head {
                found.push((*name).to_string());
            }
        }
    }
    found.truncate(6);
    found
}

fn fresh_outbox(transcript: &[String], outbox: Vec<String>) -> Vec<String> {
    outbox
        .into_iter()
        .filter(|line| {
            let text = line
                .trim()
                .strip_prefix("user  ")
                .unwrap_or(line.trim())
                .trim();
            !text.is_empty() && !transcript.iter().any(|row| row.contains(text))
        })
        .collect()
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
                window: None,
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
                window: None,
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
        let gold = screen.frame.split("38;5;220m>").nth(1).unwrap_or("");
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
                window: None,
        });
        let key = app.mesh_key();
        app.mesh_accept.push((key, AcceptLevel::Full));
        let face = app.composer_face();
        assert_eq!(face.agent, AgentKind::Grok);
        assert_eq!(face.model, "grok-4.7");
        assert_eq!(face.effort, "xhigh");
        assert_eq!(face.permit, "always-approve");
        let idle = crate::ui::Screen::paint(&app, 100, 24);
        assert!(idle.frame.contains("grok-4.7"), "{}", idle.frame);
        assert!(idle.frame.contains("always-approve"));
        assert!(!idle.frame.contains("accept "), "{}", idle.frame);
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
    fn the_browser_hides_the_chat_controls() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(120, 30);
        let chats = crate::ui::Screen::paint(&app, 120, 30);
        assert!(!chats.frame.contains("accept "), "{}", chats.frame);
        let chat_chip = (0..30).any(|y| {
            (0..120).any(|x| matches!(chats.spot_at(x, y), Some(crate::ui::Spot::Chip(_))))
        });
        assert!(chat_chip, "chat chips stay on the desk");
        app.section = Section::Browser;
        let browser = crate::ui::Screen::paint(&app, 120, 30);
        assert!(!browser.frame.contains("accept "), "{}", browser.frame);
        assert!(!browser.frame.contains("always-approve"), "{}", browser.frame);
        let browser_chip = (0..30).any(|y| {
            (0..120).any(|x| matches!(browser.spot_at(x, y), Some(crate::ui::Spot::Chip(_))))
        });
        assert!(!browser_chip, "chat chips stay off the browser");
        assert!(
            (0..120).any(|x| browser.spot_at(x, 0) == Some(crate::ui::Spot::Theme)),
            "theme stays"
        );
        assert!(
            (0..120).any(|x| browser.spot_at(x, 0) == Some(crate::ui::Spot::Lang)),
            "language stays"
        );
        assert!(browser.frame.contains("[reload]"), "{}", browser.frame);
        assert!(browser.frame.contains("js "), "{}", browser.frame);
    }

    #[test]
    fn a_claude_chip_uses_its_model_and_its_own_permission() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-claude-face-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-claude-dir-{n}"));
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            home.join(".claude/settings.json"),
            "{\"model\":\"opus[1m]\",\"effortLevel\":\"xhigh\",\"hooks\":{\"keep\":true}}\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".grok/config.toml"),
            "[ui]\npermission_mode = \"ask\"\n\n[models]\ndefault = \"grok-4.7\"\ndefault_reasoning_effort = \"xhigh\"\n",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.open_id = "claude".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Claude),
        };
        let face = app.composer_face();
        assert_eq!(face.model, "opus[1m]");
        assert_eq!(face.effort, "xhigh");
        assert_eq!(face.permit, "default");
        assert_ne!(face.model, face.effort);
        let screen = crate::ui::Screen::paint(&app, 140, 28);
        assert!(screen.frame.contains("opus[1m]"), "{}", screen.frame);
        choose_chip(&mut app, 140, 28, crate::ui::Chip::Permit, "acceptEdits");
        let local = std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        assert!(local.contains("acceptEdits"), "{local}");
        let grok = std::fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(grok.contains("permission_mode = \"ask\""), "{grok}");
        choose_chip(&mut app, 140, 28, crate::ui::Chip::Model, "opus");
        let local = std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap();
        assert!(local.contains("\"model\": \"opus\""), "{local}");
        assert!(!local.contains("opus[1m]"), "{local}");
        assert!(local.contains("acceptEdits"), "{local}");
        assert_eq!(app.composer_face().model, "opus");
        app.cmd = "/effort ".into();
        assert!(app.suggestions().iter().any(|name| name == "/effort max"));
        assert!(!app.suggestions().iter().any(|name| name == "/always-approve"));
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_claude_model_click_follows_the_open_chat() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-claude-cwd-{n}"));
        let desk = std::env::temp_dir().join(format!("granttap-claude-desk-{n}"));
        let repo = std::env::temp_dir().join(format!("granttap-claude-repo-{n}"));
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&desk).unwrap();
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        std::fs::write(
            home.join(".claude/settings.json"),
            "{\"model\":\"opus[1m]\",\"effortLevel\":\"xhigh\",\"ultracode\":true,\"hooks\":{\"keep\":true}}\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".claude/settings.local.json"),
            "{\"permissions\":{\"defaultMode\":\"bypassPermissions\"}}\n",
        )
        .unwrap();
        let mut app = App::new(desk.clone(), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Claude,
            id: "live-claude".into(),
            title: "front".into(),
            path: PathBuf::new(),
            cwd: repo.display().to_string(),
            live: false,
            at: 1,
            tokens: None,
                window: None,
        });
        app.open_id = "live-claude".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Claude),
        };
        let face = app.composer_face();
        assert_eq!(face.model, "opus[1m]");
        assert_eq!(face.effort, "xhigh");
        assert!(face.ultra);
        assert_eq!(face.permit, "bypassPermissions");
        let screen = crate::ui::Screen::paint(&app, 160, 28);
        assert!(screen.frame.contains("xhigh · ultra"), "{}", screen.frame);
        choose_chip(&mut app, 160, 28, crate::ui::Chip::Model, "opus");
        assert_eq!(app.composer_face().model, "opus");
        let user = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(user.contains("\"model\":\"opus\""), "{user}");
        assert!(user.contains("\"hooks\":{\"keep\":true}"), "{user}");
        assert!(user.contains("\"ultracode\":true"), "{user}");
        let local = std::fs::read_to_string(repo.join(".claude/settings.local.json")).unwrap();
        assert!(local.contains("\"model\": \"opus\""), "{local}");
        assert!(local.contains("bypassPermissions"), "{local}");
        assert!(!desk.join(".claude/settings.local.json").exists());
        choose_chip(&mut app, 160, 28, crate::ui::Chip::Effort, "max");
        assert_eq!(app.composer_face().effort, "max");
        let user = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(user.contains("\"effortLevel\":\"xhigh\""), "{user}");
        assert!(!user.contains("modelSettings"), "{user}");
        let again = crate::ui::Screen::paint(&app, 160, 28);
        assert!(again.frame.contains("max · ultra"), "{}", again.frame);
        choose_chip(&mut app, 160, 28, crate::ui::Chip::Effort, "low");
        assert_eq!(app.composer_face().effort, "low");
        let user = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        assert!(user.contains("\"effortLevel\":\"low\""), "{user}");
        assert!(user.contains("claude-opus-5-5"), "{user}");
        assert!(user.contains("\"hooks\":{\"keep\":true}"), "{user}");
        assert!(user.contains("\"ultracode\":true"), "{user}");
        let shown = crate::ui::Screen::paint(&app, 160, 28);
        assert!(shown.frame.contains("low · ultra"), "{}", shown.frame);
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(desk);
        let _ = std::fs::remove_dir_all(repo);
    }

    fn find_chip(screen: &crate::ui::Screen, chip: crate::ui::Chip) -> (usize, usize) {
        for y in 0..40 {
            for x in 0..200 {
                if screen.spot_at(x, y) == Some(crate::ui::Spot::Chip(chip)) {
                    return (x, y);
                }
            }
        }
        panic!("missing chip");
    }

    fn choose_chip(app: &mut App, cols: usize, rows: usize, chip: crate::ui::Chip, label: &str) {
        let screen = crate::ui::Screen::paint(app, cols, rows);
        let (x, y) = find_chip(&screen, chip);
        app.press(screen.spot_at(x, y).expect("chip"), 1);
        assert_eq!(app.pick(), Some(chip));
        let shown = crate::ui::Screen::paint(app, cols, rows);
        assert!(shown.frame.contains("┌"), "picker frame");
        let mut picked = None;
        for y in 0..rows {
            for x in 0..cols {
                if let Some(crate::ui::Spot::Pick(index)) = shown.spot_at(x, y)
                    && app.pick_choices().get(usize::from(index)) == Some(&label)
                {
                    picked = Some(crate::ui::Spot::Pick(index));
                }
            }
        }
        let Some(spot) = picked else {
            panic!("missing picker row {label}");
        };
        app.press(spot, 1);
        assert!(app.pick().is_none());
    }

    #[test]
    fn a_chip_opens_a_framed_picker_and_escape_closes_it() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-pick-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-pick-dir-{n}"));
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
            title: "Ship".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 1,
            tokens: None,
            window: None,
        });
        app.open_id = "s".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        let screen = crate::ui::Screen::paint(&app, 120, 28);
        let (x, y) = find_chip(&screen, crate::ui::Chip::Permit);
        app.press(screen.spot_at(x, y).unwrap(), 1);
        assert_eq!(app.pick(), Some(crate::ui::Chip::Permit));
        let shown = crate::ui::Screen::paint(&app, 120, 28);
        assert!(shown.frame.contains("Approval"), "{}", shown.frame);
        assert!(shown.frame.contains("┌"), "{}", shown.frame);
        assert!(shown.frame.contains("[X]"), "{}", shown.frame);
        assert!(shown.frame.contains("always-approve"), "{}", shown.frame);
        assert!(shown.frame.contains("Esc"), "{}", shown.frame);
        assert_eq!(shown.spot_at(2, 2), Some(crate::ui::Spot::PickClose));
        app.press(shown.spot_at(x, y).unwrap(), 1);
        assert!(app.pick().is_none());
        let screen = crate::ui::Screen::paint(&app, 120, 28);
        let (x, y) = find_chip(&screen, crate::ui::Chip::Model);
        app.press(screen.spot_at(x, y).unwrap(), 1);
        let shown = crate::ui::Screen::paint(&app, 120, 28);
        assert!(shown.frame.contains("Model"), "{}", shown.frame);
        assert!(shown.frame.contains("grok-4.7"), "{}", shown.frame);
        app.on_key(Key::Down);
        app.on_key(Key::Enter);
        assert!(app.pick().is_none());
        assert_eq!(app.composer_face().model, "grok-4.5");
        let screen = crate::ui::Screen::paint(&app, 120, 28);
        let (x, y) = find_chip(&screen, crate::ui::Chip::Effort);
        app.press(screen.spot_at(x, y).unwrap(), 1);
        assert_eq!(app.pick(), Some(crate::ui::Chip::Effort));
        app.on_key(Key::Esc);
        assert!(app.pick().is_none());
        assert_eq!(app.composer_face().effort, "xhigh");
        app.press(crate::ui::Spot::Chip(crate::ui::Chip::Model), 1);
        app.press(crate::ui::Spot::PickClose, 1);
        assert!(app.pick().is_none());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_drag_copies_the_cells_and_a_click_still_presses() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.keep_rows(vec!["hello desk".into(), "second line".into()]);
        app.apply_keys(
            &[Key::MouseDown { x: 2, y: 3 }],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert!(!app.desc_open);
        assert!(app.selection().is_none());
        app.apply_keys(
            &[
                Key::MouseDown { x: 1, y: 1 },
                Key::Drag { x: 5, y: 1 },
                Key::Release { x: 5, y: 1 },
            ],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert_eq!(app.copied, "hello");
        assert!(!app.desc_open);
        assert!(app.selection().is_some());
        let painted = crate::ui::Screen::paint(&app, 80, 24);
        assert!(painted.frame.contains("1;30;48;5;220"), "{}", painted.frame);
        app.keep_rows(vec!["hello desk".into(), "second line".into()]);
        app.apply_keys(
            &[
                Key::MouseDown { x: 1, y: 1 },
                Key::Drag { x: 3, y: 2 },
                Key::Release { x: 3, y: 2 },
            ],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert_eq!(app.copied, "hello desk\nsec");
        assert!(!app.desc_open);
        app.apply_keys(
            &[Key::MouseDown { x: 2, y: 3 }, Key::Release { x: 2, y: 3 }],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert!(app.desc_open);
        assert!(app.selection().is_none());
        app.apply_keys(
            &[
                Key::MouseDown { x: 4, y: 4 },
                Key::Drag { x: 5, y: 4 },
                Key::Release { x: 5, y: 4 },
            ],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert!(!app.desc_open);
        app.apply_keys(
            &[Key::Click { x: 2, y: 3 }],
            |_, _| Some(crate::ui::Spot::Desc),
        );
        assert!(app.desc_open);
        app.on_key(Key::Char('z'));
        assert!(app.selection().is_none());
    }

    #[test]
    fn a_drag_copies_only_its_cells_and_ctrl_c_quits_on_the_second_press() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.keep_rows(vec![
            "aaaaHELLO bbbb".into(),
            "cccc world dddd".into(),
            "eeeeeeeeeeeeee".into(),
        ]);
        app.apply_keys(
            &[
                Key::MouseDown { x: 5, y: 1 },
                Key::Drag { x: 9, y: 1 },
                Key::Release { x: 9, y: 1 },
            ],
            |_, _| None,
        );
        assert_eq!(app.copied, "HELLO");
        assert!(!app.copied.contains("world"));
        assert!(!app.copied.contains("eeee"));
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        assert_eq!(app.note, "copied");
        assert_eq!(app.copied, "HELLO");
        assert!(app.selection().is_none());
        assert_eq!(app.on_key(Key::CtrlC), Effect::Quit);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.keep_rows(vec!["one".into(), "two".into(), "three".into()]);
        app.apply_keys(
            &[
                Key::MouseDown { x: 1, y: 1 },
                Key::Drag { x: 3, y: 9000 },
                Key::Release { x: 3, y: 9000 },
            ],
            |_, _| None,
        );
        assert!(app.copied.lines().count() <= 3, "{}", app.copied);
        assert!(app.copied.len() < 40, "{}", app.copied);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        assert_eq!(app.note, "ctrl+c  repeat to quit");
        assert_eq!(app.on_key(Key::Char('z')), Effect::None);
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        assert_eq!(app.on_key(Key::CtrlC), Effect::Quit);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        assert_eq!(app.on_key(Key::F(10)), Effect::Quit);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.on_key(Key::Char(','));
        assert!(app.settings_open());
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        assert!(app.settings_open());
        assert_eq!(app.note, "ctrl+c  repeat to quit");
        assert_eq!(app.on_key(Key::CtrlC), Effect::Quit);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        app.apply_keys(&[Key::Move { x: 4, y: 4 }], |_, _| None);
        assert_eq!(app.on_key(Key::CtrlC), Effect::Quit);

        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        app.apply_keys(&[Key::Click { x: 2, y: 3 }], |_, _| None);
        assert_eq!(app.on_key(Key::CtrlC), Effect::None);
        assert_eq!(app.note, "ctrl+c  repeat to quit");
    }

    #[test]
    fn a_folder_folds_and_a_markdown_file_opens_in_a_wide_frame() {
        let lines = vec![
            "folder  .".into(),
            "stat  readme.md  1  0".into(),
            "folder  src".into(),
            "stat  app.rs  2  1".into(),
            "stat  notes.md  3  1".into(),
        ];
        let open = super::fold_lines(&lines, &[], &[]);
        assert_eq!(open[0].tree, Some(super::TREE_ALL));
        assert!(open[0].text.contains('▼'), "{}", open[0].text);
        assert!(open[0].text.contains("6"), "{}", open[0].text);
        assert!(open[0].text.contains("2"), "{}", open[0].text);
        assert_eq!(open[1].tree, Some(0));
        assert_eq!(open[2].doc, Some(0));
        assert!(open[4].doc.is_none(), "{:?}", open[4].text);
        assert_eq!(open[5].doc, Some(1));
        let shut = super::fold_lines(&lines, &[], &["src".into()]);
        let shown = shut
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains('▶'), "{shown}");
        assert!(shown.contains("readme.md"), "{shown}");
        assert!(!shown.contains("app.rs"), "{shown}");
        assert!(!shown.contains("notes.md"), "{shown}");
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-doc-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let readme = dir.join("readme.md");
        std::fs::write(&readme, "# Hello\n\nbody line of the note\n").unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.section = Section::Chats;
        app.fill = true;
        app.mode = Mode::Split {
            lines: vec![
                "folder  .".into(),
                "stat  readme.md  1  0".into(),
                "folder  src".into(),
                "stat  app.rs  2  1".into(),
            ],
            agent: Some(AgentKind::Grok),
        };
        let screen = crate::ui::Screen::paint(&app, 120, 36);
        assert!(screen.frame.contains("▼ ./"), "{}", screen.frame);
        assert!(screen.frame.contains("▼ src/"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;220mreadme.md"), "{}", screen.frame);
        assert!(
            (0..36).any(|y| (0..120).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Tree(0)))),
            "folder row is not clickable"
        );
        assert!(
            (0..36).any(|y| (0..120).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Doc(0)))),
            "markdown row is not clickable"
        );
        assert!(
            !(0..36).any(|y| (0..120).any(|x| matches!(screen.spot_at(x, y), Some(crate::ui::Spot::Doc(1))))),
            "a rust row must stay inert"
        );
        app.press(crate::ui::Spot::Tree(1), 1);
        let folded = crate::ui::Screen::paint(&app, 120, 36);
        assert!(folded.frame.contains("▶ src/"), "{}", folded.frame);
        assert!(!folded.frame.contains("app.rs"), "{}", folded.frame);
        app.press(crate::ui::Spot::Tree(1), 1);
        app.press(crate::ui::Spot::Doc(0), 1);
        assert!(app.doc_open());
        assert!(matches!(app.mode, Mode::Split { .. }));
        let frame = crate::ui::Screen::paint(&app, 120, 36);
        assert!(frame.frame.contains("readme.md"), "{}", frame.frame);
        assert!(frame.frame.contains("body line of the note"), "{}", frame.frame);
        assert!(frame.frame.contains("38;5;220m # Hello"), "{}", frame.frame);
        assert!(frame.frame.contains('┌'), "{}", frame.frame);
        assert!(frame.frame.contains("[X]"), "{}", frame.frame);
        assert!(
            frame.frame.contains(&"─".repeat(90)),
            "the frame should be wider than the context box"
        );
        assert!(!frame.frame.contains("vim  readme"), "{}", frame.frame);
        assert_eq!(frame.spot_at(0, 3), Some(crate::ui::Spot::DocClose));
        app.on_key(Key::Down);
        assert_eq!(app.doc_scroll, 1);
        app.on_key(Key::Char('z'));
        assert!(app.cmd.is_empty());
        app.on_key(Key::Esc);
        assert!(!app.doc_open());
        app.press(crate::ui::Spot::Doc(0), 1);
        app.press(crate::ui::Spot::DocClose, 1);
        assert!(!app.doc_open());
        app.opens.push(OpenFile {
            n: 1,
            path: readme,
            line: 0,
        });
        app.press(crate::ui::Spot::Open(1), 1);
        assert!(app.doc_open());
        assert!(matches!(app.mode, Mode::Split { .. }));
        assert_eq!(app.doc_title, "readme.md");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_zero_diff_stays_out_and_the_tree_starts_as_one_total() {
        let lines = vec![
            "folder  .".into(),
            "stat  ui.rs  2945  106".into(),
            "stat  usage.rs  0  0".into(),
            "folder  план".into(),
            "stat  агенты.md  0  0".into(),
            "assistant  keep the talk".into(),
        ];
        let shut = super::fold_lines(&lines, &[], &["*".into()]);
        let text = shut
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains('▶'), "{text}");
        assert!(text.contains("2945"), "{text}");
        assert!(text.contains("106"), "{text}");
        assert!(!text.contains("usage.rs"), "{text}");
        assert!(!text.contains("агенты"), "{text}");
        assert!(!text.contains("ui.rs"), "{text}");
        assert!(text.contains("keep the talk"), "{text}");
        let open = super::fold_lines(&lines, &[], &[]);
        let shown = open
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("ui.rs"), "{shown}");
        assert!(shown.contains('▼'), "{shown}");
        assert!(!shown.contains("usage.rs"), "{shown}");
        assert!(!shown.contains("агенты"), "{shown}");
        assert!(!shown.contains("план"), "{shown}");
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-zero-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines: lines.clone(),
            agent: Some(AgentKind::Grok),
        };
        app.tree_shut = vec!["*".into()];
        let collapsed = crate::ui::Screen::paint(&app, 100, 24);
        assert!(collapsed.frame.contains("+2945"), "{}", collapsed.frame);
        assert!(collapsed.frame.contains("-106"), "{}", collapsed.frame);
        assert!(!collapsed.frame.contains("usage.rs"), "{}", collapsed.frame);
        assert!(!collapsed.frame.contains("ui.rs"), "{}", collapsed.frame);
        assert!(
            (0..24).any(|y| {
                (0..100).any(|x| {
                    collapsed.spot_at(x, y) == Some(crate::ui::Spot::Tree(super::TREE_ALL))
                })
            }),
            "the total row is not clickable"
        );
        app.press(crate::ui::Spot::Tree(super::TREE_ALL), 1);
        let expanded = crate::ui::Screen::paint(&app, 100, 24);
        assert!(expanded.frame.contains("ui.rs"), "{}", expanded.frame);
        assert!(expanded.frame.contains("+2945"), "{}", expanded.frame);
        assert!(!expanded.frame.contains("usage.rs"), "{}", expanded.frame);
        assert!(!expanded.frame.contains("агенты"), "{}", expanded.frame);
        app.press(crate::ui::Spot::Tree(super::TREE_ALL), 1);
        let again = crate::ui::Screen::paint(&app, 100, 24);
        assert!(!again.frame.contains("ui.rs"), "{}", again.frame);
        assert!(again.frame.contains("+2945"), "{}", again.frame);
        let mut chat = sample_chat(
            AgentKind::Grok,
            "pane",
            &dir.display().to_string(),
            10,
            None,
        );
        chat.path = dir.clone();
        chat.id = "pane-id".into();
        app.open_id = "somewhere-else".into();
        app.show_chat(&chat, false);
        assert!(
            app.tree_shut.iter().any(|item| item == "*"),
            "{:?}",
            app.tree_shut
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_open_tree_stays_put_when_older_lines_arrive() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-tree-scroll-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut older = String::new();
        for index in 0..30 {
            older.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"older-{index:02}\"}}\n"
            ));
        }
        let marker_at = older.len() as u64;
        older.push_str("{\"type\":\"user\",\"content\":\"tail-marker\"}\n");
        let history = dir.join("chat_history.jsonl");
        std::fs::write(&history, older).unwrap();
        let mut lines = vec!["folder  src".into()];
        for index in 0..20 {
            lines.push(format!("stat  file-{index:02}.rs  1  0"));
        }
        let hist_at = lines.len();
        for index in 0..40 {
            lines.push(format!("assistant  TAIL-{index:02}"));
        }
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.set_size(100, 28);
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines,
            agent: Some(AgentKind::Grok),
        };
        app.tree_shut.clear();
        app.hist_path = history;
        app.hist_from = marker_at;
        app.hist_at = hist_at;
        app.pane_from_end = 40;
        let before = crate::ui::Screen::paint(&app, 100, 28);
        assert!(before.frame.contains("file-01.rs"), "{}", before.frame);
        app.grow_older();
        let after = crate::ui::Screen::paint(&app, 100, 28);
        assert!(
            after.frame.contains("file-01.rs"),
            "the open list left the screen\n{}",
            after.frame
        );
        assert!(
            app.transcript_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("older-00")),
            "older page was not read"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_tall_dropdown_keeps_its_head_on_screen() {
        let mut lines = vec!["folder  src".into()];
        for index in 0..40 {
            lines.push(format!("stat  file-{index:02}.rs  2  1"));
        }
        lines.push("assistant  keep the talk".into());
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(100, 24);
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines,
            agent: Some(AgentKind::Grok),
        };
        app.tree_shut = vec!["*".into()];
        let collapsed = crate::ui::Screen::paint(&app, 100, 24);
        assert!(collapsed.frame.contains("keep the talk"), "{}", collapsed.frame);
        assert!(!collapsed.frame.contains("file-00.rs"), "{}", collapsed.frame);
        app.press(crate::ui::Spot::Tree(super::TREE_ALL), 1);
        let opened = crate::ui::Screen::paint(&app, 100, 24);
        assert!(opened.frame.contains("file-00.rs"), "{}", opened.frame);
        assert!(opened.frame.contains("file-01.rs"), "{}", opened.frame);
        assert!(
            !opened.frame.contains("file-39.rs"),
            "the list jumped to its tail\n{}",
            opened.frame
        );
    }

    #[test]
    fn task_has_one_owner_and_move_changes_the_chat() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-task-{n}"));
        let mut app = App::new(PathBuf::from("."), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Claude,
            id: "abc".into(),
            title: "abc".into(),
            path: PathBuf::new(),
            cwd: String::new(),
            live: false,
            at: 0,
            tokens: None,
            window: None,
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
        let _ = std::fs::remove_dir_all(home);
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
            window: None,
        });
        app.cursor = 0;
        app.cmd = "task write the desk".into();
        app.on_key(Key::Enter);
        assert_eq!(app.tasks[0].owner, AgentKind::Claude);
        let lines = app.lines();
        assert!(
            lines.first().is_some_and(|line| line.contains("новая задача")),
            "{lines:?}"
        );
        let wait = lines.iter().position(|line| line.contains("ждут")).unwrap();
        app.cursor = lines
            .iter()
            .position(|line| line.contains("write the desk"))
            .unwrap();
        assert!(wait < app.cursor, "{lines:?}");
        let row = lines[app.cursor].clone();
        assert!(row.contains("Claude"), "{row}");
        assert!(row.contains('◇'), "{row}");
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
        assert!(app.description_line().contains("F2 or F4 writes"));
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
                window: None,
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
                window: None,
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
                window: None,
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
                window: None,
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
        assert!(
            tasks[0].contains("новая задача"),
            "{tasks:?}"
        );
        assert!(tasks.iter().any(|line| line.contains("ждут")), "{tasks:?}");
        assert!(
            tasks.iter().any(|line| line.contains("desk towers")),
            "{tasks:?}"
        );
        assert!(tasks.iter().all(|line| !line.ends_with("Grok")), "{tasks:?}");
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
            window: None,
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
        assert!(tasks[0].contains("новая задача"), "{tasks:?}");
        assert!(at("новая задача") < at("ждут"), "{tasks:?}");
        assert!(tasks[at("grok job")].contains('◇'), "{tasks:?}");
        assert!(at("grok job") > at("ждут"), "{tasks:?}");
        assert!(at("claude job") > at("ждут"), "{tasks:?}");
        assert!(tasks[at("grok job")].ends_with("Grok"), "{tasks:?}");
        assert!(tasks[at("claude job")].ends_with("Claude"), "{tasks:?}");
        assert!(
            tasks.iter().all(|line| line.contains('◇')
                || line.contains('▼')
                || line.contains("новая задача")),
            "{tasks:?}"
        );
    }

    #[test]
    fn context_tip_clock_and_a_collapsed_fold_use_real_counts() {
        let tip = super::context_tip_line(203_543, 256_000);
        assert!(tip.contains("79.5%"), "{tip}");
        assert!(tip.contains("203.5k / 256k"), "{tip}");
        assert!(tip.contains("dashboard"), "{tip}");
        assert_eq!(super::clock_face("11-11 14:54"), "11-11  2:54 PM");
        assert_eq!(super::clock_face("11-11 00:05"), "11-11  12:05 AM");
        let think = super::fold_lines(&["think".into(), "assistant  done".into()], &[], &[]);
        assert_eq!(think[0].text, "◆  Thinking...");
        assert_eq!(think[1].text, "assistant  done");
        let tools = [
            "tool  read_file".into(),
            "tool  search_replace  app.rs  +63  -13".into(),
        ];
        let closed = super::fold_lines(&tools, &[], &[]);
        assert_eq!(
            closed.len(),
            1,
            "{:?}",
            closed.iter().map(|row| &row.text).collect::<Vec<_>>()
        );
        assert!(closed[0].text.starts_with('◆'), "{}", closed[0].text);
        assert!(closed[0].text.contains("+63"), "{}", closed[0].text);
        assert!(closed[0].text.contains("-13"), "{}", closed[0].text);
        let open = super::fold_lines(&tools, &[0], &[]);
        let shown = open
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("app.rs"), "{shown}");
        assert!(shown.contains("Read"), "{shown}");
        assert!(shown.contains("+63/-13"), "{shown}");
    }

    #[test]
    fn context_sheet_names_the_grok_buckets_inside_a_frame() {
        let mut app = App::new(PathBuf::from("/tmp/night"), PathBuf::from("/nowhere-home"));
        app.chats = vec![crate::model::Chat {
            agent: AgentKind::Grok,
            id: "id-desk".into(),
            title: "desk towers".into(),
            path: PathBuf::new(),
            cwd: r"C:\no\RIGHTMARK".into(),
            live: false,
            at: 86_400,
            tokens: Some(100_000),
            window: Some(256_000),
        }];
        app.open_id = "id-desk".into();
        app.meter_open = true;
        app.meter_model = "grok-4.7".into();
        app.meter_split = chats::ContextSplit {
            system: 2_000,
            messages: 60_000,
            reasoning: 38_000,
            tools_tokens: 9_000,
            tools: 4,
            skills_tokens: 0,
            skills: 0,
            flows_tokens: 100,
            flows: 1,
            mcp_tokens: 0,
            mcps: 0,
        };
        let lines = app.meter_body();
        let text = lines.join("\n");
        assert!(text.contains("100k / 256k tokens (39.06%)"), "{text}");
        assert!(text.contains("grok-4.7"), "{text}");
        assert!(text.contains("System prompt"), "{text}");
        assert!(text.contains("Messages"), "{text}");
        assert!(text.contains("Reasoning/overhead"), "{text}");
        assert!(text.contains("Free"), "{text}");
        assert!(text.contains("Already counted above"), "{text}");
        assert!(text.contains("Tool definitions"), "{text}");
        assert!(text.contains("4 tools"), "{text}");
        assert!(text.contains("1 workflow"), "{text}");
        assert!(text.contains("(0.1%)"), "{text}");
        assert!(!text.contains("Skills"), "{text}");
        assert!(!text.contains("MCP"), "{text}");
        assert!(text.contains('◆'), "{text}");
        assert!(text.contains('◇'), "{text}");
        assert!(text.contains("Auto-compact at 80% · ~105k tokens remaining"), "{text}");
        assert!(text.contains("[compact context]"), "{text}");
        let screen = crate::ui::Screen::paint(&app, 120, 40);
        assert!(screen.frame.contains('┌'), "{}", screen.frame);
        assert!(screen.frame.contains("System prompt"), "{}", screen.frame);
        assert!(screen.frame.contains("Context usage"), "{}", screen.frame);
        assert!(screen.frame.contains("copy session ID"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;81"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;141"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;240"), "{}", screen.frame);
        assert!(
            screen.frame.contains("RIGHTMARK"),
            "the chat beside the box was wiped"
        );
        app.meter_tab = 2;
        app.press(crate::ui::Spot::Meter, 1);
        assert!(app.meter_open());
        assert_eq!(app.meter_tab(), 2);
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
                window: Some(100_000),
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
        let gold = screen.frame.split("38;5;220m>").nth(1).unwrap_or("");
        assert!(gold.contains("Name the desk"), "{gold}");
        app.cursor = 0;
        let screen = crate::ui::Screen::paint(&app, 80, 24);
        assert!(screen.frame.contains("38;5;39"), "{}", screen.frame);
    }

    #[test]
    fn windows_cwd_uses_the_project_folder_as_the_mesh() {
        let root = PathBuf::from(r"C:\work\weavatrix");
        let mut app = App::new(root.clone(), PathBuf::from("/nowhere-home"));
        app.repos = vec![("weavatrix".into(), root)];
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "GrantTap Term Windows",
            r"C:\work\weavatrix\src",
            86_400,
            None,
        )];
        let lines = app.lines();
        let row = lines
            .iter()
            .find(|line| line.contains("GrantTap Term Windows"))
            .unwrap();
        assert!(row.contains("src"), "{row}");
        assert!(!row.contains("sessions"), "{row}");
        assert!(
            lines.iter().any(|line| line.contains("weavatrix")),
            "{lines:?}"
        );
        let outside = sample_chat(
            AgentKind::Grok,
            "Edge desk",
            r"C:\work\edge-analytics",
            90_000,
            None,
        );
        app.chats.push(outside);
        let lines = app.lines();
        let edge = lines.iter().find(|line| line.contains("Edge desk")).unwrap();
        assert!(edge.contains("edge-analytics"), "{edge}");
        let linked = sample_chat(
            AgentKind::Claude,
            "heuristic",
            r"C:\work\blazingly\.claude\worktrees\heuristic-gagarin",
            100_000,
            None,
        );
        app.chats.push(linked);
        let lines = app.lines();
        let row = lines.iter().find(|line| line.contains("heuristic")).unwrap();
        assert!(row.contains("blazingly"), "{row}");
        assert!(!row.contains("worktrees"), "{row}");
        app.chats.push(sample_chat(
            AgentKind::Grok,
            "Christmas bauble",
            r"C:\Users\x\.ambiloom\grok\m1793630097601-slwb64ec",
            110_000,
            Some(1000),
        ));
        app.chats.push(sample_chat(
            AgentKind::Grok,
            "Prague lane",
            r"C:\Users\x\.ambiloom\grok\m1793549585734",
            120_000,
            Some(1000),
        ));
        let lines = app.lines();
        assert!(
            !lines.iter().any(|line| line.contains("m179")),
            "{lines:?}"
        );
        let bauble = lines
            .iter()
            .find(|line| line.contains("Christmas bauble"))
            .unwrap();
        assert!(bauble.contains("—"), "{bauble}");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("edge-analytics") && !line.contains("Edge desk")),
            "{lines:?}"
        );
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
            frame[..answer_at].ends_with("38;5;223m"),
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
            "{\"type\":\"assistant\",\"content\":\"intro\\n\\n# Title\\n\\n```rs\\n    let desk = 1;\\n```\\n\"}\n",
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
        let title = frame.find("Title").expect("heading");
        assert!(frame[..title].ends_with("38;5;220m"), "{frame}");
        assert!(!frame.contains("# Title"), "{frame}");
        assert!(
            frame.contains("38;5;196mlet"),
            "code fence lost its syntax color: {frame}"
        );
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
        assert!(
            app.suggestions().iter().any(|name| name == "/model"),
            "{:?}",
            app.suggestions()
        );
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
        let rows = app.rows_for(80);
        app.cursor = rows
            .iter()
            .position(|line| line.contains("first title"))
            .expect("first chat");
        app.on_key(Key::Enter);
        assert!(app.lines().join("\n").contains("first desk"));
        let index = rows
            .iter()
            .position(|line| line.contains("second title"))
            .expect("second chat");
        app.press(crate::ui::Spot::Row(index), 1);
        let opened = app.lines().join("\n");
        assert!(opened.contains("second desk"), "{opened}");
        assert!(!opened.contains("first desk"), "{opened}");
        assert!(opened.contains("second title"), "{opened}");
        let screen = crate::ui::Screen::paint(&app, 120, 24);
        assert_eq!(screen.spot_at(2, 0), Some(crate::ui::Spot::Back));
        app.press(crate::ui::Spot::Back, 1);
        assert!(app.pane().is_none());
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
                window: None,
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
                window: None,
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
    fn the_now_list_has_its_own_scroll() {
        assert_eq!(Section::Chats.label(), "now");
        assert_eq!(Section::parse("now"), Some(Section::Chats));
        assert_eq!(Section::parse("Chats"), Some(Section::Chats));
        let mut app = App::new(PathBuf::from("/nowhere-list"), PathBuf::from("/nowhere-home"));
        app.set_size(100, 24);
        app.chats = (0..40)
            .map(|index| {
                sample_chat(
                    AgentKind::Grok,
                    &format!("title-{index:02}"),
                    "/tmp/night",
                    index,
                    None,
                )
            })
            .collect();
        app.section = Section::Chats;
        let rows = app.section_rows();
        app.cursor = rows.len() - 1;
        let kept = app.cursor;
        app.on_key(Key::Enter);
        assert_eq!(app.list_from, usize::MAX);
        let bottom = crate::ui::Screen::paint(&app, 100, 24);
        assert!(bottom.frame.contains("now  40"), "{}", bottom.frame);
        assert!(!bottom.frame.contains("Chats"), "{}", bottom.frame);
        assert!(bottom.frame.contains("title-00"), "{}", bottom.frame);
        assert!(!bottom.frame.contains("title-39"), "{}", bottom.frame);
        let bar = crate::ui::Screen::list_column(100).saturating_sub(2);
        let track = |frame: &crate::ui::Screen| {
            frame.plain_rows().iter().any(|row| {
                row.chars()
                    .nth(bar)
                    .is_some_and(|ch| matches!(ch, '┬' | '│' | '█' | '┴' | '↑'))
            })
        };
        assert!(track(&bottom), "list bar");
        app.hover = Some((1, 4));
        for _ in 0..20 {
            app.on_key(Key::Wheel(-1));
        }
        let top = crate::ui::Screen::paint(&app, 100, 24);
        assert!(top.frame.contains("title-39"), "{}", top.frame);
        assert_eq!(app.cursor, kept);
        assert!(top.frame.contains("title-00"), "{}", top.frame);
        assert!(track(&top), "list bar after scroll");
    }

    #[test]
    fn a_list_click_keeps_the_window() {
        let mut app = App::new(PathBuf::from("/nowhere-list"), PathBuf::from("/nowhere-home"));
        app.set_size(100, 24);
        app.chats = (0..40)
            .map(|index| {
                sample_chat(
                    AgentKind::Grok,
                    &format!("title-{index:02}"),
                    "/tmp/night",
                    index,
                    None,
                )
            })
            .collect();
        app.section = Section::Chats;
        let rows = app.section_rows();
        app.cursor = rows.len() - 1;
        app.on_key(Key::Enter);
        assert_eq!(app.list_from, usize::MAX);
        let before = crate::ui::Screen::paint(&app, 100, 24);
        let names = app.rows_for(40);
        let picked = (0..24).find_map(|y| match before.spot_at(2, y) {
            Some(crate::ui::Spot::Row(index))
                if index != app.cursor
                    && names.get(index).is_some_and(|line| line.contains("title-")) =>
            {
                Some(index)
            }
            _ => None,
        });
        let picked = picked.expect("a visible chat above the tail");
        let left = |screen: &crate::ui::Screen| {
            screen
                .plain_rows()
                .iter()
                .map(|row| row.chars().take(40).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let before_left = left(&before);
        assert!(before_left.contains("title-00"), "{before_left}");
        assert!(!before_left.contains("title-39"), "{before_left}");
        app.press(crate::ui::Spot::Row(picked), 1);
        assert_ne!(app.list_from, usize::MAX);
        let held = crate::ui::Screen::paint(&app, 100, 24);
        let held_left = left(&held);
        assert!(held_left.contains("title-00"), "{held_left}");
        assert!(!held_left.contains("title-39"), "{held_left}");
        app.hover = Some((1, 4));
        for _ in 0..20 {
            app.on_key(Key::Wheel(-1));
        }
        let pinned = app.list_from;
        assert_ne!(pinned, usize::MAX);
        let top = crate::ui::Screen::paint(&app, 100, 24);
        assert!(left(&top).contains("title-39"), "{}", left(&top));
        let title_row = names
            .iter()
            .position(|line| line.contains("title-39"))
            .expect("title-39");
        app.press(crate::ui::Spot::Row(title_row), 1);
        assert_eq!(app.list_from, pinned);
        let stayed = crate::ui::Screen::paint(&app, 100, 24);
        assert!(left(&stayed).contains("title-39"), "{}", left(&stayed));
    }

    #[test]
    fn a_long_transcript_paints_its_tail_quickly() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        let blob = "word ".repeat(6_000);
        let mut lines = Vec::new();
        lines.push(format!("assistant  PINWORD {blob}"));
        for index in 1..24 {
            lines.push(format!("assistant  BURIED{index:02} {blob}"));
        }
        lines.push("assistant  TAILMARKER".into());
        app.open_id = "paint".into();
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines,
            agent: Some(AgentKind::Grok),
        };
        let started = std::time::Instant::now();
        let screen = crate::ui::Screen::paint(&app, 120, 28);
        let elapsed = started.elapsed();
        assert!(
            screen.frame.contains("TAILMARKER"),
            "tail missing after {elapsed:?}"
        );
        assert!(screen.frame.contains("PINWORD"), "pin missing");
        assert!(
            !screen.frame.contains("BURIED"),
            "off-screen line was drawn"
        );
        assert!(
            elapsed.as_millis() < 400,
            "paint took {elapsed:?}"
        );
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
    fn the_row_shows_context_and_scroll_loads_the_older_page() {
        let mut listed = App::new(PathBuf::from("/tmp/night"), PathBuf::from("/nowhere-home"));
        listed.chats = vec![sample_chat(
            AgentKind::Grok,
            "context chat",
            "/tmp/night",
            10,
            Some(203_543),
        )];
        let row = listed
            .lines()
            .into_iter()
            .find(|line| line.contains("context chat"))
            .unwrap();
        assert!(row.contains("203.5k"), "{row}");
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-page-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = String::new();
        for index in 0..1_600 {
            body.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"paragraph {index:04}\"}}\n"
            ));
        }
        std::fs::write(dir.join("chat_history.jsonl"), body).unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Claude,
            "paged chat",
            "/tmp/night",
            10,
            Some(203_543),
        )];
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.set_size(100, 24);
        let started = std::time::Instant::now();
        app.on_key(Key::Enter);
        let tail = crate::ui::Screen::paint(&app, 100, 24);
        assert!(
            started.elapsed().as_millis() < 800,
            "open took {:?}",
            started.elapsed()
        );
        assert!(tail.frame.contains("paragraph 1599"), "{}", tail.frame);
        assert!(!tail.frame.contains("paragraph 0000"), "{}", tail.frame);
        for _ in 0..1_400 {
            app.on_key(Key::Wheel(-1));
        }
        let older = crate::ui::Screen::paint(&app, 100, 24);
        assert!(
            older.frame.contains("paragraph 0000"),
            "older page never arrived"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn folded_tools_load_the_older_page_without_a_long_scroll() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-fold-page-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = String::from("{\"type\":\"user\",\"content\":\"OLDERMARKER\"}\n");
        let mut tools = String::new();
        let mut index = 0u32;
        while tools.len() < 60 * 1024 {
            tools.push_str(&format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"tool{index:04}\",\"input\":{{\"command\":\"x\"}}}}]}}}}\n"
            ));
            index += 1;
        }
        body.push_str(&tools);
        body.push_str("{\"type\":\"user\",\"content\":\"TAILMARKER\"}\n");
        std::fs::write(dir.join("chat_history.jsonl"), body).unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "folded chat",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.set_size(100, 12);
        app.on_key(Key::Enter);
        let opened = app.lines();
        assert!(
            opened.iter().any(|line| line.contains("TAILMARKER")),
            "{opened:?}"
        );
        assert!(
            opened.iter().all(|line| !line.contains("OLDERMARKER")),
            "the tail already held the older line"
        );
        for _ in 0..8 {
            app.on_key(Key::Wheel(-1));
        }
        let loaded = app.lines();
        assert!(
            loaded.iter().any(|line| line.contains("OLDERMARKER")),
            "older page never arrived"
        );
        let frame = crate::ui::Screen::paint(&app, 100, 28);
        assert!(
            frame.frame.contains("OLDERMARKER"),
            "older line stayed above the window"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn old_desk_lines_do_not_cover_the_latest_message() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-outbox-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = String::new();
        body.push_str("{\"type\":\"user\",\"content\":\"say hi\"}\n");
        body.push_str("{\"type\":\"user\",\"content\":\"/effort\"}\n");
        body.push_str("{\"type\":\"user\",\"content\":\"какой итог\"}\n");
        for index in 0..400 {
            body.push_str(&format!(
                "{{\"type\":\"user\",\"content\":\"paragraph {index:03} {}\"}}\n",
                "word ".repeat(30)
            ));
        }
        body.push_str("{\"type\":\"user\",\"content\":\"latest desk line\"}\n");
        std::fs::write(dir.join("chat_history.jsonl"), &body).unwrap();
        let out = home.join(".granttap-term/outbox");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("id-desk.txt"), "say hi\n/effort\nкакой итог\n").unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "desk",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir;
        app.section = Section::Chats;
        app.set_size(100, 24);
        app.on_key(Key::Enter);
        let lines = app.lines();
        assert!(
            lines.iter().any(|line| line.contains("latest desk line")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("say hi")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("/effort")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("какой итог")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|line| line == "No transcript."));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_blank_page_does_not_wipe_the_open_talk() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-keep-talk-{n}"));
        let dir = home.join("chat");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("chat_history.jsonl"), "").unwrap();
        let mut app = App::new(home.clone(), home.clone());
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "desk",
            "/tmp/night",
            10,
            None,
        )];
        app.chats[0].path = dir.clone();
        app.open_id = app.chats[0].id.clone();
        app.open_stamp = Some(std::time::SystemTime::UNIX_EPOCH);
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines: vec!["assistant  keep the reply".into()],
            agent: Some(AgentKind::Grok),
        };
        app.refresh_open();
        let lines = app.lines();
        assert!(
            lines.iter().any(|line| line.contains("keep the reply")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|line| line == "No transcript."));
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_dirty_repo_does_not_cover_the_reply() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-talk-{n}"));
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
        for index in 0..30 {
            std::fs::write(dir.join(format!("a{index:02}.rs")), "fn old() {}\n").unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-m", "start"]);
        for index in 0..30 {
            std::fs::write(dir.join(format!("a{index:02}.rs")), "fn new() {}\n").unwrap();
        }
        let session = dir.join("session");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            session.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"the reply stays\"}]}\n{\"type\":\"assistant\",\"content\":\"still here\"}\n",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), dir.clone());
        app.chats = vec![sample_chat(
            AgentKind::Grok,
            "windows console",
            &dir.display().to_string(),
            10,
            None,
        )];
        app.chats[0].path = session;
        app.section = Section::Chats;
        app.on_key(Key::Enter);
        let lines = app.lines();
        let stat = lines
            .iter()
            .position(|line| line.starts_with("stat  a00.rs"))
            .expect("diff missing");
        let reply = lines
            .iter()
            .position(|line| line.contains("the reply stays"))
            .expect("reply missing");
        assert!(stat < reply, "{}", lines.join("\n"));
        let screen = crate::ui::Screen::paint(&app, 120, 24);
        assert!(screen.frame.contains("the reply stays"), "{}", screen.frame);
        assert!(screen.frame.contains("still here"), "{}", screen.frame);
        assert!(!screen.frame.contains("a00.rs"), "{}", screen.frame);
        let _ = std::fs::remove_dir_all(dir);
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
                window: None,
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
        assert!(joined.contains("vim  a.rs"), "{joined}");
        assert!(joined.contains("where  a.rs"), "{joined}");
        assert!(joined.contains("changed  1 lines"), "{joined}");
        assert!(joined.contains("fn new() {}"), "{joined}");
        assert!(joined.contains("let x = 1;"), "{joined}");
        assert!(!joined.contains("* fn"), "{joined}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn term_tab_sits_on_the_bar() {
        let dir = std::env::temp_dir().join("granttap-term-tab");
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(dir, PathBuf::from("/nowhere-home"));
        app.section = Section::Term;
        app.shell.push_line("> if true; then");
        assert!(
            app.lines()
                .iter()
                .any(|line| line.contains("shell") || line.contains("ls"))
        );
        let screen = crate::ui::Screen::paint(&app, 120, 24);
        assert!(screen.frame.contains("Term"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;196mif"), "{}", screen.frame);
        assert!(!screen.frame.contains("48;5;17"), "{}", screen.frame);
        assert!(screen.cursor.is_some());
    }

    #[test]
    fn open_chat_paints_the_user_line_on_a_bar() {
        let dir = std::env::temp_dir().join("granttap-user-bar");
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(dir, PathBuf::from("/nowhere-home"));
        app.section = Section::Chats;
        app.fill = true;
        app.mode = Mode::Split {
            lines: vec![
                "user  keep the desk".into(),
                "still mine".into(),
                "```rust".into(),
                "fn desk() {}".into(),
                "```".into(),
                "assistant  ok".into(),
                "folder  src".into(),
                "stat  app.rs  2  1".into(),
                "worked  1h 30m".into(),
            ],
            agent: Some(AgentKind::Grok),
        };
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(!screen.frame.contains("48;5;17"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;196mfn"), "{}", screen.frame);
        assert!(screen.frame.contains("src/"), "{}", screen.frame);
        assert!(screen.frame.contains("app.rs"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;40m+2"), "{}", screen.frame);
        assert!(screen.frame.contains("38;5;196m-1"), "{}", screen.frame);
        assert!(screen.frame.contains("keep the desk"));
        assert!(screen.cursor.is_some());
        assert_eq!(app.prompt_hint(), "");
        assert!(!screen.frame.contains(" · say"), "{}", screen.frame);
        assert!(!screen.frame.contains("131m say"), "{}", screen.frame);
        assert!(app.description_line().contains("F6"));
    }

    #[test]
    fn a_key_burst_matches_a_clipboard_paste() {
        let keys = [
            Key::Char('o'),
            Key::Char('n'),
            Key::Char('e'),
            Key::Enter,
            Key::Char('t'),
        ];
        assert!(burst_is_paste(&keys, "one\ntwo\n"));
        assert!(!burst_is_paste(&keys, "nope\n"));
        assert!(!burst_is_paste(&[Key::Char('h'), Key::Enter], "hello\n"));
    }

    #[test]
    fn a_copied_file_and_a_clipboard_picture_land_in_the_line() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-clip-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-clip-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("notes.txt");
        let shot = dir.join("shot 1.png");
        std::fs::write(&note, "hello").unwrap();
        image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
            .unwrap()
            .save(&shot)
            .unwrap();
        let mut wide = Vec::new();
        wide.extend_from_slice(&20u32.to_le_bytes());
        wide.extend_from_slice(&0u32.to_le_bytes());
        wide.extend_from_slice(&0u32.to_le_bytes());
        wide.extend_from_slice(&0u32.to_le_bytes());
        wide.extend_from_slice(&1u32.to_le_bytes());
        for unit in "C:\\a.txt".encode_utf16() {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        wide.extend_from_slice(&0u16.to_le_bytes());
        for unit in "C:\\b.rs".encode_utf16() {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        wide.extend_from_slice(&0u16.to_le_bytes());
        wide.extend_from_slice(&0u16.to_le_bytes());
        let parsed = parse_hdrop(&wide);
        assert_eq!(
            parsed,
            vec![PathBuf::from("C:\\a.txt"), PathBuf::from("C:\\b.rs")]
        );
        let mut app = App::new(dir.clone(), home.clone());
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.cmd = "see".into();
        app.place_clip(vec![note.clone()], None, None);
        assert!(app.cmd.starts_with("see "), "{}", app.cmd);
        assert!(app.cmd.contains(&note.display().to_string()), "{}", app.cmd);
        assert_eq!(app.note, "pasted  notes.txt");
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(!lines.iter().any(|line| line.starts_with("user  ")), "{lines:?}");
        app.cmd = "look".into();
        app.place_clip(vec![], Some(shot.clone()), Some("http://example.com/pic".into()));
        let quoted = format!("\"{}\"", shot.display());
        assert!(app.cmd.contains(&quoted), "{}", app.cmd);
        assert!(!app.cmd.contains("http://"), "{}", app.cmd);
        assert_eq!(app.note, "pasted  shot 1.png");
        let hits = crate::ui::picture_paths(&app.cmd);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].2, shot);
        let other = dir.join("plain.rs");
        std::fs::write(&other, "fn main() {}\n").unwrap();
        app.cmd.clear();
        app.place_clip(vec![other.clone(), note.clone()], Some(shot), None);
        assert!(app.cmd.contains(&other.display().to_string()), "{}", app.cmd);
        assert!(app.cmd.contains("notes.txt"), "{}", app.cmd);
        assert!(!app.cmd.contains("shot"), "{}", app.cmd);
        assert_eq!(app.note, "pasted  2 files");
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_multiline_paste_is_one_markdown_file() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-paste-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-paste-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir, home);
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.on_key(Key::Paste("one\ntwo\n".into()));
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        let said: Vec<_> = lines
            .iter()
            .filter(|line| line.starts_with("user  "))
            .cloned()
            .collect();
        assert_eq!(said.len(), 1, "{said:?}");
        let path = after_user(&said[0]);
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("one\n"), "{body}");
        assert!(body.contains("two\n"), "{body}");
        assert!(path.ends_with(".md"), "{path}");
        app.on_key(Key::Char('o'));
        app.on_key(Key::Enter);
        let waiting = crate::ui::Screen::paint(&app, 120, 28);
        let now = find_steer(&waiting, 0);
        app.press(waiting.spot_at(now.0, now.1).unwrap(), 1);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert_eq!(
            lines.iter().filter(|line| line.starts_with("user  ")).count(),
            2
        );
    }

    fn after_user(line: &str) -> &str {
        let rest = line.strip_prefix("user  ").unwrap_or(line);
        if rest.len() >= 13 && super::clock_prefix(rest) {
            &rest[13..]
        } else {
            rest
        }
    }

    #[test]
    fn a_turn_shows_when_it_was_sent_and_what_it_spent() {
        let marks = vec![chats::TurnMark {
            at: 1_700_000_000,
            head: "hello desk from the seat".into(),
            done: 1_700_000_040,
            tokens: Some(2400),
        }];
        let lines = stamp_turns(
            vec!["user  hello desk from the seat".into(), "assistant  ok".into()],
            &marks,
        );
        assert!(lines[0].starts_with("user  "), "{lines:?}");
        assert!(lines[0].contains("hello desk from the seat"), "{lines:?}");
        assert!(super::clock_prefix(lines[0].trim_start_matches("user  ")), "{lines:?}");
        assert!(
            lines.iter().any(|line| line.starts_with("итог  ") && line.contains("2.4k")),
            "{lines:?}"
        );
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.open_id = "keep".into();
        app.busy_since = Some(super::unix_now().saturating_sub(12));
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        let footer = app.work_footer().unwrap();
        assert!(footer.contains("работает"), "{footer}");
        assert!(footer.contains("12s") || footer.contains("13s") || footer.contains("11s"), "{footer}");
        let screen = crate::ui::Screen::paint(&app, 120, 30);
        assert!(screen.frame.contains("работает"), "{}", screen.frame);
        assert!(screen.frame.contains("\x1b]0;GrantTap Term\x07"), "{}", screen.frame);
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        assert!(
            frames.iter().any(|ch| footer.contains(*ch)),
            "{footer}"
        );
        assert!(screen.spot_at(1, 0) == Some(crate::ui::Spot::Back));
        app.press(crate::ui::Spot::Back, 1);
        assert!(app.pane().is_none());
    }

    fn age_log(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let when = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(secs))
            .unwrap();
        file.set_modified(when).unwrap();
    }

    #[test]
    fn a_typed_line_from_the_live_seat_shows_in_the_open_chat() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-said-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-said-dir-{n}"));
        let session = dir.join("sess");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            session.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"keep the desk\"}]}\n",
        )
        .unwrap();
        std::fs::write(
            session.join("updates.jsonl"),
            concat!(
                r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"<system-reminder>\nkeep out\n</system-reminder>"}}}"#,
                "\n",
                r#"{"timestamp":1001,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"какой итог"}}}"#,
                "\n",
            ),
        )
        .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Grok,
            id: "seat".into(),
            title: "Seat".into(),
            path: session.clone(),
            cwd: dir.display().to_string(),
            live: true,
            at: 1_700_000_000,
            tokens: None,
                window: None,
        });
        app.open_id = "seat".into();
        app.live_at = Some(0);
        app.open_stamp = Some(SystemTime::UNIX_EPOCH);
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.pump();
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        let hits = lines
            .iter()
            .filter(|line| line.contains("какой итог"))
            .count();
        assert_eq!(hits, 1, "{lines:?}");
        assert!(lines.iter().any(|line| line.starts_with("user  ") && line.contains("какой итог")));
        assert!(lines.iter().any(|line| line.contains("keep the desk")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("keep out")), "{lines:?}");
        app.pump();
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert_eq!(
            lines.iter().filter(|line| line.contains("какой итог")).count(),
            1,
            "{lines:?}"
        );
        std::fs::write(
            session.join("chat_history.jsonl"),
            concat!(
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"keep the desk\"}]}\n",
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"какой итог\"}]}\n",
            ),
        )
        .unwrap();
        app.open_stamp = Some(SystemTime::UNIX_EPOCH);
        app.pump();
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert_eq!(
            lines.iter().filter(|line| line.contains("какой итог")).count(),
            1,
            "{lines:?}"
        );
        assert!(app.live_said.is_empty(), "{}", app.live_said);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_chats_leaves_the_live_reply_behind() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-switch-home-{n}"));
        let root = std::env::temp_dir().join(format!("granttap-switch-dir-{n}"));
        let alpha = root.join("alpha");
        let beta = root.join("beta");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(
            alpha.join("chat_history.jsonl"),
            "{\"type\":\"assistant\",\"content\":\"ALPHAKEEP\"}\n",
        )
        .unwrap();
        std::fs::write(
            beta.join("chat_history.jsonl"),
            "{\"type\":\"assistant\",\"content\":\"BETAKEEP\"}\n",
        )
        .unwrap();
        let chat = |id: &str, title: &str, path: std::path::PathBuf| crate::model::Chat {
            agent: AgentKind::Grok,
            id: id.into(),
            title: title.into(),
            path,
            cwd: root.display().to_string(),
            live: false,
            at: 1_700_000_000,
            tokens: None,
            window: None,
        };
        let alpha_chat = chat("alpha", "Alpha", alpha);
        let beta_chat = chat("beta", "Beta", beta);
        let mut app = App::new(root.clone(), home.clone());
        app.chats = vec![alpha_chat.clone(), beta_chat.clone()];
        app.show_chat(&alpha_chat, false);
        app.live_reply = "FOREIGNREPLY from the other seat".into();
        app.show_chat(&beta_chat, false);
        let lines = app.lines();
        assert!(
            lines.iter().any(|line| line.contains("BETAKEEP")),
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|line| !line.contains("FOREIGNREPLY")),
            "{lines:?}"
        );
        assert!(app.live_reply.is_empty(), "{}", app.live_reply);
        app.live_reply = "STILLHERE".into();
        app.pane_key.clear();
        app.show_chat(&beta_chat, false);
        let lines = app.lines();
        assert!(
            lines.iter().any(|line| line.contains("STILLHERE")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("BETAKEEP")),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_quiet_open_tool_keeps_the_footer_and_a_finished_turn_settles() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-spin-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-spin-dir-{n}"));
        let session = dir.join("sess");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&session).unwrap();
        let log = session.join("updates.jsonl");
        std::fs::write(
            &log,
            concat!(
                r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"hello desk from the seat"}}}"#,
                "\n",
                r#"{"timestamp":1010,"update":{"sessionUpdate":"tool_call","toolCallId":"call-1","title":"run"}}"#,
                "\n",
            ),
        )
        .unwrap();
        age_log(&log, 40);
        let mut app = App::new(dir.clone(), home.clone());
        app.chats.push(crate::model::Chat {
            agent: AgentKind::Grok,
            id: "spin".into(),
            title: "Quiet tool".into(),
            path: session,
            cwd: dir.display().to_string(),
            live: true,
            at: 1_700_000_000,
            tokens: None,
                window: None,
        });
        app.open_id = "spin".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        let old = super::unix_now() - 30;
        app.busy_since = Some(old);
        app.last_pulse = old;
        app.settle_work();
        assert!(app.busy_since.is_some());
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            !lines.iter().any(|line| line.starts_with("итог")),
            "{lines:?}"
        );
        let footer = app.work_footer().unwrap();
        assert!(footer.contains("работает"), "{footer}");
        let row = app
            .rows_for(120)
            .into_iter()
            .find(|line| line.contains("Quiet tool"))
            .unwrap();
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        assert!(frames.iter().any(|ch| row.contains(*ch)), "{row}");
        std::fs::write(
            &log,
            concat!(
                r#"{"timestamp":1000,"update":{"sessionUpdate":"user_message_chunk","content":{"text":"hello desk from the seat"}}}"#,
                "\n",
                r#"{"timestamp":1010,"update":{"sessionUpdate":"tool_call","toolCallId":"call-1","title":"run"}}"#,
                "\n",
                r#"{"timestamp":1040,"update":{"sessionUpdate":"tool_call_update","toolCallId":"call-1","status":"completed"}}"#,
                "\n",
            ),
        )
        .unwrap();
        age_log(&log, 40);
        app.busy_since = Some(old);
        app.last_pulse = old;
        app.settle_work();
        assert!(app.busy_since.is_none());
        assert!(app.work_footer().is_none());
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            lines.iter().any(|line| line.starts_with("итог")),
            "{lines:?}"
        );
        let row = app
            .rows_for(120)
            .into_iter()
            .find(|line| line.contains("Quiet tool"))
            .unwrap();
        assert!(row.contains("* Quiet tool"), "{row}");
        assert!(!frames.iter().any(|ch| row.contains(*ch)), "{row}");
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_effort_menu_stays_out_of_the_chat() {
        let menu = "\
Worked for 3m46s
────────────────
> Extra High (active) Maximum reasoning for the hardest tasks.
High Thorough reasoning and quality. Recommended.
Medium Strong quality with a faster turnaround.
Low Fastest responses. Best for simple tasks.
> /effort <level>
Grok 4.7 (xhigh)
always-approve
Enter:send | Shift+Enter/Alt+Enter:newline | Shift+Tab:mode | Ctrl+x:shortcuts
";
        let kept = screen_reply(&["assistant  earlier".into()], menu);
        assert!(kept.contains("Worked for 3m46s"), "{kept}");
        assert!(!kept.contains("Extra High"), "{kept}");
        assert!(!kept.contains("Enter:send"), "{kept}");
        assert!(!kept.contains("Grok 4.7"), "{kept}");
        assert!(!kept.contains("<level>"), "{kept}");
        let reply = screen_reply(
            &["assistant  earlier".into()],
            "The build finished\n12 tests passed",
        );
        assert!(reply.contains("The build finished"), "{reply}");
        assert!(reply.contains("12 tests passed"), "{reply}");
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-effort-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-effort-dir-{n}"));
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            home.join(".grok/config.toml"),
            "[models]\ndefault = \"grok-4.7\"\ndefault_reasoning_effort = \"low\"\n",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.section = Section::Chats;
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.cmd = "/effort".into();
        app.on_key(Key::Enter);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(lines.iter().any(|line| line.contains("assistant  hi")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("/effort")), "{lines:?}");
        assert!(app.note.contains("low"), "{}", app.note);
        assert!(app.note.contains("xhigh"), "{}", app.note);
        app.cmd = "/effort xhigh".into();
        app.on_key(Key::Enter);
        assert!(app.note.contains("xhigh"), "{}", app.note);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert_eq!(lines.len(), 1, "{lines:?}");
        let saved = std::fs::read_to_string(home.join(".grok/config.toml")).unwrap();
        assert!(saved.contains("default_reasoning_effort"), "{saved}");
        assert!(saved.contains("xhigh"), "{saved}");
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn say_posts_the_line_into_the_open_chat() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-say-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-say-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.section = Section::Chats;
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.cmd = "keep the desk".into();
        app.on_key(Key::Enter);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            lines.iter().any(|line| line.starts_with("user  ") && line.contains("keep the desk")),
            "{lines:?}"
        );
        let saved = std::fs::read_to_string(home.join(".granttap-term/outbox/keep.txt")).unwrap();
        assert!(saved.contains("keep the desk"), "{saved}");
        app.on_key(Key::F(6));
        assert_eq!(app.prompt_hint(), "queue");
        app.cmd = "wait here".into();
        app.on_key(Key::Enter);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            !lines.iter().any(|line| line.contains("wait here")),
            "{lines:?}"
        );
        assert!(app.tasks.is_empty());
        assert_eq!(app.section, Section::Chats);
        let queued = crate::ui::Screen::paint(&app, 140, 30);
        assert!(queued.frame.contains(" · queue"), "{}", queued.frame);
        assert!(queued.frame.contains("[Send now]"), "{}", queued.frame);
        assert!(queued.frame.contains("[edit]"), "{}", queued.frame);
        assert!(queued.frame.contains("[cancel]"), "{}", queued.frame);
        let edit = find_steer(&queued, 1);
        app.press(queued.spot_at(edit.0, edit.1).unwrap(), 1);
        assert_eq!(app.cmd, "wait here");
        app.busy_since = Some(super::unix_now());
        app.on_key(Key::Enter);
        let held = crate::ui::Screen::paint(&app, 140, 30);
        assert!(held.frame.contains("[Send now]"), "{}", held.frame);
        let cancel = find_steer(&held, 2);
        app.press(held.spot_at(cancel.0, cancel.1).unwrap(), 1);
        assert_eq!(app.note, "cancelled");
        app.cmd = "send this now".into();
        app.on_key(Key::Enter);
        let ready = crate::ui::Screen::paint(&app, 140, 30);
        let now = find_steer(&ready, 0);
        app.press(ready.spot_at(now.0, now.1).unwrap(), 1);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            lines.iter().any(|line| line.starts_with("user  ") && line.contains("send this now")),
            "{lines:?}"
        );
        let saved = std::fs::read_to_string(home.join(".granttap-term/outbox/keep.txt")).unwrap();
        assert!(saved.contains("send this now"), "{saved}");
        assert!(app.note.contains("not live"), "{}", app.note);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn pick_setting(app: &mut App, id: &str) {
        let index = app
            .setting_rows()
            .iter()
            .position(|row| row.id == id)
            .unwrap_or_else(|| panic!("missing {id}"));
        app.press(crate::ui::Spot::Setting(index as u16), 1);
    }

    fn find_steer(screen: &crate::ui::Screen, kind: u16) -> (usize, usize) {
        for y in 0..40 {
            for x in 0..160 {
                if let Some(crate::ui::Spot::Steer(code)) = screen.spot_at(x, y) {
                    if code & 3 == kind {
                        return (x, y);
                    }
                }
            }
        }
        panic!("missing queue button {kind}");
    }

    #[test]
    fn dib_bottom_up_24bit_becomes_rgb() {
        let mut bytes = vec![0u8; 40 + 16];
        bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&2i32.to_le_bytes());
        bytes[8..12].copy_from_slice(&2i32.to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&24u16.to_le_bytes());
        let pixels = 40;
        bytes[pixels..pixels + 3].copy_from_slice(&[255, 0, 0]);
        bytes[pixels + 3..pixels + 6].copy_from_slice(&[255, 255, 255]);
        bytes[pixels + 8..pixels + 11].copy_from_slice(&[0, 0, 255]);
        bytes[pixels + 11..pixels + 14].copy_from_slice(&[0, 255, 0]);
        let image = decode_dib(&bytes).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0]);
        assert_eq!(image.get_pixel(0, 1).0, [0, 0, 255]);
        assert_eq!(image.get_pixel(1, 1).0, [255, 255, 255]);
        assert!(decode_dib(&bytes[..30]).is_none());
    }

    #[test]
    fn dib_bitfields_32bit_reads_the_masks() {
        let mut bytes = vec![0u8; 52 + 4];
        bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&1i32.to_le_bytes());
        bytes[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
        bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
        bytes[52..56].copy_from_slice(&[0, 0, 255, 0]);
        let image = decode_dib(&bytes).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    }

    fn mosaic_marks(text: &str) -> usize {
        text.chars()
            .filter(|ch| {
                ('\u{2580}'..='\u{259F}').contains(ch) || ('\u{1FB00}'..='\u{1FB3B}').contains(ch)
            })
            .count()
    }

    #[test]
    fn a_picture_path_becomes_a_button_and_opens_on_the_pane() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-shot-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-shot-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("shot.png");
        let mut raw = vec![0u8; 8 * 4 * 3];
        for pixel in 0..16 {
            raw[pixel * 3] = 255;
        }
        for pixel in 16..32 {
            raw[pixel * 3 + 2] = 255;
        }
        image::RgbImage::from_raw(8, 4, raw)
            .unwrap()
            .save(&png)
            .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.set_size(100, 36);
        app.section = Section::Chats;
        app.open_id = "pic".into();
        let full = png.display().to_string();
        app.mode = Mode::Split {
            lines: vec![format!("user  11-09 18:02  {full} смотри")],
            agent: Some(AgentKind::Grok),
        };
        app.pump();
        let screen = crate::ui::Screen::paint(&app, 100, 36);
        assert!(crate::draw::has_blocks(&screen.frame), "{}", screen.frame);
        assert!(screen.frame.contains("▣ shot.png"), "{}", screen.frame);
        assert!(screen.frame.contains("38;2;"), "{}", screen.frame);
        assert!(!screen.frame.contains(&full), "{}", screen.frame);
        assert!(screen.frame.contains("смотри"), "{}", screen.frame);
        let clickable = (0..36).any(|y| {
            (0..100).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Image(1)))
        });
        assert!(clickable, "picture button is not clickable");
        app.press(crate::ui::Spot::Image(1), 1);
        assert!(app.image_outline().is_some());
        let opened = crate::ui::Screen::paint(&app, 100, 36);
        let big = mosaic_marks(&opened.frame);
        let small = mosaic_marks(&screen.frame);
        assert!(big > small, "opened {big} thumb {small}");
        app.press(crate::ui::Spot::Image(1), 1);
        assert!(app.image_outline().is_none());
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cited_file_opens_in_vim_on_that_line() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-open-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-open-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let js = dir.join("own.js");
        let mut body = String::new();
        for number in 1..=120 {
            body.push_str(&format!("L{number:03}\n"));
        }
        std::fs::write(&js, &body).unwrap();
        let memory = dir.join("MEMORY.md");
        std::fs::write(&memory, "memo\n").unwrap();
        let full = js.display().to_string().replace('\\', "/");
        let mut app = App::new(dir.clone(), home.clone());
        app.set_size(120, 36);
        app.section = Section::Chats;
        app.open_id = "cite".into();
        app.mode = Mode::Split {
            lines: vec![
                format!("assistant  риск в {full}:81. Для другого пользователя 404."),
                "<oai-mem-citation>".into(),
                "<citation_entries>".into(),
                "MEMORY.md:1-2|note=[identified inherited authorization risks and".into(),
                "reverified locally]".into(),
                "</citation_entries>".into(),
                "<rollout_ids>".into(),
                "01a197fc-ad40-7c83-bbcc-3c9883fb3a4e".into(),
                "</rollout_ids>".into(),
                "</oai-mem-citation>".into(),
            ],
            agent: Some(AgentKind::Codex),
        };
        app.pump();
        let screen = crate::ui::Screen::paint(&app, 120, 36);
        assert!(screen.frame.contains("▣ own.js:81"), "{}", screen.frame);
        assert!(!screen.frame.contains(&full), "{}", screen.frame);
        assert!(screen.frame.contains("identified inherited"), "{}", screen.frame);
        assert!(screen.frame.contains("reverified locally"), "{}", screen.frame);
        assert!(!screen.frame.contains("<oai-mem-citation>"), "{}", screen.frame);
        assert!(!screen.frame.contains("|note="), "{}", screen.frame);
        assert!(screen.frame.contains("01a197fc-ad40-7c83-bbcc-3c9883fb3a4e"), "{}", screen.frame);
        let clickable = (0..36).any(|y| {
            (0..120).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Open(1)))
        });
        assert!(clickable, "file button is not clickable");
        app.press(crate::ui::Spot::Open(1), 1);
        assert!(app.doc_open());
        assert!(matches!(app.mode, Mode::Split { .. }));
        assert_eq!(app.doc_title(), "own.js");
        assert_eq!(
            app.doc_lines().get(app.doc_scroll()).map(String::as_str),
            Some("L081")
        );
        let frame = crate::ui::Screen::paint(&app, 120, 36);
        assert!(frame.frame.contains("L081"), "{}", frame.frame);
        assert!(frame.frame.contains("[X]"), "{}", frame.frame);
        app.on_key(Key::Esc);
        assert!(!app.doc_open());
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat did not return");
        };
        assert!(lines.iter().any(|line| line.contains("own.js")), "{lines:?}");
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrolling_up_keeps_a_short_window_and_draws_the_bar() {
        let rows: Vec<super::Folded> = (0..300)
            .map(|n| super::Folded {
                text: format!("row {n} {}", "abcd ".repeat(8)),
                fold: None,
                tree: None,
                doc: None,
            })
            .collect();
        let (lo, hi, skip, top) = crate::ui::view_span(&rows, 40, 0, 12);
        assert!(!top);
        assert_eq!(skip, 0);
        assert_eq!(hi, 300);
        assert!(hi - lo < 40, "{lo} {hi}");
        let (lo, hi, _, _) = crate::ui::view_span(&rows, 40, 100, 12);
        assert!(hi < 300, "{hi}");
        assert!(hi > lo, "{lo} {hi}");
        assert!(hi - lo < 40, "{lo} {hi}");
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(100, 36);
        app.section = Section::Chats;
        app.open_id = "scroll".into();
        let mut lines = vec!["assistant  FIRST".into()];
        for n in 0..160 {
            lines.push(format!("assistant  ROW-{n:04} {}", "word ".repeat(12)));
        }
        lines.insert(40, format!("assistant  HUGE {}", "z".repeat(40_000)));
        app.mode = Mode::Split {
            lines,
            agent: Some(AgentKind::Grok),
        };
        let bottom = crate::ui::Screen::paint(&app, 100, 36);
        assert!(bottom.frame.contains("ROW-0159"), "{}", bottom.frame);
        assert!(!bottom.frame.contains("HUGE"), "{}", bottom.frame);
        assert!(bottom.frame.contains('┴'), "{}", bottom.frame);
        assert!(bottom.frame.contains('┬'), "{}", bottom.frame);
        assert!(bottom.frame.contains("[close]"), "{}", bottom.frame);
        assert!(bottom.frame.contains("[full]"), "{}", bottom.frame);
        assert!(
            (0..100).any(|x| bottom.spot_at(x, 0) == Some(crate::ui::Spot::Gear)),
            "settings chip"
        );
        assert!(
            (0..100).any(|x| bottom.spot_at(x, 0) == Some(crate::ui::Spot::Help)),
            "help chip"
        );
        app.pane_from_end = 80;
        let mid = crate::ui::Screen::paint(&app, 100, 36);
        assert!(!mid.frame.contains("ROW-0159"), "{}", mid.frame);
        assert!(
            mid.frame.contains("ROW-00") || mid.frame.contains("ROW-01"),
            "{}",
            mid.frame
        );
        app.press(crate::ui::Spot::Help, 1);
        assert!(app.keys_open());
        let help = crate::ui::Screen::paint(&app, 100, 36);
        assert!(help.frame.contains("Keyboard Shortcuts"), "{}", help.frame);
        assert!(help.frame.contains("[close]"), "{}", help.frame);
        app.on_key(Key::Esc);
        app.press(crate::ui::Spot::Gear, 1);
        assert!(app.settings_open());
        let look = app
            .setting_rows()
            .iter()
            .position(|row| row.group && row.label.starts_with("Look"))
            .expect("look group");
        app.press(crate::ui::Spot::Setting(look as u16), 1);
        pick_setting(&mut app, "look:cyberdeck");
        assert_eq!(app.look_name(), "cyberdeck");
        let tinted = crate::ui::Screen::paint(&app, 100, 36);
        assert!(tinted.frame.contains("38;2;"), "{}", tinted.frame);
        assert!(!tinted.frame.contains("38;5;51"), "{}", tinted.frame);
        pick_setting(&mut app, "look:parchment");
        pick_setting(&mut app, "look:ink");
        pick_setting(&mut app, "look:dos");
        assert_eq!(app.look_name(), "dos");
        let dos = crate::ui::Screen::paint(&app, 100, 36);
        assert!(dos.frame.contains("48;2;0;0;170"), "{}", dos.frame);
        pick_setting(&mut app, "look:linux");
        assert_eq!(app.look_name(), "linux");
        let linux = crate::ui::Screen::paint(&app, 100, 36);
        assert!(linux.frame.contains("(•)"), "{}", linux.frame);
        assert!(linux.frame.contains("48;2;0;0;0"), "{}", linux.frame);
        assert!(linux.frame.contains("38;2;255;255;85"), "{}", linux.frame);
        pick_setting(&mut app, "look:cyberpunk");
        let plain = crate::ui::Screen::paint(&app, 100, 36);
        assert!(plain.frame.contains("38;5;51"), "{}", plain.frame);
        let font = app
            .setting_rows()
            .iter()
            .position(|row| row.group && row.label.starts_with("Font"))
            .expect("font group");
        app.press(crate::ui::Spot::Setting(font as u16), 1);
        pick_setting(&mut app, "face:Cascadia Mono");
        pick_setting(&mut app, "size:20");
        let picture = app
            .setting_rows()
            .iter()
            .position(|row| row.group && row.label.starts_with("Picture"))
            .expect("picture group");
        app.press(crate::ui::Spot::Setting(picture as u16), 1);
        app.set_size(200, 48);
        pick_setting(&mut app, "quality:low");
        assert_eq!(app.picture_budget().0, 64);
        pick_setting(&mut app, "quality:medium");
        assert_eq!(app.picture_budget().0, 140);
        pick_setting(&mut app, "quality:high");
        assert_eq!(app.picture_budget().0, 192);
    }

    #[test]
    fn a_long_line_scrolls_inside_itself_and_links_light_up() {
        let width = 46usize;
        let mut blocks = String::new();
        for n in 0..80 {
            let mark = format!("L{n:02}");
            blocks.push_str(&mark);
            blocks.push_str(&"z".repeat(width - mark.chars().count()));
        }
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(100, 36);
        app.section = Section::Chats;
        app.open_id = "long".into();
        app.mode = Mode::Split {
            lines: vec![blocks, "TAIL-LINE".into()],
            agent: Some(AgentKind::Grok),
        };
        let bottom = crate::ui::Screen::paint(&app, 100, 36);
        assert!(bottom.frame.contains("L79"), "{}", bottom.frame);
        assert!(!bottom.frame.contains("L00"), "{}", bottom.frame);
        app.pane_from_end = 20;
        let mid = crate::ui::Screen::paint(&app, 100, 36);
        assert!(!mid.frame.contains("L79"), "{}", mid.frame);
        assert!(
            mid.frame.contains("L50") || mid.frame.contains("L40"),
            "{}",
            mid.frame
        );
        app.pane_from_end = 0;
        app.mode = Mode::Split {
            lines: vec!["assistant  see https://example.com/docs end".into()],
            agent: Some(AgentKind::Grok),
        };
        let plain = crate::ui::Screen::paint(&app, 100, 36);
        let mut at = None;
        for (y, row) in plain.plain_rows().iter().enumerate() {
            if let Some(x) = row.find("https") {
                at = Some((x, y));
                break;
            }
        }
        let (x, y) = at.expect("url on the pane");
        app.hover = Some((x, y));
        let hot = crate::ui::Screen::paint(&app, 100, 36);
        assert!(
            hot.frame.contains("1;4;30;48;5;220"),
            "{}",
            hot.frame
        );
        let close = (0..36)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .find(|(x, y)| hot.spot_at(*x, *y) == Some(crate::ui::Spot::ChatClose))
            .expect("close");
        app.hover = Some(close);
        let button = crate::ui::Screen::paint(&app, 100, 36);
        assert!(
            button.frame.contains("1;4;30;48;5;220"),
            "{}",
            button.frame
        );
        let gear = (0..100)
            .find(|x| button.spot_at(*x, 0) == Some(crate::ui::Spot::Gear))
            .expect("settings");
        app.hover = Some((gear, 0));
        let corner = crate::ui::Screen::paint(&app, 100, 36);
        assert!(
            corner.frame.contains("1;4;30;48;5;220"),
            "{}",
            corner.frame
        );
    }

    #[test]
    fn a_lesson_has_sections_and_a_practice() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-lesson-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-lesson-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.set_size(100, 36);
        app.cmd = "lessons".into();
        app.on_key(Key::Enter);
        let lines = app.lines();
        for name in ["Desk", "Agents", "Vim", "Terminal", "Kubernetes"] {
            assert!(lines.iter().any(|line| line.contains(name)), "{lines:?}");
        }
        let vim = lines
            .iter()
            .position(|line| line.contains("  Vim"))
            .unwrap();
        app.cursor = vim;
        app.on_key(Key::Enter);
        let lines = app.lines();
        for name in ["Open", "Edit", "Write", "practice  Scratch file"] {
            assert!(lines.iter().any(|line| line.contains(name)), "{lines:?}");
        }
        assert!(lines.iter().any(|line| line.contains("Change the second line")), "{lines:?}");
        let screen = crate::ui::Screen::paint(&app, 100, 36);
        assert!(screen.frame.contains("Open"), "{}", screen.frame);
        assert!(screen.frame.contains("Scratch file"), "{}", screen.frame);
        app.on_key(Key::Enter);
        assert!(app.vim_active());
        let Mode::View { lines } = &app.mode else {
            panic!("scratch did not open");
        };
        assert_eq!(lines[app.scroll], "change this line");
        let saved = std::fs::read_to_string(home.join(".granttap-term/practice/scratch.txt")).unwrap();
        assert!(saved.contains("change this line"), "{saved}");
        app.on_key(Key::Esc);
        assert!(app.lines().iter().any(|line| line.contains("practice  Scratch file")));
        app.on_key(Key::Esc);
        let lines = app.lines();
        assert!(lines.iter().any(|line| line.contains("Kubernetes")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("Scratch file")), "{lines:?}");
        let kube = lines
            .iter()
            .position(|line| line.contains("  Kubernetes"))
            .unwrap();
        app.cursor = kube;
        app.on_key(Key::Enter);
        app.on_key(Key::Enter);
        assert_eq!(app.section, Section::Term);
        assert!(app.cmd.starts_with("kubectl"), "{}", app.cmd);
        assert!(
            app.suggestions().iter().any(|line| line.contains("get")),
            "{:?}",
            app.suggestions()
        );
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pasted_image_stays_in_the_chat_and_a_click_draws_the_outline() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-image-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-image-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("tiny.png");
        let rgb = image::RgbImage::from_raw(
            2,
            2,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
        )
        .unwrap();
        rgb.save(&png).unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.section = Section::Chats;
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        app.on_key(Key::Paste(png.display().to_string()));
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        let said: Vec<_> = lines
            .iter()
            .filter(|line| line.starts_with("user  "))
            .cloned()
            .collect();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("[Image #1]  png  "), "{said:?}");
        assert!(said[0].contains("bytes"), "{said:?}");
        assert!(said[0].contains("tiny.png"), "{said:?}");
        let saved = std::fs::read_to_string(home.join(".granttap-term/outbox/keep.txt")).unwrap();
        assert!(saved.contains("[Image #1]"), "{saved}");
        assert!(saved.contains("tiny.png"), "{saved}");
        let screen = crate::ui::Screen::paint(&app, 120, 30);
        assert!(crate::draw::has_blocks(&screen.frame), "{}", screen.frame);
        assert!(!screen.frame.contains("granttap-image-dir"), "{}", screen.frame);
        assert!(screen.frame.contains("tiny.png"), "{}", screen.frame);
        let clickable = (0..30).any(|y| {
            (0..120).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Image(1)))
        });
        assert!(clickable, "image line is not clickable");
        app.press(crate::ui::Spot::Image(1), 1);
        let shown = crate::ui::Screen::paint(&app, 120, 30);
        assert!(crate::draw::has_blocks(&shown.frame), "{}", shown.frame);
        assert!(shown.frame.contains("[X]"), "{}", shown.frame);
        assert!(matches!(app.mode, Mode::Split { .. }));
        app.on_key(Key::Esc);
        assert!(app.image_outline().is_none());
        assert!(matches!(app.mode, Mode::Split { .. }));
        app.press(crate::ui::Spot::Image(1), 1);
        assert!(app.image_outline().is_some());
        app.press(crate::ui::Spot::Image(1), 1);
        assert!(app.image_outline().is_none());
        assert!(app.pane().is_some());
        let hidden = crate::ui::Screen::paint(&app, 120, 30);
        assert!(crate::draw::has_blocks(&hidden.frame), "{}", hidden.frame);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_typed_image_path_becomes_the_picture() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-type-home-{n}"));
        let dir = std::env::temp_dir().join(format!("granttap-type-dir-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("Screenshot 2026-11-10 092151.png");
        let mut raw = vec![0u8; 8 * 4 * 3];
        for pixel in 0..16 {
            raw[pixel * 3] = 255;
        }
        for pixel in 16..32 {
            raw[pixel * 3 + 2] = 255;
        }
        image::RgbImage::from_raw(8, 4, raw)
            .unwrap()
            .save(&png)
            .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.set_size(120, 32);
        app.section = Section::Chats;
        app.open_id = "keep".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        for ch in png.display().to_string().chars() {
            app.on_key(Key::Char(ch));
        }
        assert!(app.cmd.is_empty(), "{}", app.cmd);
        let Mode::Split { lines, .. } = &app.mode else {
            panic!("chat closed");
        };
        assert!(
            lines.iter().any(|line| line.contains("[Image #")),
            "{lines:?}"
        );
        let screen = crate::ui::Screen::paint(&app, 120, 32);
        assert!(crate::draw::has_blocks(&screen.frame), "{}", screen.frame);
        assert!(!screen.frame.contains("granttap-type-dir"), "{}", screen.frame);
        assert!(screen.frame.contains("092151.png"), "{}", screen.frame);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_suggestion_click_inserts_the_command_and_the_row_follows_typing() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(120, 30);
        app.section = Section::Chats;
        app.open_id = "s".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Grok),
        };
        let screen = crate::ui::Screen::paint(&app, 120, 30);
        assert!(screen.frame.contains(" /model"), "{}", screen.frame);
        assert!(screen.frame.contains(" /effort"), "{}", screen.frame);
        assert!(!screen.frame.contains("F6 switches"), "{}", screen.frame);
        assert!(!screen.frame.contains("Wheel scrolls"), "{}", screen.frame);
        let hit = (0..30).any(|y| {
            (0..120).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Suggest(0)))
        });
        assert!(hit, "suggestion is not clickable");
        app.press(crate::ui::Spot::Suggest(0), 1);
        assert_eq!(app.cmd, "/model ");
        app.cmd = "/e".into();
        let typing = crate::ui::Screen::paint(&app, 120, 30);
        assert!(typing.frame.contains(" /effort"), "{}", typing.frame);
        assert!(!typing.frame.contains(" /always-approve"), "{}", typing.frame);
    }

    #[test]
    fn files_follow_the_selected_chat_mesh() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let alpha = std::env::temp_dir().join(format!("granttap-files-a-{n}"));
        let beta = std::env::temp_dir().join(format!("granttap-files-b-{n}"));
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(alpha.join("alpha.txt"), "a").unwrap();
        std::fs::write(beta.join("beta.txt"), "b").unwrap();
        let mut app = App::new(alpha.clone(), PathBuf::from("/nowhere-home"));
        app.chats = vec![
            sample_chat(AgentKind::Grok, "alpha", &alpha.display().to_string(), 1, None),
            sample_chat(AgentKind::Grok, "beta", &beta.display().to_string(), 2, None),
        ];
        app.section = Section::Chats;
        app.cursor = app
            .lines()
            .iter()
            .position(|line| line.contains("beta"))
            .unwrap();
        app.cmd = "files".into();
        app.on_key(Key::Enter);
        assert_eq!(app.section, Section::Files);
        let names: Vec<_> = app.file_rows().iter().map(|row| row.name.clone()).collect();
        assert!(names.iter().any(|name| name == "beta.txt"), "{names:?}");
        assert!(!names.iter().any(|name| name == "alpha.txt"), "{names:?}");
        let _ = std::fs::remove_dir_all(alpha);
        let _ = std::fs::remove_dir_all(beta);
    }

    #[test]
    fn browser_follows_a_local_link() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-browse-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.html"),
            "<title>First</title><p>Go <a href=\"b.html\">next</a></p>",
        )
        .unwrap();
        std::fs::write(dir.join("b.html"), "<title>Second</title><p>Landed</p>").unwrap();
        let mut app = App::new(dir.clone(), PathBuf::from("/nowhere-home"));
        app.cmd = "browse a.html".into();
        app.on_key(Key::Enter);
        let joined = app.lines().join("\n");
        assert!(joined.contains("First"), "{joined}");
        assert!(joined.contains("<b.html>"), "{joined}");
        app.cursor = app
            .lines()
            .iter()
            .position(|line| line.contains("<b.html>"))
            .unwrap();
        app.cmd.clear();
        app.on_key(Key::Enter);
        let next = app.lines().join("\n");
        assert!(next.contains("Second"), "{next}");
        assert!(next.contains("Landed"), "{next}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_browser_lists_sites_by_visits_then_latest() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.cmd = "browser".into();
        app.on_key(Key::Enter);
        assert_eq!(app.section, Section::Browser);
        assert!(app.page.is_empty());
        let idle = app.lines();
        assert!(
            idle.iter().any(|line| line.contains("browse a site")),
            "{idle:?}"
        );
        assert!(
            idle.iter()
                .all(|line| !line.contains("киберпанк") && !line.contains("granttap-term mcp")),
            "{idle:?}"
        );
        app.visits = vec![
            store::Visit {
                url: "https://rare.test/a".into(),
                count: 1,
                last: 10,
            },
            store::Visit {
                url: "https://often.test/old".into(),
                count: 5,
                last: 3,
            },
            store::Visit {
                url: "https://www.also.test/new".into(),
                count: 5,
                last: 9,
            },
        ];
        let lines = app.lines();
        assert!(lines[0].contains("also.test"), "{lines:?}");
        assert!(lines[0].contains('5'), "{lines:?}");
        assert!(lines[1].contains("often.test"), "{lines:?}");
        assert!(lines[2].contains("rare.test"), "{lines:?}");
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(
            screen.frame.contains("Most visited first"),
            "{}",
            screen.frame
        );
        assert!(!screen.frame.contains("киберпанк"), "{}", screen.frame);
    }

    #[test]
    fn a_live_page_fills_the_pane_and_esc_leaves_the_field() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.set_size(100, 30);
        app.section = Section::Browser;
        app.web_on = true;
        app.page = vec![
            "https://example.com".into(),
            "Hello desk".into(),
            "[Find      ]".into(),
        ];
        app.web_hits = vec![crate::engine::WebHit {
            x: 0,
            y: 2,
            w: 12,
            h: 1,
            kind: crate::engine::KIND_FIELD,
            href: String::new(),
            label: "Find".into(),
            px: 16,
            py: 40,
        }];
        app.web_focus = Some(0);
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(screen.frame.contains("Hello desk"), "{}", screen.frame);
        assert!(screen.frame.contains("[Find"), "{}", screen.frame);
        let hit = (0..30).any(|y| {
            (0..100).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Web(0)))
        });
        assert!(hit, "field is not clickable");
        app.on_key(Key::Esc);
        assert!(app.web_focus.is_none());
        assert_eq!(app.note, "field left");
        assert!(app.web_on);
    }

    #[test]
    fn mesh_drills_to_a_file_and_the_browser_marks_it() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-mesh-drill-{n}"));
        let home = std::env::temp_dir().join(format!("granttap-mesh-home-{n}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(root.join("src/main.rs"), "mod gone;\nfn main() {}\n").unwrap();
        std::fs::write(root.join("src/app.rs"), "pub fn keep() {}\n").unwrap();
        let mut app = App::new(root.clone(), home.clone());
        app.section = Section::Mesh;
        let lines = app.lines();
        let src = lines
            .iter()
            .position(|line| line.contains("src/") && line.contains("ln"))
            .expect("src folder");
        assert!(lines.iter().any(|line| line.contains("Unhealthy")), "{lines:?}");
        assert!(lines[src].contains('!'), "{lines:?}");
        app.cursor = src;
        app.on_key(Key::Enter);
        let inside = app.lines();
        let main = inside
            .iter()
            .position(|line| line.contains("main.rs") && line.contains("ln"))
            .expect("main.rs");
        assert!(inside[main].contains("ch"), "{inside:?}");
        assert!(inside[main].contains('!'), "{inside:?}");
        let app_row = inside.iter().find(|line| line.contains("app.rs")).unwrap();
        assert!(!app_row.contains('!'), "{app_row}");
        app.cursor = main;
        app.on_key(Key::Enter);
        let opened = app.lines().join("\n");
        assert!(opened.contains("unhealthy  src/main.rs"), "{opened}");
        assert!(opened.contains("gone"), "{opened}");
        app.on_key(Key::Esc);
        assert!(app.lines().iter().any(|line| line.contains("main.rs")));
        app.on_key(Key::Esc);
        assert!(app.lines().iter().any(|line| line.contains("src/")));
        app.cmd = "browse src/main.rs".into();
        app.on_key(Key::Enter);
        let page = app.lines().join("\n");
        assert!(page.contains("unhealthy"), "{page}");
        assert!(page.contains("src/main.rs"), "{page}");
        app.cmd = "refresh".into();
        app.on_key(Key::Enter);
        assert_eq!(app.note, "refreshed");
        app.note.clear();
        app.section = Section::Mesh;
        app.on_key(Key::F(5));
        assert_eq!(app.note, "refreshed");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn an_open_chat_stays_open_and_files_use_its_mesh() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-keep-{n}"));
        let alpha = std::env::temp_dir().join(format!("granttap-keep-a-{n}"));
        let beta = std::env::temp_dir().join(format!("granttap-keep-b-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(alpha.join("alpha.txt"), "a").unwrap();
        std::fs::write(beta.join("beta.txt"), "b").unwrap();
        let mut app = App::new(alpha.clone(), home.clone());
        let chat = sample_chat(AgentKind::Grok, "beta", &beta.display().to_string(), 2, None);
        app.open_id = chat.id.clone();
        app.chats = vec![chat];
        app.mode = Mode::Split {
            lines: vec!["GROK  beta".into(), "user  stay".into()],
            agent: Some(AgentKind::Grok),
        };
        app.section = Section::Chats;
        app.cmd = "files".into();
        app.on_key(Key::Enter);
        assert_eq!(app.section, Section::Files);
        assert!(app.pane().is_some());
        assert!(app.lines().iter().any(|line| line == "user  stay"));
        let names: Vec<_> = app.file_rows().iter().map(|row| row.name.clone()).collect();
        assert!(names.iter().any(|name| name == "beta.txt"), "{names:?}");
        assert!(!names.iter().any(|name| name == "alpha.txt"), "{names:?}");
        assert!(app.suggestions().iter().any(|name| name == "/model"));
        app.cmd = "/eff".into();
        assert!(app.suggestions().iter().any(|name| name == "/effort"));
        app.on_key(Key::Tab);
        assert_eq!(app.cmd, "/effort ");
        assert!(app.suggestions().iter().any(|name| name == "/effort high"));
        assert!(app.suggestions().iter().any(|name| name == "/effort xhigh"));
        app.cursor = app
            .file_rows()
            .iter()
            .position(|row| row.name == "beta.txt")
            .unwrap();
        app.cmd = "edit".into();
        app.on_key(Key::Enter);
        let Mode::View { lines } = &app.mode else {
            panic!("vim did not open");
        };
        assert!(lines.iter().any(|line| line.starts_with("vim  beta.txt")), "{lines:?}");
        app.on_key(Key::Esc);
        assert!(app.lines().iter().any(|line| line == "user  stay"));
        app.cmd = "say hi".into();
        app.on_key(Key::Enter);
        assert_eq!(app.note, "not live");
        assert!(app.lines().iter().any(|line| line.starts_with("user  ") && line.contains("say hi")));
        let saved = std::fs::read_to_string(home.join(".granttap-term/outbox/id-beta.txt")).unwrap();
        assert!(saved.contains("say hi"), "{saved}");
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(alpha);
        let _ = std::fs::remove_dir_all(beta);
    }

    #[test]
    fn each_agent_offers_its_own_commands() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.open_id = "id-live".into();
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Claude),
        };
        assert!(app.suggestions().iter().any(|name| name == "/compact"));
        assert!(!app.suggestions().iter().any(|name| name == "/always-approve"));
        assert!(!app.suggestions().iter().any(|name| name.starts_with("/vim")));
        app.cmd = "/effort ".into();
        assert!(app.suggestions().iter().any(|name| name == "/effort max"));
        app.mode = Mode::Split {
            lines: vec!["assistant  hi".into()],
            agent: Some(AgentKind::Codex),
        };
        app.cmd.clear();
        assert!(app.suggestions().iter().any(|name| name == "/approvals"));
        assert!(!app.suggestions().iter().any(|name| name == "/always-approve"));
        app.cmd = "/effort ".into();
        assert!(app.suggestions().iter().any(|name| name == "/effort ultra"));
    }

    #[test]
    fn split_opens_another_shell_and_kubectl_joins_suggestions() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.section = Section::Term;
        app.cmd = "split".into();
        app.on_key(Key::Enter);
        assert_eq!(app.shell.count(), 2);
        assert!(app.shell.lines().iter().any(|line| line.contains("shell 2")));
        assert!(app.note.contains("2 of 2"), "{}", app.note);
        let screen = crate::ui::Screen::paint(&app, 100, 24);
        assert!(screen.frame.contains('|'), "{}", screen.frame);
        assert!(screen.frame.contains("shell 2"), "{}", screen.frame);
        app.cmd = "ku".into();
        assert!(app.suggestions().iter().any(|name| name == "kubectl"));
        app.cmd = "kubectl ".into();
        assert!(app.suggestions().iter().any(|name| name == "kubectl get"));
        app.shell.remember("helm");
        app.cmd.clear();
        assert!(app.suggestions().iter().any(|name| name == "helm"));
        app.open_id = "id-live".into();
        app.mode = Mode::Split {
            lines: vec!["user  stay".into()],
            agent: Some(AgentKind::Grok),
        };
        app.cmd = "/model".into();
        assert!(app.suggestions().is_empty() || app.suggestions().iter().all(|name| name.starts_with('/')));
        app.cmd = "/mo".into();
        assert!(app.suggestions().iter().any(|name| name == "/model"));
        app.cmd = "spl".into();
        assert!(app.suggestions().iter().any(|name| name == "split"));
        let left = app.rows_for(40);
        assert!(left.iter().any(|line| line.contains("shell 2")), "{left:?}");
    }

    #[test]
    fn a_repeated_outbox_line_is_not_pasted_again() {
        let transcript = vec!["user  stay".into(), "assistant  ok".into()];
        let outbox = vec!["user  stay".into(), "user  say hi".into()];
        let fresh = fresh_outbox(&transcript, outbox);
        assert_eq!(fresh, vec!["user  say hi".to_string()]);
    }

    #[test]
    fn an_open_chat_marks_the_reply_and_shows_its_commands() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.open_id = "id-live".into();
        app.section = Section::Chats;
        app.mode = Mode::Split {
            lines: vec![
                "GROK  desk".into(),
                "assistant  run `say hi` then **Enter** and `/model`".into(),
                "the `/effort high` line stays".into(),
            ],
            agent: Some(AgentKind::Grok),
        };
        let idle = crate::ui::Screen::paint(&app, 120, 28).frame;
        assert!(idle.contains(" /model"), "{idle}");
        assert!(idle.contains(" /effort"), "{idle}");
        assert!(!idle.contains("F6 switches"), "{idle}");
        assert!(!idle.contains("Wheel scrolls"), "{idle}");
        app.cmd = "/mo".into();
        let frame = crate::ui::Screen::paint(&app, 120, 28).frame;
        assert!(frame.contains("/model"), "{frame}");
        assert!(frame.contains("38;5;51msay hi"), "{frame}");
        assert!(frame.contains("38;5;220mEnter"), "{frame}");
        assert!(frame.contains("38;5;51m/model"), "{frame}");
        assert!(frame.contains("38;5;51m/effort high"), "{frame}");
        assert!(!frame.contains("`say hi`"), "{frame}");
        assert!(frame.contains('|'), "{frame}");
    }

    #[test]
    fn browser_theme_and_language_open_from_the_header() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-look-{n}"));
        let mut app = App::new(PathBuf::from("."), home.clone());
        app.section = Section::Browser;
        app.set_size(100, 30);
        assert_eq!(app.theme_label(), "auto");
        assert_eq!(app.lang_label(), "en");
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(screen.frame.contains("auto"), "{}", screen.frame);
        let theme = (0..100).any(|x| screen.spot_at(x, 0) == Some(crate::ui::Spot::Theme));
        let lang = (0..100).any(|x| screen.spot_at(x, 0) == Some(crate::ui::Spot::Lang));
        assert!(theme, "theme chip is missing");
        assert!(lang, "language chip is missing");
        app.press(crate::ui::Spot::Theme, 1);
        assert!(app.settings_open());
        assert_eq!(app.theme_label(), "auto");
        app.press(crate::ui::Spot::Lang, 1);
        assert!(app.settings_open());
        assert_eq!(app.lang_label(), "en");
        let sheet = crate::ui::Screen::paint(&app, 100, 30);
        assert!(sheet.frame.contains("Settings"), "{}", sheet.frame);
        assert!(sheet.frame.contains("dark"), "{}", sheet.frame);
        assert!(sheet.frame.contains("Language"), "{}", sheet.frame);
        assert!(sheet.frame.contains("Browser"), "{}", sheet.frame);
        assert!(sheet.frame.contains("Desk"), "{}", sheet.frame);
        let close = (0..100).any(|x| sheet.spot_at(x, 1) == Some(crate::ui::Spot::SettingClose));
        assert!(close, "the themes close chip is on the title row");
        pick_setting(&mut app, "theme:dark");
        assert_eq!(app.theme_label(), "dark");
        assert!(app.note.contains("theme dark"), "{}", app.note);
        assert!(app.note.contains("lang en"), "{}", app.note);
        assert!(app.settings_open());
        pick_setting(&mut app, "lang:ru");
        assert_eq!(app.lang_label(), "ru");
        assert!(app.note.contains("lang ru"), "{}", app.note);
        let loaded = store::load(&home);
        assert_eq!(loaded.web_theme, "dark");
        assert_eq!(loaded.web_lang, "ru");
        pick_setting(&mut app, "theme:light");
        assert_eq!(app.theme_label(), "light");
        pick_setting(&mut app, "theme:auto");
        assert_eq!(app.theme_label(), "auto");
        let browser = app
            .setting_rows()
            .iter()
            .position(|row| row.group && row.label.starts_with("Browser"))
            .expect("browser group");
        app.press(crate::ui::Spot::Setting(browser as u16), 1);
        pick_setting(&mut app, "js");
        assert!(!app.web_js_on());
        assert!(app.note.contains("js off"), "{}", app.note);
        assert!(!app.browser_pending());
        let desk = app
            .setting_rows()
            .iter()
            .position(|row| row.group && row.label.starts_with("Desk"))
            .expect("desk group");
        app.press(crate::ui::Spot::Setting(desk as u16), 1);
        pick_setting(&mut app, "accept:full");
        assert_eq!(app.accept_label(), "full");
        assert!(app.settings_open());
        app.on_key(Key::Esc);
        assert!(!app.settings_open());
        assert!(!scheme_is_dark("light", true));
        assert!(scheme_is_dark("dark", false));
        assert!(scheme_is_dark("auto", true));
        assert!(!scheme_is_dark("auto", false));
        assert_eq!(locale_tag("en", "he-IL"), "en-US");
        assert_eq!(locale_tag("ru", "en-US"), "ru-RU");
        assert_eq!(locale_tag("he", ""), "he-IL");
        assert_eq!(locale_tag("auto", "uk-UA"), "uk-UA");
        assert_eq!(locale_tag("auto", ""), "en-US");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn the_browser_bar_marks_and_steps_and_a_tall_control_is_clickable() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("granttap-bar-{n}"));
        let mut app = App::new(PathBuf::from("."), home.clone());
        app.section = Section::Browser;
        app.set_size(100, 30);
        assert!(app.web_js_on());
        assert!(app.web_live_on());
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(screen.frame.contains("[reload]"), "{}", screen.frame);
        assert!(screen.frame.contains("js on"), "{}", screen.frame);
        assert!(screen.frame.contains("live on"), "{}", screen.frame);
        assert!(screen.frame.contains("0b"), "{}", screen.frame);
        let theme = (0..100).any(|x| screen.spot_at(x, 0) == Some(crate::ui::Spot::Theme));
        assert!(theme, "theme chip stays on the header");
        let chip = |kind: u8, y: usize| {
            (0..100).any(|x| screen.spot_at(x, y) == Some(crate::ui::Spot::Browse(kind)))
        };
        assert!(chip(0, 1) && chip(2, 1) && chip(5, 1) && chip(6, 1), "bar chips sit above the rule");
        assert!(chip(0, 2) && chip(2, 2) && chip(5, 2) && chip(6, 2), "a click on the rule still hits the chip");
        app.press(crate::ui::Spot::Browse(5), 1);
        assert!(!app.web_js_on());
        assert!(!app.web_on());
        assert!(app.note.contains("js off"), "{}", app.note);
        app.press(crate::ui::Spot::Browse(6), 1);
        assert!(!app.web_live_on());
        assert!(app.note.contains("live off"), "{}", app.note);
        app.press(crate::ui::Spot::Browse(2), 1);
        assert_eq!(app.note, "no page");
        app.page_url = "https://example.com/desk".into();
        app.press(crate::ui::Spot::Browse(3), 1);
        assert_eq!(app.note, "marked");
        assert!(app.page_marked());
        let saved = crate::ui::Screen::paint(&app, 100, 30);
        assert!(saved.frame.contains("[saved]"), "{}", saved.frame);
        app.press(crate::ui::Spot::Browse(4), 1);
        assert!(app.web_panel());
        assert!(app.lines().iter().any(|line| line.contains("example.com")));
        app.on_key(Key::Esc);
        assert!(!app.web_panel());
        assert_eq!(app.note, "page");
        app.web_on = true;
        app.page = vec!["https://example.com/desk".into(), "card".into(), "Search".into()];
        app.web_hits = vec![crate::engine::WebHit {
            x: 1,
            y: 0,
            w: 10,
            h: 3,
            kind: crate::engine::KIND_BUTTON,
            href: String::new(),
            label: "Search".into(),
            px: 40,
            py: 24,
        }];
        let page = crate::ui::Screen::paint(&app, 100, 30);
        assert!(page.spot_at(2, 3) == Some(crate::ui::Spot::Web(0)), "top row");
        assert!(page.spot_at(2, 4) == Some(crate::ui::Spot::Web(0)), "label row");
        assert!(page.spot_at(2, 5) == Some(crate::ui::Spot::Web(0)), "bottom row");
        app.web_on = false;
        app.page_url = "local-a".into();
        app.page = vec!["alpha".into()];
        app.page_fwd.push(("local-c".into(), vec!["gamma".into()]));
        app.remember_page();
        assert!(app.page_fwd.is_empty());
        app.page_hist.clear();
        app.page_url = "local-a".into();
        app.page = vec!["alpha".into()];
        app.page_hist.push(("local-b".into(), vec!["beta".into()]));
        app.browse_back();
        assert!(app.lines().iter().any(|line| line.contains("beta")), "{:?}", app.lines());
        app.browse_forward();
        assert!(app.lines().iter().any(|line| line.contains("alpha")), "{:?}", app.lines());
        assert_eq!(compact_bytes(48), "48b");
        assert_eq!(compact_bytes(48 * 1024), "48kb");
        assert_eq!(compact_bytes(3 * 1024 * 1024 / 2), "1.5mb");
        let loaded = store::load(&home);
        assert!(!loaded.web_js);
        assert!(!loaded.web_live);
        assert_eq!(loaded.bookmarks, vec!["https://example.com/desk".to_string()]);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_text_page_reload_keeps_its_size_when_the_picture_is_large() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-reload-{n}"));
        let home = std::env::temp_dir().join(format!("granttap-reload-home-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut raw = vec![30u8; 200 * 200 * 3];
        for pixel in raw.chunks_mut(3) {
            pixel.copy_from_slice(&[30, 90, 200]);
        }
        image::RgbImage::from_raw(200, 200, raw)
            .unwrap()
            .save(dir.join("g.png"))
            .unwrap();
        let html = dir.join("page.html");
        std::fs::write(
            &html,
            "<title>Google Images</title><p>Search</p><img alt=\"G\" src=\"g.png\"><img src=\"gone.png\">",
        )
        .unwrap();
        let mut app = App::new(dir.clone(), home.clone());
        app.section = Section::Browser;
        app.set_size(100, 30);
        app.page_url = html.display().to_string();
        app.page = vec!["stale".into()];
        app.web_bytes = 0;
        app.press(crate::ui::Spot::Browse(2), 1);
        assert!(app.browser_pending());
        assert_eq!(app.note, "loading");
        app.run_browser_pending();
        assert!(!app.web_on());
        assert!(app.web_bytes > 0, "{}", app.web_bytes);
        assert_ne!(app.web_size_label(), "0b");
        assert!(app.note.starts_with("reloaded"), "{}", app.note);
        assert!(
            app.lines().iter().any(|line| line.contains("Google Images")),
            "{:?}",
            app.lines()
        );
        let blocks = app
            .lines()
            .iter()
            .filter(|line| crate::draw::has_blocks(line))
            .count();
        assert!(blocks <= 4, "{blocks}");
        app.press(crate::ui::Spot::Browse(5), 1);
        assert!(app.browser_pending());
        assert!(!app.web_js_on());
        app.run_browser_pending();
        assert!(!app.web_on());
        assert!(app.web_bytes > 0);
        assert_ne!(app.web_size_label(), "0b");
        assert!(app.note.starts_with("reloaded"), "{}", app.note);
        assert!(
            app.lines().iter().any(|line| line.contains("Search")),
            "{:?}",
            app.lines()
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_bar_click_lights_queues_and_covers_the_control() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.section = Section::Browser;
        app.set_size(100, 30);
        app.web_on = true;
        app.web_theme = "dark".into();
        app.page_url = "https://example.com/desk".into();
        app.page = vec!["https://example.com/desk".into(), "Open docs".into()];
        app.web_hits = vec![crate::engine::WebHit {
            x: 0,
            y: 1,
            w: 10,
            h: 1,
            kind: crate::engine::KIND_LINK,
            href: "https://example.com/docs".into(),
            label: "Open docs".into(),
            px: 8,
            py: 16,
        }];
        app.press(crate::ui::Spot::Browse(2), 1);
        assert!(app.browser_pending(), "reload waits for the spinner frame");
        assert!(app.web_busy());
        assert_eq!(app.browse_hot(), Some(2));
        assert_eq!(app.note, "loading");
        let hot = crate::ui::Screen::paint(&app, 100, 30);
        assert!(hot.frame.contains("48;5;220"), "{}", hot.frame);
        assert!(hot.frame.contains("loading") || hot.frame.contains('['), "{}", hot.frame);
        assert!(
            hot.frame.contains("48;5;234"),
            "the link keeps a hit slab {}",
            hot.frame
        );
        app.press(crate::ui::Spot::Web(0), 1);
        assert_eq!(app.web_hot(), Some(0));
        assert!(app.browser_pending());
        let pressed = crate::ui::Screen::paint(&app, 100, 30);
        assert!(pressed.frame.contains("30;48;5;220"), "{}", pressed.frame);
        app.on_key(Key::Char('?'));
        assert!(app.keys_open());
        let menu = crate::ui::Screen::paint(&app, 100, 30);
        assert!(menu.frame.contains("Keyboard Shortcuts"), "{}", menu.frame);
        assert!(menu.frame.contains("Essentials"), "{}", menu.frame);
        assert!(menu.frame.contains("F10 / Ctrl+c"), "{}", menu.frame);
        assert!(menu.frame.contains("[X]"), "{}", menu.frame);
        let close = (0..30).any(|y| {
            (0..100).any(|x| menu.spot_at(x, y) == Some(crate::ui::Spot::KeysClose))
        });
        assert!(close, "the close chip is on the menu");
        let plain = menu.plain_rows();
        assert!(
            plain.iter().any(|row| row.contains('┌')),
            "shortcuts sit in a box {plain:?}"
        );
        assert!(
            plain.iter().any(|row| row.contains("Open docs")),
            "the page stays beside the box {plain:?}"
        );
        app.on_key(Key::Esc);
        assert!(!app.keys_open());
        app.on_key(Key::Char(','));
        assert!(app.settings_open());
        let themes = crate::ui::Screen::paint(&app, 100, 30);
        assert!(themes.frame.contains("Theme"), "{}", themes.frame);
        assert!(themes.frame.contains("Language"), "{}", themes.frame);
        app.on_key(Key::Char('?'));
        assert!(app.keys_open());
        assert!(!app.settings_open());
        app.on_key(Key::Esc);
        assert!(!app.keys_open());
        app.cmd = "browse ".into();
        app.on_key(Key::Char('?'));
        assert!(!app.keys_open());
        assert!(app.cmd.contains('?'));
    }

    #[test]
    fn a_light_page_keeps_its_paper_and_typed_text() {
        let mut app = App::new(PathBuf::from("."), PathBuf::from("/nowhere-home"));
        app.section = Section::Browser;
        app.set_size(100, 30);
        app.web_on = true;
        app.web_theme = "light".into();
        app.page = vec![
            "\x1b[38;2;32;33;36;48;2;255;255;255mhttps://www.google.com/".into(),
            "\x1b[48;2;255;255;255mНайти".into(),
        ];
        app.web_hits = vec![
            crate::engine::WebHit {
                x: 0,
                y: 1,
                w: 24,
                h: 1,
                kind: crate::engine::KIND_BUTTON,
                href: String::new(),
                label: "search".into(),
                px: 40,
                py: 40,
            },
            crate::engine::WebHit {
                x: 2,
                y: 1,
                w: 16,
                h: 1,
                kind: crate::engine::KIND_FIELD,
                href: String::new(),
                label: "Найти".into(),
                px: 20,
                py: 40,
            },
        ];
        assert_eq!(app.field_under(0), 1);
        let page = crate::ui::Screen::paint(&app, 100, 30);
        assert!(
            !page.frame.contains("48;5;234"),
            "a light page has no dark link slab {}",
            page.frame
        );
        assert!(
            !page.frame.contains("48;5;17"),
            "a light field is not a navy slab {}",
            page.frame
        );
        let field_y = (0..30).find(|y| page.spot_at(4, *y) == Some(crate::ui::Spot::Web(1)));
        assert!(field_y.is_some(), "the field wins the click");
        app.web_focus = Some(1);
        app.web_draft = "привет".into();
        app.cmd = "left over".into();
        let typed = crate::ui::Screen::paint(&app, 100, 30);
        assert!(
            typed.plain_rows().iter().any(|row| row.contains("привет")),
            "typed text is on the field {:?}",
            typed.plain_rows()
        );
        assert_eq!(app.cmd, "left over");
    }

    #[test]
    fn the_mesh_keeps_the_red_mark_on_the_selected_row() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("granttap-mesh-row-{n}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "mod gone;\nfn main() {}\n").unwrap();
        let _ = std::process::Command::new("git")
            .arg("init")
            .arg(&root)
            .status();
        let mut app = App::new(root.clone(), PathBuf::from("/nowhere-home"));
        app.section = Section::Mesh;
        app.set_size(100, 30);
        let rows = app.lines();
        let index = rows
            .iter()
            .position(|line| line.contains("38;5;196m!") && line.contains("missing gone.rs"))
            .expect("broken row");
        app.cursor = index;
        let screen = crate::ui::Screen::paint(&app, 100, 30);
        assert!(screen.frame.contains("38;5;196m!"), "{}", screen.frame);
        assert!(screen.frame.contains("missing gone.rs"), "{}", screen.frame);
        assert!(screen.frame.contains('█'), "{}", screen.frame);
        let _ = std::fs::remove_dir_all(root);
    }
}
