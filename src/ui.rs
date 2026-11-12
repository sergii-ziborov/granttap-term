use crate::app::App;
use crate::model::{AgentKind, Section};

const RED: &str = "\x1b[38;5;196m";
const GREEN: &str = "\x1b[38;5;40m";
const CYAN: &str = "\x1b[38;5;51m";
const GOLD: &str = "\x1b[38;5;220m";
const SALMON: &str = "\x1b[38;5;203m";
/// Assistant prose. Distinct from user cyan and from the salmon chat list.
const PAPER: &str = "\x1b[38;5;223m";
const BOLD_GOLD: &str = "\x1b[1;38;5;220m";
const DIM: &str = "\x1b[38;5;131m";

const RULE: &str = "\x1b[38;5;160m";
const KEEP: &str = "\x1b[0;48;5;0m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    Model,
    Effort,
    Permit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Tab(Section),
    Row(usize),
    Ask(usize),
    Cmd,
    Desc,
    Image(u16),
    Open(u16),
    Chip(Chip),
    Back,
    Suggest(u16),
    Web(u16),
    /// Queue row button. High bits are the row, low two bits are send, edit, or cancel.
    Steer(u16),
    Theme,
    Lang,
    /// Browser bar. 0 back, 1 forward, 2 reload, 3 mark, 4 history, 5 js, 6 live.
    Browse(u8),
    /// One row of the shortcuts menu.
    Keys(u16),
    KeysClose,
    /// One row of the themes modal.
    Setting(u16),
    SettingClose,
    /// Context chip and its hover tip.
    Meter,
    MeterClose,
    MeterTab(u8),
    Compact,
    /// Collapsed work row. The value is the source line index.
    Fold(u16),
    /// One folder in the dirty tree. The value is its place among folder rows.
    Tree(u16),
    /// One markdown file in the dirty tree.
    Doc(u16),
    DocClose,
    /// The inside of the markdown frame. A click here stays put.
    DocBox,
    /// One row of the model, effort, or approval box.
    Pick(u16),
    PickClose,
    /// Top-corner settings chip.
    Gear,
    /// Top-corner shortcuts chip.
    Help,
    /// Close the open chat.
    ChatClose,
    /// Expand or shrink the open chat.
    ChatFill,
    PicClose,
    /// The inside of the picture frame. A click here stays put.
    PicBox,
}

struct Piece {
    text: String,
    ink: &'static str,
    image: u16,
    open: u16,
    fold: u16,
    tree: u16,
    doc: u16,
    clock: String,
}

struct Hit {
    x: usize,
    y: usize,
    w: usize,
    spot: Spot,
}

pub struct Screen {
    pub frame: String,
    pub cursor: Option<(usize, usize)>,
    hits: Vec<Hit>,
    /// Underlined link runs. Hover paints these even when the line is not a button.
    hots: Vec<(usize, usize, usize)>,
    glyphs: Vec<Vec<char>>,
}

impl Screen {
    pub fn paint(app: &App, cols: usize, rows: usize) -> Self {
        let mut screen = Self {
            frame: String::new(),
            cursor: None,
            hits: Vec::new(),
            hots: Vec::new(),
            glyphs: vec![vec![' '; cols]; rows],
        };
        screen
            .frame
            .push_str(&format!("\x1b]0;{}\x07", app.window_title()));
        if rows > 0 && cols > 1 {
            screen.frame.push_str(&format!("\x1b[1;{rows}r"));
            for y in 0..rows {
                screen.line(y, cols, "", DIM);
            }
        }
        if cols < 40 || rows < 8 {
            screen.line(0, cols, "too small   F10 quits", RED);
            return screen;
        }
        let tab_y = rows - 1;
        let input_y = rows - 2;
        let desc_y = rows - 3;
        // An open chat and the Term tab keep their menu up, so the agent's
        // commands and the shell tools are visible before the first key.
        let menu = !app.vim_active()
            && (!app.cmd.is_empty()
                || app.transcript_lines().is_some()
                || app.section == Section::Term);
        let suggestions = if menu {
            app.suggestions()
        } else {
            Vec::new()
        };
        let show_suggest = !suggestions.is_empty() && app.note.is_empty() && !app.desc_open;
        let show_band = app.desc_open
            || !app.note.is_empty()
            || show_suggest
            || app.transcript_lines().is_none();
        let rule_y = if show_band { desc_y - 1 } else { desc_y };
        let list_top = if app.fill {
            1
        } else if app.section == Section::Browser {
            3
        } else {
            2
        };
        let history = if app.fill {
            Vec::new()
        } else {
            app.history_band()
        };
        let asks = if app.fill {
            Vec::new()
        } else {
            app.ask_lines()
        };
        let ask_n = asks.len().min(3).min(rule_y.saturating_sub(list_top));
        let queued = app.queue_items();
        let queue_cap = rule_y.saturating_sub(list_top + ask_n);
        let queue_n = queued.len().min(queue_cap);
        let hist_room = rule_y
            .saturating_sub(list_top + ask_n + queue_n)
            .saturating_sub(1);
        let hist_n = history.len().min(2).min(hist_room);
        let body_end = rule_y.saturating_sub(hist_n + queue_n);
        if app.fill {
            screen.line(0, cols, &app.fill_bar(), RED);
            screen.hit(0, 0, cols.saturating_sub(1), Spot::Back);
            screen.paint_corner(cols);
            if app.look_name() == "linux" {
                screen.paint_penguin(app.fill_bar().chars().count() + 2, cols);
            }
        } else {
            screen.chrome(app, cols);
        }
        for (index, line) in asks.iter().take(ask_n).enumerate() {
            let y = list_top + index;
            screen.line(y, cols, line, RED);
            screen.hit(0, y, cols.saturating_sub(1), Spot::Ask(index));
        }
        screen.body(app, cols, list_top + ask_n, body_end);
        for (index, line) in history.iter().take(hist_n).enumerate() {
            let y = body_end + index;
            let ink = if index == 0 { CYAN } else { DIM };
            screen.line(y, cols, line, ink);
        }
        for (index, text) in queued.iter().take(queue_n).enumerate() {
            let y = body_end + hist_n + index;
            screen.queue_row(y, cols, index as u16, text);
        }
        let rule = "-".repeat(cols.saturating_sub(1));
        screen.line(rule_y, cols, &rule, RULE);
        if app.desc_open {
            screen.line(desc_y, cols, &app.description_body(), DIM);
            screen.hit(0, desc_y, cols.saturating_sub(1), Spot::Desc);
        } else if !app.note.is_empty() {
            screen.line(desc_y, cols, &app.note, DIM);
        } else if show_suggest {
            screen.suggest_row(&suggestions, app.suggest, cols, desc_y);
        } else if show_band {
            screen.line(desc_y, cols, &app.description_line(), DIM);
            screen.hit(0, desc_y, cols.saturating_sub(1), Spot::Desc);
        }
        screen.composer(app, cols, input_y);
        if let Some(text) = app.tip_line() {
            let w = text.chars().count();
            let x = cols.saturating_sub(w + 1);
            let y = input_y.saturating_sub(1);
            screen.put(x, y, cols, &text, "\x1b[38;5;223;48;5;236m");
            screen.hit(x, y, w, Spot::Meter);
        }
        screen.tabs(app.section, cols, tab_y);
        if let Some(sel) = app.selection() {
            screen.paint_selection(sel);
        }
        screen.overlay(app);
        if app.pick().is_some() {
            screen.paint_pick(app, cols, 1, rule_y);
        } else if app.meter_open() {
            screen.paint_meter(app, cols, 1, rule_y);
        } else if app.doc_open() {
            screen.paint_doc(app, cols, 1, rule_y);
        } else if app.image_outline().is_some() {
            screen.paint_picture(app, cols, 1, rule_y);
        } else if app.settings_open() {
            screen.paint_sheet(app, cols, 1, rule_y, true);
        } else if app.keys_open() {
            screen.paint_sheet(app, cols, 1, rule_y, false);
        }
        screen.paint_hover(app.hover_cell());
        if app.look_name() != "cyberpunk" {
            screen.frame = tint_frame(&screen.frame, app.look_name());
        }
        screen
    }

    fn composer(&mut self, app: &App, cols: usize, y: usize) {
        let face = app.composer_face();
        let label = face.agent.label();
        let lane = shown_lane(face.lane);
        let effort = if face.ultra {
            format!("{} · ultra", face.effort)
        } else {
            face.effort.clone()
        };
        let status = format!("{} · {effort} · {}{lane}", face.model, face.permit);
        // The model, effort, and permit chips drive a chat. On the browser
        // they do nothing, so the address keeps the whole row.
        let browser = app.section == Section::Browser;
        let status_w = if browser {
            0
        } else {
            label.chars().count() + 1 + status.chars().count()
        };
        let at = if browser {
            cols
        } else {
            cols.saturating_sub(status_w + 1)
        };
        let text_end = at.saturating_sub(1).max(4);
        let room = text_end.saturating_sub(2).max(1);
        let typed = if app.cmd.is_empty() {
            let hint = app.prompt_hint();
            if hint.is_empty() {
                "\x1b[38;5;51m❯".to_string()
            } else {
                format!("\x1b[38;5;51m❯\x1b[38;5;131m {hint}")
            }
        } else {
            let shown = clip_chars(&app.cmd, room);
            format!("\x1b[38;5;51m❯\x1b[38;5;203m {shown}")
        };
        self.line(y, cols, &typed, SALMON);
        self.hit(0, y, cols.saturating_sub(1), Spot::Cmd);
        if let Some((chip_x, _, chip_w)) = app.meter_box() {
            if let Some(label) = app.context_chip() {
                let text = format!("[{label}]");
                self.put(chip_x, y, cols, &text, "\x1b[30;48;5;220m");
                self.hit(chip_x, y, chip_w, Spot::Meter);
            }
        }
        if !browser && at > 6 {
            self.put(at, y, cols, label, agent_color(face.agent));
            let mut x = at + label.chars().count() + 1;
            x = self.chip(x, y, cols, &face.model, Chip::Model);
            x = self.gap(x, y, cols);
            x = self.chip(x, y, cols, &effort, Chip::Effort);
            x = self.gap(x, y, cols);
            x = self.chip(x, y, cols, &face.permit, Chip::Permit);
            if !lane.is_empty() {
                self.put(x, y, cols, &lane, DIM);
            }
        }
        if app.web_focus().is_none()
            && (app.mode_is_desk() || app.fill || app.transcript_lines().is_some())
        {
            let col = (3 + app.cmd.chars().count()).min(text_end.max(3));
            self.cursor = Some((col, y + 1));
        }
    }

    fn suggest_row(&mut self, suggestions: &[String], selected: usize, cols: usize, y: usize) {
        self.line(y, cols, "", DIM);
        if suggestions.is_empty() {
            return;
        }
        let labels: Vec<(String, usize)> = suggestions
            .iter()
            .map(|name| {
                let label = format!(" {name}");
                let width = label.chars().count();
                (label, width)
            })
            .collect();
        let selected = selected.min(labels.len() - 1);
        let limit = cols.saturating_sub(1);
        let mut start = 0usize;
        loop {
            let mut x = 1usize;
            let mut seen = false;
            for (index, (_, width)) in labels.iter().enumerate().skip(start) {
                if x + width >= limit {
                    break;
                }
                if index == selected {
                    seen = true;
                }
                x += width + 1;
            }
            if seen || start >= selected {
                break;
            }
            start += 1;
        }
        let mut x = 1usize;
        for (index, (label, width)) in labels.iter().enumerate().skip(start) {
            if x + width >= limit {
                break;
            }
            let ink = if index == selected { GOLD } else { CYAN };
            self.put(x, y, cols, label, ink);
            self.hit(x, y, *width, Spot::Suggest(index as u16));
            x += width + 1;
        }
    }

    fn queue_row(&mut self, y: usize, cols: usize, index: u16, text: &str) {
        let buttons = [("[Send now]", 0u16), ("[edit]", 1), ("[cancel]", 2)];
        let width: usize = buttons.iter().map(|(label, _)| label.chars().count()).sum();
        let at = cols.saturating_sub(width + 1);
        let room = at.saturating_sub(1).max(1);
        self.line(y, cols, &fit_tail(text, room), SALMON);
        if at > 2 {
            let mut x = at;
            for (label, kind) in buttons {
                let w = label.chars().count();
                self.put(x, y, cols, label, GOLD);
                self.hit(x, y, w, Spot::Steer((index << 2) | kind));
                x += w;
            }
        }
    }

    fn chip(&mut self, x: usize, y: usize, cols: usize, text: &str, chip: Chip) -> usize {
        let width = text.chars().count();
        self.put(x, y, cols, text, GOLD);
        self.hit(x, y, width, Spot::Chip(chip));
        x + width
    }

    fn gap(&mut self, x: usize, y: usize, cols: usize) -> usize {
        self.put(x, y, cols, " · ", DIM);
        x + 3
    }

    fn chrome(&mut self, app: &App, cols: usize) {
        self.line(0, cols, "", DIM);
        let brand = app.window_title();
        self.put(0, 0, cols, &brand, CYAN);
        let section = app.section_title();
        let section_at = brand.chars().count() + 2;
        self.put(section_at, 0, cols, &section, RED);
        if !app.mode_is_desk() {
            let span = section_at + section.chars().count();
            self.hit(0, 0, span.max(1), Spot::Back);
        }
        let section_end = section_at + section.chars().count();
        let reserve = corner_reserve();
        if app.section == Section::Browser {
            let theme = app.theme_label();
            let lang = app.lang_label();
            let need = theme.chars().count() + 2 + lang.chars().count();
            let pair_at = cols.saturating_sub(need + 1 + reserve);
            if pair_at > section_end {
                self.put(pair_at, 0, cols, theme, GOLD);
                self.hit(pair_at, 0, theme.chars().count() + 1, Spot::Theme);
                let lang_at = pair_at + theme.chars().count() + 2;
                self.put(lang_at, 0, cols, lang, GOLD);
                self.hit(lang_at, 0, lang.chars().count() + 1, Spot::Lang);
            }
        }
        self.paint_corner(cols);
        let mark_at = section_end + 2;
        let linux = app.look_name() == "linux";
        if linux {
            self.paint_penguin(mark_at, cols);
        }
        let rule = "-".repeat(cols.saturating_sub(1));
        if app.section == Section::Browser {
            self.paint_browse_bar(app, cols, 1);
            self.line(2, cols, &rule, RULE);
        } else {
            self.line(1, cols, &rule, RULE);
            if linux {
                self.paint_penguin_feet(mark_at, cols);
            }
        }
    }

    /// A one-cell-wide bird for the linux look. White body, yellow beak.
    fn paint_penguin(&mut self, x: usize, cols: usize) {
        let limit = cols.saturating_sub(corner_reserve());
        if x + 4 >= limit {
            return;
        }
        let white = "\x1b[38;5;203m";
        let beak = "\x1b[38;5;220m";
        self.put(x, 0, cols, "(•)", white);
        self.put(x + 3, 0, cols, ">", beak);
    }

    fn paint_penguin_feet(&mut self, x: usize, cols: usize) {
        let limit = cols.saturating_sub(corner_reserve());
        if x + 4 >= limit {
            return;
        }
        let foot = "\x1b[38;5;220m";
        self.put(x + 1, 1, cols, "^", foot);
        self.put(x + 2, 1, cols, "^", foot);
    }

    fn overlay(&mut self, app: &App) {
        let overlay = app.image_overlay();
        if !overlay.is_empty() {
            self.frame.push_str("\x1b[2;1H");
            self.frame.push_str(overlay);
        }
    }

    pub fn spot_at(&self, x: usize, y: usize) -> Option<Spot> {
        self.hits
            .iter()
            .rev()
            .find_map(|hit| (hit.y == y && x >= hit.x && x < hit.x + hit.w).then_some(hit.spot))
    }

    pub fn plain_rows(&self) -> Vec<String> {
        self.glyphs
            .iter()
            .map(|row| row.iter().collect())
            .collect()
    }

