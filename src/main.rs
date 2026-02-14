#[macro_use]
mod log;

mod backend;
mod config;
mod jmap;
mod tui;

use config::Config;
use jmap::client::JmapClient;
use std::io::{self, BufRead, Write};
use std::os::unix::io::AsRawFd;
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
    let stdin_fd = io::stdin().as_raw_fd();
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let got_termios = unsafe { libc::tcgetattr(stdin_fd, &mut termios) } == 0;

    if got_termios {
        let mut noecho = termios;
        noecho.c_lflag &= !libc::ECHO;
        unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &noecho) };
    }

    let result = prompt(msg);

    if got_termios {
        unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &termios) };
    }

    eprintln!(); // newline after hidden password input
    result
}

fn main() {
    let config_path = default_config_path();
    let config = match Config::load(&config_path) {
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
    let username = prompt("Username: ");
    let password = prompt_password("Password: ");

    // JMAP session discovery
    eprint!("Connecting to {}...", config.jmap.well_known_url);
    io::stderr().flush().ok();

    let (_session, client) =
        match JmapClient::discover(&config.jmap.well_known_url, &username, &password) {
            Ok(result) => {
                eprintln!(" OK");
                result
            }
            Err(e) => {
                eprintln!(" FAILED");
                eprintln!("JMAP discovery error: {}", e);
                std::process::exit(1);
            }
        };

    // Enter TUI
    if let Err(e) = tui::run(client, config.ui.page_size) {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }
}
