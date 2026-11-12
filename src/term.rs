use std::io;
#[cfg(unix)]
use std::io::{ErrorKind, Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    F(u8),
    CtrlC,
    CtrlN,
    CtrlP,
    CtrlV,
    Click { x: usize, y: usize },
    /// Left button went down. The cell is 1-based, same as Click.
    MouseDown { x: usize, y: usize },
    /// Left button is held and the pointer moved.
    Drag { x: usize, y: usize },
    /// Left button went up.
    Release { x: usize, y: usize },
    /// Mouse moved with the button up. One cell, no click.
    Move { x: usize, y: usize },
    Wheel(i8),
    Page(i8),
    #[cfg_attr(all(windows, not(test)), allow(dead_code))]
    Paste(String),
    Ignore,
}

pub enum Polled {
    Keys(Vec<Key>),
    #[cfg_attr(windows, allow(dead_code))]
    Eof,
}

pub struct Term {
    raw: Raw,
    on: bool,
    out: String,
    dec: Decoder,
}

#[cfg(unix)]
struct Raw {
    original: libc::termios,
}

#[cfg(windows)]
struct Raw {
    input: u32,
    output: u32,
    in_cp: u32,
    out_cp: u32,
}

impl Term {
    pub fn tty() -> bool {
        use std::io::IsTerminal;
        io::stdin().is_terminal() && io::stdout().is_terminal()
    }

    #[cfg(unix)]
    pub fn enter() -> io::Result<Self> {
        let mut term = Self {
            raw: Raw {
                original: termios_get(0)?,
            },
            on: false,
            out: String::with_capacity(64 * 1024),
            dec: Decoder::default(),
        };
        term.resume()?;
        Ok(term)
    }

    #[cfg(windows)]
    pub fn enter() -> io::Result<Self> {
        let mut term = Self {
            raw: console::save()?,
            on: false,
            out: String::with_capacity(64 * 1024),
            dec: Decoder::default(),
        };
        term.resume()?;
        Ok(term)
    }

    #[cfg(unix)]
    pub fn resume(&mut self) -> io::Result<()> {
        let mut raw = self.raw.original;
        unsafe { libc::cfmakeraw(&mut raw) };
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        termios_set(0, &raw)?;
        self.on = true;
        enable_desk()
    }

    #[cfg(windows)]
    pub fn resume(&mut self) -> io::Result<()> {
        console::enable()?;
        self.on = true;
        enable_desk()
    }

    #[cfg(unix)]
    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.on {
            return Ok(());
        }
        self.on = false;
        let restored = termios_set(0, &self.raw.original);
        restored.and(disable_desk())
    }

    #[cfg(windows)]
    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.on {
            return Ok(());
        }
        self.on = false;
        let restored = console::restore(&self.raw);
        restored.and(disable_desk())
    }

    pub fn present(&mut self, frame: &str, cursor: Option<(usize, usize)>) -> io::Result<()> {
        self.out.clear();
        // Keep the drag on the desk. If this lapses, the terminal marks the
        // buffer and a copy takes the whole window.
        self.out.push_str(mouse_tracking_on());
        // No full-screen clear. On a large window that clear flashes before the
        // new frame arrives. Synchronized update keeps the paint in one step.
        self.out.push_str("\x1b[?2026h\x1b[?25l\x1b[H");
        self.out.push_str(frame);
        if let Some((col, row)) = cursor {
            push_cup(&mut self.out, row, col);
            self.out.push_str("\x1b[?25h");
        }
        self.out.push_str("\x1b[?2026l");
        write_out(&self.out)
    }

    pub fn size() -> (usize, usize) {
        #[cfg(unix)]
        {
            for fd in [1, 0] {
                if let Some(size) = winsize(fd) {
                    return (size.0.min(512), size.1.min(256));
                }
            }
        }
        #[cfg(windows)]
        if let Some(size) = console::size() {
            return (size.0.min(512), size.1.min(256));
        }
        (0, 0)
    }

    pub fn poll(&mut self) -> io::Result<Polled> {
        let wait = if self.dec.pending == [0x1b] { 280 } else { 100 };
        #[cfg(unix)]
        {
            return match unix_ready(wait)? {
                Ready::Timeout => Ok(Polled::Keys(self.dec.expire())),
                Ready::Eof => Ok(Polled::Eof),
                Ready::Bytes(buf) => Ok(Polled::Keys(self.dec.feed(&buf))),
            };
        }
        #[cfg(windows)]
        {
            console::poll_keys(wait)
        }
    }
}

/// 1003 is any-event tracking, so a hover reaches the desk. 1000 alone
/// reports presses only. 1002 is motion while a button is down, so a drag
/// stays with the desk instead of marking the terminal buffer. 9999 is
/// unknown to ConPTY, so that bundle is forwarded to Windows Terminal.
/// A plain 1000 stays inside ConPTY, the click selects text, and the next
/// frame write waits until that ends. The known modes are also sent alone:
/// a modern host applies those and may drop the unknown bundle.
fn mouse_tracking_on() -> &'static str {
    "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?1000;1002;1003;1006;9999h"
}

fn mouse_tracking_off() -> &'static str {
    "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1000;1002;1003;1006;9999l"
}

