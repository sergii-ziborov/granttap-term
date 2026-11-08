mod app;
mod chats;
mod draw;
mod lessons;
mod mcp;
mod mesh;
mod model;
mod term;
mod ui;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use app::{App, Effect};
use term::{Key, Polled, Term};

fn main() -> ExitCode {
    if env::args().skip(1).any(|arg| arg == "mcp") {
        return match mcp::serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("granttap-term: {err}");
                ExitCode::from(1)
            }
        };
    }
    if !Term::tty() {
        eprintln!("granttap-term: run this inside a terminal");
        return ExitCode::from(1);
    }
    let dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.clone());
    let mut app = App::new(dir, home);
    let mut term = match Term::enter() {
        Ok(term) => term,
        Err(err) => {
            eprintln!("granttap-term: {err}");
            return ExitCode::from(1);
        }
    };
    loop {
        let (cols, rows) = Term::size();
        let screen = ui::Screen::paint(&app, cols, rows);
        if term.present(&screen.frame, screen.cursor).is_err() {
            break;
        }
        let polled = match term.poll() {
            Ok(polled) => polled,
            Err(_) => break,
        };
        let Polled::Keys(keys) = polled else {
            break;
        };
        let mut quit = false;
        for key in keys {
            let effect = if let Key::Click { x, y } = key {
                if x == 0 || y == 0 {
                    Effect::None
                } else {
                    match screen.spot_at(x - 1, y - 1) {
                        Some(spot) => app.on_spot(spot),
                        None => Effect::None,
                    }
                }
            } else {
                app.on_key(key)
            };
            match effect {
                Effect::Quit => quit = true,
                Effect::Edit(path) => {
                    let _ = term.suspend();
                    app::edit(&path, &app.dir);
                    let _ = term.resume();
                    app.reload();
                }
                Effect::None => {}
            }
        }
        if quit {
            break;
        }
    }
    ExitCode::SUCCESS
}
