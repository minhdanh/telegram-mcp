use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a single captured message from a monitored Telegram channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelMessage {
    pub message_id: i32,
    pub channel_id: i64,
    pub channel_username: Option<String>,
    pub channel_title: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub text: String,
}

impl ChannelMessage {
    /// Format the message into a human- and LLM-friendly string.
    pub fn formatted(&self) -> String {
        let channel_display = match (&self.channel_title, &self.channel_username) {
            (Some(title), Some(username)) => format!("{} (@{})", title, username),
            (Some(title), None) => title.clone(),
            (None, Some(username)) => format!("@{}", username),
            (None, None) => format!("ID: {}", self.channel_id),
        };

        format!(
            "[{}] [{}] (msg_id: {})\n{}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            channel_display,
            self.message_id,
            self.text
        )
    }

    /// Check if this message matches a given channel query (by username, channel ID, or title).
    pub fn matches_channel(&self, query: &str) -> bool {
        let query_clean = query.trim().trim_start_matches('@').to_lowercase();
        let query_raw = query.trim();

        // Match against channel ID
        if let Ok(target_id) = query_raw.parse::<i64>() {
            if self.channel_id == target_id {
                return true;
            }
        }

        // Match against username
        if let Some(username) = &self.channel_username {
            if username.to_lowercase() == query_clean {
                return true;
            }
        }

        // Match against channel title
        if let Some(title) = &self.channel_title {
            if title.to_lowercase().contains(&query_clean) {
                return true;
            }
        }

        false
    }
}

/// JSON-RPC 2.0 Request structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC 2.0 Error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Arguments for `get_recent_channel_messages` tool.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GetRecentChannelMessagesArgs {
    pub limit: Option<usize>,
    pub channel_name: Option<String>,
}

/// Arguments for `send_telegram_message` tool.
#[derive(Debug, Clone, Deserialize)]
pub struct SendTelegramMessageArgs {
    pub text: String,
    pub chat_id: Option<String>,
    pub parse_mode: Option<String>,
}
