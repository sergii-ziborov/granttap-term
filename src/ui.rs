use crate::app::App;
use crate::model::Section;

const RED: &str = "\x1b[38;5;196m";
const BLUE: &str = "\x1b[38;5;39m";
const DIM: &str = "\x1b[38;5;67m";
const PLAIN: &str = "\x1b[38;5;153m";
const RESET: &str = "\x1b[0m";
const TAB_ON: &str = "\x1b[1;30;48;5;196m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Tab(Section),
    Row(usize),
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
        if cols < 40 || rows < 12 {
            screen.line(0, cols, "too small   F10 quits", RED);
            return screen;
        }
        if app.fill {
            screen.line(0, cols, &app.fill_bar(), RED);
            screen.body(app, cols, 1, rows);
            return screen;
        }
        let title = format!(
            "GrantTap Term  {}  accept {}",
            app.section.label(),
            app.accept_label()
        );
        screen.line(0, cols, &title, RED);
        let suggestions = app.suggestions();
        let desc_y = rows - 3;
        let input_y = rows - 2;
        let tab_y = rows - 1;
        let suggest_n = suggestions.len().min(3);
        let body_end = desc_y.saturating_sub(suggest_n);
        screen.body(app, cols, 1, body_end);
        for (index, suggestion) in suggestions.iter().take(suggest_n).enumerate() {
            let y = body_end + index;
            let mark = if index == app.suggest { '>' } else { ' ' };
            screen.line(y, cols, &format!("{mark} {suggestion}"), BLUE);
        }
        let desc = if app.desc_open {
            app.description_body()
        } else {
            app.description_line()
        };
        screen.line(desc_y, cols, &desc, DIM);
        screen.hit(0, desc_y, cols, Spot::Desc);
        let input = format!("\x1b[38;5;196m❯\x1b[38;5;39m {}", app.cmd);
        screen.line(input_y, cols, &input, PLAIN);
        screen.hit(0, input_y, cols.saturating_sub(8), Spot::Cmd);
        let vim = if app.vim { "Vim on" } else { "Vim off" };
        screen.put(
            cols.saturating_sub(7),
            input_y,
            vim,
            if app.vim { BLUE } else { DIM },
        );
        if app.note.is_empty() && app.mode_is_desk() {
            screen.cursor = Some((3 + app.cmd.chars().count(), input_y + 1));
        }
        screen.tabs(app.section, cols, tab_y);
        screen
    }

    pub fn spot_at(&self, x: usize, y: usize) -> Option<Spot> {
        self.hits
            .iter()
            .rev()
            .find_map(|hit| (hit.y == y && x >= hit.x && x < hit.x + hit.w).then_some(hit.spot))
    }

    fn body(&mut self, app: &App, cols: usize, y0: usize, y1: usize) {
        let lines = app.lines();
        let room = y1.saturating_sub(y0);
        if lines.is_empty() {
            self.line(y0, cols, app.empty_line(), DIM);
            return;
        }
        let start = if app.mode_is_desk() {
            app.cursor.saturating_sub(room.saturating_sub(1))
        } else {
            app.scroll.min(lines.len().saturating_sub(room.max(1)))
        };
        for (index, line) in lines.iter().enumerate().skip(start).take(room) {
            let y = y0 + index - start;
            let selected = app.mode_is_desk() && index == app.cursor;
            let ink = if selected { RED } else { PLAIN };
            let mark = if selected { '>' } else { ' ' };
            self.line(y, cols, &format!("{mark} {line}"), ink);
            self.hit(0, y, cols, Spot::Row(index));
        }
    }

    fn tabs(&mut self, current: Section, cols: usize, y: usize) {
        self.line(y, cols, "", DIM);
        let mut x = 1usize;
        for section in Section::ALL {
            let label = format!(" {} ", section.label());
            let ink = if section == current { TAB_ON } else { BLUE };
            self.put(x, y, &label, ink);
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
        self.frame.push_str(RESET);
    }

    fn put(&mut self, x: usize, y: usize, text: &str, ink: &str) {
        self.frame
            .push_str(&format!("\x1b[{};{}H{ink}{text}{RESET}", y + 1, x + 1));
    }

    fn hit(&mut self, x: usize, y: usize, w: usize, spot: Spot) {
        if w > 0 {
            self.hits.push(Hit { x, y, w, spot });
        }
    }
}
