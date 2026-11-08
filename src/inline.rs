use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Sixel,
    Kitty,
    Iterm,
}

pub fn detect() -> Option<Proto> {
    detect_from(
        &env::var("TERM").unwrap_or_default(),
        &env::var("TERM_PROGRAM").unwrap_or_default(),
        env::var_os("KITTY_WINDOW_ID").is_some()
            || env::var("TERM").unwrap_or_default().to_ascii_lowercase().contains("kitty"),
        env::var("LC_TERMINAL").unwrap_or_default() == "iTerm2",
    )
}

pub fn detect_from(term: &str, program: &str, kitty: bool, iterm_lc: bool) -> Option<Proto> {
    let term = term.to_ascii_lowercase();
    if kitty || term.contains("kitty") {
        return Some(Proto::Kitty);
    }
    if program == "iTerm.app" || iterm_lc {
        return Some(Proto::Iterm);
    }
    if program == "WezTerm" || term.contains("mlterm") || term.contains("yaft") {
        return Some(Proto::Sixel);
    }
    None
}

pub fn choose(sixel: &str, kitty: &str, iterm: &str) -> String {
    match detect() {
        Some(Proto::Sixel) => sixel.to_string(),
        Some(Proto::Kitty) => kitty.to_string(),
        Some(Proto::Iterm) => iterm.to_string(),
        None => String::new(),
    }
}

pub fn kitty(width: usize, height: usize, rgb: &[u8]) -> String {
    if width == 0 || height == 0 || rgb.len() < width * height * 3 {
        return String::new();
    }
    format!(
        "\u{1b}_Ga=d,d=i,i=1,q=2\u{1b}\\\u{1b}_Ga=T,f=24,s={width},v={height},i=1,q=2;{}\u{1b}\\",
        b64(&rgb[..width * height * 3])
    )
}

pub fn iterm(width: usize, height: usize, rgb: &[u8]) -> String {
    let bmp = bmp(width, height, rgb);
    if bmp.is_empty() {
        return String::new();
    }
    format!(
        "\u{1b}]1337;File=inline=1;width={width}px;height={height}px;preserveAspectRatio=1:{}\u{7}",
        b64(&bmp)
    )
}

fn bmp(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    if width == 0 || height == 0 || rgb.len() < width * height * 3 || width > 512 || height > 512 {
        return Vec::new();
    }
    let row = (width * 3 + 3) & !3;
    let pixels = row * height;
    let offset = 54usize;
    let mut out = vec![0u8; offset + pixels];
    out[0] = b'B';
    out[1] = b'M';
    put_u32(&mut out, 2, (offset + pixels) as u32);
    put_u32(&mut out, 10, offset as u32);
    put_u32(&mut out, 14, 40);
    put_u32(&mut out, 18, width as u32);
    put_u32(&mut out, 22, height as u32);
    out[26] = 1;
    out[28] = 24;
    put_u32(&mut out, 34, pixels as u32);
    for y in 0..height {
        let src_y = height - 1 - y;
        for x in 0..width {
            let src = (src_y * width + x) * 3;
            let dst = offset + y * row + x * 3;
            out[dst] = rgb[src + 2];
            out[dst + 1] = rgb[src + 1];
            out[dst + 2] = rgb[src];
        }
    }
    out
}

fn put_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn b64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let n = u32::from(data[index]) << 16 | u32::from(data[index + 1]) << 8 | u32::from(data[index + 2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    let rest = data.len() - index;
    if rest == 1 {
        let n = u32::from(data[index]) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = u32::from(data[index]) << 16 | u32::from(data[index + 1]) << 8;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_follow_the_terminal_name() {
        assert_eq!(detect_from("xterm-kitty", "", false, false), Some(Proto::Kitty));
        assert_eq!(detect_from("xterm", "iTerm.app", false, false), Some(Proto::Iterm));
        assert_eq!(detect_from("xterm", "WezTerm", false, false), Some(Proto::Sixel));
        assert_eq!(detect_from("xterm-256color", "Apple_Terminal", false, false), None);
    }

    #[test]
    fn kitty_and_iterm_carry_the_same_pixels() {
        let rgb = [255, 0, 0, 0, 0, 255];
        let kitty_text = kitty(2, 1, &rgb);
        assert!(kitty_text.starts_with("\u{1b}_Ga=d"));
        assert!(kitty_text.contains("s=2,v=1"));
        let iterm_text = iterm(2, 1, &rgb);
        assert!(iterm_text.starts_with("\u{1b}]1337;File=inline=1"));
        assert!(iterm_text.contains("width=2px"));
        assert!(!kitty(0, 0, &rgb).contains("s=0"));
    }
}
