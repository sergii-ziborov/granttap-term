use std::io::{self, ErrorKind, Read, Write};

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
    Wheel(i8),
    Page(i8),
    Paste(String),
    Ignore,
}

pub enum Polled {
    Keys(Vec<Key>),
    Eof,
}

pub struct Term {
    original: libc::termios,
    on: bool,
    out: String,
    dec: Decoder,
}

impl Term {
    pub fn tty() -> bool {
        unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
    }

    pub fn enter() -> io::Result<Self> {
        let mut term = Self {
            original: termios_get(0)?,
            on: false,
            out: String::with_capacity(64 * 1024),
            dec: Decoder::default(),
        };
        term.resume()?;
        Ok(term)
    }

    pub fn resume(&mut self) -> io::Result<()> {
        let mut raw = self.original;
        unsafe { libc::cfmakeraw(&mut raw) };
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        termios_set(0, &raw)?;
        self.on = true;
        let mut out = io::stdout().lock();
        out.write_all(
            b"\x1b[?1049h\x1b[?7l\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b]11;#000000\x07\x1b[3J",
        )?;
        out.flush()?;
        Ok(())
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.on {
            return Ok(());
        }
        self.on = false;
        let restored = termios_set(0, &self.original);
        let mut out = io::stdout().lock();
        let written = out
            .write_all(
                b"\x1b[?1000l\x1b[?1006l\x1b[?2004l\x1b]111\x07\x1b[0m\x1b[?25h\x1b[?7h\x1b[?1049l",
            )
            .and_then(|()| out.flush());
        restored.and(written)
    }

    pub fn present(&mut self, frame: &str, cursor: Option<(usize, usize)>) -> io::Result<()> {
        self.out.clear();
        self.out.push_str("\x1b[?25l\x1b[48;5;0m\x1b[2J\x1b[H");
        self.out.push_str(frame);
        if let Some((col, row)) = cursor {
            push_cup(&mut self.out, row, col);
            self.out.push_str("\x1b[?25h");
        }
        let mut out = io::stdout().lock();
        out.write_all(self.out.as_bytes())?;
        out.flush()?;
        Ok(())
    }

    pub fn size() -> (usize, usize) {
        for fd in [1, 0] {
            if let Some(size) = winsize(fd) {
                return (size.0.min(512), size.1.min(256));
            }
        }
        (0, 0)
    }

    pub fn poll(&mut self) -> io::Result<Polled> {
        let wait = if self.dec.pending == [0x1b] { 280 } else { 100 };
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pfd, 1, wait) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted || Self::tty() {
                return Ok(Polled::Keys(self.dec.expire()));
            }
            return Err(err);
        }
        let hup = (pfd.revents & libc::POLLHUP) != 0;
        let readable = (pfd.revents & libc::POLLIN) != 0;
        if !readable {
            if hup && !Self::tty() {
                return Ok(Polled::Eof);
            }
            return Ok(Polled::Keys(self.dec.expire()));
        }
        let mut buf = [0u8; 256];
        let n = match io::stdin().read(&mut buf) {
            Ok(n) => n,
            Err(err)
                if err.kind() == ErrorKind::Interrupted || err.kind() == ErrorKind::WouldBlock =>
            {
                return Ok(Polled::Keys(self.dec.expire()));
            }
            Err(_) if Self::tty() => return Ok(Polled::Keys(self.dec.expire())),
            Err(err) => return Err(err),
        };
        if n == 0 {
            if Self::tty() {
                return Ok(Polled::Keys(Vec::new()));
            }
            return Ok(Polled::Eof);
        }
        Ok(Polled::Keys(self.dec.feed(&buf[..n])))
    }
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
struct Decoder {
    pending: Vec<u8>,
    pending_esc: bool,
    paste: Option<Vec<u8>>,
    utf8: Vec<u8>,
}

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
                self.pending.clear();
                self.pending_esc = false;
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

fn function(byte: u8) -> Option<Key> {
    match byte {
        b'P' => Some(Key::F(1)),
        b'Q' => Some(Key::F(2)),
        b'R' => Some(Key::F(3)),
        b'S' => Some(Key::F(4)),
        _ => None,
    }
}

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

fn mouse_button(button: i32, x: usize, y: usize) -> Key {
    if button & 32 != 0 {
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

fn termios_get(fd: i32) -> io::Result<libc::termios> {
    let mut term = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut term) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(term)
    }
}

fn termios_set(fd: i32, term: &libc::termios) -> io::Result<()> {
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, term) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
        assert_eq!(keys, vec![Key::Wheel(-1)]);
        let keys = dec.feed(b"\x1b[5~");
        assert_eq!(keys, vec![Key::Page(-1)]);
        let keys = dec.feed(b"\x1b[6~");
        assert_eq!(keys, vec![Key::Page(1)]);
        let keys = dec.feed(b"\x1b[200~/tmp/a.png\x1b[201~");
        assert_eq!(keys, vec![Key::Paste("/tmp/a.png".into())]);
        let keys = dec.feed(&[0xD1, 0x8F]);
        assert_eq!(keys, vec![Key::Char('я')]);
    }
}
