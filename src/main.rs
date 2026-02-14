#[macro_use]
mod log;

mod config;
mod jmap;

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

    log_info!("tmc starting, JMAP server: {}", config.jmap.well_known_url);

    // Prompt for credentials
    let username = prompt("Username: ");
    let password = prompt_password("Password: ");

    if username.is_empty() || password.is_empty() {
        eprintln!("Username and password are required.");
        std::process::exit(1);
    }

    // Discover JMAP session
    log_info!("Connecting to JMAP server...");
    let (_session, client) =
        match jmap::client::JmapClient::discover(&config.jmap.well_known_url, &username, &password)
        {
            Ok(result) => result,
            Err(e) => {
                log_error!("JMAP discovery failed: {}", e);
                std::process::exit(1);
            }
        };

    log_info!("Connected. Account ID: {}", client.account_id());

    // Fetch mailboxes as a quick test
    match client.get_mailboxes() {
        Ok(mailboxes) => {
            println!("Mailboxes:");
            for mb in &mailboxes {
                println!(
                    "  {} ({}) - {} total, {} unread",
                    mb.name,
                    mb.role.as_deref().unwrap_or("-"),
                    mb.total_emails,
                    mb.unread_emails
                );
            }
        }
        Err(e) => {
            log_error!("Failed to fetch mailboxes: {}", e);
            std::process::exit(1);
        }
    }
}
