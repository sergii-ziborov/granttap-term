mod agents;
mod app;
mod chats;
mod draw;
mod gitread;
mod health;
mod history;
mod hl;
mod inline;
mod lessons;
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
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::ExitCode;

use app::{App, Effect};
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
        if cols == 0 || rows == 0 {
            match term.poll() {
                Ok(Polled::Eof) if !Term::tty() => break,
                Ok(_) => continue,
                Err(_) if Term::tty() => continue,
                Err(_) => break,
            }
        }
        let screen = ui::Screen::paint(&app, cols, rows);
        if let Err(err) = term.present(&screen.frame, screen.cursor) {
            let kind = err.kind();
            let soft = kind == ErrorKind::Interrupted || kind == ErrorKind::WouldBlock;
            if !soft && !Term::tty() {
                break;
            }
        }
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