fn enable_desk() -> io::Result<()> {
    write_out(&format!(
        "\x1b[?1049h\x1b[?7l\x1b[?2004h{on}\x1b]11;#000000\x07\x1b[3J",
        on = mouse_tracking_on()
    ))
}

fn disable_desk() -> io::Result<()> {
    write_out(&format!(
        "{off}\x1b[?2004l\x1b]111\x07\x1b[0m\x1b[?25h\x1b[?7h\x1b[?1049l",
        off = mouse_tracking_off()
    ))
}

fn write_out(text: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())?;
        out.flush()?;
        Ok(())
    }
    #[cfg(windows)]
    console::write_text(text)
}

#[cfg(unix)]
enum Ready {
    Timeout,
    Eof,
    Bytes(Vec<u8>),
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}

pub fn push_cup(out: &mut String, row: usize, col: usize) {
    out.push_str(&format!("\x1b[{row};{col}H"));
}

#[derive(Default)]
#[cfg_attr(all(windows, not(test)), allow(dead_code))]
struct Decoder {
    pending: Vec<u8>,
    pending_esc: bool,
    paste: Option<Vec<u8>>,
    utf8: Vec<u8>,
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
impl Decoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Key> {
        let mut keys = Vec::new();
        for byte in bytes {
            self.push(*byte, &mut keys);
        }
        keys
    }

    fn expire(&mut self) -> Vec<Key> {
        if self.pending_esc && self.pending == [0x1b] {
            self.pending.clear();
            self.pending_esc = false;
            vec![Key::Esc]
        } else {
            Vec::new()
        }
    }

    fn push(&mut self, byte: u8, keys: &mut Vec<Key>) {
        if let Some(paste) = &mut self.paste {
            paste.push(byte);
            if paste.ends_with(b"\x1b[201~") {
                let text = String::from_utf8_lossy(&paste[..paste.len() - 6]).into_owned();
                self.paste = None;
                keys.push(Key::Paste(text));
            } else if paste.len() > 64 * 1024 {
                self.paste = None;
            }
            return;
        }
        self.pending.push(byte);
        if self.pending == b"\x1b[200~" {
            self.pending.clear();
            self.pending_esc = false;
            self.paste = Some(Vec::new());
            return;
        }
        if self.pending.first() == Some(&0x1b) {
            self.pending_esc = true;
            if let Some(key) = csi(&self.pending) {
                let hover = wheel_move(&self.pending);
                self.pending.clear();
                self.pending_esc = false;
                if let Some(mov) = hover {
                    keys.push(mov);
                }
                keys.push(key);
            } else if self.pending.len() > 48 || csi_finished(&self.pending) {
                self.pending.clear();
                self.pending_esc = false;
                keys.push(Key::Ignore);
            }
            return;
        }
        self.pending.clear();
        self.pending_esc = false;
        if byte >= 0x80 {
            self.push_utf8(byte, keys);
            return;
        }
        self.utf8.clear();
        match byte {
            0x03 => keys.push(Key::CtrlC),
            0x0e => keys.push(Key::CtrlN),
            0x10 => keys.push(Key::CtrlP),
            0x16 => keys.push(Key::CtrlV),
            0x09 => keys.push(Key::Tab),
            0x0d | 0x0a => keys.push(Key::Enter),
            0x7f | 0x08 => keys.push(Key::Backspace),
            0x1b => {
                self.pending.push(0x1b);
                self.pending_esc = true;
            }
            b if b >= 0x20 => keys.push(Key::Char(b as char)),
            _ => keys.push(Key::Ignore),
        }
    }