    fn body(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        if app.section == Section::Browser
            && app.web_on()
            && !app.web_panel()
            && app.transcript_lines().is_none()
        {
            self.paint_web(app, cols, y0, y1);
            return;
        }
        if app.fill
            && let Some(lines) = app.transcript_lines()
        {
            let lines = lines.to_vec();
            self.paint_transcript(app, 0, cols, y0, y1, &lines);
            return;
        }
        if let Some(pane) = app.pane() {
            let pane = pane.to_vec();
            self.paint_columns(app, cols, y0, y1, &pane);
            return;
        }
        if app.section == Section::Term && app.mode_is_desk() {
            self.paint_term(app, cols, y0, y1);
            return;
        }
        let lines = if app.mode_is_desk() && matches!(app.section, Section::Chats | Section::Tasks)
        {
            app.rows_for(cols.saturating_sub(3))
        } else {
            app.lines()
        };
        let room = y1.saturating_sub(y0);
        if lines.is_empty() {
            self.line(y0, cols, app.empty_line(), DIM);
            return;
        }
        let selected_at = app.focus_index();
        let follow = app.mode_is_desk()
            || matches!(app.section, Section::Browser | Section::Lessons);
        let track = matches!(app.section, Section::Chats | Section::Tasks);
        let start = if track {
            list_origin(lines.len(), room, selected_at, app.list_from)
        } else if follow {
            selected_at.saturating_sub(room.saturating_sub(1))
        } else {
            app.scroll.min(lines.len().saturating_sub(room.max(1)))
        };
        let bar_x = cols.saturating_sub(2);
        let side = app.side_lines();
        let split = !side.is_empty();
        for (index, line) in lines.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let selected = follow && index == selected_at;
            let mark = if selected { '>' } else { ' ' };
            let shown = if keep_color(line) {
                line.clone()
            } else if selected || split {
                strip_sgr(line)
            } else {
                line.clone()
            };
            let shown = if app.section == Section::Browser && !keep_color(line) {
                let room = if split { cols / 2 - 4 } else { cols.saturating_sub(2) };
                fit_visible(&shown, room.max(8))
            } else if split {
                clip_chars(&shown, cols / 2 - 4)
            } else {
                shown
            };
            let ink = if selected { GOLD } else { SALMON };
            self.line(y, cols, &format!("{mark} {shown}"), ink);
            if let Some(name) = side.get(index) {
                self.put(cols / 2, y, cols, &clip_chars(name, 36), CYAN);
            }
            let hit_w = if track {
                bar_x.max(1)
            } else {
                cols.saturating_sub(1)
            };
            self.hit(0, y, hit_w, Spot::Row(index));
        }
        if track && bar_x > 1 {
            let from_end = lines.len().saturating_sub(start.saturating_add(room));
            self.paint_scroll(bar_x, y0, room, lines.len().max(1), from_end, false);
        }
    }

    fn paint_browse_bar(&mut self, app: &App, cols: usize, y: usize) {
        let mark = if app.page_marked() { "[saved]" } else { "[mark]" };
        let js = if app.web_js_on() { "js on" } else { "js off" };
        let live = if app.web_live_on() { "live on" } else { "live off" };
        let reload = if app.web_busy() {
            format!("[  {}   ]", crate::app::busy_glyph())
        } else {
            "[reload]".into()
        };
        let parts = [
            ("[<]".to_string(), Some(0u8)),
            ("[>]".to_string(), Some(1)),
            (reload, Some(2)),
            (mark.to_string(), Some(3)),
            ("[hist]".to_string(), Some(4)),
            (app.web_size_label(), None),
            (js.to_string(), Some(5)),
            (live.to_string(), Some(6)),
            (format!("net {}", app.web_net_label()), None),
        ];
        let mut x = 0usize;
        for (text, spot) in parts {
            let width = text.chars().count();
            let gap = usize::from(x > 0);
            if x + gap + width >= cols {
                break;
            }
            x += gap;
            let hot = spot.is_some_and(|kind| app.browse_hot() == Some(kind));
            let ink = if hot {
                "\x1b[30;48;5;220m"
            } else if spot.is_some() {
                "\x1b[38;5;220;48;5;236m"
            } else {
                DIM
            };
            self.put(x, y, cols, &text, ink);
            if let Some(kind) = spot {
                let cover = width + usize::from(x + width + 1 < cols);
                self.hit(x, y, cover, Spot::Browse(kind));
                self.hit(x, y + 1, cover, Spot::Browse(kind));
            }
            x += width;
        }
    }

    fn paint_web(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        let room = y1.saturating_sub(y0);
        for (index, line) in app.web_lines().iter().take(room).enumerate() {
            self.span(0, y0 + index, cols, line, "");
        }
        let hits = app.web_hits();
        let mut order: Vec<usize> = (0..hits.len())
            .filter(|&index| hits[index].kind != crate::engine::KIND_FIELD)
            .collect();
        order.extend(
            (0..hits.len()).filter(|&index| hits[index].kind == crate::engine::KIND_FIELD),
        );
        for index in order {
            let hit = &hits[index];
            let w = hit.w.min(cols.saturating_sub(hit.x));
            if w == 0 {
                continue;
            }
            let hot = app.web_hot() == Some(index as u16);
            for row in 0..hit.h.max(1) {
                let y = y0 + hit.y + row;
                if y >= y1 {
                    break;
                }
                let line_index = hit.y + row;
                if let Some(line) = app.web_lines().get(line_index)
                    && !crate::draw::has_blocks(line)
                {
                    let light = !app.web_scheme_dark() || line_is_light(line);
                    if hot || !light {
                        self.veil(hit.x, y, cols, w, line, hit.kind, hot);
                    }
                }
                self.hit(hit.x, y, w, Spot::Web(index as u16));
            }
        }
        if let Some(index) = app.web_focus()
            && let Some(hit) = app.web_hits().get(usize::from(index))
        {
            let draft = app.web_draft();
            if !draft.is_empty() {
                let row = y0 + hit.y + if hit.h >= 3 { hit.h / 2 } else { 0 };
                let col = hit.x + if hit.h >= 3 { 2 } else { 1 };
                if row < y1 {
                    let room = hit.w.saturating_sub(if hit.h >= 3 { 4 } else { 2 }).max(1);
                    let mut shown: String = draft.chars().take(room).collect();
                    let pad = room.saturating_sub(shown.chars().count());
                    shown.push_str(&" ".repeat(pad));
                    let ink = if app.web_scheme_dark() {
                        "\x1b[38;5;51;48;5;17m"
                    } else {
                        "\x1b[30;48;2;255;255;255m"
                    };
                    self.put(col, row, cols, &shown, ink);
                    let caret = col + draft.chars().count().min(room);
                    self.cursor = Some((caret + 1, row + 1));
                }
            } else {
                let (col, row) = field_caret(hit.x, y0 + hit.y, hit.h);
                if row.saturating_sub(1) < y1 {
                    self.cursor = Some((col, row));
                }
            }
        }
    }

    fn paint_columns(&mut self, app: &App, cols: usize, y0: usize, y1: usize, pane: &[String]) {
        let room = y1.saturating_sub(y0);
        if room == 0 {
            return;
        }
        let left = Self::list_column(cols);
        let split = left < cols.saturating_sub(8);
        let bar_x = if split {
            left.saturating_sub(2)
        } else {
            cols.saturating_sub(2)
        };
        let text_end = bar_x.max(8).min(left);
        let list = app.rows_for(text_end.saturating_sub(3));
        let selected_at = app.focus_index();
        let start = list_origin(list.len(), room, selected_at, app.list_from);
        for (index, line) in list.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let selected = index == selected_at;
            let mark = if selected { '>' } else { ' ' };
            let shown = if keep_color(line) {
                line.clone()
            } else if selected {
                strip_sgr(line)
            } else {
                line.clone()
            };
            let ink = if selected { GOLD } else { SALMON };
            self.span(0, y, text_end.min(cols), &format!("{mark} {shown}"), ink);
            self.hit(0, y, text_end.max(1), Spot::Row(index));
        }
        if bar_x > 1 {
            let from_end = list.len().saturating_sub(start.saturating_add(room));
            self.paint_scroll(bar_x, y0, room, list.len().max(1), from_end, false);
        }
        if !split {
            return;
        }
        for y in y0..y1 {
            self.span(left, y, left + 2, "|", CYAN);
        }
        self.paint_transcript(app, left + 2, cols, y0, y1, pane);
    }

    fn paint_transcript(
        &mut self,
        app: &App,
        x: usize,
        end: usize,
        y0: usize,
        y1: usize,
        pane: &[String],
    ) {
        let mut room = y1.saturating_sub(y0);
        if room == 0 || x >= end {
            return;
        }
        let mut y_text = y0;
        if room > 4 {
            let full = if app.filled() { "[fit]" } else { "[full]" };
            self.put(x, y_text, end, "[close]", GOLD);
            self.hit(x, y_text, 7, Spot::ChatClose);
            self.put(x + 8, y_text, end, full, GOLD);
            self.hit(x + 8, y_text, full.chars().count(), Spot::ChatFill);
            y_text += 1;
            room -= 1;
        }
        let footer = app.work_footer();
        let show_foot = footer.is_some() && room > 4;
        if show_foot {
            room -= 1;
        }
        let bar_x = end.saturating_sub(2);
        let width = bar_x.saturating_sub(x).max(8);
        let cited = pane.iter().any(|line| line.contains("<oai-mem-citation>"));
        let shown;
        let source: &[String] = if cited {
            shown = present_pane(pane);
            &shown
        } else {
            pane
        };
        let roots = app.file_roots();
        let folded = crate::app::fold_lines(source, app.open_folds(), app.tree_shut());
        // Wrap only the rows that can reach the screen. A long line stays in
        // the window and the slice moves inside it. The first loaded row is
        // not pinned to the top.
        let (mut lo, hi, skip_bottom, align_top) =
            view_span(&folded, width, app.pane_from_end, room);
        let mut wrapped = Vec::new();
        let mut state;
        let mut grew = false;
        loop {
            wrapped.clear();
            state = InkState::new();
            for row in &folded[..lo] {
                advance_ink(&mut state, &row.text);
            }
            for row in &folded[lo..hi] {
                push_folded_line(&mut wrapped, app, &roots, row, width, &mut state);
            }
            let skip = skip_bottom.min(wrapped.len());
            let shown = wrapped.len() - skip;
            if shown >= room || lo == 0 || grew {
                break;
            }
            let back = 24.min(lo);
            if back == 0 {
                break;
            }
            lo -= back;
            grew = true;
        }
        if wrapped.is_empty() {
            self.paint_scroll(bar_x, y_text, room, 1, 0, app.more_above());
            return;
        }
        let skip = if align_top {
            0
        } else {
            skip_bottom.min(wrapped.len())
        };
        let cut = wrapped.len() - skip;
        let (body, from_top) = if cut == 0 {
            (&wrapped[..], true)
        } else {
            (&wrapped[..cut], align_top)
        };
        let start = if from_top {
            0
        } else {
            body.len().saturating_sub(room)
        };
        for (index, piece) in body.iter().skip(start).take(room).enumerate() {
            self.paint_piece(x, y_text + index, bar_x, piece);
        }
        let total = folded
            .iter()
            .map(|row| guess_lines(&row.text, width))
            .sum();
        self.paint_scroll(bar_x, y_text, room, total, app.pane_from_end, app.more_above());
        if show_foot {
            if let Some(text) = footer {
                self.span(x, y1 - 1, end, &text, GOLD);
            }
        }
    }

    fn paint_piece(&mut self, x: usize, y: usize, bar_x: usize, piece: &Piece) {
        let clock_w = piece.clock.chars().count();
        let text_end = if clock_w == 0 {
            bar_x
        } else {
            bar_x.saturating_sub(clock_w + 1)
        };
        self.span(x, y, text_end.max(x + 1), &piece.text, piece.ink);
        if clock_w > 0 {
            self.put(bar_x.saturating_sub(clock_w + 1), y, bar_x, &piece.clock, GOLD);
        }
        self.piece_hit(x, y, bar_x.saturating_sub(x), piece);
    }

    /// Width of the now column. The whole screen when the talk is not beside it.
    pub(crate) fn list_column(cols: usize) -> usize {
        let left_max = cols.saturating_sub(40);
        if cols < 72 || left_max < 36 {
            return cols;
        }
        let left = (cols / 2).clamp(36, left_max);
        if left >= cols.saturating_sub(8) {
            cols
        } else {
            left
        }
    }

    fn paint_scroll(
        &mut self,
        x: usize,
        y: usize,
        h: usize,
        total: usize,
        from_end: usize,
        top_open: bool,
    ) {
        if h == 0 {
            return;
        }
        let total = total.max(1);
        let inner = h.saturating_sub(2).max(1);
        let thumb = if total <= h {
            inner
        } else {
            (h.saturating_mul(h) / total).clamp(1, inner)
        };
        let travel = h.saturating_sub(thumb);
        let max_off = total.saturating_sub(h);
        let off = from_end.min(max_off);
        let slot = if max_off == 0 {
            travel
        } else {
            (max_off - off) * travel / max_off
        };
        for i in 0..h {
            let mut ch = if i >= slot && i < slot + thumb {
                '█'
            } else {
                '│'
            };
            if i == 0 {
                ch = if top_open { '↑' } else { '┬' };
            } else if i + 1 == h {
                ch = '┴';
            }
            self.put(x, y + i, x + 2, &ch.to_string(), DIM);
        }
    }

    fn paint_term(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        let lines = app.lines();
        let side = app.shell_side();
        let room = y1.saturating_sub(y0);
        if room == 0 {
            return;
        }
        if lines.is_empty() && side.is_empty() {
            self.line(y0, cols, app.empty_line(), DIM);
            return;
        }
        let split = !side.is_empty() && cols >= 72;
        let left = if split { cols / 2 } else { cols };
        let selected_at = app.focus_index().min(lines.len().saturating_sub(1));
        let start = selected_at.saturating_sub(room.saturating_sub(1));
        for (index, line) in lines.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let shown = crate::hl::paint_shell_line(line);
            if split {
                self.span(0, y, left, &shown, SALMON);
            } else {
                self.line(y, cols, &shown, SALMON);
            }
            self.hit(0, y, left.saturating_sub(1), Spot::Row(index));
        }
        if !split {
            return;
        }
        for y in y0..y1 {
            self.span(left, y, left + 2, "|", CYAN);
        }
        let side_start = side.len().saturating_sub(room);
        for (index, line) in side.iter().enumerate().skip(side_start).take(room) {
            let y = y0 + index - side_start;
            let shown = crate::hl::paint_shell_line(line);
            self.span(left + 2, y, cols, &shown, CYAN);
        }
    }

    fn span(&mut self, x: usize, y: usize, end: usize, text: &str, ink: &str) {
        if x >= end {
            return;
        }
        self.frame
            .push_str(&format!("\x1b[{};{}H{ink}", y + 1, x + 1));
        let mut count = x;
        let mut chars = text.chars().peekable();
        let mut link: Option<usize> = None;
        let mut underlined = false;
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                self.frame.push(ch);
                let mut params = String::new();
                let mut final_byte = ' ';
                let mut first = true;
                for next in chars.by_ref() {
                    self.frame.push(next);
                    if first {
                        first = false;
                        if next == '[' {
                            continue;
                        }
                    }
                    if next.is_ascii_alphabetic() {
                        final_byte = next;
                        break;
                    }
                    params.push(next);
                }
                if final_byte == 'm' {
                    let next_under = sgr_underlined(&params);
                    if underlined && !next_under {
                        self.close_link(&mut link, count, y);
                    }
                    underlined = next_under;
                    if underlined && link.is_none() {
                        link = Some(count);
                    }
                }
                continue;
            }
            if column_char(ch) && count + 1 >= end {
                break;
            }
            self.frame.push(ch);
            if column_char(ch) {
                if underlined && link.is_none() {
                    link = Some(count);
                }
                self.mark(count, y, ch);
                count += 1;
            }
        }
        self.close_link(&mut link, count, y);
        while count + 1 < end {
            self.frame.push(' ');
            self.mark(count, y, ' ');
            count += 1;
        }
        self.frame.push_str(KEEP);
    }

    fn close_link(&mut self, link: &mut Option<usize>, count: usize, y: usize) {
        if let Some(start) = link.take()
            && count > start
        {
            self.hots.push((start, y, count - start));
        }
    }

    /// A button or a link under the pointer is drawn as an underlined link.
    fn paint_hover(&mut self, hover: Option<(usize, usize)>) {
        let Some((hx, hy)) = hover else {
            return;
        };
        let mut best: Option<(usize, usize)> = None;
        if let Some(hit) = self.hits.iter().rev().find(|hit| {
            lights_up(hit.spot) && hit.y == hy && hx >= hit.x && hx < hit.x + hit.w
        }) && let Some((x, w)) = glow_span(&self.glyphs, hit.x, hit.y, hit.w)
        {
            best = Some((x, w));
        }
        for (x, y, w) in &self.hots {
            if *y == hy && hx >= *x && hx < x + w {
                match best {
                    Some((_, bw)) if bw <= *w => {}
                    _ => best = Some((*x, *w)),
                }
            }
        }
        let Some((x, w)) = best else {
            return;
        };
        let Some(row) = self.glyphs.get(hy) else {
            return;
        };
        let end = (x + w).min(row.len());
        if x >= end {
            return;
        }
        let run: String = row[x..end].iter().collect();
        self.frame.push_str(&format!(
            "\x1b[{};{}H\x1b[1;4;30;48;5;220m{run}\x1b[0m",
            hy + 1,
            x + 1
        ));
    }

    fn tabs(&mut self, current: Section, cols: usize, y: usize) {
        self.line(y, cols, "", DIM);
        let mut x = 1usize;
        for section in Section::ALL {
            let label = format!(" {} ", section.label());
            let ink = if section == current {
                "\x1b[4;38;5;51m"
            } else {
                SALMON
            };
            self.put(x, y, cols, &label, ink);
            self.hit(x, y, label.chars().count(), Spot::Tab(section));
            x += label.chars().count() + 1;
            if x >= cols {
                break;
            }
        }
    }

    fn line(&mut self, y: usize, cols: usize, text: &str, ink: &str) {
        self.frame.push_str(&format!("\x1b[{};1H{ink}", y + 1));
        let mut count = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                self.frame.push(ch);
                for next in chars.by_ref() {
                    self.frame.push(next);
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            if count + 1 >= cols {
                break;
            }
            self.frame.push(ch);
            self.mark(count, y, ch);
            count += 1;
        }
        while count + 1 < cols {
            self.frame.push(' ');
            self.mark(count, y, ' ');
            count += 1;
        }
        self.frame.push_str(KEEP);
    }

    fn put(&mut self, x: usize, y: usize, cols: usize, text: &str, ink: &str) {
        let room = cols.saturating_sub(1).saturating_sub(x);
        if room == 0 {
            return;
        }
        let shown: String = text.chars().take(room).collect();
        self.frame
            .push_str(&format!("\x1b[{};{}H{ink}{shown}{KEEP}", y + 1, x + 1));
        let mut col = x;
        for ch in shown.chars() {
            if !column_char(ch) || col + 1 >= cols {
                continue;
            }
            self.mark(col, y, ch);
            col += 1;
        }
    }

    fn veil(&mut self, x: usize, y: usize, cols: usize, w: usize, line: &str, kind: u8, hot: bool) {
        let plain = plain_cols(line);
        let mut slice: String = plain.chars().skip(x).take(w).collect();
        while slice.chars().count() < w {
            slice.push(' ');
        }
        let ink = if hot {
            "\x1b[30;48;5;220m"
        } else if kind == crate::engine::KIND_FIELD {
            "\x1b[38;5;51;48;5;17m"
        } else if kind == crate::engine::KIND_BUTTON {
            "\x1b[38;5;220;48;5;236m"
        } else {
            "\x1b[4;38;5;220;48;5;234m"
        };
        self.put(x, y, cols, &slice, ink);
    }

    fn paint_corner(&mut self, cols: usize) {
        let settings = "[settings]";
        let help = "[help]";
        let help_at = cols.saturating_sub(help.chars().count() + 1);
        let set_at = help_at.saturating_sub(settings.chars().count() + 1);
        if set_at < 12 {
            return;
        }
        self.put(set_at, 0, cols, settings, GOLD);
        self.hit(set_at, 0, settings.chars().count(), Spot::Gear);
        self.put(help_at, 0, cols, help, GOLD);
        self.hit(help_at, 0, help.chars().count(), Spot::Help);
    }

    fn paint_sheet(&mut self, app: &App, cols: usize, y0: usize, y1: usize, settings: bool) {
        let title = if settings {
            "Settings"
        } else {
            "Keyboard Shortcuts"
        };
        let close = if settings {
            Spot::SettingClose
        } else {
            Spot::KeysClose
        };
        let rows = if settings {
            app.setting_rows()
        } else {
            app.key_rows()
        };
        let tall = y1.saturating_sub(y0);
        let box_w = 78.min(cols.saturating_sub(4));
        let chrome = 7;
        if cols >= 60 && box_w >= 48 && tall >= chrome + 1 {
            self.paint_sheet_box(app, cols, y0, y1, settings, title, close, &rows, box_w);
            return;
        }
        self.paint_sheet_fill(app, cols, y0, y1, settings, title, close, &rows);
    }

    fn paint_sheet_fill(
        &mut self,
        app: &App,
        cols: usize,
        y0: usize,
        y1: usize,
        settings: bool,
        title: &str,
        close: Spot,
        rows: &[crate::app::KeyRow],
    ) {
        if y1 <= y0 + 4 {
            self.line(y0, cols, &format!("{title}   Esc closes"), GOLD);
            self.hit(0, y0, cols.saturating_sub(1), close);
            return;
        }
        for y in y0..y1 {
            self.line(y, cols, "", DIM);
        }
        let mark = "[X]";
        let rule_w = cols.saturating_sub(title.chars().count() + mark.chars().count() + 6);
        let head = format!(" {title} {} {mark}", "-".repeat(rule_w));
        self.line(y0, cols, &head, GOLD);
        let close_at = head.chars().count().saturating_sub(mark.chars().count());
        self.hit(close_at, y0, mark.chars().count(), close);
        let (finding, query, at) = sheet_state(app, settings);
        let search = if finding || !query.is_empty() {
            format!(" / {query}")
        } else {
            " / to search".into()
        };
        self.line(y0 + 1, cols, &search, if finding { GOLD } else { DIM });
        self.line(y0 + 2, cols, &"-".repeat(cols.saturating_sub(1)), RULE);
        let foot = y1.saturating_sub(2);
        let room = foot.saturating_sub(y0 + 3);
        let at = at.min(rows.len().saturating_sub(1));
        let start = at.saturating_sub(room.saturating_sub(1));
        for (offset, row) in rows.iter().enumerate().skip(start).take(room) {
            let y = y0 + 3 + offset - start;
            let selected = offset == at;
            let bullet = if row.group {
                if row.open { "◆" } else { ">" }
            } else {
                "◆"
            };
            let label = format!(" {bullet} {}", row.label);
            let shortcut = row.shortcut.clone();
            let gap = cols
                .saturating_sub(4)
                .saturating_sub(label.chars().count())
                .saturating_sub(shortcut.chars().count());
            let text = format!("{label}{}{shortcut}", " ".repeat(gap));
            let ink = if selected {
                "\x1b[48;5;236;38;5;223m"
            } else if row.group {
                GOLD
            } else {
                SALMON
            };
            self.span(1, y, cols.saturating_sub(1), &text, ink);
            let spot = if settings {
                Spot::Setting(offset as u16)
            } else {
                Spot::Keys(offset as u16)
            };
            self.hit(0, y, cols.saturating_sub(1), spot);
        }
        let foot_a = if settings {
            " up/down nav | f filter | e/Space/right expand | left collapse"
        } else {
            " up/down nav | f filter | e/Space/right expand | left collapse"
        };
        let foot_b = if settings {
            " Enter applies | / search | Esc close"
        } else {
            " Enter details | / search | Esc close"
        };
        self.line(foot, cols, foot_a, DIM);
        self.line(foot + 1, cols, foot_b, DIM);
    }

    fn paint_sheet_box(
        &mut self,
        app: &App,
        cols: usize,
        y0: usize,
        y1: usize,
        settings: bool,
        title: &str,
        close: Spot,
        rows: &[crate::app::KeyRow],
        box_w: usize,
    ) {
        for y in y0..y1 {
            self.hit(0, y, cols.saturating_sub(1), close);
        }
        let (finding, query, at) = sheet_state(app, settings);
        let tall = y1.saturating_sub(y0);
        let chrome = 7;
        let room = tall.saturating_sub(chrome).max(1);
        let shown = rows.len().min(room).max(1);
        let box_h = shown + chrome;
        let left = (cols.saturating_sub(1).saturating_sub(box_w)) / 2;
        let right = left + box_w;
        let top = y0 + tall.saturating_sub(box_h.min(tall)) / 5;
        let border = "\x1b[38;5;245m";
        let bar = "─".repeat(box_w.saturating_sub(2));
        let mut y = top;
        let end = y1;
        let row = |screen: &mut Self, y: usize| {
            if y >= end {
                return false;
            }
            screen.span(left, y, left + 2, "│", border);
            screen.span(left + 1, y, right, "", PAPER);
            screen.span(right - 1, y, right + 1, "│", border);
            screen.hit(left, y, box_w, Spot::Cmd);
            true
        };
        if y >= end {
            return;
        }
        self.span(left, y, right + 1, &format!("┌{bar}┐"), border);
        self.hit(left, y, box_w, Spot::Cmd);
        y += 1;
        if !row(self, y) {
            return;
        }
        self.put(left + 2, y, right, title, GOLD);
        let mark = "[X]";
        let close_at = right.saturating_sub(mark.chars().count() + 2);
        self.put(close_at, y, right, mark, border);
        self.hit(close_at, y, mark.chars().count(), close);
        y += 1;
        if !row(self, y) {
            return;
        }
        let search = if finding || !query.is_empty() {
            format!("/ {query}")
        } else {
            "/ to search".into()
        };
        self.put(left + 2, y, right, &search, if finding { GOLD } else { DIM });
        y += 1;
        if y >= end {
            return;
        }
        self.span(left, y, right + 1, &format!("├{bar}┤"), border);
        self.hit(left, y, box_w, Spot::Cmd);
        y += 1;
        let at = at.min(rows.len().saturating_sub(1));
        let start = at.saturating_sub(shown.saturating_sub(1));
        for (offset, item) in rows.iter().enumerate().skip(start).take(shown) {
            if !row(self, y) {
                return;
            }
            let selected = offset == at;
            let bullet = if item.group {
                if item.open { "◆" } else { ">" }
            } else {
                "◆"
            };
            let label = format!("{bullet} {}", item.label.trim_start());
            let shortcut = item.shortcut.clone();
            let inner = box_w.saturating_sub(4);
            let gap = inner
                .saturating_sub(label.chars().count())
                .saturating_sub(shortcut.chars().count());
            let text = format!("{label}{}{shortcut}", " ".repeat(gap.min(inner)));
            let ink = if selected {
                "\x1b[48;5;236;38;5;223m"
            } else if item.group {
                GOLD
            } else {
                SALMON
            };
            self.put(left + 2, y, right, &text, ink);
            let spot = if settings {
                Spot::Setting(offset as u16)
            } else {
                Spot::Keys(offset as u16)
            };
            self.hit(left + 1, y, box_w.saturating_sub(2), spot);
            y += 1;
        }
        let foot_a = "up/down  f filter  e/right expand  left collapse";
        let foot_b = if settings {
            "Enter applies   / search   Esc close"
        } else {
            "Enter details   / search   Esc close"
        };
        if !row(self, y) {
            return;
        }
        self.put(left + 2, y, right, foot_a, DIM);
        y += 1;
        if !row(self, y) {
            return;
        }
        self.put(left + 2, y, right, foot_b, DIM);
        y += 1;
        if y >= end {
            return;
        }
        self.span(left, y, right + 1, &format!("└{bar}┘"), border);
        self.hit(left, y, box_w, Spot::Cmd);
    }

    fn paint_meter(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        if y1 <= y0 + 6 {
            self.line(y0, cols, "Context   Esc closes", GOLD);
            self.hit(0, y0, cols.saturating_sub(1), Spot::MeterClose);
            return;
        }
        for y in y0..y1 {
            self.hit(0, y, cols.saturating_sub(1), Spot::Meter);
        }
        let body = app.meter_body();
        let note = !app.note.is_empty();
        let chrome = 5 + usize::from(note);
        let tall = y1.saturating_sub(y0);
        let box_w = 76.min(cols.saturating_sub(3));
        if cols < 60 || box_w < 60 || tall < chrome + 1 {
            self.paint_meter_sheet(app, cols, y0, y1, &body);
            return;
        }
        let room = tall - chrome;
        let shown = body.len().min(room).max(1);
        let left = (cols.saturating_sub(1).saturating_sub(box_w)) / 2;
        let right = left + box_w;
        let border = "\x1b[38;5;245m";
        let mut y = y0;
        let top = format!("┌{}┐", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &top, border);
        y += 1;
        self.paint_meter_tabs(app, left, right, y);
        y += 1;
        let rule = format!("├{}┤", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &rule, border);
        y += 1;
        let start = app.meter_scroll().min(body.len().saturating_sub(shown));
        for line in body.iter().skip(start).take(shown) {
            self.paint_meter_line(left, right, y, line);
            y += 1;
        }
        if note {
            self.paint_box_row(left, right, y, &format!(" {}", app.note), GOLD);
            y += 1;
        }
        self.paint_box_row(left, right, y, &meter_footer(box_w.saturating_sub(2)), "");
        y += 1;
        let bot = format!("└{}┘", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &bot, border);
    }

    fn paint_meter_sheet(&mut self, app: &App, cols: usize, y0: usize, y1: usize, body: &[String]) {
        for y in y0..y1 {
            self.line(y, cols, "", DIM);
        }
        self.paint_meter_tabs(app, 0, cols.saturating_sub(1), y0);
        self.line(y0 + 1, cols, &"-".repeat(cols.saturating_sub(1)), RULE);
        let foot = y1.saturating_sub(1 + usize::from(!app.note.is_empty()));
        let room = foot.saturating_sub(y0 + 2);
        let start = app.meter_scroll().min(body.len().saturating_sub(room.max(1)));
        for (offset, line) in body.iter().enumerate().skip(start).take(room) {
            self.paint_meter_line(0, cols.saturating_sub(1), y0 + 2 + offset - start, line);
        }
        if !app.note.is_empty() {
            self.line(foot.saturating_sub(1), cols, &app.note, GOLD);
        }
        self.line(foot, cols, &meter_footer(cols.saturating_sub(2)), "");
    }

    fn paint_meter_tabs(&mut self, app: &App, left: usize, right: usize, y: usize) {
        let border = "\x1b[38;5;245m";
        if left > 0 {
            self.span(left, y, left + 2, "│", border);
        }
        self.span(left + usize::from(left > 0), y, right, "", PAPER);
        if left > 0 {
            self.span(right - 1, y, right + 1, "│", border);
        }
        let tabs = ["Context usage", "Usage limit", "Session info"];
        let mut x = left + 2;
        for (index, name) in tabs.iter().enumerate() {
            let ink = if index == usize::from(app.meter_tab()) {
                "\x1b[4;38;5;255m"
            } else {
                "\x1b[38;5;245m"
            };
            self.put(x, y, right, name, ink);
            self.hit(x, y, name.chars().count(), Spot::MeterTab(index as u8));
            x += name.chars().count() + 2;
        }
        let mark = "[X]";
        let close_at = right.saturating_sub(mark.chars().count() + 2);
        self.put(close_at, y, right, mark, "\x1b[38;5;245m");
        self.hit(close_at, y, mark.chars().count(), Spot::MeterClose);
    }

    fn paint_meter_line(&mut self, left: usize, right: usize, y: usize, line: &str) {
        let border = "\x1b[38;5;245m";
        if left > 0 {
            self.span(left, y, left + 2, "│", border);
        }
        let inner = left + usize::from(left > 0);
        if line == "[compact context]" {
            self.span(inner, y, right, "", PAPER);
            self.put(inner + 1, y, right, line, "\x1b[30;48;5;220m");
            self.hit(inner + 1, y, line.chars().count(), Spot::Compact);
        } else if line.contains('\u{1b}') {
            self.span(inner, y, right, line, "");
        } else if line.is_empty() {
            self.span(inner, y, right, "", PAPER);
        } else {
            self.span(inner, y, right, &format!(" {line}"), PAPER);
        }
        if left > 0 {
            self.span(right - 1, y, right + 1, "│", border);
        }
    }

    fn paint_pick(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        let choices = app.pick_choices();
        if choices.is_empty() || cols < 40 || y1 <= y0 + 4 {
            return;
        }
        for y in y0..y1 {
            self.hit(0, y, cols.saturating_sub(1), Spot::PickClose);
        }
        let title = app.pick_title();
        let current = app.pick_current();
        let at = app.pick_at().min(choices.len().saturating_sub(1));
        let longest = choices
            .iter()
            .map(|item| item.chars().count())
            .max()
            .unwrap_or(8);
        let inner = longest.max(title.chars().count()).saturating_add(6);
        let edge = cols.saturating_sub(1);
        let box_w = (inner.saturating_add(2)).clamp(28, 46).min(edge);
        if box_w < 16 {
            return;
        }
        let left = edge.saturating_sub(box_w);
        let right = left + box_w;
        let box_h = choices.len() + 4;
        let top = y1.saturating_sub(box_h).max(y0);
        let border = "\x1b[38;5;245m";
        let bar = "─".repeat(box_w.saturating_sub(2));
        let mut y = top;
        if y >= y1 {
            return;
        }
        self.span(left, y, right + 1, &format!("┌{bar}┐"), border);
        y += 1;
        if y >= y1 {
            return;
        }
        self.span(left, y, left + 2, "│", border);
        self.span(left + 1, y, right, "", PAPER);
        self.put(left + 2, y, cols, title, "\x1b[1;38;5;255m");
        let mark = "[X]";
        let close_at = right.saturating_sub(mark.chars().count() + 2);
        if close_at > left + 2 {
            self.put(close_at, y, cols, mark, "\x1b[38;5;245m");
            self.hit(close_at, y, mark.chars().count(), Spot::PickClose);
        }
        self.span(right - 1, y, right + 1, "│", border);
        y += 1;
        for (index, name) in choices.iter().enumerate() {
            if y + 2 >= y1 {
                break;
            }
            let bullet = if *name == current { "◆" } else { " " };
            let label = format!(" {bullet} {name}");
            let ink = if index == at && *name == current {
                "\x1b[48;5;236;38;5;220m"
            } else if *name == current {
                "\x1b[38;5;220m"
            } else if index == at {
                "\x1b[48;5;236;38;5;223m"
            } else {
                PAPER
            };
            self.span(left, y, left + 2, "│", border);
            self.span(left + 1, y, right, &label, ink);
            self.span(right - 1, y, right + 1, "│", border);
            self.hit(
                left + 1,
                y,
                box_w.saturating_sub(2),
                Spot::Pick(index as u16),
            );
            y += 1;
        }
        if y + 1 >= y1 {
            return;
        }
        self.span(left, y, left + 2, "│", border);
        self.span(left + 1, y, right, " Esc", GOLD);
        self.span(right - 1, y, right + 1, "│", border);
        y += 1;
        self.span(left, y, right + 1, &format!("└{bar}┘"), border);
    }

    fn paint_selection(&mut self, sel: (usize, usize, usize, usize)) {
        let (x0, y0, x1, y1) = ordered_sel(sel);
        for y in y0..=y1 {
            let Some(row) = self.glyphs.get(y) else {
                continue;
            };
            if row.is_empty() {
                continue;
            }
            let last = row.len() - 1;
            let (start, end) = if y0 == y1 {
                (x0.min(last), x1.min(last))
            } else if y == y0 {
                (x0.min(last), last)
            } else if y == y1 {
                (0, x1.min(last))
            } else {
                (0, last)
            };
            if start > end {
                continue;
            }
            let run: String = row[start..=end].iter().collect();
            self.frame.push_str(&format!(
                "\x1b[{};{}H\x1b[1;30;48;5;220m{run}\x1b[0m",
                y + 1,
                start + 1
            ));
        }
    }

    fn mark(&mut self, x: usize, y: usize, ch: char) {
        if let Some(row) = self.glyphs.get_mut(y)
            && let Some(cell) = row.get_mut(x)
        {
            *cell = ch;
        }
    }

    fn paint_box_row(&mut self, left: usize, right: usize, y: usize, text: &str, ink: &str) {
        let border = "\x1b[38;5;245m";
        self.span(left, y, left + 2, "│", border);
        self.span(left + 1, y, right, text, ink);
        self.span(right - 1, y, right + 1, "│", border);
    }

    fn hit(&mut self, x: usize, y: usize, w: usize, spot: Spot) {
        if w > 0 {
            self.hits.push(Hit { x, y, w, spot });
        }
    }

    fn piece_hit(&mut self, x: usize, y: usize, w: usize, piece: &Piece) {
        if piece.image != 0 {
            self.hit(x, y, w, Spot::Image(piece.image));
        } else if piece.open != 0 {
            self.hit(x, y, w, Spot::Open(piece.open));
        } else if piece.fold != 0 {
            self.hit(x, y, w, Spot::Fold(piece.fold - 1));
        } else if piece.tree != 0 {
            self.hit(x, y, w, Spot::Tree(piece.tree - 1));
        } else if piece.doc != 0 {
            self.hit(x, y, w, Spot::Doc(piece.doc - 1));
        }
    }

    fn paint_doc(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        if y1 <= y0 + 4 {
            self.line(y0, cols, &format!("{}   Esc closes", app.doc_title()), GOLD);
            self.hit(0, y0, cols.saturating_sub(1), Spot::DocClose);
            return;
        }
        for y in y0..y1 {
            self.hit(0, y, cols.saturating_sub(1), Spot::DocClose);
        }
        let body = app.doc_lines();
        let box_w = 104.min(cols.saturating_sub(3));
        let tall = y1.saturating_sub(y0);
        if box_w < 24 || tall < 5 {
            self.line(y0, cols, &format!("{}   Esc closes", app.doc_title()), GOLD);
            return;
        }
        let room = tall - 4;
        let shown = body.len().min(room).max(1);
        let left = (cols.saturating_sub(1).saturating_sub(box_w)) / 2;
        let right = left + box_w;
        let border = "\x1b[38;5;245m";
        let mut y = y0;
        let top = format!("┌{}┐", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &top, border);
        self.hit(left, y, box_w, Spot::DocBox);
        y += 1;
        self.paint_doc_title(app, left, right, y);
        y += 1;
        let start = app.doc_scroll().min(body.len().saturating_sub(shown));
        if body.is_empty() {
            self.paint_box_row(left, right, y, "", PAPER);
            self.hit(left, y, box_w, Spot::DocBox);
            y += 1;
        } else {
            for line in body.iter().skip(start).take(shown) {
                let ink = if line.trim_start().starts_with('#') {
                    GOLD
                } else {
                    PAPER
                };
                self.paint_box_row(left, right, y, &format!(" {line}"), ink);
                self.hit(left, y, box_w, Spot::DocBox);
                y += 1;
            }
        }
        self.paint_box_row(left, right, y, &doc_footer(box_w.saturating_sub(2)), "");
        self.hit(left, y, box_w, Spot::DocBox);
        y += 1;
        let bot = format!("└{}┘", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &bot, border);
        self.hit(left, y, box_w, Spot::DocBox);
    }

    fn paint_doc_title(&mut self, app: &App, left: usize, right: usize, y: usize) {
        let border = "\x1b[38;5;245m";
        self.span(left, y, left + 2, "│", border);
        self.span(left + 1, y, right, "", PAPER);
        self.span(right - 1, y, right + 1, "│", border);
        self.hit(left, y, right.saturating_sub(left), Spot::DocBox);
        let room = right.saturating_sub(left).saturating_sub(8).max(4);
        let shown: String = app.doc_title().chars().take(room).collect();
        self.put(left + 2, y, right, &shown, GOLD);
        let mark = "[X]";
        let close_at = right.saturating_sub(mark.chars().count() + 2);
        self.put(close_at, y, right, mark, "\x1b[38;5;245m");
        self.hit(close_at, y, mark.chars().count(), Spot::DocClose);
    }

    fn paint_picture(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        let picture = app.image_outline().unwrap_or("");
        let body: Vec<&str> = picture.lines().filter(|line| !line.is_empty()).collect();
        if y1 <= y0 + 4 {
            self.line(y0, cols, "image   Esc closes", GOLD);
            self.hit(0, y0, cols.saturating_sub(1), Spot::PicClose);
            return;
        }
        for y in y0..y1 {
            self.hit(0, y, cols.saturating_sub(1), Spot::PicClose);
        }
        let (bw, _) = app.picture_budget();
        let box_w = (bw + 4).min(cols.saturating_sub(2)).max(20);
        let tall = y1.saturating_sub(y0);
        if box_w < 20 || tall < 5 {
            self.line(y0, cols, "image   Esc closes", GOLD);
            return;
        }
        let room = tall - 4;
        let shown = body.len().min(room).max(1);
        let left = (cols.saturating_sub(1).saturating_sub(box_w)) / 2;
        let right = left + box_w;
        let border = "\x1b[38;5;245m";
        let mut y = y0;
        let top = format!("┌{}┐", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &top, border);
        self.hit(left, y, box_w, Spot::PicBox);
        y += 1;
        self.span(left, y, left + 2, "│", border);
        self.span(left + 1, y, right, "", PAPER);
        self.span(right - 1, y, right + 1, "│", border);
        self.hit(left, y, right.saturating_sub(left), Spot::PicBox);
        self.put(left + 2, y, right, "image", GOLD);
        let mark = "[X]";
        let close_at = right.saturating_sub(mark.chars().count() + 2);
        self.put(close_at, y, right, mark, "\x1b[38;5;245m");
        self.hit(close_at, y, mark.chars().count(), Spot::PicClose);
        y += 1;
        let start = app.image_scroll().min(body.len().saturating_sub(shown));
        if body.is_empty() {
            self.paint_box_row(left, right, y, "", PAPER);
            self.hit(left, y, box_w, Spot::PicBox);
            y += 1;
        } else {
            for line in body.iter().skip(start).take(shown) {
                self.paint_box_row(left, right, y, line, "");
                self.hit(left, y, box_w, Spot::PicBox);
                y += 1;
            }
        }
        self.paint_box_row(left, right, y, &doc_footer(box_w.saturating_sub(2)), "");
        self.hit(left, y, box_w, Spot::PicBox);
        y += 1;
        let bot = format!("└{}┘", "─".repeat(box_w.saturating_sub(2)));
        self.span(left, y, right + 1, &bot, border);
        self.hit(left, y, box_w, Spot::PicBox);
    }
}

