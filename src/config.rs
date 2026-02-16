use regex::Regex;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AccountConfig {
    pub name: String,
    pub well_known_url: String,
    pub username: String,
    pub password_command: String,
}

#[derive(Debug)]
pub struct Config {
    pub accounts: Vec<AccountConfig>,
    pub ui: UiConfig,
    pub mail: MailConfig,
}

#[derive(Debug)]
pub struct UiConfig {
    pub editor: Option<String>,
    pub page_size: u32,
    pub mouse: bool,
    pub sync_interval_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RetentionPolicyConfig {
    pub name: String,
    pub folder: String,
    pub days: u32,
}

#[derive(Debug)]
pub struct MailConfig {
    pub archive_folder: String,
    pub deleted_folder: String,
    pub rules_mailbox_regex: String,
    pub retention_policies: Vec<RetentionPolicyConfig>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "failed to read config file: {}", e),
            ConfigError::Parse(e) => write!(f, "failed to parse config file: {}", e),
        }
    }
}

/// Parse a double-quoted string value, processing escape sequences.
/// Input should NOT include the surrounding quotes.
fn parse_escape_sequences(s: &str, line_num: usize) -> Result<String, ConfigError> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some(other) => {
                    return Err(ConfigError::Parse(format!(
                        "line {}: unknown escape sequence '\\{}'",
                        line_num, other
                    )));
                }
                None => {
                    return Err(ConfigError::Parse(format!(
                        "line {}: trailing backslash in string",
                        line_num
                    )));
                }
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}

/// Strip inline comment from an unquoted value.
/// e.g. `some_value # this is a comment` -> `some_value`
fn strip_inline_comment(value: &str) -> &str {
    if let Some(pos) = value.find(" #") {
        value[..pos].trim_end()
    } else {
        value
    }
}

/// Parse the value part of a `key = value` line.
/// Handles quoted strings (with escapes) and unquoted values (with inline comments).
fn parse_value(raw_value: &str, line_num: usize) -> Result<String, ConfigError> {
    let trimmed = raw_value.trim();
    if let Some(inner) = trimmed.strip_prefix('"') {
        // Find the closing quote, respecting escapes
        let mut end = None;
        let mut i = 0;
        let bytes = inner.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2; // skip escaped char
            } else if bytes[i] == b'"' {
                end = Some(i);
                break;
            } else {
                i += 1;
            }
        }
        match end {
            Some(pos) => parse_escape_sequences(&inner[..pos], line_num),
            None => Err(ConfigError::Parse(format!(
                "line {}: unmatched quote in value",
                line_num
            ))),
        }
    } else {
        // Unquoted value: strip inline comments
        Ok(strip_inline_comment(trimmed).to_string())
    }
}