    fn push_utf8(&mut self, byte: u8, keys: &mut Vec<Key>) {
        self.utf8.push(byte);
        let need = match self.utf8[0] {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => 0,
        };
        if need == 0 || self.utf8.len() > need {
            self.utf8.clear();
            keys.push(Key::Ignore);
            return;
        }
        if self.utf8.len() < need {
            return;
        }
        let bytes = std::mem::take(&mut self.utf8);
        match String::from_utf8(bytes) {
            Ok(text) => {
                if let Some(ch) = text.chars().next() {
                    keys.push(Key::Char(ch));
                }
            }
            Err(_) => keys.push(Key::Ignore),
        }
    }
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
fn csi_finished(buf: &[u8]) -> bool {
    if buf.len() < 2 || buf[0] != 0x1b {
        return false;
    }
    if buf[1] != b'[' && buf[1] != b'O' {
        return true;
    }
    if buf.starts_with(b"\x1b[M") {
        return buf.len() >= 6;
    }
    match buf[2..].last() {
        Some(b'~' | b'M' | b'm') => true,
        Some(byte) if byte.is_ascii_alphabetic() => true,
        _ => false,
    }
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
fn csi(buf: &[u8]) -> Option<Key> {
    if let Some(rest) = buf.strip_prefix(b"\x1b[M") {
        if rest.len() < 3 {
            return None;
        }
        return Some(mouse_button(
            i32::from(rest[0].saturating_sub(32)),
            usize::from(rest[1].saturating_sub(32)),
            usize::from(rest[2].saturating_sub(32)),
        ));
    }
    if buf == b"\x1b[A" {
        return Some(Key::Up);
    }
    if buf == b"\x1b[B" {
        return Some(Key::Down);
    }
    if buf == b"\x1b[C" {
        return Some(Key::Right);
    }
    if buf == b"\x1b[D" {
        return Some(Key::Left);
    }
    if buf == b"\x1b[H" || buf == b"\x1b[1~" {
        return Some(Key::Home);
    }
    if buf == b"\x1b[F" || buf == b"\x1b[4~" {
        return Some(Key::End);
    }
    if let Some(n) = buf.strip_prefix(b"\x1bO") {
        return function(n.first().copied()?);
    }
    if let Some(rest) = buf.strip_prefix(b"\x1b[") {
        if rest.ends_with(b"~") {
            let digits = std::str::from_utf8(&rest[..rest.len() - 1]).ok()?;
            return match digits {
                "15" => Some(Key::F(5)),
                "17" => Some(Key::F(6)),
                "18" => Some(Key::F(7)),
                "19" => Some(Key::F(8)),
                "20" => Some(Key::F(9)),
                "21" => Some(Key::F(10)),
                "11" => Some(Key::F(1)),
                "12" => Some(Key::F(2)),
                "13" => Some(Key::F(3)),
                "14" => Some(Key::F(4)),
                "5" => Some(Key::Page(-1)),
                "6" => Some(Key::Page(1)),
                _ => None,
            };
        }
        if rest.ends_with(b"M") || rest.ends_with(b"m") {
            return mouse(rest);
        }
    }
    None
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
fn function(byte: u8) -> Option<Key> {
    match byte {
        b'P' => Some(Key::F(1)),
        b'Q' => Some(Key::F(2)),
        b'R' => Some(Key::F(3)),
        b'S' => Some(Key::F(4)),
        _ => None,
    }
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
fn mouse(rest: &[u8]) -> Option<Key> {
    if rest.last() == Some(&b'm') {
        return Some(Key::Ignore);
    }
    let text = std::str::from_utf8(rest).ok()?;
    let body = text.strip_prefix('<')?.strip_suffix('M')?;
    let mut parts = body.split(';');
    let button: i32 = parts.next()?.parse().ok()?;
    let x: usize = parts.next()?.parse().ok()?;
    let y: usize = parts.next()?.parse().ok()?;
    Some(mouse_button(button, x, y))
}

#[cfg_attr(all(windows, not(test)), allow(dead_code))]
/// A wheel report also names the cell, so the list under the pointer can scroll.
fn wheel_move(buf: &[u8]) -> Option<Key> {
    if let Some(rest) = buf.strip_prefix(b"\x1b[M") {
        if rest.len() < 3 {
            return None;
        }
        let button = i32::from(rest[0].saturating_sub(32));
        if button & 64 == 0 {
            return None;
        }
        let x = usize::from(rest[1].saturating_sub(32));
        let y = usize::from(rest[2].saturating_sub(32));
        return (x > 0 && y > 0).then_some(Key::Move { x, y });
    }
    let rest = buf.strip_prefix(b"\x1b[<")?;
    let text = std::str::from_utf8(rest).ok()?;
    let mut parts = text.split(';');
    let button: i32 = parts.next()?.parse().ok()?;
    if button & 64 == 0 {
        return None;
    }
    let x: usize = parts.next()?.trim_end_matches(['M', 'm']).parse().ok()?;
    let y: usize = parts.next()?.trim_end_matches(['M', 'm']).parse().ok()?;
    (x > 0 && y > 0).then_some(Key::Move { x, y })
}

fn mouse_button(button: i32, x: usize, y: usize) -> Key {
    if button & 32 != 0 {
        if x > 0 && y > 0 {
            return Key::Move { x, y };
        }
        return Key::Ignore;
    }
    if button & 64 != 0 {
        let which = button & 3;
        if which <= 1 {
            return Key::Wheel(if which == 0 { -1 } else { 1 });
        }
        return Key::Ignore;
    }
    if button & 3 == 0 && x > 0 && y > 0 {
        return Key::Click { x, y };
    }
    Key::Ignore
}

#[cfg(unix)]
fn unix_ready(wait: i32) -> io::Result<Ready> {
    let mut pfd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let rc = unsafe { libc::poll(&mut pfd, 1, wait) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == ErrorKind::Interrupted || Term::tty() {
            return Ok(Ready::Timeout);
        }
        return Err(err);
    }
    let hup = (pfd.revents & libc::POLLHUP) != 0;
    let readable = (pfd.revents & libc::POLLIN) != 0;
    if !readable {
        if hup && !Term::tty() {
            return Ok(Ready::Eof);
        }
        return Ok(Ready::Timeout);
    }
    let mut buf = [0u8; 256];
    let n = match io::stdin().read(&mut buf) {
        Ok(n) => n,
        Err(err) if err.kind() == ErrorKind::Interrupted || err.kind() == ErrorKind::WouldBlock => {
            return Ok(Ready::Timeout);
        }
        Err(_) if Term::tty() => return Ok(Ready::Timeout),
        Err(err) => return Err(err),
    };
    if n == 0 {
        if Term::tty() {
            return Ok(Ready::Timeout);
        }
        return Ok(Ready::Eof);
    }
    Ok(Ready::Bytes(buf[..n].to_vec()))
}

#[cfg(unix)]
fn winsize(fd: i32) -> Option<(usize, usize)> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } == -1 {
        return None;
    }
    if size.ws_col == 0 || size.ws_row == 0 {
        None
    } else {
        Some((usize::from(size.ws_col), usize::from(size.ws_row)))
    }
}

#[cfg(unix)]
fn termios_get(fd: i32) -> io::Result<libc::termios> {
    let mut term = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut term) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(term)
    }
}