fn ordered_sel(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (ax, ay, bx, by) = sel;
    if (ay, ax) <= (by, bx) {
        (ax, ay, bx, by)
    } else {
        (bx, by, ax, ay)
    }
}

/// Cup is 1-based. A tall field draws its text on the middle row, so the
/// caret sits there instead of on the top border.
pub(crate) fn field_caret(x: usize, top: usize, h: usize) -> (usize, usize) {
    let row = if h >= 3 { top + h / 2 } else { top };
    let col = if h >= 3 { x + 2 } else { x + 1 };
    (col + 1, row + 1)
}

fn shown_lane(lane: Option<&str>) -> String {
    match lane {
        Some(name) if name != "say" => format!(" · {name}"),
        _ => String::new(),
    }
}

fn doc_footer(inner: usize) -> String {
    let plain = "↑/↓ scroll | Esc close";
    let pad = inner.saturating_sub(plain.chars().count()) / 2;
    format!(
        "{}\x1b[38;5;220m↑/↓\x1b[38;5;245m scroll | \x1b[38;5;220mEsc\x1b[38;5;245m close",
        " ".repeat(pad)
    )
}

fn meter_footer(inner: usize) -> String {
    let plain = "Tab switch | ↑/↓ scroll | c copy session ID | Esc close";
    let pad = inner.saturating_sub(plain.chars().count()) / 2;
    format!(
        "{}\x1b[38;5;220mTab\x1b[38;5;245m switch | \x1b[38;5;220m↑/↓\x1b[38;5;245m scroll | \x1b[38;5;220mc\x1b[38;5;245m copy session ID | \x1b[38;5;220mEsc\x1b[38;5;245m close",
        " ".repeat(pad)
    )
}

