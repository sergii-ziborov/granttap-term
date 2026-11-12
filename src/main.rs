mod agents;
mod app;
mod chats;
mod deliver;
mod draw;
mod engine;
mod gitread;
mod health;
mod history;
mod hl;
mod inline;
mod lessons;
mod shell;
mod mcp;
mod mesh;
mod model;
mod page;
mod shot;
mod sixel;
mod skills;
mod store;
mod term;
mod ui;
mod usage;

use std::env;
use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use app::{App, Effect};
use store::user_home;
use term::{Polled, Term};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("mcp") => {
            return match mcp::serve() {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("granttap-term: {err}");
                    ExitCode::from(1)
                }
            };
        }
        Some("--inject") => {
            let Some(pid) = args.next().and_then(|text| text.parse::<u32>().ok()) else {
                eprintln!("granttap-term: inject needs a pid");
                return ExitCode::from(2);
            };
            let mut text = String::new();
            let _ = std::io::stdin().read_to_string(&mut text);
            return match deliver::inject_attached(pid, &text) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::from(1)
                }
            };
        }
        Some("frame") => {
            let Some(dir) = args.next() else {
                eprintln!("granttap-term: frame needs a directory");
                return ExitCode::from(1);
            };
            return match shot::write_all(std::path::Path::new(&dir)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("granttap-term: {err}");
                    ExitCode::from(1)
                }
            };
        }
        _ => {}
    }
    if !Term::tty() {
        eprintln!("granttap-term: run this inside a terminal");
        return ExitCode::from(1);
    }
    let dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = user_home().unwrap_or_else(|| dir.clone());
    let mut app = App::new(dir, home);
    app.host_mcp();
    let mut term = match Term::enter() {
        Ok(term) => term,
        Err(err) => {
            eprintln!("granttap-term: {err}");
            return ExitCode::from(1);
        }
    };
    app.apply_font();
    let mut drawn = String::new();
    let mut drawn_cursor = None;
    loop {
        app.pump();
        let (cols, rows) = Term::size();
        app.set_size(cols, rows);
        if cols == 0 || rows == 0 {
            match term.poll() {
                Ok(Polled::Eof) if !Term::tty() => break,
                Ok(_) => continue,
                Err(_) if Term::tty() => continue,
                Err(_) => break,
            }
        }
        let screen = ui::Screen::paint(&app, cols, rows);
        app.keep_rows(screen.plain_rows());
        if screen.frame != drawn || screen.cursor != drawn_cursor {
            match term.present(&screen.frame, screen.cursor) {
                Ok(()) => {
                    drawn = screen.frame.clone();
                    drawn_cursor = screen.cursor;
                }
                Err(err) => {
                    let kind = err.kind();
                    let soft = kind == ErrorKind::Interrupted || kind == ErrorKind::WouldBlock;
                    if !soft && !Term::tty() {
                        break;
                    }
                }
            }
        }
        if app.browser_pending() {
            app.run_browser_pending();
            continue;
        }
        app.release_press();
        let polled = match term.poll() {
            Ok(polled) => polled,
            Err(_) if Term::tty() => continue,
            Err(_) => break,
        };
        let Polled::Keys(keys) = polled else {
            if Term::tty() {
                continue;
            }
            break;
        };
        if app.apply_keys(&keys, |x, y| screen.spot_at(x, y)) == Effect::Quit {
            break;
        }
    }
    ExitCode::SUCCESS
}
