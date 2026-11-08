const KEYWORD: &str = "\x1b[38;5;196m";
const STRING: &str = "\x1b[38;5;39m";
const COMMENT: &str = "\x1b[38;5;67m";
const INTEGER: &str = "\x1b[38;5;45m";
const TEXT: &str = "\x1b[38;5;153m";
const RESET: &str = "\x1b[0m";

pub fn paint(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut color = "";
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            emit(&mut out, &mut color, COMMENT, &line[i..]);
            break;
        } else if bytes[i] == b'"' {
            let end = end_string(line, i);
            emit(&mut out, &mut color, STRING, &line[i..end]);
            i = end;
        } else if bytes[i] == b'\'' {
            if let Some(end) = end_char(line, i) {
                emit(&mut out, &mut color, TEXT, &line[i..end]);
                i = end;
            } else {
                let end = end_text(line, i);
                emit(&mut out, &mut color, TEXT, &line[i..end]);
                i = end;
            }
        } else if is_ident_start(bytes[i]) {
            let end = end_ident(bytes, i);
            let word = &line[i..end];
            let ink = if is_keyword(word) { KEYWORD } else { TEXT };
            emit(&mut out, &mut color, ink, word);
            i = end;
        } else if bytes[i].is_ascii_digit() {
            let end = end_integer(bytes, i);
            emit(&mut out, &mut color, INTEGER, &line[i..end]);
            i = end;
        } else {
            let end = end_text(line, i);
            emit(&mut out, &mut color, TEXT, &line[i..end]);
            i = end;
        }
    }
    out.push_str(RESET);
    out
}

fn emit(out: &mut String, color: &mut &str, next: &'static str, text: &str) {
    if text.is_empty() {
        return;
    }
    if *color != next {
        out.push_str(next);
        *color = next;
    }
    out.push_str(text);
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "let"
            | "mut"
            | "pub"
            | "struct"
            | "enum"
            | "impl"
            | "use"
            | "mod"
            | "if"
            | "else"
            | "return"
            | "match"
    )
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn end_ident(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    i
}

fn end_integer(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    while i + 1 < bytes.len() && bytes[i] == b'_' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    i
}

fn end_string(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            i += line[i..].chars().next().unwrap().len_utf8();
            continue;
        }
        if bytes[i] == b'"' {
            return i + 1;
        }
        i += line[i..].chars().next().unwrap().len_utf8();
    }
    i
}

// A quote pair with one character inside is a char, not a string. Lifetimes have no closer.
fn end_char(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'\\' {
        i += 1;
        if i >= bytes.len() {
            return None;
        }
        match bytes[i] {
            b'u' => {
                i += 1;
                if i >= bytes.len() || bytes[i] != b'{' {
                    return None;
                }
                i += 1;
                let hex = i;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                if i == hex || i >= bytes.len() || bytes[i] != b'}' {
                    return None;
                }
                i += 1;
            }
            b'x' => {
                if i + 2 >= bytes.len()
                    || !bytes[i + 1].is_ascii_hexdigit()
                    || !bytes[i + 2].is_ascii_hexdigit()
                {
                    return None;
                }
                i += 3;
            }
            _ => i += line[i..].chars().next()?.len_utf8(),
        }
    } else {
        i += line[i..].chars().next()?.len_utf8();
    }
    if i < bytes.len() && bytes[i] == b'\'' {
        Some(i + 1)
    } else {
        None
    }
}

fn end_text(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start;
    loop {
        i += line[i..].chars().next().unwrap().len_utf8();
        if i >= bytes.len() || starts_token(bytes, i) {
            break;
        }
    }
    i
}

fn starts_token(bytes: &[u8], i: usize) -> bool {
    let byte = bytes[i];
    if byte == b'"' || byte == b'\'' || byte.is_ascii_digit() || is_ident_start(byte) {
        return true;
    }
    byte == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_contains_dim() {
        assert!(paint("// keep the city").contains("38;5;67"));
    }

    #[test]
    fn string_contains_blue() {
        assert!(paint("\"neon\"").contains("38;5;39"));
    }

    #[test]
    fn fn_contains_red() {
        assert!(paint("fn main() {}").contains("38;5;196"));
    }
}