fn list_origin(total: usize, room: usize, selected: usize, from: usize) -> usize {
    let room = room.max(1);
    let max_start = total.saturating_sub(room);
    if from == usize::MAX {
        selected
            .saturating_sub(room.saturating_sub(1))
            .min(max_start)
    } else {
        from.min(max_start)
    }
}

fn keep_color(line: &str) -> bool {
    crate::draw::has_blocks(line) || line.contains('\u{1b}')
}

fn sheet_state(app: &App, settings: bool) -> (bool, &str, usize) {
    if settings {
        (
            app.settings_finding(),
            app.settings_query(),
            app.settings_at(),
        )
    } else {
        (app.keys_finding(), app.keys_query(), app.keys_at())
    }
}

/// A page row whose own background is paper. Dark slabs would cover it.
fn line_is_light(line: &str) -> bool {
    let mut rest = line;
    while let Some(at) = rest.find("48;2;") {
        rest = &rest[at + 5..];
        let bytes = rest.as_bytes();
        let mut nums = [0u16; 3];
        let mut n = 0usize;
        let mut cur = 0u16;
        let mut saw = false;
        let mut done = false;
        for &byte in bytes {
            if byte.is_ascii_digit() {
                saw = true;
                cur = cur.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
                if cur > 255 {
                    break;
                }
            } else if byte == b';' && n < 2 && saw {
                nums[n] = cur;
                n += 1;
                cur = 0;
                saw = false;
            } else {
                if saw && n == 2 {
                    nums[2] = cur.min(255);
                    done = true;
                }
                break;
            }
        }
        if done {
            let lum = u32::from(nums[0]) * 3 + u32::from(nums[1]) * 6 + u32::from(nums[2]);
            if lum >= 255 * 5 {
                return true;
            }
        }
    }
    false
}

fn plain_cols(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if column_char(ch) {
            out.push(ch);
        }
    }
    out
}

fn column_char(ch: char) -> bool {
    !matches!(
        ch,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

fn image_number(text: &str) -> Option<u16> {
    let rest = text.split("[Image #").nth(1)?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    let n = digits.parse().ok()?;
    if n == 0 { None } else { Some(n) }
}

fn paint_stat(rest: &str) -> String {
    let name_ink = if md_stat_name(rest) { GOLD } else { "" };
    let mut parts = rest.split("  ");
    let name = parts.next().unwrap_or("");
    let named = if name_ink.is_empty() {
        name.to_string()
    } else {
        format!("{name_ink}{name}{KEEP}")
    };
    let Some(plus) = parts.next() else {
        return format!("  {named}");
    };
    let minus = parts.next().unwrap_or("0");
    format!("  {named}  {GREEN}+{plus}{KEEP}  {RED}-{minus}")
}

fn md_stat_name(rest: &str) -> bool {
    let name = rest.split("  ").next().unwrap_or("");
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("md"))
}

fn paint_sum(rest: &str) -> String {
    if let Some((stamp, tail)) = split_stamp(rest) {
        format!("{DIM}итог  {CYAN}{stamp}  {GOLD}{tail}")
    } else {
        format!("{DIM}итог  {GOLD}{rest}")
    }
}

fn split_stamp(body: &str) -> Option<(&str, &str)> {
    let bytes = body.as_bytes();
    if bytes.len() < 13
        || bytes[2] != b'-'
        || bytes[5] != b' '
        || bytes[8] != b':'
        || bytes[11] != b' '
        || bytes[12] != b' '
        || !bytes[0].is_ascii_digit()
        || !bytes[1].is_ascii_digit()
        || !bytes[3].is_ascii_digit()
        || !bytes[4].is_ascii_digit()
        || !bytes[6].is_ascii_digit()
        || !bytes[7].is_ascii_digit()
        || !bytes[9].is_ascii_digit()
        || !bytes[10].is_ascii_digit()
    {
        return None;
    }
    Some((&body[..11], &body[13..]))
}

fn accent(base: &str) -> &'static str {
    if base.contains("38;5;51") { GOLD } else { CYAN }
}

