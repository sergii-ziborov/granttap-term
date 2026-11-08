use std::io::{self, Read, Write};

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
        out.write_all(b"\x1b[?1049h\x1b[?7l\x1b[?2004h\x1b[?1000h\x1b[?1006h")?;
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
            .write_all(b"\x1b[?1000l\x1b[?1006l\x1b[?2004l\x1b[0m\x1b[?25h\x1b[?7h\x1b[?1049l")
            .and_then(|()| out.flush());
        restored.and(written)
    }

    pub fn present(&mut self, frame: &str, cursor: Option<(usize, usize)>) -> io::Result<()> {
        self.out.clear();
        self.out.push_str("\x1b[?25l");
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
        let wait = if self.dec.pending_esc { 30 } else { 100 };
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pfd, 1, wait) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(Polled::Keys(self.dec.expire()));
            }
            return Err(err);
        }
        let hup = (pfd.revents & libc::POLLHUP) != 0;
        let readable = (pfd.revents & libc::POLLIN) != 0;
        if hup && !readable {
            return Ok(Polled::Eof);
        }
        if !readable {
            return Ok(Polled::Keys(self.dec.expire()));
        }
        let mut buf = [0u8; 256];
        let n = io::stdin().read(&mut buf)?;
        if n == 0 {
            return Ok(Polled::Keys(Vec::new()));
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
            } else if self.pending.len() > 48 {
                self.pending.clear();
                self.pending_esc = false;
            }
            return;
        }
        self.pending.clear();
        self.pending_esc = false;
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
            b => {
                if let Some(ch) = utf8_char(b) {
                    keys.push(Key::Char(ch));
                }
            }
        }
    }
}

fn utf8_char(byte: u8) -> Option<char> {
    if byte.is_ascii() && byte >= 0x20 {
        Some(byte as char)
    } else {
        None
    }
}

fn csi(buf: &[u8]) -> Option<Key> {
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
    if button & 32 != 0 {
        return Some(Key::Ignore);
    }
    if button & 64 != 0 {
        return Some(Key::Wheel(if button & 1 == 0 { -1 } else { 1 }));
    }
    if button & 3 == 0 {
        return Some(Key::Click { x, y });
    }
    Some(Key::Ignore)
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
        let keys = dec.feed(b"\x1b[<64;1;1M");
        assert_eq!(keys, vec![Key::Wheel(-1)]);
        let keys = dec.feed(b"\x1b[200~/tmp/a.png\x1b[201~");
        assert_eq!(keys, vec![Key::Paste("/tmp/a.png".into())]);
    }
}
