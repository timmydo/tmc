#[macro_use]
mod log;

mod backend;
mod compose;
mod config;
mod jmap;
mod rules;
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
        PathBuf::from(home)
            .join(".config")
            .join("tmc")
            .join("config.toml")
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
    let status = Command::new(&pager).arg(&path).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to launch pager '{}': {}", pager, e);
            std::process::exit(1);
        }
    }
}

fn print_prompt(topic: &str) {
    match topic {
        "config" => {
            let config_path = default_config_path();
            print!(
                r#"I need help generating a configuration file for tmc (Timmy's Mail Console), a terminal email client that connects via JMAP.

The config file goes at: {}

Here is the format:

```toml
[ui]
editor = "nvim"          # optional: editor for composing ($EDITOR fallback)
page_size = 100           # optional: emails per page (default 500)
mouse = true              # optional: enable mouse support (default true)
sync_interval_secs = 60   # optional: background sync interval (default 60, 0 = off)

[mail]
archive_folder = "Archive"  # optional: target folder for 'a' archive action (default "archive")
deleted_folder = "Trash"    # optional: target folder for 'd' delete action (default "trash")

[retention.archive]
folder = "Archive"
days = 365                  # expire mail older than 365 days in Archive when pressing X

[retention.trash]
folder = "Trash"
days = 30                   # expire mail older than 30 days in Trash when pressing X

[account.personal]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "me@example.com"
password_command = "pass show email/example.com"

[account.work]
well_known_url = "https://mx.work.com/.well-known/jmap"
username = "me@work.com"
password_command = "pass show email/work.com"
```

Rules:
- At least one [account.NAME] section is required (or legacy [jmap] with the same three fields).
- `well_known_url`, `username`, and `password_command` are required per account.
- `password_command` is a shell command that prints the password to stdout.
- Quoted strings support \", \\, \n, \t escapes.
- `archive_folder` and `deleted_folder` are mailbox targets for `a` and `d` in list views.
- `[retention.NAME]` sections are optional folder retention policies used by `x` (preview) and `X` (expire) in mailbox view.
- Retention policy fields:
  - `folder` (required): mailbox name, role, or path (e.g. "INBOX/Alerts")
  - `days` (required): positive integer; emails older than this are deleted on `X`.

Please ask me for my email provider, username, and how I store passwords, then generate a config file.
"#,
                config_path.display()
            );
        }
        "rules" => {
            let config_path = default_config_path();
            let rules_path = config_path
                .parent()
                .map(|p| p.join("rules.toml"))
                .unwrap_or_else(|| PathBuf::from("rules.toml"));
            print!(
                r#"I need help generating a rules file for tmc (Timmy's Mail Console), a terminal email client.

The rules file goes at: {}

Here is the format:

```toml
# Simple rule: match a header with a regex, apply actions
[[rule]]
name = "mark newsletters read"
[rule.match]
header = "From"
regex = "newsletter@"
[rule.actions]
mark_read = true

# Compound conditions: all, any, not
[[rule]]
name = "flag urgent from boss"
[rule.match]
all = [
    {{ header = "From", regex = "boss@example\\.com" }},
    {{ header = "Subject", regex = "(?i)urgent" }},
]
[rule.actions]
flag = true

# Move to folder
[[rule]]
name = "move alerts to subfolder"
[rule.match]
header = "Subject"
regex = "\\[ALERT\\]"
[rule.actions]
move_to = "INBOX/Alerts"

# Continue processing allows subsequent rules to also match
[[rule]]
name = "tag and continue"
continue_processing = true
[rule.match]
header = "To"
regex = "dev-team@"
[rule.actions]
flag = true

# Not condition
[[rule]]
name = "mark non-boss read"
[rule.match]
not = {{ header = "From", regex = "boss@" }}
[rule.actions]
mark_read = true
```

Available match headers: From, To, Cc, Reply-To, Subject, Message-ID, plus any custom header (e.g. X-Spam-Score, X-Mailing-List).

Available actions:
- mark_read = true
- mark_unread = true
- flag = true
- unflag = true
- move_to = "MailboxName"  (supports name, role, or path like "INBOX/Sub")
- delete = true  (moves to Trash)

Conditions support: header/regex, all = [...], any = [...], not = {{...}}

By default, only the first matching rule applies per email. Set `continue_processing = true` to allow subsequent rules to also match.

Please ask me what kinds of emails I receive and how I want them organized, then generate a rules file.
"#,
                rules_path.display()
            );
        }
        _ => {
            eprintln!(
                "Unknown prompt topic '{}'. Available topics: config, rules",
                topic
            );
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: tmc [--log] [--prompt=TOPIC]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --log            View the log file in $PAGER");
        eprintln!("  --prompt=TOPIC   Print an AI-friendly prompt (config, rules)");
        eprintln!("  --help           Show this help");
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--log") {
        show_log();
        std::process::exit(0);
    }

    if let Some(prompt_arg) = args.iter().find(|a| a.starts_with("--prompt=")) {
        let topic = &prompt_arg["--prompt=".len()..];
        print_prompt(topic);
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--prompt") {
        eprintln!("Usage: --prompt=TOPIC (available topics: config, rules)");
        std::process::exit(1);
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

    // Load rules (optional — missing file = no rules)
    let rules_path = config_path.parent().unwrap().join("rules.toml");
    let (compiled_rules, custom_headers) = if rules_path.exists() {
        match rules::load_rules(&rules_path) {
            Ok(rules) => {
                let headers = rules::extract_custom_headers(&rules);
                eprintln!(
                    "Loaded {} filtering rule(s){}",
                    rules.len(),
                    if headers.is_empty() {
                        String::new()
                    } else {
                        format!(" ({} custom header(s))", headers.len())
                    }
                );
                (rules, headers)
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to load rules from {}: {}",
                    rules_path.display(),
                    e
                );
                (Vec::new(), Vec::new())
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };

    let first_account = &config.accounts[0];

    // Connect to the first account
    eprint!(
        "Connecting to {} ({})...",
        first_account.name, first_account.well_known_url
    );
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
        config.ui.mouse,
        config.ui.sync_interval_secs,
        config.mail.archive_folder,
        config.mail.deleted_folder,
        config.mail.retention_policies,
        compiled_rules,
        custom_headers,
    ) {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }
}
