use std::sync::Arc;
use serde_json::json;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::buffer::MessageBuffer;
use crate::config::Config;
use crate::types::{
    GetRecentChannelMessagesArgs, JsonRpcRequest, JsonRpcResponse, SendTelegramMessageArgs,
};

/// Handle an individual incoming JSON-RPC request.
/// Returns `Some(response)` if a response should be sent, or `None` if the request is a notification.
pub async fn handle_request(
    request: JsonRpcRequest,
    buffer: &MessageBuffer,
    bot: &Bot,
    config: &Config,
) -> Option<JsonRpcResponse> {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let is_notification = request.id.is_none();

    match request.method.as_str() {
        "initialize" => {
            debug!("Handling initialize request");
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "telegram-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            });
            Some(JsonRpcResponse::success(id, result))
        }

        "notifications/initialized" => {
            info!("Received initialized notification from client");
            None
        }

        "ping" => {
            debug!("Handling ping request");
            Some(JsonRpcResponse::success(id, json!({})))
        }

        "tools/list" => {
            debug!("Handling tools/list request");
            let tools = json!({
                "tools": [
                    {
                        "name": "get_recent_channel_messages",
                        "description": "Retrieves the most recently captured messages from the connected Telegram channels or chats. Use this to check for real-time announcements, incoming instructions, or alerts.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "limit": {
                                    "type": "integer",
                                    "description": "Number of messages to retrieve (defaults to 10)."
                                },
                                "channel_name": {
                                    "type": "string",
                                    "description": "Filter by a specific channel or chat if monitoring multiple."
                                }
                            }
                        }
                    },
                    {
                        "name": "send_telegram_message",
                        "description": "Sends a message to a Telegram channel or chat. Use this to report delivery progress, alert the user of blockers, or ask questions when blocked.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "text": {
                                    "type": "string",
                                    "description": "The text message content to send."
                                },
                                "chat_id": {
                                    "type": "string",
                                    "description": "Target Telegram chat ID (e.g. '-1001234567890') or channel username ('@my_channel'). If omitted, defaults to default_chat_id or the first monitored channel."
                                },
                                "parse_mode": {
                                    "type": "string",
                                    "enum": ["Markdown", "MarkdownV2", "HTML"],
                                    "description": "Optional Telegram message parse mode formatting."
                                }
                            },
                            "required": ["text"]
                        }
                    }
                ]
            });
            Some(JsonRpcResponse::success(id, tools))
        }

        "tools/call" => {
            debug!("Handling tools/call request: {:?}", request.params);
            let params = request.params.unwrap_or(serde_json::Value::Null);
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");

            match tool_name {
                "get_recent_channel_messages" => {
                    let args: GetRecentChannelMessagesArgs = match params.get("arguments") {
                        Some(arg_val) => serde_json::from_value(arg_val.clone()).unwrap_or_default(),
                        None => GetRecentChannelMessagesArgs::default(),
                    };

                    let limit = args.limit.unwrap_or(10).clamp(1, 1000);
                    let channel_filter = args.channel_name.as_deref();

                    let messages = buffer.get_recent(limit, channel_filter).await;

                    let response_text = if messages.is_empty() {
                        match channel_filter {
                            Some(ch) => format!(
                                "No messages found matching channel '{}'. Buffer holds {} total message(s).",
                                ch,
                                buffer.len().await
                            ),
                            None => format!(
                                "No messages currently in buffer. Total buffer count: {}. Make sure the bot is added as an administrator to the target channels.",
                                buffer.len().await
                            ),
                        }
                    } else {
                        let count = messages.len();
                        let header = format!(
                            "Retrieved {} recent message(s) (ordered newest to oldest):\n\n",
                            count
                        );
                        let body = messages
                            .iter()
                            .map(|m| m.formatted())
                            .collect::<Vec<String>>()
                            .join("\n\n---\n\n");
                        format!("{}{}", header, body)
                    };

                    let content = json!({
                        "content": [
                            {
                                "type": "text",
                                "text": response_text
                            }
                        ],
                        "isError": false
                    });

                    Some(JsonRpcResponse::success(id, content))
                }

                "send_telegram_message" => {
                    let args: SendTelegramMessageArgs = match params.get("arguments") {
                        Some(arg_val) => match serde_json::from_value(arg_val.clone()) {
                            Ok(a) => a,
                            Err(e) => {
                                return Some(JsonRpcResponse::error(
                                    id,
                                    -32602,
                                    format!("Invalid arguments: {}", e),
                                ));
                            }
                        },
                        None => {
                            return Some(JsonRpcResponse::error(
                                id,
                                -32602,
                                "Missing arguments for send_telegram_message",
                            ));
                        }
                    };

                    let target_chat = args
                        .chat_id
                        .as_deref()
                        .or(config.default_chat_id.as_deref())
                        .or_else(|| {
                            config
                                .monitored_channels
                                .iter()
                                .find(|c| c.trim() != "*")
                                .map(|s| s.as_str())
                        });

                    let target = match target_chat {
                        Some(t) => t,
                        None => {
                            let content = json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Error: No target chat_id provided and no default channel configured in config.yaml."
                                    }
                                ],
                                "isError": true
                            });
                            return Some(JsonRpcResponse::success(id, content));
                        }
                    };

                    let recipient = crate::telegram::parse_recipient(target);
                    let parse_mode = match args.parse_mode.as_deref() {
                        Some("HTML") | Some("html") => Some(ParseMode::Html),
                        Some("MarkdownV2") | Some("markdownv2") => Some(ParseMode::MarkdownV2),
                        Some("Markdown") | Some("markdown") => Some(ParseMode::MarkdownV2),
                        _ => None,
                    };

                    match crate::telegram::send_telegram_message(bot, recipient, &args.text, parse_mode).await {
                        Ok(sent_msg) => {
                            let content = json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("Successfully sent message to {} (msg_id: {})", target, sent_msg.id.0)
                                    }
                                ],
                                "isError": false
                            });
                            Some(JsonRpcResponse::success(id, content))
                        }
                        Err(e) => {
                            let content = json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("Failed to send Telegram message to {}: {}", target, e)
                                    }
                                ],
                                "isError": true
                            });
                            Some(JsonRpcResponse::success(id, content))
                        }
                    }
                }

                _ => Some(JsonRpcResponse::error(
                    id,
                    -32601,
                    format!("Tool '{}' not found", tool_name),
                )),
            }
        }

        other => {
            if is_notification {
                debug!(method = other, "Ignoring unknown notification");
                None
            } else {
                warn!(method = other, "Received unknown JSON-RPC method");
                Some(JsonRpcResponse::error(
                    id,
                    -32601,
                    format!("Method '{}' not found", other),
                ))
            }
        }
    }
}

