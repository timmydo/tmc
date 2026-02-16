use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
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
    pub archive_mailbox_id: Option<String>,
    pub deleted_mailbox_id: Option<String>,
    pub rules_mailbox_regex: String,
    pub my_email_regex: String,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    ui: RawUiConfig,
    #[serde(default)]
    mail: RawMailConfig,
    #[serde(default)]
    jmap: Option<RawAccountFields>,
    #[serde(default)]
    account: BTreeMap<String, RawAccountFields>,
    #[serde(default)]
    retention: BTreeMap<String, RawRetentionPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUiConfig {
    #[serde(default)]
    editor: Option<String>,
    #[serde(default = "default_page_size")]
    page_size: u32,
    #[serde(default = "default_mouse")]
    mouse: bool,
    #[serde(default = "default_sync_interval_secs")]
    sync_interval_secs: u64,
}

impl Default for RawUiConfig {
    fn default() -> Self {
        Self {
            editor: None,
            page_size: default_page_size(),
            mouse: default_mouse(),
            sync_interval_secs: default_sync_interval_secs(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMailConfig {
    #[serde(default = "default_archive_folder")]
    archive_folder: String,
    #[serde(default = "default_deleted_folder")]
    deleted_folder: String,
    #[serde(default)]
    archive_mailbox_id: Option<String>,
    #[serde(default)]
    deleted_mailbox_id: Option<String>,
    #[serde(default = "default_rules_mailbox_regex")]
    rules_mailbox_regex: String,
    #[serde(default = "default_my_email_regex")]
    my_email_regex: String,
}

impl Default for RawMailConfig {
    fn default() -> Self {
        Self {
            archive_folder: default_archive_folder(),
            deleted_folder: default_deleted_folder(),
            archive_mailbox_id: None,
            deleted_mailbox_id: None,
            rules_mailbox_regex: default_rules_mailbox_regex(),
            my_email_regex: default_my_email_regex(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAccountFields {
    well_known_url: Option<String>,
    username: Option<String>,
    password_command: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRetentionPolicy {
    folder: Option<String>,
    days: Option<u32>,
}

fn default_page_size() -> u32 {
    500
}

fn default_mouse() -> bool {
    true
}

fn default_sync_interval_secs() -> u64 {
    60
}

fn default_archive_folder() -> String {
    "archive".to_string()
}

fn default_deleted_folder() -> String {
    "trash".to_string()
}

fn default_rules_mailbox_regex() -> String {
    "^INBOX$".to_string()
}

fn default_my_email_regex() -> String {
    "^$".to_string()
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::parse(&contents)
    }

    fn parse(contents: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig =
            toml::from_str(contents).map_err(|e| ConfigError::Parse(e.to_string()))?;

        Regex::new(&raw.mail.rules_mailbox_regex).map_err(|e| {
            ConfigError::Parse(format!(
                "invalid regex '{}' for rules_mailbox_regex: {}",
                raw.mail.rules_mailbox_regex, e
            ))
        })?;
        Regex::new(&raw.mail.my_email_regex).map_err(|e| {
            ConfigError::Parse(format!(
                "invalid regex '{}' for my_email_regex: {}",
                raw.mail.my_email_regex, e
            ))
        })?;

        let mut retention_policies = Vec::new();
        for (name, policy) in raw.retention {
            let folder = policy.folder.ok_or_else(|| {
                ConfigError::Parse(format!("missing folder in [retention.{}]", name))
            })?;
            let days = policy.days.ok_or_else(|| {
                ConfigError::Parse(format!("missing days in [retention.{}]", name))
            })?;
            if days == 0 {
                return Err(ConfigError::Parse(format!(
                    "days must be greater than 0 in [retention.{}]",
                    name
                )));
            }
            retention_policies.push(RetentionPolicyConfig { name, folder, days });
        }

        let mut accounts = Vec::new();
        for (name, account) in raw.account {
            let account_name = name.clone();
            accounts.push(AccountConfig {
                name,
                well_known_url: require_field(
                    account.well_known_url,
                    &format!("missing well_known_url in [account.{}]", account_name),
                )?,
                username: require_field(
                    account.username,
                    &format!("missing username in [account.{}]", account_name),
                )?,
                password_command: require_field(
                    account.password_command,
                    &format!("missing password_command in [account.{}]", account_name),
                )?,
            });
        }

        if accounts.is_empty() {
            let jmap = raw.jmap.ok_or_else(|| {
                ConfigError::Parse(
                    "missing well_known_url (in [jmap] or [account.NAME])".to_string(),
                )
            })?;
            accounts.push(AccountConfig {
                name: "default".to_string(),
                well_known_url: require_field(
                    jmap.well_known_url,
                    "missing well_known_url (in [jmap] or [account.NAME])",
                )?,
                username: require_field(
                    jmap.username,
                    "missing username (in [jmap] or [account.NAME])",
                )?,
                password_command: require_field(
                    jmap.password_command,
                    "missing password_command (in [jmap] or [account.NAME])",
                )?,
            });
        }

        Ok(Config {
            accounts,
            ui: UiConfig {
                editor: raw.ui.editor,
                page_size: raw.ui.page_size,
                mouse: raw.ui.mouse,
                sync_interval_secs: if raw.ui.sync_interval_secs == 0 {
                    None
                } else {
                    Some(raw.ui.sync_interval_secs)
                },
            },
            mail: MailConfig {
                archive_folder: raw.mail.archive_folder,
                deleted_folder: raw.mail.deleted_folder,
                archive_mailbox_id: raw.mail.archive_mailbox_id,
                deleted_mailbox_id: raw.mail.deleted_mailbox_id,
                rules_mailbox_regex: raw.mail.rules_mailbox_regex,
                my_email_regex: raw.mail.my_email_regex,
                retention_policies,
            },
        })
    }
}

fn require_field(value: Option<String>, err: &str) -> Result<String, ConfigError> {
    value.ok_or_else(|| ConfigError::Parse(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let config = Config::parse(
            r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"

[ui]
page_size = 25
"#,
        )
        .unwrap();

        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].name, "default");
        assert_eq!(config.ui.page_size, 25);
        assert_eq!(config.ui.sync_interval_secs, Some(60));
        assert!(config.ui.mouse);
        assert_eq!(config.mail.rules_mailbox_regex, "^INBOX$");
    }

    #[test]
    fn test_parse_multi_account_config() {
        let config = Config::parse(
            r#"
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
"#,
        )
        .unwrap();

        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].name, "personal");
        assert_eq!(config.accounts[1].name, "work");
        assert_eq!(config.ui.editor.as_deref(), Some("nvim"));
    }

    #[test]
    fn test_defaults_and_sync_interval_zero() {
        let config = Config::parse(&jmap_config("[ui]\nsync_interval_secs = 0")).unwrap();
        assert_eq!(config.ui.page_size, 500);
        assert_eq!(config.ui.sync_interval_secs, None);
    }

    #[test]
    fn test_unknown_section_or_key_errors() {
        let err = Config::parse(
            r#"
[bogus]
foo = "bar"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#,
        )
        .unwrap_err();
        match err {
            ConfigError::Parse(msg) => assert!(msg.contains("unknown field"), "got: {}", msg),
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn test_missing_required_account_fields() {
        let err = Config::parse(
            r#"
[account.broken]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
"#,
        )
        .unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("missing password_command"), "got: {}", msg)
            }
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn test_invalid_regex_validation() {
        let err = Config::parse(&jmap_config("[mail]\nrules_mailbox_regex = \"(\"")).unwrap_err();
        match err {
            ConfigError::Parse(msg) => {
                assert!(msg.contains("invalid regex"), "got: {}", msg);
                assert!(msg.contains("rules_mailbox_regex"), "got: {}", msg);
            }
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn test_retention_policies() {
        let config = Config::parse(
            r#"
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
"#,
        )
        .unwrap();

        assert_eq!(config.mail.retention_policies.len(), 2);
        assert_eq!(config.mail.retention_policies[0].name, "archive");
        assert_eq!(config.mail.retention_policies[1].days, 30);
    }

    #[test]
    fn test_mailbox_id_overrides() {
        let config = Config::parse(
            r#"
[mail]
archive_mailbox_id = "mbox-archive"
deleted_mailbox_id = "mbox-trash"

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"
"#,
        )
        .unwrap();

        assert_eq!(
            config.mail.archive_mailbox_id.as_deref(),
            Some("mbox-archive")
        );
        assert_eq!(
            config.mail.deleted_mailbox_id.as_deref(),
            Some("mbox-trash")
        );
    }
}