fn at_boundary(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let prev = chars[index - 1];
    prev.is_whitespace() || matches!(prev, '`' | '(' | '[' | '"' | '\'' | '«' | '“' | '‘')
}

fn quoted_windows_path(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index) != Some(&'"') {
        return None;
    }
    windows_path(chars, index + 1)?;
    let mut end = index + 1;
    while end < chars.len() && chars[end] != '"' {
        end += 1;
    }
    if chars.get(end) == Some(&'"') {
        Some(end)
    } else {
        None
    }
}

fn windows_path(chars: &[char], index: usize) -> Option<usize> {
    if !at_boundary(chars, index) {
        return None;
    }
    let ch = chars.get(index).copied()?;
    if !ch.is_ascii_alphabetic() {
        return None;
    }
    if chars.get(index + 1) != Some(&':') {
        return None;
    }
    let slash = chars.get(index + 2).copied()?;
    if slash != '\\' && slash != '/' {
        return None;
    }
    Some(path_end(chars, index))
}

fn path_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && !chars[index].is_whitespace() {
        index += 1;
    }
    loop {
        if index >= chars.len() || chars[index] != ' ' {
            break;
        }
        let mut next = index + 1;
        if next >= chars.len() {
            break;
        }
        let piece_start = next;
        while next < chars.len() && !chars[next].is_whitespace() {
            next += 1;
        }
        if !filename_piece(&chars[piece_start..next]) {
            break;
        }
        index = next;
    }
    index
}

fn filename_piece(piece: &[char]) -> bool {
    if piece.is_empty() {
        return false;
    }
    let mut fileish = false;
    for ch in piece {
        if ch.is_ascii_digit() || *ch == '.' || *ch == '-' || *ch == '_' {
            fileish = true;
            continue;
        }
        if ch.is_ascii_alphabetic() {
            continue;
        }
        return false;
    }
    fileish
}

fn slash_command(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index) != Some(&'/') || !at_boundary(chars, index) {
        return None;
    }
    let next = chars.get(index + 1).copied()?;
    if !next.is_ascii_alphabetic() {
        return None;
    }
    let mut end = index + 2;
    while end < chars.len()
        && (chars[end].is_ascii_alphanumeric() || chars[end] == '-' || chars[end] == '_')
    {
        end += 1;
    }
    Some(end)
}

fn shot_hint(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains(".png")
        || lower.contains(".jpg")
        || lower.contains(".jpeg")
        || lower.contains(".gif")
}

pub(crate) fn picture_paths(text: &str) -> Vec<(usize, usize, std::path::PathBuf)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '"'
            && let Some(end) = quoted_windows_path(&chars, index)
        {
            let path: String = chars[index + 1..end].iter().collect();
            if is_picture_file(&path) {
                out.push((index, end + 1, std::path::PathBuf::from(path)));
                index = end + 1;
                continue;
            }
        }
        if let Some(end) = windows_path(&chars, index) {
            if end > index + 3 {
                let path: String = chars[index..end].iter().collect();
                if is_picture_file(&path) {
                    out.push((index, end, std::path::PathBuf::from(path)));
                    index = end;
                    continue;
                }
            }
        }
        index += 1;
    }
    out
}

fn is_picture_file(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif") {
        return false;
    }
    std::path::Path::new(path).is_file()
}

#[derive(Debug)]
pub(crate) struct Cited {
    pub path: std::path::PathBuf,
    pub line: usize,
    pub label: String,
}

/// Turn a Codex memory block into citation rows. The raw tags stay out of the pane.
pub(crate) fn present_pane(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let body = line_body(&lines[index]);
        let Some(at) = body.find("<oai-mem-citation>") else {
            out.push(lines[index].clone());
            index += 1;
            continue;
        };
        let head = body[..at].trim_end();
        if !head.is_empty() {
            out.push(keep_speaker(&lines[index], head));
        }
        let mut block = body[at + "<oai-mem-citation>".len()..].to_string();
        index += 1;
        let mut closed = false;
        while index < lines.len() {
            let next = line_body(&lines[index]);
            if let Some(end) = next.find("</oai-mem-citation>") {
                if !next[..end].trim().is_empty() {
                    block.push('\n');
                    block.push_str(&next[..end]);
                }
                let tail = next[end + "</oai-mem-citation>".len()..].trim();
                index += 1;
                out.extend(format_citation(&block));
                if !tail.is_empty() {
                    out.push(tail.to_string());
                }
                closed = true;
                break;
            }
            block.push('\n');
            block.push_str(&next);
            index += 1;
        }
        if !closed {
            out.extend(format_citation(&block));
        }
    }
    out
}

fn line_body(line: &str) -> String {
    let plain = strip_sgr(line);
    plain
        .strip_prefix("user  ")
        .or_else(|| plain.strip_prefix("assistant  "))
        .unwrap_or(plain.as_str())
        .to_string()
}

fn keep_speaker(line: &str, head: &str) -> String {
    let plain = strip_sgr(line);
    if let Some(prefix) = ["user  ", "assistant  "]
        .into_iter()
        .find(|prefix| plain.starts_with(prefix))
    {
        format!("{prefix}{head}")
    } else {
        head.to_string()
    }
}

fn format_citation(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(at) = rest.find("|note=[") {
        let path_start = rest[..at]
            .rfind(['\n', '>'])
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let path = rest[path_start..at].trim();
        let after = &rest[at + "|note=[".len()..];
        let Some(end) = after.find(']') else {
            break;
        };
        let note = after[..end].split_whitespace().collect::<Vec<_>>().join(" ");
        if !path.is_empty() && !path.contains('<') {
            out.push(format!("cite  {path}  {note}"));
        }
        rest = &after[end + 1..];
    }
    if let Some(start) = block.find("<rollout_ids>") {
        let after = &block[start + "<rollout_ids>".len()..];
        let end = after.find("</rollout_ids>").unwrap_or(after.len());
        for id in after[..end].split_whitespace() {
            if rollout_id(id) {
                out.push(format!("след  {id}"));
            }
        }
    }
    out
}

fn rollout_id(text: &str) -> bool {
    let text = text.trim();
    (32..=40).contains(&text.len())
        && text.contains('-')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

pub(crate) fn file_spans(text: &str, roots: &[std::path::PathBuf]) -> Vec<(usize, usize, Cited)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '`' {
            index += 1;
            continue;
        }
        if chars[index] == '"'
            && let Some(end) = quoted_windows_path(&chars, index)
            && let Some(cited) = cite_token(&chars[index + 1..end], roots)
        {
            out.push((index, end + 1, cited));
            index = end + 1;
            continue;
        }
        if let Some(end) = windows_path(&chars, index) {
            let end = shrink_token(&chars, index, end);
            if let Some(cited) = cite_token(&chars[index..end], roots) {
                let (start, end) = tick_wrap(&chars, index, end);
                out.push((start, end, cited));
                index = end.max(index + 1);
                continue;
            }
            index = end.max(index + 1);
            continue;
        }
        if at_boundary(&chars, index) && token_start(chars[index]) {
            let mut end = index;
            while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '`' {
                end += 1;
            }
            let end = shrink_token(&chars, index, end);
            if let Some(cited) = cite_token(&chars[index..end], roots) {
                let (start, end) = tick_wrap(&chars, index, end);
                out.push((start, end, cited));
                index = end.max(index + 1);
                continue;
            }
            index = end.max(index + 1);
            continue;
        }
        index += 1;
    }
    out
}

fn tick_wrap(chars: &[char], start: usize, end: usize) -> (usize, usize) {
    let open = start > 0 && chars[start - 1] == '`';
    let close = end < chars.len() && chars[end] == '`';
    if open && close {
        (start - 1, end + 1)
    } else {
        (start, end)
    }
}

fn token_start(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '.' || ch == '/' || ch == '\\'
}

fn shrink_token(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start && matches!(chars[end - 1], '.' | ',' | ';' | ')' | ']' | '"' | '\'' | '»')
    {
        let inner: String = chars[start..end - 1].iter().collect();
        let (path, _, _) = split_cite(inner.trim());
        if !path.contains('.') && !path.contains('/') && !path.contains('\\') {
            break;
        }
        end -= 1;
    }
    end
}

fn cite_token(chars: &[char], roots: &[std::path::PathBuf]) -> Option<Cited> {
    if chars.len() < 3 {
        return None;
    }
    let raw: String = chars.iter().collect();
    if raw.contains("://") || raw.contains('<') || raw.contains('>') {
        return None;
    }
    let (path, line, suffix) = split_cite(raw.trim());
    if path.is_empty() {
        return None;
    }
    let candidate = std::path::PathBuf::from(path);
    if !path_worth_stat(&candidate) {
        return None;
    }
    if picture_ext(&candidate) {
        return None;
    }
    let resolved = resolve_file(&candidate, roots)?;
    let name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let label = if suffix.is_empty() {
        name
    } else {
        format!("{name}{suffix}")
    };
    Some(Cited {
        path: resolved,
        line,
        label,
    })
}

fn path_worth_stat(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    if text.contains('/') || text.contains('\\') || text.contains(':') {
        return ext_ok(path);
    }
    ext_ok(path)
}

fn ext_ok(path: &std::path::Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    (1..=8).contains(&ext.len()) && ext.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn picture_ext(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif")
}

fn resolve_file(path: &std::path::Path, roots: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    roots.iter().find_map(|root| {
        let full = root.join(path);
        full.is_file().then_some(full)
    })
}

fn split_cite(token: &str) -> (&str, usize, &str) {
    let Some(colon) = token.rfind(':') else {
        return (token, 0, "");
    };
    let after = &token[colon + 1..];
    if !line_spec(after) {
        return (token, 0, "");
    }
    let path = &token[..colon];
    if path.len() <= 2 || !(path.contains('/') || path.contains('\\') || path.contains('.')) {
        return (token, 0, "");
    }
    let line = after
        .split(['-', ':'])
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0);
    (path, line, &token[colon..])
}

fn line_spec(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut digits = false;
    let mut seps = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits = true;
            continue;
        }
        if (ch == '-' || ch == ':') && digits && seps < 1 {
            seps += 1;
            digits = false;
            continue;
        }
        return false;
    }
    digits
}

fn push_speech(wrapped: &mut Vec<Piece>, plain: &str, ink: &'static str, width: usize) {
    let (head, body) = speech_parts(plain);
    let (stamp, body) = split_stamp(body).unwrap_or(("", body));
    let marked = if head.is_empty() && stamp.is_empty() {
        mark_speech(body, ink)
    } else {
        let mut line = String::new();
        if !head.is_empty() {
            line.push_str(DIM);
            line.push_str(head);
        }
        if !stamp.is_empty() {
            line.push_str(DIM);
            line.push_str(stamp);
            line.push_str("  ");
        }
        line.push_str(ink);
        line.push_str(&mark_speech(body, ink));
        line
    };
    let marker = list_hang(body);
    let hang = if marker == 0 {
        0
    } else {
        head.chars().count()
            + if stamp.is_empty() {
                0
            } else {
                stamp.chars().count() + 2
            }
            + marker
    };
    for part in wrap_hung(&marked, width, hang) {
        wrapped.push(Piece {
            text: part,
            ink,
            image: 0,
            open: 0,
            fold: 0, tree: 0, doc: 0,
            clock: String::new(),
        });
    }
}

fn fit_tail(name: &str, room: usize) -> String {
    let room = room.max(1);
    if name.chars().count() <= room {
        return name.to_string();
    }
    let keep = room.saturating_sub(1).max(1);
    let tail: String = name
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

fn push_shot(wrapped: &mut Vec<Piece>, app: &App, n: u16, plain: &str, width: usize) {
    let name = picture_paths(plain)
        .into_iter()
        .next()
        .map(|(_, _, path)| {
            let file = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "image".into());
            fit_tail(&file, width.saturating_sub(4).max(4))
        })
        .unwrap_or_else(|| format!("image {n}"));
    wrapped.push(Piece {
        text: format!("▣ {name}"),
        ink: GOLD,
        image: n,
        open: 0,
        fold: 0, tree: 0, doc: 0,
        clock: String::new(),
    });
    if let Some(thumb) = app.shot_thumb(n) {
        for line in thumb.lines() {
            if line.is_empty() {
                continue;
            }
            wrapped.push(Piece {
                text: line.to_string(),
                ink: "",
                image: n,
                open: 0,
                fold: 0, tree: 0, doc: 0,
                clock: String::new(),
            });
        }
    }
}

fn push_marks(
    wrapped: &mut Vec<Piece>,
    plain: &str,
    shots: &[(usize, usize, std::path::PathBuf)],
    files: &[(usize, usize, Cited)],
    ink: &'static str,
    width: usize,
    app: &App,
) {
    struct Mark {
        start: usize,
        end: usize,
        image: u16,
        open: u16,
        label: String,
    }
    let mut marks = Vec::new();
    for (start, end, path) in shots {
        let n = app.shot_number(path).unwrap_or(0);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        marks.push(Mark {
            start: *start,
            end: *end,
            image: n,
            open: 0,
            label: name,
        });
    }
    for (start, end, cited) in files {
        if marks
            .iter()
            .any(|mark| *start < mark.end && *end > mark.start)
        {
            continue;
        }
        marks.push(Mark {
            start: *start,
            end: *end,
            image: 0,
            open: app.open_number(&cited.path, cited.line),
            label: cited.label.clone(),
        });
    }
    marks.sort_by_key(|mark| mark.start);
    let chars: Vec<char> = plain.chars().collect();
    let mut cursor = 0usize;
    let mut kept = Vec::new();
    for mark in marks {
        if mark.start < cursor {
            continue;
        }
        cursor = mark.end;
        kept.push(mark);
    }
    cursor = 0;
    for mark in kept {
        if mark.start > cursor {
            let before: String = chars[cursor..mark.start].iter().collect();
            push_speech(wrapped, &before, ink, width);
        }
        let label = fit_tail(&mark.label, width.saturating_sub(4).max(4));
        wrapped.push(Piece {
            text: format!("▣ {label}"),
            ink: GOLD,
            image: mark.image,
            open: mark.open,
            fold: 0, tree: 0, doc: 0,
            clock: String::new(),
        });
        if mark.image != 0
            && let Some(thumb) = app.shot_thumb(mark.image)
        {
            for line in thumb.lines() {
                if line.is_empty() {
                    continue;
                }
                wrapped.push(Piece {
                    text: line.to_string(),
                    ink: "",
                    image: mark.image,
                    open: 0,
                    fold: 0, tree: 0, doc: 0,
                    clock: String::new(),
                });
            }
        }
        cursor = mark.end;
    }
    if cursor < chars.len() {
        let after: String = chars[cursor..].iter().collect();
        push_speech(wrapped, &after, ink, width);
    }
}

fn speech_parts(plain: &str) -> (&str, &str) {
    for prefix in ["user  ", "assistant  "] {
        if let Some(rest) = plain.strip_prefix(prefix) {
            return (prefix, rest);
        }
    }
    ("", plain)
}