#[cfg(unix)]
fn termios_set(fd: i32, term: &libc::termios) -> io::Result<()> {
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, term) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn desk_input_mode() -> u32 {
    const ENABLE_MOUSE_INPUT: u32 = 0x0010;
    const ENABLE_WINDOW_INPUT: u32 = 0x0008;
    const ENABLE_QUICK_EDIT_MODE: u32 = 0x0040;
    const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;
    (ENABLE_WINDOW_INPUT | ENABLE_MOUSE_INPUT | ENABLE_EXTENDED_FLAGS) & !ENABLE_QUICK_EDIT_MODE
}

#[cfg(any(windows, test))]
fn keys_from_input(
    event_type: u16,
    event: &[u8],
    origin_x: i16,
    origin_y: i16,
    cols: usize,
    rows: usize,
    left_down: &mut bool,
) -> Vec<Key> {
    if event.len() < 16 {
        return Vec::new();
    }
    if event_type == 1 {
        return key_record(event);
    }
    if event_type == 2 {
        return mouse_record(event, origin_x, origin_y, cols, rows, left_down);
    }
    Vec::new()
}

#[cfg(any(windows, test))]
fn key_record(event: &[u8]) -> Vec<Key> {
    let down = u32::from_le_bytes(event[0..4].try_into().unwrap_or([0; 4])) != 0;
    if !down {
        return Vec::new();
    }
    let repeat = u16::from_le_bytes(event[4..6].try_into().unwrap_or([0; 2])).clamp(1, 8);
    let vk = u16::from_le_bytes(event[6..8].try_into().unwrap_or([0; 2]));
    let ch = u16::from_le_bytes(event[10..12].try_into().unwrap_or([0; 2]));
    let mods = u32::from_le_bytes(event[12..16].try_into().unwrap_or([0; 4]));
    let ctrl = mods & (0x0004 | 0x0008) != 0;
    let alt = mods & (0x0001 | 0x0002) != 0;
    let key = if ctrl {
        match vk {
            0x43 => Some(Key::CtrlC),
            0x56 => Some(Key::CtrlV),
            0x4E => Some(Key::CtrlN),
            0x50 => Some(Key::CtrlP),
            _ => match ch {
                0x03 => Some(Key::CtrlC),
                0x16 => Some(Key::CtrlV),
                0x0E => Some(Key::CtrlN),
                0x10 => Some(Key::CtrlP),
                _ => Some(Key::Ignore),
            },
        }
    } else if alt && vk == 0x56 {
        // Windows Terminal consumes Ctrl+V and sends nothing for an image-only clipboard.
        Some(Key::CtrlV)
    } else {
        match vk {
            0x0D => Some(Key::Enter),
            0x08 => Some(Key::Backspace),
            0x09 => Some(Key::Tab),
            0x1B => Some(Key::Esc),
            0x26 => Some(Key::Up),
            0x28 => Some(Key::Down),
            0x27 => Some(Key::Right),
            0x25 => Some(Key::Left),
            0x24 => Some(Key::Home),
            0x23 => Some(Key::End),
            0x21 => Some(Key::Page(-1)),
            0x22 => Some(Key::Page(1)),
            0x70..=0x7B => Some(Key::F((vk - 0x70 + 1) as u8)),
            _ => char::from_u32(u32::from(ch)).filter(|ch| !ch.is_control()).map(Key::Char),
        }
    };
    match key {
        Some(key) => vec![key; usize::from(repeat)],
        None => Vec::new(),
    }
}

#[cfg(any(windows, test))]
fn mouse_record(
    event: &[u8],
    origin_x: i16,
    origin_y: i16,
    cols: usize,
    rows: usize,
    left_down: &mut bool,
) -> Vec<Key> {
    let x = i16::from_le_bytes(event[0..2].try_into().unwrap_or([0; 2]));
    let y = i16::from_le_bytes(event[2..4].try_into().unwrap_or([0; 2]));
    let buttons = u32::from_le_bytes(event[4..8].try_into().unwrap_or([0; 4]));
    let flags = u32::from_le_bytes(event[12..16].try_into().unwrap_or([0; 4]));
    if flags & 0x0004 != 0 {
        let delta = (buttons >> 16) as i16;
        let mut keys = Vec::new();
        if let Some((col, row)) = mouse_cell(x, y, origin_x, origin_y, cols, rows) {
            keys.push(Key::Move { x: col, y: row });
        }
        if delta > 0 {
            keys.push(Key::Wheel(-1));
        } else if delta < 0 {
            keys.push(Key::Wheel(1));
        }
        return keys;
    }
    let left = buttons & 0x0001 != 0;
    if !left {
        let was = *left_down;
        *left_down = false;
        if was && let Some((col, row)) = mouse_cell(x, y, origin_x, origin_y, cols, rows) {
            return vec![Key::Release { x: col, y: row }];
        }
        if flags & 0x0001 != 0
            && let Some((col, row)) = mouse_cell(x, y, origin_x, origin_y, cols, rows)
        {
            return vec![Key::Move { x: col, y: row }];
        }
        return Vec::new();
    }
    // A press often arrives with MOUSE_MOVED set. One rising edge is the button down.
    if *left_down {
        if flags & 0x0001 != 0
            && let Some((col, row)) = mouse_cell(x, y, origin_x, origin_y, cols, rows)
        {
            return vec![Key::Drag { x: col, y: row }];
        }
        return Vec::new();
    }
    *left_down = true;
    match mouse_cell(x, y, origin_x, origin_y, cols, rows) {
        Some((col, row)) => vec![Key::MouseDown { x: col, y: row }],
        None => Vec::new(),
    }
}

