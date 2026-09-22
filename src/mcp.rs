use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::buffer::MessageBuffer;
use crate::types::{GetRecentChannelMessagesArgs, JsonRpcRequest, JsonRpcResponse};

/// Handle an individual incoming JSON-RPC request.
/// Returns `Some(response)` if a response should be sent, or `None` if the request is a notification.
pub async fn handle_request(
    request: JsonRpcRequest,
    buffer: &MessageBuffer,
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
                        "description": "Retrieves the most recently captured messages from the connected Telegram channels. Use this to check for real-time announcements or alerts.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "limit": {
                                    "type": "integer",
                                    "description": "Number of messages to retrieve (defaults to 10)."
                                },
                                "channel_name": {
                                    "type": "string",
                                    "description": "Filter by a specific channel if monitoring multiple."
                                }
                            }
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

            if tool_name != "get_recent_channel_messages" {
                return Some(JsonRpcResponse::error(
                    id,
                    -32601,
                    format!("Tool '{}' not found", tool_name),
                ));
            }

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

        if let Some(response) = handle_request(request, &buffer).await {
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

    #[tokio::test]
    async fn test_initialize() {
        let buffer = MessageBuffer::new(10);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer).await.unwrap();
        assert_eq!(resp.id, json!(1));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "telegram-mcp");
    }

    #[tokio::test]
    async fn test_tools_list() {
        let buffer = MessageBuffer::new(10);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer).await.unwrap();
        assert_eq!(resp.id, json!(2));
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_recent_channel_messages");
    }

    #[tokio::test]
    async fn test_tools_call_with_messages() {
        let buffer = MessageBuffer::new(10);
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

        let resp = handle_request(req, &buffer).await.unwrap();
        assert_eq!(resp.id, json!(3));
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Breaking update: Telegram MCP is live!"));
        assert!(text.contains("News Channel (@news)"));
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let buffer = MessageBuffer::new(10);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(99)),
            method: "non_existent_method".to_string(),
            params: None,
        };

        let resp = handle_request(req, &buffer).await.unwrap();
        assert_eq!(resp.id, json!(99));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }
}