fn list_hang(body: &str) -> usize {
    let trimmed = body.trim_start();
    let lead = body.chars().count() - trimmed.chars().count();
    if task_mark(trimmed).is_some() {
        return lead + 2;
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| trimmed.strip_prefix("> "))
        && !rest.is_empty()
    {
        return lead + 2;
    }
    let bytes = trimmed.as_bytes();
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if (1..=2).contains(&digits)
        && bytes.get(digits) == Some(&b'.')
        && bytes.get(digits + 1) == Some(&b' ')
        && trimmed.len() > digits + 2
    {
        return lead + digits + 2;
    }
    0
}

fn mark_speech(body: &str, base: &str) -> String {
    let trimmed = body.trim_start();
    let indent = &body[..body.len() - trimmed.len()];
    if is_rule(trimmed) {
        return format!("{DIM}{}", "─".repeat(24));
    }
    if let Some(title) = heading(trimmed) {
        return format!(
            "{indent}{BOLD_GOLD}{}{base}",
            mark_inline(title, BOLD_GOLD)
        );
    }
    if looks_like_table(trimmed) {
        return mark_table(trimmed, base);
    }
    if let Some((mark, rest)) = task_mark(trimmed) {
        return format!("{indent}{GOLD}{mark}{base} {}", mark_inline(rest, base));
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        && !rest.is_empty()
    {
        return format!("{indent}{GOLD}•{base} {}", mark_inline(rest, base));
    }
    if let Some(rest) = trimmed.strip_prefix("> ")
        && !rest.is_empty()
    {
        return format!("{indent}{GOLD}│{base} {}", mark_inline(rest, base));
    }
    let bytes = trimmed.as_bytes();
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if (1..=2).contains(&digits)
        && bytes.get(digits) == Some(&b'.')
        && bytes.get(digits + 1) == Some(&b' ')
    {
        let number = &trimmed[..digits];
        let rest = &trimmed[digits + 2..];
        return format!("{indent}{GOLD}{number}.{base} {}", mark_inline(rest, base));
    }
    mark_inline(body, base)
}

fn is_rule(trimmed: &str) -> bool {
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 3 {
        return false;
    }
    let kind = chars[0];
    (kind == '-' || kind == '*' || kind == '_') && chars.iter().all(|ch| *ch == kind)
}

fn heading(trimmed: &str) -> Option<&str> {
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let title = rest.trim();
    if title.is_empty() { None } else { Some(title) }
}

fn task_mark(trimmed: &str) -> Option<(&'static str, &str)> {
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    if let Some(body) = rest.strip_prefix("[ ] ") {
        return Some(("○", body));
    }
    if let Some(body) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
        return Some(("✓", body));
    }
    None
}

fn looks_like_table(trimmed: &str) -> bool {
    let line = trimmed.trim();
    line.starts_with('|') && line.matches('|').count() >= 2
}

fn mark_table(line: &str, base: &str) -> String {
    if table_rule(line) {
        return format!("{DIM}{}", "┄".repeat(24));
    }
    let cells: Vec<&str> = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    let mut out = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            out.push_str(DIM);
            out.push_str(" │ ");
            out.push_str(base);
        }
        out.push_str(&mark_inline(cell, base));
    }
    out
}

fn table_rule(line: &str) -> bool {
    let cells: Vec<&str> = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .collect();
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let body = cell.trim_matches(':');
            body.len() >= 3 && body.chars().all(|ch| ch == '-')
        })
}

fn mark_inline(text: &str, base: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '`' && chars.get(index + 1) != Some(&'`') {
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == '`') {
                let end = index + 1 + end;
                if end > index + 1 {
                    paint_span(&mut out, &chars[index + 1..end], code_ink(base), base);
                    index = end + 1;
                    continue;
                }
            }
        }
        if chars[index] == '~'
            && chars.get(index + 1) == Some(&'~')
            && at_boundary(&chars, index)
        {
            let rest = &chars[index + 2..];
            if let Some(end) = rest.windows(2).position(|pair| pair == ['~', '~']) {
                let end = index + 2 + end;
                if end > index + 2 {
                    paint_span(&mut out, &chars[index + 2..end], "\x1b[9;38;5;131m", base);
                    index = end + 2;
                    continue;
                }
            }
        }
        if chars[index] == '*'
            && chars.get(index + 1) == Some(&'*')
            && chars.get(index + 2) == Some(&'*')
        {
            let rest = &chars[index + 3..];
            if let Some(end) = rest.windows(3).position(|pair| pair == ['*', '*', '*']) {
                let end = index + 3 + end;
                if end > index + 3 {
                    paint_span(
                        &mut out,
                        &chars[index + 3..end],
                        "\x1b[1;3;38;5;220m",
                        base,
                    );
                    index = end + 3;
                    continue;
                }
            }
        }
        if (chars[index] == '*' || chars[index] == '_') && chars.get(index + 1) == Some(&chars[index])
        {
            let tick = chars[index];
            let rest = &chars[index + 2..];
            if let Some(end) = rest.windows(2).position(|pair| pair == [tick, tick]) {
                let end = index + 2 + end;
                if end > index + 2 {
                    paint_span(&mut out, &chars[index + 2..end], BOLD_GOLD, base);
                    index = end + 2;
                    continue;
                }
            }
        }
        if let Some(end) = emph_span(&chars, index) {
            paint_span(&mut out, &chars[index + 1..end], italic_of(base), base);
            index = end + 1;
            continue;
        }
        if chars[index] == '[' {
            if let Some(label_end) = chars[index + 1..].iter().position(|ch| *ch == ']') {
                let label_end = index + 1 + label_end;
                if label_end > index + 1 && chars.get(label_end + 1) == Some(&'(') {
                    if let Some(url_end) = chars[label_end + 2..].iter().position(|ch| *ch == ')') {
                        let url_end = label_end + 2 + url_end;
                        paint_span(&mut out, &chars[index + 1..label_end], link_ink(base), DIM);
                        out.push(' ');
                        paint_span(&mut out, &chars[label_end + 2..url_end], DIM, base);
                        index = url_end + 1;
                        continue;
                    }
                }
            }
        }
        if let Some(end) = windows_path(&chars, index) {
            if end > index + 2 {
                paint_span(&mut out, &chars[index..end], accent(base), base);
                index = end;
                continue;
            }
        }
        if let Some(end) = bare_url(&chars, index) {
            paint_span(&mut out, &chars[index..end], link_ink(base), base);
            index = end;
            continue;
        }
        if let Some(end) = loose_path(&chars, index) {
            paint_span(&mut out, &chars[index..end], accent(base), base);
            index = end;
            continue;
        }
        if let Some(end) = slash_command(&chars, index) {
            paint_span(&mut out, &chars[index..end], accent(base), base);
            index = end;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn code_ink(base: &str) -> &'static str {
    if base.contains("38;5;51") {
        "\x1b[1;38;5;220m"
    } else {
        "\x1b[1;38;5;51m"
    }
}

fn link_ink(base: &str) -> &'static str {
    if base.contains("38;5;51") {
        "\x1b[4;38;5;220m"
    } else {
        "\x1b[4;38;5;51m"
    }
}

fn italic_of(base: &str) -> &'static str {
    if base.contains("38;5;51") {
        "\x1b[3;38;5;51m"
    } else if base.contains("38;5;223") {
        "\x1b[3;38;5;223m"
    } else if base.contains("38;5;220") {
        "\x1b[3;38;5;220m"
    } else {
        "\x1b[3;38;5;203m"
    }
}

fn emph_span(chars: &[char], index: usize) -> Option<usize> {
    let tick = chars.get(index).copied()?;
    if tick != '*' && tick != '_' {
        return None;
    }
    if chars.get(index + 1) == Some(&tick) || !at_boundary(chars, index) {
        return None;
    }
    let start = index + 1;
    if start >= chars.len() || chars[start].is_whitespace() {
        return None;
    }
    let mut end = start;
    while end < chars.len() && end - start < 80 {
        if chars[end] == tick {
            return Some(end);
        }
        end += 1;
    }
    None
}

fn bare_url(chars: &[char], index: usize) -> Option<usize> {
    if !at_boundary(chars, index) {
        return None;
    }
    let scheme = if has_prefix(chars, index, "https://") {
        8
    } else if has_prefix(chars, index, "http://") {
        7
    } else {
        return None;
    };
    if index + scheme >= chars.len() {
        return None;
    }
    let mut end = index + scheme;
    while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '`' {
        end += 1;
    }
    while end > index + scheme && matches!(chars[end - 1], '.' | ',' | ';' | ')' | ']' | '"' | '\'')
    {
        end -= 1;
    }
    Some(end)
}

fn has_prefix(chars: &[char], index: usize, prefix: &str) -> bool {
    let mut rest = chars[index..].iter().copied();
    prefix.chars().all(|ch| rest.next() == Some(ch))
}

fn loose_path(chars: &[char], index: usize) -> Option<usize> {
    if !at_boundary(chars, index) {
        return None;
    }
    let ch = chars.get(index).copied()?;
    if !(ch.is_alphanumeric() || ch == '.' || ch == '/' || ch == '\\') {
        return None;
    }
    if ch.is_ascii_alphabetic() && chars.get(index + 1) == Some(&':') {
        return None;
    }
    let mut end = index;
    while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '`' {
        end += 1;
    }
    let end = shrink_token(chars, index, end);
    if looks_like_path(&chars[index..end]) {
        Some(end)
    } else {
        None
    }
}

fn looks_like_path(chars: &[char]) -> bool {
    if chars.len() < 3 {
        return false;
    }
    if chars.windows(3).any(|part| part == [':', '/', '/']) {
        return false;
    }
    if !chars.contains(&'/') && !chars.contains(&'\\') {
        return false;
    }
    let Some(dot) = chars.iter().rposition(|ch| *ch == '.') else {
        return false;
    };
    let ext = &chars[dot + 1..];
    (1..=8).contains(&ext.len()) && ext.iter().all(|ch| ch.is_ascii_alphabetic())
}

fn paint_span(out: &mut String, text: &[char], ink: &str, base: &str) {
    out.push_str(ink);
    for ch in text {
        out.push(*ch);
    }
    out.push_str(base);
}

fn push_plain(
    wrapped: &mut Vec<Piece>,
    plain: &str,
    width: usize,
    ink: &'static str,
) {
    if plain.is_empty() {
        wrapped.push(Piece {
            text: String::new(),
            ink,
            image: 0,
            open: 0,
            fold: 0, tree: 0, doc: 0,
            clock: String::new(),
        });
        return;
    }
    for part in wrap_words(plain, width) {
        wrapped.push(Piece {
            text: part,
            ink,
            image: 0,
            open: 0,
            fold: 0, tree: 0, doc: 0,
            clock: String::new(),
        });
    }
}

#[derive(Clone)]
struct InkState {
    speaker: &'static str,
    user_block: bool,
    code: bool,
    lang: &'static str,
}

impl InkState {
    fn new() -> Self {
        Self {
            speaker: PAPER,
            user_block: false,
            code: false,
            lang: "rs",
        }
    }
}

/// First source line that can reach the visible window. Earlier lines stay
/// out of wrapping, markup, and file stats.
fn corner_reserve() -> usize {
    "[settings] [help]".chars().count() + 1
}

/// Source rows whose wrapped height covers the screen, not the scrolled-away tail.
/// `skip_bottom` is how many visual lines of that wrap sit below the viewport,
/// so a long row scrolls inside itself instead of jumping over.
pub(crate) fn view_span(
    rows: &[crate::app::Folded],
    width: usize,
    from_end: usize,
    room: usize,
) -> (usize, usize, usize, bool) {
    if rows.is_empty() {
        return (0, 0, 0, false);
    }
    let mut hi = rows.len();
    let mut skipped = 0usize;
    let mut skip_bottom = 0usize;
    while hi > 0 && skipped < from_end {
        let height = guess_lines(&rows[hi - 1].text, width).max(1);
        if skipped + height > from_end {
            skip_bottom = from_end - skipped;
            break;
        }
        hi -= 1;
        skipped += height;
    }
    if hi == 0 {
        let end = rows.len().min(room.max(1));
        return (0, end, 0, true);
    }
    let mut lo = hi;
    let mut got = 0usize;
    let need = room.saturating_add(6);
    while lo > 0 && got < need {
        lo -= 1;
        got += guess_lines(&rows[lo].text, width).max(1);
    }
    let align_top = lo == 0 && from_end > 0 && skip_bottom == 0 && got <= room;
    (lo, hi, skip_bottom, align_top)
}

fn sgr_underlined(params: &str) -> bool {
    params.split(';').any(|part| part == "4")
}

fn lights_up(spot: Spot) -> bool {
    // The composer is a field. The inside of a frame is not a button.
    !matches!(spot, Spot::Cmd | Spot::DocBox | Spot::PicBox)
}

/// First glyph through the last glyph of a hit. Trailing blanks stay dark.
fn glow_span(glyphs: &[Vec<char>], x: usize, y: usize, w: usize) -> Option<(usize, usize)> {
    let row = glyphs.get(y)?;
    let end = (x + w).min(row.len());
    let mut first = None;
    let mut last = x;
    for col in x..end {
        if row.get(col).is_some_and(|ch| *ch != ' ') {
            if first.is_none() {
                first = Some(col);
            }
            last = col + 1;
        }
    }
    let start = first?;
    Some((start, last - start))
}

