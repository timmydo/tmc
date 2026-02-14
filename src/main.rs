#[macro_use]
mod log;

mod config;
mod jmap;
mod tui;

use config::Config;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

fn default_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("tmc").join("config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("tmc").join("config.toml")
    } else {
        PathBuf::from("config.toml")
    }
}

fn prompt(msg: &str) -> String {
    eprint!("{}", msg);
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).ok();
    line.trim().to_string()
}

fn prompt_password(msg: &str) -> String {
    // TODO: disable echo for password input (termios)
    prompt(msg)
}

fn main() {
    let config_path = default_config_path();
    let _config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config from {}: {}", config_path.display(), e);
            eprintln!("Create a config file with:");
            eprintln!();
            eprintln!("  [jmap]");
            eprintln!("  well_known_url = \"https://your-server/.well-known/jmap\"");
            std::process::exit(1);
        }
    };

    // Credentials are prompted before entering TUI mode (normal terminal)
    let _username = prompt("Username: ");
    let _password = prompt_password("Password: ");

    // TODO: JMAP discovery will happen in Phase 2 when we wire up the backend

    // Enter TUI
    if let Err(e) = tui::run() {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }
}