const KNOWN_UI_KEYS: &[&str] = &["editor", "page_size", "mouse", "sync_interval_secs"];
const KNOWN_MAIL_KEYS: &[&str] = &["archive_folder", "deleted_folder", "rules_mailbox_regex"];
const KNOWN_JMAP_KEYS: &[&str] = &["well_known_url", "username", "password_command"];
const KNOWN_ACCOUNT_KEYS: &[&str] = &["well_known_url", "username", "password_command"];
const KNOWN_RETENTION_KEYS: &[&str] = &["folder", "days"];

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::parse(&contents)
    }

    fn parse(contents: &str) -> Result<Self, ConfigError> {
        let mut editor = None;
        let mut page_size = 500u32;
        let mut mouse = true;
        let mut sync_interval_secs = Some(60u64);
        let mut archive_folder = "archive".to_string();
        let mut deleted_folder = "trash".to_string();
        let mut rules_mailbox_regex = "^INBOX$".to_string();

        // Legacy [jmap] fields
        let mut jmap_well_known_url = None;
        let mut jmap_username = None;
        let mut jmap_password_command = None;

        // Named accounts: (name, well_known_url, username, password_command)
        #[allow(clippy::type_complexity)]
        let mut accounts: Vec<(String, Option<String>, Option<String>, Option<String>)> =
            Vec::new();
        let mut retention_policies: Vec<(String, Option<String>, Option<u32>)> = Vec::new();

        let mut current_section = String::new();

        for (line_idx, line) in contents.lines().enumerate() {
            let line_num = line_idx + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_string();
                // Validate section name
                if current_section != "ui"
                    && current_section != "mail"
                    && current_section != "jmap"
                    && !current_section.starts_with("account.")
                    && !current_section.starts_with("retention.")
                {
                    return Err(ConfigError::Parse(format!(
                        "line {}: unknown section [{}]",
                        line_num, current_section
                    )));
                }
                // Pre-create account entry when we see [account.NAME]
                if let Some(name) = current_section.strip_prefix("account.") {
                    if name.is_empty() {
                        return Err(ConfigError::Parse(format!(
                            "line {}: empty account name in [account.]",
                            line_num
                        )));
                    }
                    if accounts.iter().any(|(n, _, _, _)| n == name) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: duplicate account name '{}'",
                            line_num, name
                        )));
                    }
                    accounts.push((name.to_string(), None, None, None));
                }
                // Pre-create retention entry when we see [retention.NAME]
                if let Some(name) = current_section.strip_prefix("retention.") {
                    if name.is_empty() {
                        return Err(ConfigError::Parse(format!(
                            "line {}: empty policy name in [retention.]",
                            line_num
                        )));
                    }
                    if retention_policies.iter().any(|(n, _, _)| n == name) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: duplicate retention policy name '{}'",
                            line_num, name
                        )));
                    }
                    retention_policies.push((name.to_string(), None, None));
                }
                continue;
            }

            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let raw_value = &line[eq_pos + 1..];
                let value = parse_value(raw_value, line_num)?;

                if current_section == "ui" {
                    if !KNOWN_UI_KEYS.contains(&key) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: unknown key '{}' in [ui]",
                            line_num, key
                        )));
                    }
                    match key {
                        "editor" => editor = Some(value),
                        "page_size" => {
                            page_size = value.parse().map_err(|_| {
                                ConfigError::Parse(format!(
                                    "line {}: invalid numeric value '{}' for page_size",
                                    line_num, value
                                ))
                            })?;
                        }
                        "mouse" => {
                            mouse = match value.as_str() {
                                "true" => true,
                                "false" => false,
                                _ => {
                                    return Err(ConfigError::Parse(format!(
                                        "line {}: invalid boolean value '{}' for mouse (expected true or false)",
                                        line_num, value
                                    )));
                                }
                            };
                        }
                        "sync_interval_secs" => {
                            let secs: u64 = value.parse().map_err(|_| {
                                ConfigError::Parse(format!(
                                    "line {}: invalid numeric value '{}' for sync_interval_secs",
                                    line_num, value
                                ))
                            })?;
                            sync_interval_secs = if secs == 0 { None } else { Some(secs) };
                        }
                        _ => {}
                    }
                } else if current_section == "mail" {
                    if !KNOWN_MAIL_KEYS.contains(&key) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: unknown key '{}' in [mail]",
                            line_num, key
                        )));
                    }
                    match key {
                        "archive_folder" => archive_folder = value,
                        "deleted_folder" => deleted_folder = value,
                        "rules_mailbox_regex" => {
                            Regex::new(&value).map_err(|e| {
                                ConfigError::Parse(format!(
                                    "line {}: invalid regex '{}' for rules_mailbox_regex: {}",
                                    line_num, value, e
                                ))
                            })?;
                            rules_mailbox_regex = value;
                        }
                        _ => {}
                    }
                } else if current_section == "jmap" {
                    if !KNOWN_JMAP_KEYS.contains(&key) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: unknown key '{}' in [jmap]",
                            line_num, key
                        )));
                    }
                    match key {
                        "well_known_url" => jmap_well_known_url = Some(value),
                        "username" => jmap_username = Some(value),
                        "password_command" => jmap_password_command = Some(value),
                        _ => {}
                    }
                } else if let Some(name) = current_section.strip_prefix("account.") {
                    if !KNOWN_ACCOUNT_KEYS.contains(&key) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: unknown key '{}' in [account.{}]",
                            line_num, key, name
                        )));
                    }
                    if let Some(acct) = accounts.iter_mut().find(|(n, _, _, _)| n == name) {
                        match key {
                            "well_known_url" => acct.1 = Some(value),
                            "username" => acct.2 = Some(value),
                            "password_command" => acct.3 = Some(value),
                            _ => {}
                        }
                    }
                } else if let Some(name) = current_section.strip_prefix("retention.") {
                    if !KNOWN_RETENTION_KEYS.contains(&key) {
                        return Err(ConfigError::Parse(format!(
                            "line {}: unknown key '{}' in [retention.{}]",
                            line_num, key, name
                        )));
                    }
                    if let Some(policy) = retention_policies.iter_mut().find(|(n, _, _)| n == name)
                    {
                        match key {
                            "folder" => policy.1 = Some(value),
                            "days" => {
                                let days: u32 = value.parse().map_err(|_| {
                                    ConfigError::Parse(format!(
                                        "line {}: invalid numeric value '{}' for days",
                                        line_num, value
                                    ))
                                })?;
                                if days == 0 {
                                    return Err(ConfigError::Parse(format!(
                                        "line {}: days must be greater than 0 in [retention.{}]",
                                        line_num, name
                                    )));
                                }
                                policy.2 = Some(days);
                            }
                            _ => {}
                        }
                    }
                } else if !current_section.is_empty() {
                    // This shouldn't happen since unknown sections are caught above,
                    // but handle it for safety
                    return Err(ConfigError::Parse(format!(
                        "line {}: unknown section [{}]",
                        line_num, current_section
                    )));
                }
            }
        }

        // Build final retention policy list
        let mut final_retention_policies = Vec::new();
        for (name, folder, days) in retention_policies {
            let folder = folder.ok_or_else(|| {
                ConfigError::Parse(format!("missing folder in [retention.{}]", name))
            })?;
            let days = days.ok_or_else(|| {
                ConfigError::Parse(format!("missing days in [retention.{}]", name))
            })?;
            final_retention_policies.push(RetentionPolicyConfig { name, folder, days });
        }

        // Build final account list
        let mut final_accounts = Vec::new();

        // Named accounts first
        for (name, url, user, pass) in accounts {
            let well_known_url = url.ok_or_else(|| {
                ConfigError::Parse(format!("missing well_known_url in [account.{}]", name))
            })?;
            let username = user.ok_or_else(|| {
                ConfigError::Parse(format!("missing username in [account.{}]", name))
            })?;
            let password_command = pass.ok_or_else(|| {
                ConfigError::Parse(format!("missing password_command in [account.{}]", name))
            })?;
            final_accounts.push(AccountConfig {
                name,
                well_known_url,
                username,
                password_command,
            });
        }

        // Legacy [jmap] fallback
        if final_accounts.is_empty() {
            let well_known_url = jmap_well_known_url.ok_or_else(|| {
                ConfigError::Parse(
                    "missing well_known_url (in [jmap] or [account.NAME])".to_string(),
                )
            })?;
            let username = jmap_username.ok_or_else(|| {
                ConfigError::Parse("missing username (in [jmap] or [account.NAME])".to_string())
            })?;
            let password_command = jmap_password_command.ok_or_else(|| {
                ConfigError::Parse(
                    "missing password_command (in [jmap] or [account.NAME])".to_string(),
                )
            })?;
            final_accounts.push(AccountConfig {
                name: "default".to_string(),
                well_known_url,
                username,
                password_command,
            });
        }

        Ok(Config {
            accounts: final_accounts,
            ui: UiConfig {
                editor,
                page_size,
                mouse,
                sync_interval_secs,
            },
            mail: MailConfig {
                archive_folder,
                deleted_folder,
                rules_mailbox_regex,
                retention_policies: final_retention_policies,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to build a minimal valid config string with [jmap] section
    fn jmap_config(extra_ui: &str) -> String {
        format!(
            r#"
{extra_ui}
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#
        )
    }

    #[test]
    fn test_parse_legacy_jmap_config() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"

[ui]
page_size = 25
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].name, "default");
        assert_eq!(
            config.accounts[0].well_known_url,
            "https://mx.example.com/.well-known/jmap"
        );
        assert_eq!(config.accounts[0].username, "user@example.com");
        assert_eq!(
            config.accounts[0].password_command,
            "pass show email/example.com"
        );
        assert_eq!(config.ui.page_size, 25);
        assert!(config.ui.editor.is_none());
        assert_eq!(config.ui.sync_interval_secs, Some(60));
        assert_eq!(config.mail.archive_folder, "archive");
        assert_eq!(config.mail.deleted_folder, "trash");
        assert_eq!(config.mail.rules_mailbox_regex, "^INBOX$");
        assert!(config.mail.retention_policies.is_empty());
    }

    #[test]
    fn test_parse_multi_account_config() {
        let toml = r#"
[ui]
editor = "nvim"
page_size = 100

[account.personal]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"

[account.work]
well_known_url = "https://mx.work.com/.well-known/jmap"
username = "user@work.com"
password_command = "pass show email/work.com"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].name, "personal");
        assert_eq!(config.accounts[0].username, "user@example.com");
        assert_eq!(config.accounts[1].name, "work");
        assert_eq!(config.accounts[1].username, "user@work.com");
        assert_eq!(config.ui.page_size, 100);
        assert_eq!(config.ui.editor.as_deref(), Some("nvim"));
        assert_eq!(config.ui.sync_interval_secs, Some(60));
        assert_eq!(config.mail.archive_folder, "archive");
        assert_eq!(config.mail.deleted_folder, "trash");
        assert_eq!(config.mail.rules_mailbox_regex, "^INBOX$");
    }

    #[test]
    fn test_default_page_size_is_500() {
        let toml = &jmap_config("");
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.page_size, 500);
        assert_eq!(config.ui.sync_interval_secs, Some(60));
    }

    #[test]
    fn test_mouse_config() {
        let toml = &jmap_config("");
        let config = Config::parse(toml).unwrap();
        assert!(config.ui.mouse); // default is true

        let toml = &jmap_config("[ui]\nmouse = false");
        let config = Config::parse(toml).unwrap();
        assert!(!config.ui.mouse);

        let toml = &jmap_config("[ui]\nmouse = true");
        let config = Config::parse(toml).unwrap();
        assert!(config.ui.mouse);
    }

    #[test]
    fn test_sync_interval_config() {
        let toml = &jmap_config("[ui]\nsync_interval_secs = 180");
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.sync_interval_secs, Some(180));

        let toml = &jmap_config("[ui]\nsync_interval_secs = 0");
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.sync_interval_secs, None);
    }

    #[test]
    fn test_parse_missing_url() {
        let toml = r#"
[ui]
page_size = 10
"#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn test_parse_account_missing_field() {
        let toml = r#"
[account.broken]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
"#;
        assert!(Config::parse(toml).is_err());
    }

    // --- New tests for parser robustness ---

    #[test]
    fn test_escape_sequences_in_quoted_strings() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass \"show\" email\\path"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(
            config.accounts[0].password_command,
            "pass \"show\" email\\path"
        );
    }

    #[test]
    fn test_escape_newline_tab() {
        let toml = r#"
[ui]
editor = "vim\t--noplugin"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "line1\nline2"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.editor.as_deref(), Some("vim\t--noplugin"));
        assert_eq!(config.accounts[0].password_command, "line1\nline2");
    }

    #[test]
    fn test_unmatched_quote_error() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com