/// Run the MCP server loop over standard input and output.
/// Incoming JSON-RPC lines are read from stdin, and responses are written to stdout.
pub async fn run_mcp_server(
    buffer: MessageBuffer,
    bot: Bot,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting MCP server over stdio...");

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Some(line) = reader.next_line().await? {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line_trimmed) {
            Ok(req) => req,
            Err(e) => {
                error!(error = %e, raw_line = line_trimmed, "Failed to parse JSON-RPC request");
                let err_resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let resp_str = serde_json::to_string(&err_resp)?;
                stdout.write_all(resp_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        if let Some(response) = handle_request(request, &buffer, &bot, &config).await {
            let resp_str = serde_json::to_string(&response)?;
            stdout.write_all(resp_str.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    info!("MCP stdin stream reached EOF. Shutting down MCP server.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChannelMessage;
    use chrono::Utc;

    fn test_config() -> Config {
        Config {
            telegram_bot_token: "123456789:ABCdefGhIJKlmNoPQRsTUVwxyZ".to_string(),
            monitored_channels: vec!["@test_channel".to_string()],
            buffer_size: 10,
            default_chat_id: Some("@test_channel".to_string()),
        }
    }

    #[tokio::test]
    async fn test_initialize() {
        let buffer = MessageBuffer::new(10);
        let config = test_config();
        let bot = Bot::new(&config.telegram_bot_token);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer, &bot, &config).await.unwrap();
        assert_eq!(resp.id, json!(1));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "telegram-mcp");
    }

    #[tokio::test]
    async fn test_tools_list() {
        let buffer = MessageBuffer::new(10);
        let config = test_config();
        let bot = Bot::new(&config.telegram_bot_token);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer, &bot, &config).await.unwrap();
        assert_eq!(resp.id, json!(2));
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "get_recent_channel_messages");
        assert_eq!(tools[1]["name"], "send_telegram_message");
    }

    #[tokio::test]
    async fn test_tools_call_with_messages() {
        let buffer = MessageBuffer::new(10);
        let config = test_config();
        let bot = Bot::new(&config.telegram_bot_token);
        buffer
            .push(ChannelMessage {
                message_id: 42,
                channel_id: -100123,
                channel_username: Some("news".to_string()),
                channel_title: Some("News Channel".to_string()),
                timestamp: Utc::now(),
                text: "Breaking update: Telegram MCP is live!".to_string(),
            })
            .await;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_recent_channel_messages",
                "arguments": {
                    "limit": 5
                }
            })),
        };

        let resp = handle_request(req, &buffer, &bot, &config).await.unwrap();
        assert_eq!(resp.id, json!(3));
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Breaking update: Telegram MCP is live!"));
        assert!(text.contains("News Channel (@news)"));
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let buffer = MessageBuffer::new(10);
        let config = test_config();
        let bot = Bot::new(&config.telegram_bot_token);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(99)),
            method: "non_existent_method".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer, &bot, &config).await.unwrap();
        assert_eq!(resp.id, json!(99));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_send_telegram_message_missing_args() {
        let buffer = MessageBuffer::new(10);
        let config = test_config();
        let bot = Bot::new(&config.telegram_bot_token);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(4)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "send_telegram_message",
                "arguments": null
            })),
        };

        let resp = handle_request(req, &buffer, &bot, &config).await.unwrap();
        assert_eq!(resp.id, json!(4));
        assert!(resp.error.is_some());
    }
}
