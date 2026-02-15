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
}

#[derive(Debug)]
pub struct UiConfig {
    pub editor: Option<String>,
    pub page_size: u32,
    pub mouse: bool,
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

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::parse(&contents)
    }

    fn parse(contents: &str) -> Result<Self, ConfigError> {
        let mut editor = None;
        let mut page_size = 500u32;
        let mut mouse = true;

        // Legacy [jmap] fields
        let mut jmap_well_known_url = None;
        let mut jmap_username = None;
        let mut jmap_password_command = None;

        // Named accounts: (name, well_known_url, username, password_command)
        #[allow(clippy::type_complexity)]
        let mut accounts: Vec<(String, Option<String>, Option<String>, Option<String>)> =
            Vec::new();

        let mut current_section = String::new();

        for line in contents.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_string();
                // Pre-create account entry when we see [account.NAME]
                if let Some(name) = current_section.strip_prefix("account.") {
                    if !name.is_empty() && !accounts.iter().any(|(n, _, _, _)| n == name) {
                        accounts.push((name.to_string(), None, None, None));
                    }
                }
                continue;
            }

            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let value = line[eq_pos + 1..].trim();
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(value);

                if current_section == "ui" {
                    match key {
                        "editor" => editor = Some(value.to_string()),
                        "page_size" => page_size = value.parse().unwrap_or(500),
                        "mouse" => mouse = value != "false",
                        _ => {}
                    }
                } else if current_section == "jmap" {
                    match key {
                        "well_known_url" => jmap_well_known_url = Some(value.to_string()),
                        "username" => jmap_username = Some(value.to_string()),
                        "password_command" => jmap_password_command = Some(value.to_string()),
                        _ => {}
                    }
                } else if let Some(name) = current_section.strip_prefix("account.") {
                    if let Some(acct) = accounts.iter_mut().find(|(n, _, _, _)| n == name) {
                        match key {
                            "well_known_url" => acct.1 = Some(value.to_string()),
                            "username" => acct.2 = Some(value.to_string()),
                            "password_command" => acct.3 = Some(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
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
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_legacy_jmap_config() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email/example.com"

[ui]
# editor = "vim"
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
    }

    #[test]
    fn test_default_page_size_is_500() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.ui.page_size, 500);
    }

    #[test]
    fn test_mouse_config() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let config = Config::parse(toml).unwrap();
        assert!(config.ui.mouse); // default is true

        let toml_disabled = r#"
[ui]
mouse = false

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let config = Config::parse(toml_disabled).unwrap();
        assert!(!config.ui.mouse);

        let toml_enabled = r#"
[ui]
mouse = true

[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "user@example.com"
password_command = "pass show email"
"#;
        let config = Config::parse(toml_enabled).unwrap();
        assert!(config.ui.mouse);
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
}