username = "user@example.com"
password_command = "pass show email"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("unmatched quote"), "got: {}", msg);
                assert!(msg.contains("line 3"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_unknown_key_in_ui() {
        let toml = &jmap_config("[ui]\nfoo = bar");
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("unknown key 'foo'"), "got: {}", msg);
                assert!(msg.contains("[ui]"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_unknown_key_in_jmap() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
bogus = "nope"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("unknown key 'bogus'"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_unknown_key_in_account() {
        let toml = r#"
[account.test]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
unknown_field = "value"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("unknown key 'unknown_field'"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_unknown_section_error() {
        let toml = r#"
[bogus]
key = "value"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("unknown section [bogus]"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_invalid_boolean_for_mouse() {
        let toml = &jmap_config("[ui]\nmouse = yes");
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("invalid boolean"), "got: {}", msg);
                assert!(msg.contains("mouse"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_invalid_numeric_for_page_size() {
        let toml = &jmap_config("[ui]\npage_size = abc");
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("invalid numeric"), "got: {}", msg);
                assert!(msg.contains("page_size"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_invalid_numeric_for_sync_interval() {
        let toml = &jmap_config("[ui]\nsync_interval_secs = not_a_number");
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("invalid numeric"), "got: {}", msg);
                assert!(msg.contains("sync_interval_secs"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_inline_comments() {
        let toml = r#"
[ui]
page_size = 50 # half of default
mouse = true # enable mouse

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.page_size, 50);
        assert!(config.ui.mouse);
    }

    #[test]
    fn test_inline_comment_not_stripped_in_quoted_string() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email # not a comment"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(
            config.accounts[0].password_command,
            "pass show email # not a comment"
        );
    }

    #[test]
    fn test_duplicate_account_names() {
        let toml = r#"
[account.dup]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"

[account.dup]
well_known_url = "https://mx2.example.com/.well-known/jmap"
username = "user2@example.com"
password_command = "pass show email2"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("duplicate account"), "got: {}", msg);
                assert!(msg.contains("dup"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_empty_account_name() {
        let toml = r#"
[account.]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("empty account name"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_mail_config() {
        let toml = r#"
[mail]
archive_folder = "Archive"
deleted_folder = "Trash"
rules_mailbox_regex = "^INBOX$"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.mail.archive_folder, "Archive");
        assert_eq!(config.mail.deleted_folder, "Trash");
        assert_eq!(config.mail.rules_mailbox_regex, "^INBOX$");
    }

    #[test]
    fn test_rules_mailbox_regex_default_and_override() {
        let toml = &jmap_config("");
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.mail.rules_mailbox_regex, "^INBOX$");

        let toml = &jmap_config("[mail]\nrules_mailbox_regex = \"^(INBOX|Alerts)$\"");
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.mail.rules_mailbox_regex, "^(INBOX|Alerts)$");
    }

    #[test]
    fn test_invalid_rules_mailbox_regex() {
        let toml = &jmap_config("[mail]\nrules_mailbox_regex = \"(\"");
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("invalid regex"), "got: {}", msg);
                assert!(msg.contains("rules_mailbox_regex"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }

    #[test]
    fn test_retention_policies_config() {
        let toml = r#"
[retention.archive]
folder = "Archive"
days = 365

[retention.trash]
folder = "Trash"
days = 30

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.mail.retention_policies.len(), 2);
        assert_eq!(config.mail.retention_policies[0].name, "archive");
        assert_eq!(config.mail.retention_policies[0].folder, "Archive");
        assert_eq!(config.mail.retention_policies[0].days, 365);
        assert_eq!(config.mail.retention_policies[1].name, "trash");
        assert_eq!(config.mail.retention_policies[1].folder, "Trash");
        assert_eq!(config.mail.retention_policies[1].days, 30);
    }

    #[test]
    fn test_retention_policy_missing_days() {
        let toml = r#"
[retention.archive]
folder = "Archive"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#;
        let err = Config::parse(toml).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("missing days"), "got: {}", msg);
            }
            _ => panic!("expected Parse error"),
        }
    }
}
