use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct Config {
    pub jmap: JmapConfig,
    pub ui: UiConfig,
}

#[derive(Debug)]
pub struct JmapConfig {
    pub well_known_url: String,
}

#[derive(Debug)]
pub struct UiConfig {
    pub editor: Option<String>,
    pub page_size: u32,
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
        let mut well_known_url = None;
        let mut editor = None;
        let mut page_size = 50u32;

        let mut current_section = "";

        for line in contents.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header
            if line.starts_with('[') && line.ends_with(']') {
                current_section = &line[1..line.len() - 1];
                continue;
            }

            // Key = value
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let value = line[eq_pos + 1..].trim();
                // Strip quotes
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(value);

                match (current_section, key) {
                    ("jmap", "well_known_url") => {
                        well_known_url = Some(value.to_string());
                    }
                    ("ui", "editor") => {
                        editor = Some(value.to_string());
                    }
                    ("ui", "page_size") => {
                        page_size = value.parse().unwrap_or(50);
                    }
                    _ => {} // ignore unknown keys
                }
            }
        }

        let well_known_url = well_known_url.ok_or_else(|| {
            ConfigError::Parse("missing [jmap] well_known_url".to_string())
        })?;

        Ok(Config {
            jmap: JmapConfig { well_known_url },
            ui: UiConfig { editor, page_size },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml = r#"
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"

[ui]
# editor = "vim"
page_size = 25
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(
            config.jmap.well_known_url,
            "https://mx.example.com/.well-known/jmap"
        );
        assert_eq!(config.ui.page_size, 25);
        assert!(config.ui.editor.is_none());
    }

    #[test]
    fn test_parse_missing_url() {
        let toml = r#"
[ui]
page_size = 10
"#;
        assert!(Config::parse(toml).is_err());
    }
}
