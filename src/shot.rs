use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::App;
use crate::model::Section;
use crate::term::Key;
use crate::ui::Screen;

const COLS: usize = 96;
const ROWS: usize = 28;

pub fn write_all(out_dir: &Path) -> io::Result<()> {
    let home = PathBuf::from("/tmp/night-home");
    let dir = PathBuf::from("/tmp/night");
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(home.join(".codex"))?;
    fs::create_dir_all(home.join(".claude/projects/demo"))?;
    fs::create_dir_all(home.join(".grok/sessions/work/one"))?;
    fs::create_dir_all(dir.join("src"))?;
    fs::write(
        home.join(".codex/session_index.jsonl"),
        "{\"id\":\"codexdemo01\",\"thread_name\":\"Ship the desk\"}\n",
    )?;
    fs::write(home.join(".claude/projects/demo/claudedemo01.jsonl"), "{}\n")?;
    fs::write(
        home.join(".grok/sessions/work/one/summary.json"),
        "{\"generated_title\":\"Night desk\"}\n",
    )?;
    fs::write(
        dir.join("src/main.rs"),
        "mod app;\nfn main() {\n    let city = \"Night\";\n}\n",
    )?;
    fs::write(dir.join("src/app.rs"), "pub fn run() {}\nstruct Desk {}\n")?;
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"desk\"\n\n[dependencies]\nlibc = \"0.2\"\n")?;
    fs::write(
        dir.join("page.html"),
        "<title>Night City</title><p>Keep the desk.</p><ul><li>Chats</li><li>Mesh</li></ul>\n",
    )?;
    git(&dir, &["init"])?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "-m", "start the desk"])?;
    fs::create_dir_all(out_dir)?;
    let mut app = App::new(dir.clone(), home.clone());
    app.vim = true;
    app.model = "codex".into();
    save(out_dir, "chats", &app)?;
    app.cmd = "task Hold the line".into();
    app.on_key(Key::Enter);
    save(out_dir, "tasks", &app)?;
    app.cursor = 4;
    app.cmd.clear();
    app.on_key(Key::Enter);
    save(out_dir, "task", &app)?;
    app.on_key(Key::Esc);
    app.cmd = "mesh".into();
    app.on_key(Key::Enter);
    save(out_dir, "mesh", &app)?;
    app.cmd = "browse page.html".into();
    app.on_key(Key::Enter);
    save(out_dir, "browser", &app)?;
    app.on_key(Key::Esc);
    app.cmd = "files".into();
    app.on_key(Key::Enter);
    save(out_dir, "files", &app)?;
    open_source(&mut app);
    save(out_dir, "editor", &app)?;
    app.on_key(Key::Esc);
    app.cmd = "lessons".into();
    app.on_key(Key::Enter);
    save(out_dir, "lessons", &app)?;
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> io::Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "dev")
        .env("GIT_AUTHOR_EMAIL", "dev@example.com")
        .env("GIT_COMMITTER_NAME", "dev")
        .env("GIT_COMMITTER_EMAIL", "dev@example.com")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("git failed"))
    }
}

fn open_source(app: &mut App) {
    app.section = Section::Files;
    app.cursor = index_named(&app.dir, "src/").unwrap_or(0);
    app.on_key(Key::Enter);
    app.cursor = index_named(&app.dir, "main.rs").unwrap_or(0);
    app.on_key(Key::Enter);
}

fn index_named(dir: &Path, name: &str) -> Option<usize> {
    crate::chats::files(dir)
        .iter()
        .position(|row| row.name == name)
}

fn save(out_dir: &Path, name: &str, app: &App) -> io::Result<()> {
    let screen = Screen::paint(app, COLS, ROWS);
    fs::write(out_dir.join(format!("{name}.ans")), screen.frame)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn frames_show_the_desk() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("granttap-frames-{n}"));
        write_all(&dir).unwrap();
        let chats = fs::read_to_string(dir.join("chats.ans")).unwrap();
        assert!(chats.contains("GrantTap Term"));
        assert!(chats.contains("Night desk"));
        let editor = fs::read_to_string(dir.join("editor.ans")).unwrap();
        assert!(editor.contains("Night"));
        assert!(editor.contains("38;5;196"));
        let mesh = fs::read_to_string(dir.join("mesh.ans")).unwrap();
        assert!(mesh.contains("libc"));
        let _ = fs::remove_dir_all(dir);
    }
}