fn guess_lines(text: &str, width: usize) -> usize {
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

fn tint_frame(frame: &str, look: &str) -> String {
    let Some(pick) = look_color(look) else {
        return frame.to_string();
    };
    let mut out = String::with_capacity(frame.len() + 64);
    let bytes = frame.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let start = i;
            i += 2;
            let param_at = i;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'm' {
                out.push_str("\x1b[");
                out.push_str(&tint_params(&frame[param_at..i], pick));
                out.push('m');
                i += 1;
                continue;
            }
            let end = if i < bytes.len() { i + 1 } else { i };
            out.push_str(&frame[start..end]);
            i = end;
            continue;
        }
        let ch = frame[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn tint_params(params: &str, pick: fn(u8) -> Option<(u8, u8, u8)>) -> String {
    let parts: Vec<&str> = params.split(';').collect();
    let mut out = Vec::with_capacity(parts.len());
    let mut i = 0usize;
    while i < parts.len() {
        if (parts[i] == "38" || parts[i] == "48")
            && i + 2 < parts.len()
            && parts[i + 1] == "5"
            && let Ok(index) = parts[i + 2].parse::<u8>()
            && let Some((red, green, blue)) = pick(index)
        {
            out.push(format!("{};2;{red};{green};{blue}", parts[i]));
            i += 3;
            continue;
        }
        if (parts[i] == "38" || parts[i] == "48") && i + 4 < parts.len() && parts[i + 1] == "2" {
            out.push(format!(
                "{};2;{};{};{}",
                parts[i],
                parts[i + 2],
                parts[i + 3],
                parts[i + 4]
            ));
            i += 5;
            continue;
        }
        out.push(parts[i].to_string());
        i += 1;
    }
    out.join(";")
}

fn look_color(look: &str) -> Option<fn(u8) -> Option<(u8, u8, u8)>> {
    match look {
        "cyberdeck" => Some(cyberdeck_color),
        "parchment" => Some(parchment_color),
        "ink" => Some(ink_color),
        "dos" => Some(dos_color),
        "linux" => Some(linux_color),
        _ => None,
    }
}

fn cyberdeck_color(index: u8) -> Option<(u8, u8, u8)> {
    Some(match index {
        0 => (0, 0, 0),
        17 => (0, 32, 28),
        40 => (80, 255, 120),
        51 => (0, 255, 200),
        81 => (120, 255, 220),
        131 => (140, 96, 32),
        141 => (255, 120, 40),
        160 => (120, 48, 0),
        196 => (255, 64, 32),
        203 => (255, 140, 48),
        220 => (255, 196, 0),
        223 => (255, 176, 64),
        234 => (16, 12, 0),
        236 => (32, 22, 0),
        240 => (90, 70, 20),
        245 => (180, 140, 60),
        39 => (255, 176, 90),
        45 => (255, 214, 64),
        46 => (80, 255, 120),
        67 => (160, 110, 48),
        153 => (210, 160, 70),
        255 => (255, 244, 210),
        _ => return None,
    })
}

/// Claude Code dark. Near-black glass, white text, orange accent.
/// The light cream palette was the miss.
fn parchment_color(index: u8) -> Option<(u8, u8, u8)> {
    Some(match index {
        0 => (18, 18, 18),
        17 => (32, 32, 32),
        40 => (78, 186, 101),
        51 => (255, 255, 255),
        81 => (177, 185, 249),
        131 => (153, 153, 153),
        141 => (175, 135, 255),
        160 => (80, 80, 80),
        196 => (255, 107, 128),
        203 => (220, 220, 220),
        220 => (215, 119, 87),
        223 => (236, 232, 226),
        234 => (24, 24, 24),
        236 => (55, 55, 55),
        240 => (110, 110, 110),
        245 => (170, 170, 170),
        39 => (215, 119, 87),
        45 => (230, 190, 90),
        46 => (78, 186, 101),
        67 => (140, 140, 140),
        153 => (180, 180, 180),
        255 => (255, 255, 255),
        _ => return None,
    })
}

fn ink_color(index: u8) -> Option<(u8, u8, u8)> {
    Some(match index {
        0 => (16, 16, 16),
        17 => (28, 32, 40),
        40 => (120, 190, 130),
        51 => (186, 196, 210),
        81 => (150, 180, 210),
        131 => (110, 110, 110),
        141 => (170, 150, 210),
        160 => (48, 48, 48),
        196 => (255, 96, 88),
        203 => (176, 176, 176),
        220 => (255, 255, 255),
        223 => (228, 228, 228),
        234 => (24, 24, 24),
        236 => (36, 36, 36),
        240 => (90, 90, 90),
        245 => (150, 150, 150),
        39 => (140, 176, 214),
        45 => (168, 196, 150),
        46 => (120, 190, 130),
        67 => (120, 120, 120),
        153 => (168, 168, 168),
        255 => (255, 255, 255),
        _ => return None,
    })
}

/// Blue panel, white list, yellow keys. The FAR desk, named dos.
fn dos_color(index: u8) -> Option<(u8, u8, u8)> {
    Some(match index {
        0 => (0, 0, 170),
        17 => (0, 0, 0),
        40 => (85, 255, 85),
        51 => (85, 255, 255),
        81 => (170, 255, 255),
        131 => (85, 85, 200),
        141 => (255, 85, 255),
        160 => (255, 85, 85),
        196 => (255, 85, 85),
        203 => (255, 255, 255),
        220 => (255, 255, 85),
        223 => (192, 192, 192),
        234 => (0, 0, 128),
        236 => (0, 0, 100),
        240 => (128, 128, 192),
        245 => (170, 170, 170),
        39 => (85, 85, 255),
        45 => (85, 255, 85),
        46 => (85, 255, 85),
        67 => (170, 170, 170),
        153 => (170, 170, 255),
        255 => (255, 255, 255),
        _ => return None,
    })
}

/// Black glass, VGA colors. Light gray text, green and yellow on top.
fn linux_color(index: u8) -> Option<(u8, u8, u8)> {
    Some(match index {
        0 => (0, 0, 0),
        17 => (0, 0, 170),
        40 => (85, 255, 85),
        51 => (85, 255, 255),
        81 => (85, 255, 255),
        131 => (85, 85, 85),
        141 => (255, 85, 255),
        160 => (170, 0, 0),
        196 => (255, 85, 85),
        203 => (255, 255, 255),
        220 => (255, 255, 85),
        223 => (170, 170, 170),
        234 => (0, 0, 0),
        236 => (0, 0, 85),
        240 => (85, 85, 85),
        245 => (170, 170, 170),
        39 => (85, 85, 255),
        45 => (255, 255, 85),
        46 => (85, 255, 85),
        67 => (85, 85, 85),
        153 => (170, 170, 170),
        255 => (255, 255, 255),
        _ => return None,
    })
}

fn push_folded_line(
    wrapped: &mut Vec<Piece>,
    app: &App,
    roots: &[std::path::PathBuf],
    row: &crate::app::Folded,
    width: usize,
    state: &mut InkState,
) {
    let (shown, clock) = peel_clock(&row.text);
    let room = if clock.is_empty() {
        width
    } else {
        width.saturating_sub(clock.chars().count() + 1).max(8)
    };
    let before = wrapped.len();
    push_transcript_line(wrapped, app, roots, &shown, room, state);
    if let Some(id) = row.fold {
        let fold = u16::try_from(id).unwrap_or(u16::MAX).saturating_add(1);
        for piece in &mut wrapped[before..] {
            piece.fold = fold;
        }
    }
    if let Some(id) = row.tree {
        let tree = id.saturating_add(1);
        for piece in &mut wrapped[before..] {
            piece.tree = tree;
        }
    }
    if let Some(id) = row.doc {
        let doc = id.saturating_add(1);
        for piece in &mut wrapped[before..] {
            piece.doc = doc;
        }
    }
    if !clock.is_empty()
        && let Some(piece) = wrapped.get_mut(before)
    {
        piece.clock = clock;
    }
}

fn peel_clock(line: &str) -> (String, String) {
    let prefix = if line.starts_with("user  ") {
        "user  "
    } else if line.starts_with("assistant  ") {
        "assistant  "
    } else {
        return (line.to_string(), String::new());
    };
    let rest = &line[prefix.len()..];
    let Some((stamp, body)) = split_stamp(rest) else {
        return (line.to_string(), String::new());
    };
    (format!("{prefix}{body}"), crate::app::clock_face(stamp))
}

#[allow(dead_code)]
fn transcript_start(lines: &[String], width: usize, need: usize) -> usize {
    if lines.is_empty() || need == 0 {
        return 0;
    }
    let width = width.max(1);
    let cap = width.saturating_mul(need).saturating_add(width);
    let mut pieces = 0usize;
    let mut index = lines.len();
    while index > 0 && pieces < need {
        index -= 1;
        let count = lines[index].chars().take(cap).count();
        pieces = pieces.saturating_add(count / width + 1);
    }
    index
}

fn fence_body(line: &str) -> &str {
    line.strip_prefix("user  ")
        .or_else(|| line.strip_prefix("assistant  "))
        .unwrap_or(line)
        .trim_start()
}

fn is_fence_line(line: &str) -> bool {
    fence_body(line).starts_with("```")
}

/// Speaker and fence state for a line that is not drawn.
fn advance_ink(state: &mut InkState, line: &str) {
    if line.starts_with("след  ") || line.starts_with("cite  ") {
        return;
    }
    if is_fence_line(line) {
        if !state.code {
            state.lang = fence_lang(fence_body(line));
        }
        state.code = !state.code;
        return;
    }
    if state.code || image_number(line).is_some() {
        return;
    }
    if line.starts_with("tool  ") || line.starts_with('◆') || line.starts_with("v  ") {
        state.user_block = false;
        state.speaker = DIM;
    } else if line.starts_with("changed  ")
        || line.starts_with("folder  ")
        || line.starts_with("tree  ")
        || line.starts_with("stat  ")
        || line.starts_with("worked  ")
    {
        state.user_block = false;
    } else if line.starts_with("итог  ") {
        state.user_block = false;
        state.speaker = DIM;
    } else if line.starts_with("user  ") {
        state.user_block = true;
        state.speaker = CYAN;
    } else if line.starts_with("assistant  ") {
        state.user_block = false;
        state.speaker = PAPER;
    }
}

fn push_transcript_line(
    wrapped: &mut Vec<Piece>,
    app: &App,
    roots: &[std::path::PathBuf],
    line: &str,
    width: usize,
    state: &mut InkState,
) {
    let mut plain = strip_sgr(line);
    if let Some(rest) = plain.strip_prefix("след  ") {
        push_plain(wrapped, &format!("след  {rest}"), width, DIM);
        return;
    }
    if let Some(rest) = plain.strip_prefix("cite  ") {
        plain = format!("память  {rest}");
    }
    let body = plain
        .strip_prefix("user  ")
        .or_else(|| plain.strip_prefix("assistant  "))
        .unwrap_or(plain.as_str());
    if body.trim_start().starts_with("```") {
        if !state.code {
            state.lang = fence_lang(body);
        }
        state.code = !state.code;
        push_plain(wrapped, &plain, width, DIM);
        return;
    }
    if state.code {
        for part in wrap_words(&plain, width) {
            let text = if state.lang.is_empty() {
                part
            } else {
                crate::hl::paint_ext(&part, state.lang)
            };
            wrapped.push(Piece {
                text,
                ink: SALMON,
                image: 0,
                open: 0,
                fold: 0, tree: 0, doc: 0,
                clock: String::new(),
            });
        }
        return;
    }
    if let Some(n) = image_number(&plain) {
        push_shot(wrapped, app, n, &plain, width);
        return;
    }
    let ink = if plain.starts_with("tool  ")
        || plain.starts_with('◆')
        || plain.starts_with("v  ")
    {
        state.user_block = false;
        state.speaker = DIM;
        DIM
    } else if plain.starts_with("changed  ") {
        state.user_block = false;
        GOLD
    } else if let Some(rest) = plain.strip_prefix("tree  ") {
        state.user_block = false;
        let (mark, dir) = tree_face(rest);
        if let Some(total) = whole_total(dir) {
            let text = match total {
                Some((plus, minus)) => {
                    format!("{mark}  {GREEN}+{plus}{KEEP}  {RED}-{minus}")
                }
                None => mark.to_string(),
            };
            push_plain(wrapped, &text, width, GOLD);
            return;
        }
        let label = if dir.is_empty() || dir == "." {
            "./".to_string()
        } else {
            format!("{dir}/")
        };
        push_plain(wrapped, &format!("{mark} {label}"), width, GOLD);
        return;
    } else if let Some(rest) = plain.strip_prefix("folder  ") {
        state.user_block = false;
        let label = if rest == "." {
            "./".to_string()
        } else {
            format!("{rest}/")
        };
        push_plain(wrapped, &label, width, GOLD);
        return;
    } else if let Some(rest) = plain.strip_prefix("stat  ") {
        state.user_block = false;
        push_plain(wrapped, &paint_stat(rest), width, SALMON);
        return;
    } else if plain.starts_with("worked  ") {
        state.user_block = false;
        CYAN
    } else if let Some(rest) = plain.strip_prefix("итог  ") {
        state.user_block = false;
        state.speaker = DIM;
        let marked = paint_sum(rest);
        for part in wrap_words(&marked, width) {
            wrapped.push(Piece {
                text: part,
                ink: DIM,
                image: 0,
                open: 0,
                fold: 0, tree: 0, doc: 0,
                clock: String::new(),
            });
        }
        return;
    } else if plain.starts_with("user  ") {
        state.user_block = true;
        state.speaker = CYAN;
        CYAN
    } else if plain.starts_with("assistant  ") {
        state.user_block = false;
        state.speaker = PAPER;
        PAPER
    } else if state.user_block {
        CYAN
    } else if plain.is_empty() {
        DIM
    } else {
        state.speaker
    };
    let shots = if shot_hint(&plain) && !plain.contains("[Image #") {
        picture_paths(&plain)
    } else {
        Vec::new()
    };
    let files = file_spans(&plain, roots);
    if !shots.is_empty() || !files.is_empty() {
        push_marks(wrapped, &plain, &shots, &files, ink, width, app);
        return;
    }
    push_speech(wrapped, &plain, ink, width);
}

/// `*` is the whole diff. Numbers are the total, and `+0 -0` is not painted.
fn whole_total(dir: &str) -> Option<Option<(u64, u64)>> {
    let rest = dir.strip_prefix('*')?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let plus: u64 = match parts.next() {
        Some(text) => text.parse().ok()?,
        None => return Some(None),
    };
    let minus: u64 = parts.next().and_then(|text| text.parse().ok()).unwrap_or(0);
    if plus == 0 && minus == 0 {
        Some(None)
    } else {
        Some(Some((plus, minus)))
    }
}

fn tree_face(rest: &str) -> (char, &str) {
    let mut chars = rest.chars();
    let mark = chars.next().unwrap_or('▼');
    let after = chars.as_str();
    let dir = after.strip_prefix("  ").unwrap_or(after.trim_start());
    (mark, dir)
}

fn fence_lang(body: &str) -> &'static str {
    let tag = body
        .trim_start()
        .trim_start_matches('`')
        .split_whitespace()
        .next()
        .unwrap_or("");
    match tag {
        "rs" | "rust" => "rs",
        "py" | "python" => "py",
        "js" | "jsx" | "javascript" => "js",
        "ts" | "tsx" | "typescript" => "ts",
        "sh" | "bash" | "shell" | "zsh" => "sh",
        "go" | "golang" => "go",
        "c" | "h" | "cpp" | "cc" | "hpp" => "c",
        _ => "",
    }
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    if text.is_empty() {
        return vec![String::new()];
    }
    // One pass. Recounting the remainder made a 50k chat line take seconds,
    // and that ran again before every click.
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some((emit_end, next)) = wrap_cut(rest, width) else {
            out.push(rest.to_string());
            break;
        };
        if emit_end == 0 || next == 0 || next > rest.len() {
            break;
        }
        out.push(rest[..emit_end].to_string());
        rest = &rest[next..];
    }
    carry_color(if out.is_empty() {
        vec![String::new()]
    } else {
        out
    })
}

/// Where one visual line ends. `emit_end` stays, `next` is the rest.
/// `None` means the whole string fits.
fn wrap_cut(text: &str, width: usize) -> Option<(usize, usize)> {
    let min_soft = width / 2;
    let mut visible = 0usize;
    let mut char_index = 0usize;
    let mut lead = 0usize;
    let mut leading = true;
    let mut last_space: Option<usize> = None;
    let mut soft: Option<usize> = None;
    let mut chars = text.char_indices();
    while let Some((index, ch)) = chars.next() {
        if ch == '\u{1b}' {
            char_index += 1;
            leading = false;
            for (_, next) in chars.by_ref() {
                char_index += 1;
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible == width {
            return Some(finish_cut(text, index, last_space, soft));
        }
        if leading {
            if ch.is_whitespace() {
                lead += 1;
            } else {
                leading = false;
            }
        } else if char_index > lead && ch.is_whitespace() {
            last_space = Some(index);
        }
        visible += 1;
        if visible >= min_soft && matches!(ch, '/' | '\\' | '-' | '_') {
            soft = Some(index + ch.len_utf8());
        }
        char_index += 1;
    }
    None
}

fn finish_cut(
    text: &str,
    window_end: usize,
    last_space: Option<usize>,
    soft: Option<usize>,
) -> (usize, usize) {
    if let Some(byte) = last_space.filter(|byte| *byte > 0) {
        let skip = text[byte..].chars().next().map(char::len_utf8).unwrap_or(1);
        return (byte, byte + skip);
    }
    if let Some(byte) = soft.filter(|byte| *byte > 0 && *byte < text.len()) {
        return (byte, byte);
    }
    (window_end, window_end)
}

fn wrap_hung(text: &str, width: usize, hang: usize) -> Vec<String> {
    if hang == 0 || hang + 8 > width {
        return wrap_words(text, width);
    }
    let (prefix, rest) = take_visible(text, hang);
    if rest.is_empty() {
        return wrap_words(text, width);
    }
    let pad = " ".repeat(hang);
    let mut out = Vec::new();
    for (index, part) in wrap_words(rest, width - hang).into_iter().enumerate() {
        if index == 0 {
            out.push(format!("{prefix}{part}"));
        } else {
            out.push(format!("{pad}{part}"));
        }
    }
    out
}

fn take_visible(text: &str, n: usize) -> (&str, &str) {
    let mut visible = 0usize;
    let mut chars = text.char_indices();
    while let Some((index, ch)) = chars.next() {
        if ch == '\u{1b}' {
            for (_, next) in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible == n {
            return (&text[..index], &text[index..]);
        }
        visible += 1;
    }
    (text, "")
}

fn carry_color(parts: Vec<String>) -> Vec<String> {
    let mut carry = String::new();
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let line = if !carry.is_empty() && !part.starts_with('\u{1b}') {
            format!("{carry}{part}")
        } else {
            part
        };
        carry = trailing_sgr(&line);
        out.push(line);
    }
    out
}

fn trailing_sgr(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut last = "";
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            let start = index;
            index += 1;
            while index < bytes.len() && !bytes[index].is_ascii_alphabetic() {
                index += 1;
            }
            if index < bytes.len() {
                index += 1;
                last = &text[start..index];
            }
        } else {
            index += 1;
        }
    }
    last.to_string()
}

