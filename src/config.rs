use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse YAML configuration: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

fn default_buffer_size() -> usize {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Telegram Bot API Token from @BotFather
    pub telegram_bot_token: String,

    /// Monitored Telegram channels (usernames, channel IDs, or titles).
    /// If empty or contains "*", all channels the bot is added to will be monitored.
    #[serde(default)]
    pub monitored_channels: Vec<String>,

    /// Size of the in-memory ring buffer (default: 200)
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    /// Default Telegram chat ID or channel username for sending messages.
    /// If omitted, the first entry in monitored_channels is used.
    #[serde(default)]
    pub default_chat_id: Option<String>,
}

impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.telegram_bot_token.trim().is_empty() {
            return Err(ConfigError::Validation(
                "telegram_bot_token must not be empty".to_string(),
            ));
        }

        if self.buffer_size == 0 {
            return Err(ConfigError::Validation(
                "buffer_size must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if a channel post should be accepted based on `monitored_channels`.
    pub fn is_channel_monitored(
        &self,
        chat_id: i64,
        username: Option<&str>,
        title: Option<&str>,
    ) -> bool {
        if self.monitored_channels.is_empty() {
            return true;
        }

        for target in &self.monitored_channels {
            let target_trimmed = target.trim();
            if target_trimmed == "*" {
                return true;
            }

            // Match by ID
            if let Ok(id) = target_trimmed.parse::<i64>() {
                if id == chat_id {
                    return true;
                }
            }

            let clean_target = target_trimmed.trim_start_matches('@').to_lowercase();

            // Match by username
            if let Some(uname) = username {
                if uname.to_lowercase() == clean_target {
                    return true;
                }
            }

            // Match by title
            if let Some(t) = title {
                if t.to_lowercase() == clean_target || t.to_lowercase().contains(&clean_target) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_parsing() {
        let yaml = r#"
telegram_bot_token: "123:ABC"
monitored_channels:
  - "@tech_news"
  - "-1001234567890"
buffer_size: 150
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.telegram_bot_token, "123:ABC");
        assert_eq!(config.monitored_channels.len(), 2);
        assert_eq!(config.buffer_size, 150);
    }

    #[test]
    fn test_is_channel_monitored() {
        let config = Config {
            telegram_bot_token: "test".to_string(),
            monitored_channels: vec!["@tech_news".to_string(), "-100999".to_string()],
            buffer_size: 100,
            default_chat_id: None,
        };

        // Match by username with or without '@'
        assert!(config.is_channel_monitored(1234, Some("tech_news"), Some("Tech News Channel")));
        assert!(config.is_channel_monitored(1234, Some("TECH_NEWS"), None));

        // Match by ID
        assert!(config.is_channel_monitored(-100999, None, Some("Some Channel")));

        // Non-matching
        assert!(!config.is_channel_monitored(5555, Some("other_channel"), Some("Other")));
    }

    #[test]
    fn test_wildcard_channel_monitored() {
        let config = Config {
            telegram_bot_token: "test".to_string(),
            monitored_channels: vec![],
            buffer_size: 100,
            default_chat_id: None,
        };

        // Empty monitored_channels matches everything
        assert!(config.is_channel_monitored(1234, Some("any"), Some("Any Title")));
    }
}
