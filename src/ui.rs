use crate::app::App;
use crate::model::{AgentKind, Section};

const RED: &str = "\x1b[38;5;196m";
const CYAN: &str = "\x1b[38;5;51m";
const GOLD: &str = "\x1b[38;5;220m";
const SALMON: &str = "\x1b[38;5;203m";
const DIM: &str = "\x1b[38;5;131m";
const CODE: &str = "\x1b[38;5;114m";
const RULE: &str = "\x1b[38;5;160m";
const KEEP: &str = "\x1b[0;48;5;0m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Tab(Section),
    Row(usize),
    Ask(usize),
    Cmd,
    Desc,
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
}

impl Screen {
    pub fn paint(app: &App, cols: usize, rows: usize) -> Self {
        let mut screen = Self {
            frame: String::new(),
            cursor: None,
            hits: Vec::new(),
        };
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
        let typing = (app.mode_is_desk() || app.fill) && !app.cmd.is_empty();
        let suggestions = if typing {
            app.suggestions()
        } else {
            Vec::new()
        };
        let suggest_n = suggestions.len().min(3).min(desc_y.saturating_sub(3));
        let rule_y = desc_y - suggest_n - 1;
        let list_top = if app.fill { 1 } else { 2 };
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
        let hist_room = rule_y.saturating_sub(list_top + ask_n).saturating_sub(1);
        let hist_n = history.len().min(2).min(hist_room);
        let body_end = rule_y.saturating_sub(hist_n);
        if app.fill {
            screen.line(0, cols, &app.fill_bar(), RED);
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
        let rule = "-".repeat(cols.saturating_sub(1));
        screen.line(rule_y, cols, &rule, RULE);
        for (index, suggestion) in suggestions.iter().take(suggest_n).enumerate() {
            let y = rule_y + 1 + index;
            let mark = if index == app.suggest { '>' } else { ' ' };
            screen.line(y, cols, &format!("{mark} {suggestion}"), CYAN);
        }
        let desc = if app.desc_open {
            app.description_body()
        } else {
            app.description_line()
        };
        screen.line(desc_y, cols, &desc, DIM);
        screen.hit(0, desc_y, cols.saturating_sub(1), Spot::Desc);
        screen.composer(app, cols, input_y);
        screen.tabs(app.section, cols, tab_y);
        screen.overlay(app);
        screen
    }

    fn composer(&mut self, app: &App, cols: usize, y: usize) {
        let (agent, status) = app.composer_status();
        let label = agent.label();
        let status_w = label.chars().count() + 1 + status.chars().count();
        let at = cols.saturating_sub(status_w + 1);
        let text_end = at.saturating_sub(1).max(4);
        let room = text_end.saturating_sub(2).max(1);
        let typed = if app.cmd.is_empty() {
            format!("\x1b[38;5;51m❯\x1b[38;5;131m {}", app.prompt_hint())
        } else {
            let shown = clip_chars(&app.cmd, room);
            format!("\x1b[38;5;51m❯\x1b[38;5;203m {shown}")
        };
        self.line(y, cols, &typed, SALMON);
        if at > 6 {
            self.put(at, y, cols, label, agent_color(agent));
            let rest_x = at + label.chars().count() + 1;
            self.put(rest_x, y, cols, &status, DIM);
        }
        self.hit(0, y, cols.saturating_sub(1), Spot::Cmd);
        if app.mode_is_desk() || app.fill {
            let col = (3 + app.cmd.chars().count()).min(text_end.max(3));
            self.cursor = Some((col, y + 1));
        }
    }

    fn chrome(&mut self, app: &App, cols: usize) {
        self.line(0, cols, "", DIM);
        self.put(0, 0, cols, "GrantTap Term", CYAN);
        let section = app.section_title();
        self.put(15, 0, cols, &section, RED);
        let accept = format!("accept {}", app.accept_label());
        let at = cols.saturating_sub(accept.chars().count() + 1);
        if at > 15 + section.chars().count() {
            self.put(at, 0, cols, &accept, DIM);
        }
        let rule = "-".repeat(cols.saturating_sub(1));
        self.line(1, cols, &rule, RULE);
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

    fn body(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
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
        let start = if app.mode_is_desk() {
            selected_at.saturating_sub(room.saturating_sub(1))
        } else {
            app.scroll.min(lines.len().saturating_sub(room.max(1)))
        };
        let side = app.side_lines();
        let split = !side.is_empty();
        for (index, line) in lines.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let selected = app.mode_is_desk() && index == selected_at;
            let mark = if selected { '>' } else { ' ' };
            let shown = if selected || split {
                strip_sgr(line)
            } else {
                line.clone()
            };
            let shown = if split {
                clip_chars(&shown, cols / 2 - 4)
            } else {
                shown
            };
            let ink = if selected { GOLD } else { SALMON };
            self.line(y, cols, &format!("{mark} {shown}"), ink);
            if let Some(name) = side.get(index) {
                self.put(cols / 2, y, cols, &clip_chars(name, 36), CYAN);
            }
            self.hit(0, y, cols.saturating_sub(1), Spot::Row(index));
        }
    }

    fn paint_columns(&mut self, app: &App, cols: usize, y0: usize, y1: usize, pane: &[String]) {
        let room = y1.saturating_sub(y0);
        if room == 0 {
            return;
        }
        let left_max = cols.saturating_sub(40);
        let left = if cols < 72 || left_max < 36 {
            cols
        } else {
            (cols / 2).clamp(36, left_max)
        };
        let list = app.rows_for(left.saturating_sub(3));
        let selected_at = app.focus_index();
        let start = selected_at.saturating_sub(room.saturating_sub(1));
        for (index, line) in list.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let selected = index == selected_at;
            let mark = if selected { '>' } else { ' ' };
            let shown = if selected {
                strip_sgr(line)
            } else {
                line.clone()
            };
            let ink = if selected { GOLD } else { SALMON };
            self.span(0, y, left.min(cols), &format!("{mark} {shown}"), ink);
            self.hit(0, y, left.saturating_sub(1), Spot::Row(index));
        }
        if left >= cols.saturating_sub(8) {
            return;
        }
        for y in y0..y1 {
            self.span(left, y, left + 2, "|", RULE);
        }
        self.paint_transcript(app, left + 1, cols, y0, y1, pane);
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
        let room = y1.saturating_sub(y0);
        if room == 0 || x >= end {
            return;
        }
        let width = end.saturating_sub(x + 1).max(8);
        let mut wrapped = Vec::new();
        let mut speaker = SALMON;
        let mut code = false;
        for line in pane {
            let plain = strip_sgr(line);
            let body = plain
                .strip_prefix("user  ")
                .or_else(|| plain.strip_prefix("assistant  "))
                .unwrap_or(plain.as_str());
            let fence = body.trim_start().starts_with("```");
            let shown_code = code || fence;
            if fence {
                code = !code;
            }
            let ink = if plain.starts_with("tool  ") {
                DIM
            } else if shown_code {
                CODE
            } else if body.trim_start().starts_with('#') {
                GOLD
            } else if plain.starts_with("user  ") {
                speaker = CYAN;
                CYAN
            } else if plain.starts_with("assistant  ") {
                speaker = SALMON;
                SALMON
            } else if plain.is_empty() {
                DIM
            } else {
                speaker
            };
            if plain.starts_with("user  ") {
                speaker = CYAN;
            } else if plain.starts_with("assistant  ") {
                speaker = SALMON;
            }
            if plain.is_empty() {
                wrapped.push((String::new(), ink));
                continue;
            }
            for part in wrap_words(&plain, width) {
                wrapped.push((part, ink));
            }
        }
        if wrapped.is_empty() {
            return;
        }
        let pin = usize::from(wrapped.len() > room);
        if pin == 1 {
            self.span(x, y0, end, &wrapped[0].0, wrapped[0].1);
        }
        let body = if pin == 1 {
            &wrapped[1..]
        } else {
            &wrapped[..]
        };
        let visible = room - pin;
        let max_start = body.len().saturating_sub(visible);
        let start = max_start.saturating_sub(app.pane_from_end);
        for (index, (line, ink)) in body.iter().skip(start).take(visible).enumerate() {
            self.span(x, y0 + pin + index, end, line, ink);
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
            if count + 1 >= end {
                break;
            }
            self.frame.push(ch);
            count += 1;
        }
        while count + 1 < end {
            self.frame.push(' ');
            count += 1;
        }
        self.frame.push_str(KEEP);
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
            count += 1;
        }
        while count + 1 < cols {
            self.frame.push(' ');
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
    }