#[cfg_attr(not(test), allow(dead_code))]
fn visible_count(text: &str) -> usize {
    let mut count = 0usize;
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
        count += 1;
    }
    count
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
        } else {
            out.push(ch);
        }
    }
    out
}

fn agent_color(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Grok => "\x1b[38;5;196m",
        AgentKind::Claude => "\x1b[38;5;39m",
        AgentKind::Codex => "\x1b[38;5;51m",
    }
}

fn clip_chars(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    if text.chars().count() > width {
        out.pop();
        out.push('~');
    }
    out
}

/// Cuts a browser line on a space or a URL boundary so a long address
/// does not run through the border mid-token. An open `<...` is dropped
/// rather than left without its `>`.
fn fit_visible(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if width == 0 || chars.len() <= width {
        return text.to_string();
    }
    let mut end = boundary_at(&chars, width);
    if let Some(open) = chars[..end].iter().rposition(|ch| *ch == '<')
        && !chars[open..end].contains(&'>')
    {
        end = open;
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
    }
    chars.into_iter().take(end).collect()
}

fn boundary_at(chars: &[char], width: usize) -> usize {
    if chars.len() <= width {
        return chars.len();
    }
    let floor = width.saturating_sub(24).max(width / 2);
    for index in (floor..width).rev() {
        let ch = chars[index];
        if ch.is_whitespace() {
            return index.max(1);
        }
        if matches!(ch, '/' | '?' | '&' | '=' | '-' | ',' | '.') {
            return (index + 1).min(width);
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::{mark_inline, wrap_words};

    #[test]
    fn a_look_repaints_leftover_cyberpunk_indexes() {
        let sample = "\x1b[38;5;39mClaude\x1b[38;5;67m// note\x1b[38;5;45m12\x1b[38;5;46m#\x1b[38;5;153mfile\x1b[1;38;5;255mTitle\x1b[1;4;30;48;5;220mhover\x1b[48;2;255;255;255mНайти\x1b[38;2;32;33;36mhttps\x1b[38;2;255;255;85m>";
        let plain = super::tint_frame(sample, "cyberpunk");
        assert!(plain.contains("38;5;39"), "{plain}");
        assert!(plain.contains("38;5;255"), "{plain}");
        assert!(plain.contains("48;5;220"), "{plain}");
        let expect = [
            ("cyberdeck", "48;2;255;196;0", "38;2;255;176;90"),
            ("parchment", "48;2;215;119;87", "38;2;215;119;87"),
            ("ink", "48;2;255;255;255", "38;2;140;176;214"),
            ("dos", "48;2;255;255;85", "38;2;85;85;255"),
            ("linux", "48;2;255;255;85", "38;2;85;85;255"),
        ];
        for (look, hover, claude) in expect {
            let tinted = super::tint_frame(sample, look);
            for index in ["39", "45", "46", "67", "153", "255", "220"] {
                assert!(
                    !tinted.contains(&format!("38;5;{index}"))
                        && !tinted.contains(&format!("48;5;{index}")),
                    "{look} left {index}: {tinted}"
                );
            }
            assert!(tinted.contains(hover), "{look} {tinted}");
            assert!(tinted.contains(claude), "{look} {tinted}");
            assert!(tinted.contains("30;48;2;") || tinted.contains("30;48;2"), "{look}");
            assert!(tinted.contains("48;2;255;255;255"), "{look} {tinted}");
            assert!(tinted.contains("38;2;32;33;36"), "{look} {tinted}");
            assert!(tinted.contains("38;2;255;255;85"), "{look} {tinted}");
        }
    }

    #[test]
    fn the_field_caret_sits_on_the_text_row() {
        let (col, row) = super::field_caret(8, 10, 4);
        assert_eq!(col, 11);
        assert_eq!(row, 13);
        let (col, row) = super::field_caret(8, 10, 1);
        assert_eq!((col, row), (10, 11));
    }

    #[test]
    fn a_long_line_wraps_in_one_pass() {
        let line = "word ".repeat(10_000);
        let started = std::time::Instant::now();
        let parts = wrap_words(&line, 40);
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_millis() < 200,
            "wrap took {elapsed:?} for {} chars",
            line.len()
        );
        assert!(parts.len() > 100, "{}", parts.len());
        assert!(
            parts.iter().all(|part| super::visible_count(part) <= 40),
            "a piece was wider than 40"
        );
        let solid = "a".repeat(50_000);
        let started = std::time::Instant::now();
        let hard = wrap_words(&solid, 40);
        assert!(
            started.elapsed().as_millis() < 200,
            "hard wrap took {:?}",
            started.elapsed()
        );
        assert_eq!(hard.len(), 1_250);
        assert!(hard.iter().all(|part| part.len() == 40));
    }

    #[test]
    fn wrap_keeps_indent_when_the_line_fits() {
        let line = "    let desk = 1;";
        assert_eq!(wrap_words(line, 40), vec![line.to_string()]);
        assert_eq!(wrap_words("a  b", 40), vec!["a  b".to_string()]);
    }

    #[test]
    fn wrap_breaks_a_long_line_on_a_space() {
        let parts = wrap_words("one two three four", 10);
        assert_eq!(parts, vec!["one two".to_string(), "three four".to_string()]);
        let indented = wrap_words("    alpha beta gamma", 12);
        assert_eq!(indented[0], "    alpha");
        assert!(indented.iter().all(|part| part.chars().count() <= 12));
    }

    #[test]
    fn inline_code_and_bold_are_marked_and_the_fence_stays_plain() {
        let marked = mark_inline("run `say hi` then **Enter** and *now*", "\x1b[38;5;203m");
        assert!(marked.contains("\x1b[1;38;5;51msay hi"), "{marked}");
        assert!(marked.contains("\x1b[1;38;5;220mEnter"), "{marked}");
        assert!(marked.contains("\x1b[3;38;5;203mnow"), "{marked}");
        assert!(!marked.contains("`say hi`"), "{marked}");
        assert!(!marked.contains("**Enter**"), "{marked}");
        let plain = mark_inline("no marks", "\x1b[38;5;203m");
        assert_eq!(plain, "no marks");
    }

    #[test]
    fn a_reply_marks_every_command_link_and_bullet() {
        let base = "\x1b[38;5;203m";
        let line = "или `not sent` с причиной. Команды `/model`, `/effort` и [стол](files).";
        let marked = mark_inline(line, base);
        assert!(marked.contains("\x1b[1;38;5;51mnot sent"), "{marked}");
        assert!(!marked.contains("`not sent`"), "{marked}");
        for piece in ["/model", "/effort"] {
            assert!(
                marked.contains(&format!("\x1b[1;38;5;51m{piece}")),
                "{marked}"
            );
        }
        assert!(marked.contains("\x1b[4;38;5;51mстол"), "{marked}");
        assert!(marked.contains("files"), "{marked}");
        assert!(!marked.contains("[стол]"), "{marked}");
        let bullet = super::mark_speech("- `files` и `edit` остаются", base);
        assert!(bullet.contains("\x1b[38;5;220m•"), "{bullet}");
        assert!(bullet.contains("\x1b[1;38;5;51mfiles"), "{bullet}");
        assert!(bullet.contains("\x1b[1;38;5;51medit"), "{bullet}");
        let numbered = super::mark_speech("2. `split` открывает оболочку", base);
        assert!(numbered.starts_with("\x1b[38;5;220m2."), "{numbered}");
        assert!(numbered.contains("\x1b[1;38;5;51msplit"), "{numbered}");
    }

    #[test]
    fn a_sent_path_keeps_its_filename_and_uses_gold_on_cyan() {
        let marked = mark_inline(
            r"C:\Users\SergiiZiborov\Pictures\Screenshots\Screenshot 2026-11-09 165812.png в чат",
            "\x1b[38;5;51m",
        );
        assert!(
            marked.contains(
                "\x1b[38;5;220mC:\\Users\\SergiiZiborov\\Pictures\\Screenshots\\Screenshot 2026-11-09 165812.png"
            ),
            "{marked}"
        );
        assert!(marked.contains("в чат"), "{marked}");
        assert!(
            marked.contains("\x1b[38;5;51m в чат") || marked.contains("\x1b[38;5;51mв чат"),
            "{marked}"
        );
    }

    #[test]
    fn wrapped_inline_code_keeps_its_color() {
        let marked = mark_inline("please `kubectl get pods` now", "\x1b[38;5;203m");
        let parts = wrap_words(&marked, 16);
        assert!(parts.len() > 1, "{parts:?}");
        assert!(
            parts.iter().any(|part| part.contains("38;5;51mkubectl")),
            "{parts:?}"
        );
        assert!(
            parts
                .iter()
                .filter(|part| part.contains("pods"))
                .all(|part| part.contains("38;5;51")),
            "{parts:?}"
        );
    }

    #[test]
    fn a_memory_citation_hides_the_tags_and_keeps_the_note() {
        let lines = vec![
            "assistant  проверка закончена".into(),
            "<oai-mem-citation>".into(),
            "<citation_entries>".into(),
            "MEMORY.md:115-116|note=[identified inherited authorization risks and".into(),
            "reverified locally]".into(),
            "MEMORY.md:293-294|note=[dependency audit baseline checked against current packages]".into(),
            "</citation_entries>".into(),
            "<rollout_ids>".into(),
            "01a197fc-ad40-7c83-bbcc-3c9883fb3a4e".into(),
            "</rollout_ids>".into(),
            "</oai-mem-citation>".into(),
            "assistant  дальше".into(),
        ];
        let shown = super::present_pane(&lines);
        let joined = shown.join("\n");
        assert!(joined.contains("assistant  проверка закончена"), "{joined}");
        assert!(joined.contains("assistant  дальше"), "{joined}");
        assert!(
            joined.contains(
                "cite  MEMORY.md:115-116  identified inherited authorization risks and reverified locally"
            ),
            "{joined}"
        );
        assert!(
            joined.contains(
                "cite  MEMORY.md:293-294  dependency audit baseline checked against current packages"
            ),
            "{joined}"
        );
        assert!(
            joined.contains("след  01a197fc-ad40-7c83-bbcc-3c9883fb3a4e"),
            "{joined}"
        );
        assert!(!joined.contains("<oai-mem-citation>"), "{joined}");
        assert!(!joined.contains("|note="), "{joined}");
        assert!(!joined.contains("<citation_entries>"), "{joined}");
    }

    #[test]
    fn a_real_file_with_a_line_number_is_a_span_and_a_url_is_not() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-cite-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("own.js");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        let shown = file.display().to_string().replace('\\', "/");
        let line = format!("см {shown}:81. и https://example.com/Cheat_Sheet.html конец");
        let spans = super::file_spans(&line, &[dir.clone()]);
        assert_eq!(spans.len(), 1, "{spans:?} {line}");
        assert_eq!(spans[0].2.line, 81);
        assert_eq!(spans[0].2.label, "own.js:81");
        assert_eq!(spans[0].2.path, file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bullet_wrap_hangs_and_a_long_path_breaks_on_a_separator() {
        let mut wrapped = Vec::new();
        let line = "user  - Open chat survives tab switches and keeps the project directory open";
        super::push_speech(&mut wrapped, line, "\x1b[38;5;51m", 32);
        assert!(wrapped.len() > 1, "{:?}", wrapped.iter().map(|part| &part.text).collect::<Vec<_>>());
        let second = super::strip_sgr(&wrapped[1].text);
        assert!(second.starts_with("        "), "{second}");
        assert!(super::visible_count(&wrapped[0].text) <= 32, "{:?}", wrapped[0].text);
        assert!(super::visible_count(&wrapped[1].text) <= 32, "{second}");
        let quote = super::mark_speech("> keep the note", "\x1b[38;5;51m");
        assert!(quote.contains("\x1b[38;5;220m│"), "{quote}");
        let parts = wrap_words("abcdef/ghijkl", 8);
        assert!(parts[0].ends_with('/'), "{parts:?}");
        assert_eq!(parts[1], "ghijkl");
    }

    #[test]
    fn message_markup_covers_headings_rules_tables_and_strike() {
        let base = "\x1b[38;5;223m";
        let heading = super::mark_speech("## Scope root", base);
        assert!(heading.contains("\x1b[1;38;5;220mScope root"), "{heading}");
        assert!(!heading.contains('#'), "{heading}");
        let ruled = super::mark_speech("---", base);
        assert!(ruled.contains('─'), "{ruled}");
        assert!(!ruled.contains("---"), "{ruled}");
        let task = super::mark_speech("- [x] ship the desk", base);
        assert!(task.contains('✓'), "{task}");
        assert!(task.contains("ship the desk"), "{task}");
        assert!(!task.contains("[x]"), "{task}");
        let open = super::mark_speech("- [ ] still open", base);
        assert!(open.contains('○'), "{open}");
        assert!(!open.contains("[ ]"), "{open}");
        let struck = mark_inline("keep ~~old line~~ here", base);
        assert!(struck.contains("\x1b[9;38;5;131mold line"), "{struck}");
        assert!(!struck.contains("~~"), "{struck}");
        let both = mark_inline("***both***", base);
        assert!(both.contains("\x1b[1;3;38;5;220mboth"), "{both}");
        assert!(!both.contains("***"), "{both}");
        let table = super::mark_speech("| **name** | `src/ui.rs` |", base);
        assert!(table.contains('│'), "{table}");
        assert!(table.contains("\x1b[1;38;5;220mname"), "{table}");
        assert!(table.contains("\x1b[1;38;5;51msrc/ui.rs"), "{table}");
        assert!(!table.contains("**"), "{table}");
        assert!(!table.contains('`'), "{table}");
        let rule = super::mark_speech("| --- | --- |", base);
        assert!(rule.contains('┄'), "{rule}");
        assert!(!rule.contains("---"), "{rule}");
        assert_eq!(super::fence_lang("```"), "");
        assert_eq!(super::fence_lang("```text"), "");
        assert_eq!(super::fence_lang("```markdown"), "");
        assert_eq!(super::fence_lang("```rust"), "rs");
        assert_eq!(super::fence_lang("```py"), "py");
        let plus = super::mark_speech("+ one more", base);
        assert!(plus.contains("\x1b[38;5;220m•"), "{plus}");
    }

    #[test]
    fn a_url_and_a_loose_path_are_marked_and_a_relative_file_is_a_button() {
        let base = "\x1b[38;5;51m";
        let marked = mark_inline(r"see https://example.com/docs and src\missing.rs end", base);
        assert!(
            marked.contains("\x1b[4;38;5;220mhttps://example.com/docs"),
            "{marked}"
        );
        assert!(marked.contains("\x1b[38;5;220msrc\\missing.rs"), "{marked}");
        assert!(marked.contains("\x1b[38;5;51m end") || marked.ends_with("end"), "{marked}");
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-rel-{n}"));
        let notes = dir.join("notes");
        let plan = dir.join("план");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(&plan).unwrap();
        std::fs::write(notes.join("readme.md"), "x").unwrap();
        std::fs::write(plan.join("браузер.md"), "x").unwrap();
        let line = r#"see notes/readme.md and `план\браузер.md`."#;
        let spans = super::file_spans(line, &[dir.clone()]);
        assert_eq!(spans.len(), 2, "{spans:?} {line}");
        assert_eq!(spans[0].2.label, "readme.md");
        assert_eq!(spans[1].2.label, "браузер.md");
        assert_eq!(spans[1].2.path, plan.join("браузер.md"));
        let chars: Vec<char> = line.chars().collect();
        assert_eq!(chars[spans[1].0], '`');
        assert_eq!(chars[spans[1].1 - 1], '`');
        let _ = std::fs::remove_dir_all(&dir);
    }
}
