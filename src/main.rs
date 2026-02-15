#[macro_use]
mod log;

mod backend;
mod compose;
mod config;
mod jmap;
mod tui;

use config::{AccountConfig, Config};
use jmap::client::JmapClient;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

fn default_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("tmc").join("config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("tmc").join("config.toml")
    } else {
        PathBuf::from("config.toml")
    }
}

pub fn run_password_command(cmd: &str) -> Result<String, String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| format!("failed to execute password command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "password command exited with {}: {}",
            output.status, stderr
        ));
    }

    let password = String::from_utf8(output.stdout)
        .map_err(|e| format!("password command output is not valid UTF-8: {}", e))?;

    Ok(password.trim_end_matches('\n').to_string())
}

pub fn connect_account(account: &AccountConfig) -> Result<JmapClient, String> {
    let password = run_password_command(&account.password_command)?;
    let (_session, client) =
        JmapClient::discover(&account.well_known_url, &account.username, &password)
            .map_err(|e| format!("JMAP discovery error: {}", e))?;
    Ok(client)
}

fn show_log() {
    let path = log::log_path();
    if !path.exists() {
        eprintln!("No log file found at {}", path.display());
        std::process::exit(1);
    }
    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let status = Command::new(&pager)
        .arg(&path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to launch pager '{}': {}", pager, e);
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: tmc [--log]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --log    View the log file in $PAGER");
        eprintln!("  --help   Show this help");
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--log") {
        show_log();
        std::process::exit(0);
    }

    log::init();

    let config_path = default_config_path();
    let config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config from {}: {}", config_path.display(), e);
            eprintln!("Create a config file with:");
            eprintln!();
            eprintln!("  [account.personal]");
            eprintln!("  well_known_url = \"https://your-server/.well-known/jmap\"");
            eprintln!("  username = \"you@example.com\"");
            eprintln!("  password_command = \"pass show email/example.com\"");
            std::process::exit(1);
        }
    };

    let first_account = &config.accounts[0];

    // Connect to the first account
    eprint!("Connecting to {} ({})...", first_account.name, first_account.well_known_url);
    io::stderr().flush().ok();

    let client = match connect_account(first_account) {
        Ok(client) => {
            eprintln!(" OK");
            client
        }
        Err(e) => {
            eprintln!(" FAILED");
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    // Enter TUI
    if let Err(e) = tui::run(
        client,
        config.accounts,
        0,
        config.ui.page_size,
        config.ui.editor,
    ) {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }
}