    fn hit(&mut self, x: usize, y: usize, w: usize, spot: Spot) {
        if w > 0 {
            self.hits.push(Hit { x, y, w, spot });
        }
    }
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.chars().count() <= width {
            out.push(rest.to_string());
            break;
        }
        let (split_at, hard_len) = {
            let (window, _) = split_chars(rest, width);
            let lead = window.chars().take_while(|ch| ch.is_whitespace()).count();
            let split_at = window
                .char_indices()
                .enumerate()
                .filter(|(count, (_, ch))| *count > lead && ch.is_whitespace())
                .last()
                .map(|(_, (byte, _))| byte);
            (split_at, window.len())
        };
        if let Some(byte) = split_at.filter(|byte| *byte > 0) {
            out.push(rest[..byte].to_string());
            let skip = rest[byte..].chars().next().map(char::len_utf8).unwrap_or(1);
            rest = &rest[byte + skip..];
            continue;
        }
        if hard_len == 0 || hard_len > rest.len() {
            break;
        }
        out.push(rest[..hard_len].to_string());
        rest = &rest[hard_len..];
    }
    if out.is_empty() {
        vec![String::new()]
    } else {
        out
    }
}

fn split_chars(text: &str, width: usize) -> (String, &str) {
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == width {
            return (text[..index].to_string(), &text[index..]);
        }
    }
    (text.to_string(), "")
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

#[cfg(test)]
mod tests {
    use super::wrap_words;

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
}