#[cfg(any(windows, test))]
fn mouse_cell(
    x: i16,
    y: i16,
    origin_x: i16,
    origin_y: i16,
    cols: usize,
    rows: usize,
) -> Option<(usize, usize)> {
    let col = i32::from(x) - i32::from(origin_x) + 1;
    let row = i32::from(y) - i32::from(origin_y) + 1;
    if in_view(col, row, cols, rows) {
        return Some((col as usize, row as usize));
    }
    // ConPTY can report a viewport cell while the buffer origin is elsewhere.
    let raw_col = i32::from(x) + 1;
    let raw_row = i32::from(y) + 1;
    if (origin_x != 0 || origin_y != 0) && in_view(raw_col, raw_row, cols, rows) {
        return Some((raw_col as usize, raw_row as usize));
    }
    None
}

#[cfg(any(windows, test))]
fn in_view(col: i32, row: i32, cols: usize, rows: usize) -> bool {
    // A missing size used to accept every coordinate. A buffer row in the
    // thousands then copied every painted line, which is the whole desk.
    let cols = if cols == 0 { 512 } else { cols };
    let rows = if rows == 0 { 256 } else { rows };
    col > 0 && row > 0 && col as usize <= cols && row as usize <= rows
}

#[cfg(windows)]
mod console {
    use super::Raw;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(kind: u32) -> *mut core::ffi::c_void;
        fn GetConsoleMode(handle: *mut core::ffi::c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut core::ffi::c_void, mode: u32) -> i32;
        fn GetConsoleCP() -> u32;
        fn GetConsoleOutputCP() -> u32;
        fn SetConsoleCP(code_page: u32) -> i32;
        fn SetConsoleOutputCP(code_page: u32) -> i32;
        fn SetConsoleTitleW(title: *const u16) -> i32;
        fn GetConsoleScreenBufferInfo(
            handle: *mut core::ffi::c_void,
            info: *mut ScreenBuffer,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut core::ffi::c_void, millis: u32) -> u32;
        fn Sleep(millis: u32);
        fn GetNumberOfConsoleInputEvents(handle: *mut core::ffi::c_void, count: *mut u32) -> i32;
        fn ReadConsoleInputW(
            handle: *mut core::ffi::c_void,
            records: *mut InputRecord,
            len: u32,
            read: *mut u32,
        ) -> i32;
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32)
            -> i32;
        fn WriteFile(
            handle: *mut core::ffi::c_void,
            buffer: *const u8,
            len: u32,
            written: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;
        fn GetConsoleSelectionInfo(info: *mut SelectionInfo) -> i32;
        #[cfg_attr(test, allow(dead_code))]
        fn SetCurrentConsoleFontEx(
            handle: *mut core::ffi::c_void,
            maximum: i32,
            info: *mut FontEx,
        ) -> i32;
    }

    const STD_INPUT: u32 = 0xFFFF_FFF6;
    const STD_OUTPUT: u32 = 0xFFFF_FFF5;
    const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;
    const CONSOLE_SELECTION_IN_PROGRESS: u32 = 0x0001;
    static LEFT_DOWN: AtomicBool = AtomicBool::new(false);
    const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const DISABLE_NEWLINE_AUTO_RETURN: u32 = 0x0008;
    const UTF8: u32 = 65001;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;

    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[cfg_attr(test, allow(dead_code))]
    #[repr(C)]
    struct FontEx {
        cb: u32,
        index: u32,
        size: Coord,
        family: u32,
        weight: u32,
        face: [u16; 32],
    }

    #[repr(C)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }

    #[repr(C, align(4))]
    #[derive(Clone, Copy)]
    struct InputRecord {
        event_type: u16,
        _pad: u16,
        event: [u8; 16],
    }

    impl Default for InputRecord {
        fn default() -> Self {
            Self {
                event_type: 0,
                _pad: 0,
                event: [0; 16],
            }
        }
    }

    const _: () = assert!(std::mem::size_of::<InputRecord>() == 20);

    #[repr(C)]
    struct SelectionInfo {
        flags: u32,
        anchor_x: i16,
        anchor_y: i16,
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }

    const _: () = assert!(std::mem::size_of::<SelectionInfo>() == 16);

    #[repr(C)]
    struct ScreenBuffer {
        size: Coord,
        cursor: Coord,
        attributes: u16,
        window: SmallRect,
        maximum: Coord,
    }

    pub fn save() -> io::Result<Raw> {
        let input = handle(STD_INPUT)?;
        let output = handle(STD_OUTPUT)?;
        let mut in_mode = 0u32;
        let mut out_mode = 0u32;
        if unsafe { GetConsoleMode(input, &mut in_mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { GetConsoleMode(output, &mut out_mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Raw {
            input: in_mode,
            output: out_mode,
            in_cp: unsafe { GetConsoleCP() },
            out_cp: unsafe { GetConsoleOutputCP() },
        })
    }

    pub fn enable() -> io::Result<()> {
        let input = handle(STD_INPUT)?;
        let output = handle(STD_OUTPUT)?;
        let mut previous = 0u32;
        if unsafe { GetConsoleMode(input, &mut previous) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let out_mode = ENABLE_PROCESSED_OUTPUT
            | ENABLE_VIRTUAL_TERMINAL_PROCESSING
            | DISABLE_NEWLINE_AUTO_RETURN;
        unsafe {
            SetConsoleCtrlHandler(Some(ignore_ctrl), 1);
        }
        if let Err(err) = apply_input(input) {
            return Err(err);
        }
        if unsafe { SetConsoleMode(output, out_mode) } == 0 {
            let _ = unsafe { SetConsoleMode(input, previous) };
            return Err(io::Error::last_os_error());
        }
        unsafe {
            SetConsoleCP(UTF8);
            SetConsoleOutputCP(UTF8);
        }
        let title: Vec<u16> = "GrantTap Term\0".encode_utf16().collect();
        let _ = unsafe { SetConsoleTitleW(title.as_ptr()) };
        Ok(())
    }

    pub fn restore(raw: &Raw) -> io::Result<()> {
        let input = handle(STD_INPUT)?;
        let output = handle(STD_OUTPUT)?;
        let in_ok = unsafe { SetConsoleMode(input, raw.input) };
        let out_ok = unsafe { SetConsoleMode(output, raw.output) };
        unsafe {
            SetConsoleCP(raw.in_cp);
            SetConsoleOutputCP(raw.out_cp);
        }
        if in_ok == 0 || out_ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn size() -> Option<(usize, usize)> {
        let output = handle(STD_OUTPUT).ok()?;
        let mut info = ScreenBuffer {
            size: Coord { x: 0, y: 0 },
            cursor: Coord { x: 0, y: 0 },
            attributes: 0,
            window: SmallRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            maximum: Coord { x: 0, y: 0 },
        };
        if unsafe { GetConsoleScreenBufferInfo(output, &mut info) } == 0 {
            return None;
        }
        let cols = i32::from(info.window.right) - i32::from(info.window.left) + 1;
        let rows = i32::from(info.window.bottom) - i32::from(info.window.top) + 1;
        if cols <= 0 || rows <= 0 {
            None
        } else {
            Some((cols as usize, rows as usize))
        }
    }

    pub fn poll_keys(wait: i32) -> io::Result<super::Polled> {
        if ensure_input() {
            let _ = super::write_out(super::mouse_tracking_on());
        }
        let input = handle(STD_INPUT)?;
        let rc = unsafe { WaitForSingleObject(input, wait.max(0) as u32) };
        if rc == WAIT_TIMEOUT {
            return Ok(super::Polled::Keys(Vec::new()));
        }
        if rc != WAIT_OBJECT_0 {
            if super::Term::tty() {
                return Ok(super::Polled::Keys(Vec::new()));
            }
            return Err(io::Error::last_os_error());
        }
        let mut pending = 0u32;
        if unsafe { GetNumberOfConsoleInputEvents(input, &mut pending) } == 0 || pending == 0 {
            unsafe { Sleep(wait.max(1) as u32) };
            return Ok(super::Polled::Keys(Vec::new()));
        }
        let mut records = [InputRecord::default(); 32];
        let mut read = 0u32;
        let ok = unsafe {
            ReadConsoleInputW(
                input,
                records.as_mut_ptr(),
                pending.min(records.len() as u32),
                &mut read,
            )
        };
        if ok == 0 {
            if super::Term::tty() {
                return Ok(super::Polled::Keys(Vec::new()));
            }
            return Err(io::Error::last_os_error());
        }
        let (origin_x, origin_y, cols, rows) = origin();
        let mut left_down = LEFT_DOWN.load(Ordering::Relaxed);
        let mut keys = Vec::new();
        for record in records.iter().take(read as usize) {
            keys.extend(super::keys_from_input(
                record.event_type,
                &record.event,
                origin_x,
                origin_y,
                cols,
                rows,
                &mut left_down,
            ));
        }
        LEFT_DOWN.store(left_down, Ordering::Relaxed);
        Ok(super::Polled::Keys(keys))
    }

    pub fn write_text(text: &str) -> io::Result<()> {
        let output = handle(STD_OUTPUT)?;
        let bytes = text.as_bytes();
        let mut offset = 0usize;
        while offset < bytes.len() {
            // A selection makes WriteFile wait until Esc or Enter. Skip the
            // rest of this frame so the desk keeps reading keys.
            if selecting() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "console selection",
                ));
            }
            let chunk = (bytes.len() - offset).min(4096);
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    output,
                    bytes[offset..].as_ptr(),
                    chunk as u32,
                    &mut written,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if written == 0 {
                return Err(io::Error::other("console write stalled"));
            }
            offset += written as usize;
        }
        Ok(())
    }

    fn apply_input(input: *mut core::ffi::c_void) -> io::Result<()> {
        // Extended flags alone first, so the quick-edit bit actually clears.
        if unsafe { SetConsoleMode(input, ENABLE_EXTENDED_FLAGS) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { SetConsoleMode(input, super::desk_input_mode()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn ensure_input() -> bool {
        let Ok(input) = handle(STD_INPUT) else {
            return false;
        };
        let mut mode = 0u32;
        if unsafe { GetConsoleMode(input, &mut mode) } == 0 {
            return false;
        }
        let want = super::desk_input_mode();
        if mode == want {
            return false;
        }
        let mouse_lost = mode & 0x0040 != 0 || mode & 0x0010 == 0;
        let _ = apply_input(input);
        mouse_lost
    }

    fn selecting() -> bool {
        let mut info = SelectionInfo {
            flags: 0,
            anchor_x: 0,
            anchor_y: 0,
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetConsoleSelectionInfo(&mut info) } == 0 {
            return false;
        }
        info.flags & CONSOLE_SELECTION_IN_PROGRESS != 0
    }

    fn origin() -> (i16, i16, usize, usize) {
        let Ok(output) = handle(STD_OUTPUT) else {
            return (0, 0, 0, 0);
        };
        let mut info = ScreenBuffer {
            size: Coord { x: 0, y: 0 },
            cursor: Coord { x: 0, y: 0 },
            attributes: 0,
            window: SmallRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            maximum: Coord { x: 0, y: 0 },
        };
        if unsafe { GetConsoleScreenBufferInfo(output, &mut info) } == 0 {
            (0, 0, 0, 0)
        } else {
            let cols = i32::from(info.window.right) - i32::from(info.window.left) + 1;
            let rows = i32::from(info.window.bottom) - i32::from(info.window.top) + 1;
            (
                info.window.left,
                info.window.top,
                cols.max(0) as usize,
                rows.max(0) as usize,
            )
        }
    }

    unsafe extern "system" fn ignore_ctrl(kind: u32) -> i32 {
        // Ctrl+C and Ctrl+Break stay in the input buffer. Close still exits.
        if kind <= 1 { 1 } else { 0 }
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(super) fn apply_font(face: &str, height: u16) {
        let Ok(output) = handle(STD_OUTPUT) else {
            return;
        };
        let height = height.clamp(8, 32);
        let mut info = FontEx {
            cb: std::mem::size_of::<FontEx>() as u32,
            index: 0,
            size: Coord {
                x: (height / 2) as i16,
                y: height as i16,
            },
            family: 0x36,
            weight: 400,
            face: [0; 32],
        };
        for (slot, unit) in face.encode_utf16().take(31).enumerate() {
            info.face[slot] = unit;
        }
        if unsafe { SetCurrentConsoleFontEx(output, 0, &mut info) } == 0 {
            info.size.x = 0;
            let _ = unsafe { SetCurrentConsoleFontEx(output, 0, &mut info) };
        }
    }

    fn handle(kind: u32) -> io::Result<*mut core::ffi::c_void> {
        let handle = unsafe { GetStdHandle(kind) };
        if handle.is_null() || handle as isize == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(handle)
        }
    }
}

/// Set the console face and its cell height. A missing face leaves the current font.
pub fn apply_font(face: &str, height: u16) {
    if face.is_empty() || height == 0 {
        return;
    }
    #[cfg(all(windows, not(test)))]
    console::apply_font(face, height);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_click_wheel_and_paste() {
        let mut dec = Decoder::default();
        let keys = dec.feed(b"\x1b[<0;4;5M");
        assert_eq!(keys, vec![Key::Click { x: 4, y: 5 }]);
        let keys = dec.feed(b"\x1b[<0;4;5m");
        assert_eq!(keys, vec![Key::Ignore]);
        assert!(dec.expire().is_empty());
        let keys = dec.feed(b"\x1b[<0;4;5M\x1b[<0;4;5m\x1b[<0;4;5M\x1b[<0;4;5m");
        assert_eq!(
            keys.iter()
                .filter(|key| matches!(key, Key::Click { .. }))
                .count(),
            2
        );
        assert!(!keys.contains(&Key::Esc));
        let keys = dec.feed(b"\x1b[M\x20\x24\x25");
        assert_eq!(keys, vec![Key::Click { x: 4, y: 5 }]);
        assert!(dec.feed(b"\x1b").is_empty());
        let keys = dec.feed(b"[<0;9;2M");
        assert_eq!(keys, vec![Key::Click { x: 9, y: 2 }]);
        assert!(dec.feed(b"\x1b").is_empty());
        assert_eq!(dec.expire(), vec![Key::Esc]);
        let keys = dec.feed(b"\x1b[<64;1;1M");
        assert_eq!(
            keys,
            vec![Key::Move { x: 1, y: 1 }, Key::Wheel(-1)]
        );
        let keys = dec.feed(b"\x1b[5~");
        assert_eq!(keys, vec![Key::Page(-1)]);
        let keys = dec.feed(b"\x1b[6~");
        assert_eq!(keys, vec![Key::Page(1)]);
        let keys = dec.feed(b"\x1b[200~/tmp/a.png\x1b[201~");
        assert_eq!(keys, vec![Key::Paste("/tmp/a.png".into())]);
        let keys = dec.feed(&[0xD1, 0x8F]);
        assert_eq!(keys, vec![Key::Char('я')]);
    }

    fn record(event_type: u16, event: &[u8], origin_x: i16, origin_y: i16) -> Vec<Key> {
        let mut down = false;
        keys_from_input(event_type, event, origin_x, origin_y, 0, 0, &mut down)
    }

    #[test]
    fn console_records_are_keys_not_text() {
        let mut enter = [0u8; 16];
        enter[0..4].copy_from_slice(&1u32.to_le_bytes());
        enter[4..6].copy_from_slice(&1u16.to_le_bytes());
        enter[6..8].copy_from_slice(&0x0Du16.to_le_bytes());
        assert_eq!(record(1, &enter, 0, 0), vec![Key::Enter]);
        let mut letter = [0u8; 16];
        letter[0..4].copy_from_slice(&1u32.to_le_bytes());
        letter[4..6].copy_from_slice(&1u16.to_le_bytes());
        letter[6..8].copy_from_slice(&0x41u16.to_le_bytes());
        letter[10..12].copy_from_slice(&u16::from(b'A').to_le_bytes());
        assert_eq!(record(1, &letter, 0, 0), vec![Key::Char('A')]);
        let mut ctrl = letter;
        ctrl[6..8].copy_from_slice(&0x43u16.to_le_bytes());
        ctrl[10..12].copy_from_slice(&3u16.to_le_bytes());
        ctrl[12..16].copy_from_slice(&0x0008u32.to_le_bytes());
        assert_eq!(record(1, &ctrl, 0, 0), vec![Key::CtrlC]);
        let mut alt_v = letter;
        alt_v[6..8].copy_from_slice(&0x56u16.to_le_bytes());
        alt_v[10..12].copy_from_slice(&u16::from(b'v').to_le_bytes());
        alt_v[12..16].copy_from_slice(&0x0002u32.to_le_bytes());
        assert_eq!(record(1, &alt_v, 0, 0), vec![Key::CtrlV]);
        enter[0..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(record(1, &enter, 0, 0).is_empty());
        let mut click = [0u8; 16];
        click[0..2].copy_from_slice(&5i16.to_le_bytes());
        click[2..4].copy_from_slice(&6i16.to_le_bytes());
        click[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(record(2, &click, 1, 2), vec![Key::MouseDown { x: 5, y: 5 }]);
        let mut far = [0u8; 16];
        far[0..2].copy_from_slice(&3i16.to_le_bytes());
        far[2..4].copy_from_slice(&4000i16.to_le_bytes());
        far[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert!(record(2, &far, 0, 0).is_empty());
        let mut down = false;
        assert!(keys_from_input(2, &far, 0, 0, 80, 24, &mut down).is_empty());
        let mut wheel = [0u8; 16];
        wheel[4..8].copy_from_slice(&((120u32) << 16).to_le_bytes());
        wheel[12..16].copy_from_slice(&0x0004u32.to_le_bytes());
        assert_eq!(
            record(2, &wheel, 0, 0),
            vec![Key::Move { x: 1, y: 1 }, Key::Wheel(-1)]
        );
    }

    #[test]
    fn a_press_marked_as_a_move_is_one_click() {
        let mut down = false;
        let mut press = [0u8; 16];
        press[0..2].copy_from_slice(&3i16.to_le_bytes());
        press[2..4].copy_from_slice(&4i16.to_le_bytes());
        press[4..8].copy_from_slice(&1u32.to_le_bytes());
        press[12..16].copy_from_slice(&0x0001u32.to_le_bytes());
        assert_eq!(
            keys_from_input(2, &press, 0, 0, 80, 24, &mut down),
            vec![Key::MouseDown { x: 4, y: 5 }]
        );
        assert_eq!(
            keys_from_input(2, &press, 0, 0, 80, 24, &mut down),
            vec![Key::Drag { x: 4, y: 5 }]
        );
        press[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            keys_from_input(2, &press, 0, 0, 80, 24, &mut down),
            vec![Key::Release { x: 4, y: 5 }]
        );
        assert!(!down);
        press[4..8].copy_from_slice(&1u32.to_le_bytes());
        press[12..16].copy_from_slice(&0x0003u32.to_le_bytes());
        assert_eq!(
            keys_from_input(2, &press, 0, 0, 80, 24, &mut down),
            vec![Key::MouseDown { x: 4, y: 5 }]
        );
        let mut moved = [0u8; 16];
        moved[12..16].copy_from_slice(&0x0001u32.to_le_bytes());
        let mut idle = false;
        assert_eq!(
            keys_from_input(2, &moved, 0, 0, 80, 24, &mut idle),
            vec![Key::Move { x: 1, y: 1 }]
        );
        let mut view = [0u8; 16];
        view[0..2].copy_from_slice(&2i16.to_le_bytes());
        view[2..4].copy_from_slice(&3i16.to_le_bytes());
        view[4..8].copy_from_slice(&1u32.to_le_bytes());
        let mut once = false;
        assert_eq!(
            keys_from_input(2, &view, 40, 10, 80, 24, &mut once),
            vec![Key::MouseDown { x: 3, y: 4 }]
        );
        let mode = desk_input_mode();
        assert_eq!(mode & 0x0040, 0);
        assert_ne!(mode & 0x0010, 0);
        assert_ne!(mode & 0x0080, 0);
        assert_ne!(mode & 0x0008, 0);
        assert_eq!(mode & 0x0001, 0);
        let on = mouse_tracking_on();
        assert!(on.contains("1003"), "{on}");
        assert!(on.contains("9999"), "{on}");
        assert!(mouse_tracking_off().contains("1003"));
    }
}
