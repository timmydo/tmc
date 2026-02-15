#[macro_use]
mod log;

mod backend;
mod compose;
mod config;
mod jmap;
mod tui;

use config::Config;
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

fn run_password_command(cmd: &str) -> Result<String, String> {
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
            eprintln!("  username = \"you@example.com\"");
            eprintln!("  password_command = \"pass show email/example.com\"");
            std::process::exit(1);
        }
    };

    // Get password by running the configured command
    let password = match run_password_command(&config.jmap.password_command) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // JMAP session discovery
    eprint!("Connecting to {}...", config.jmap.well_known_url);
    io::stderr().flush().ok();

    let (_session, client) =
        match JmapClient::discover(&config.jmap.well_known_url, &config.jmap.username, &password) {
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
    if let Err(e) = tui::run(
        client,
        config.ui.page_size,
        config.ui.editor,
        config.jmap.username,
    ) {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }
}
