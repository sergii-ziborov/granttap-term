const KEYWORD: &str = "\x1b[38;5;196m";
const STRING: &str = "\x1b[38;5;39m";
const COMMENT: &str = "\x1b[38;5;67m";
const INTEGER: &str = "\x1b[38;5;45m";
const TEXT: &str = "\x1b[38;5;203m";
const RESET: &str = "\x1b[0;48;5;0m";

pub fn paint_ext(line: &str, ext: &str) -> String {
    if ext.is_empty() {
        return line.to_string();
    }
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut color = "";
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && hash_comment(ext) {
            emit(&mut out, &mut color, COMMENT, &line[i..]);
            break;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let end = line[i + 2..]
                .find("*/")
                .map(|at| i + 2 + at + 2)
                .unwrap_or(line.len());
            emit(&mut out, &mut color, COMMENT, &line[i..end]);
            i = end;
            continue;
        }
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
            let ink = if is_keyword(word, ext) { KEYWORD } else { TEXT };
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

fn hash_comment(ext: &str) -> bool {
    matches!(
        ext,
        "py" | "sh" | "bash" | "zsh" | "yml" | "yaml" | "toml" | "rb" | "pl" | "r"
    )
}

fn is_keyword(word: &str, ext: &str) -> bool {
    match ext {
        "py" => matches!(
            word,
            "def"
                | "class"
                | "if"
                | "elif"
                | "else"
                | "return"
                | "import"
                | "from"
                | "for"
                | "while"
                | "pass"
                | "break"
                | "continue"
                | "async"
                | "await"
                | "with"
                | "as"
                | "try"
                | "except"
                | "finally"
                | "raise"
                | "lambda"
                | "yield"
                | "True"
                | "False"
                | "None"
                | "and"
                | "or"
                | "not"
                | "in"
                | "is"
        ),
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" => matches!(
            word,
            "function"
                | "const"
                | "let"
                | "var"
                | "if"
                | "else"
                | "return"
                | "import"
                | "from"
                | "export"
                | "class"
                | "extends"
                | "async"
                | "await"
                | "for"
                | "while"
                | "switch"
                | "case"
                | "break"
                | "continue"
                | "new"
                | "this"
                | "typeof"
                | "interface"
                | "type"
        ),
        "go" => matches!(
            word,
            "func"
                | "package"
                | "import"
                | "if"
                | "else"
                | "return"
                | "for"
                | "range"
                | "type"
                | "struct"
                | "interface"
                | "var"
                | "const"
                | "go"
                | "defer"
                | "switch"
                | "case"
                | "break"
                | "continue"
                | "map"
                | "chan"
        ),
        "sh" | "bash" | "zsh" => matches!(
            word,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "while"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "function"
                | "return"
                | "in"
        ),
        "c" | "h" | "cpp" | "hpp" => matches!(
            word,
            "if" | "else"
                | "return"
                | "for"
                | "while"
                | "switch"
                | "case"
                | "break"
                | "continue"
                | "struct"
                | "enum"
                | "typedef"
                | "const"
                | "static"
                | "void"
                | "int"
                | "char"
        ),
        _ => matches!(
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
                | "for"
                | "while"
                | "loop"
                | "break"
                | "continue"
                | "async"
                | "await"
                | "trait"
                | "type"
                | "const"
                | "static"
                | "where"
                | "in"
                | "as"
                | "ref"
                | "move"
                | "self"
                | "Self"
                | "true"
                | "false"
        ),
    }
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
        assert!(paint_ext("// keep the city", "rs").contains("38;5;67"));
    }

    #[test]
    fn string_contains_blue() {
        assert!(paint_ext("\"neon\"", "rs").contains("38;5;39"));
    }

    #[test]
    fn fn_contains_red() {
        assert!(paint_ext("fn main() {}", "rs").contains("38;5;196"));
    }

    #[test]
    fn python_js_and_shell_use_their_own_words() {
        assert!(paint_ext("def run():", "py").contains("38;5;196"));
        assert!(paint_ext("# keep", "py").contains("38;5;67"));
        assert!(paint_ext("function run() {}", "js").contains("38;5;196"));
        assert!(paint_ext("if true; then", "sh").contains("38;5;196"));
        assert!(paint_ext("/* hide */ fn", "rs").contains("38;5;67"));
        assert_eq!(paint_ext("if this is prose", ""), "if this is prose");
    }
}
