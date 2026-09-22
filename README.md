# Telegram MCP Server

A Model Context Protocol (MCP) server written in Rust that monitors Telegram channels in real-time and serves the most recent messages to local LLM clients (such as Gemini Spark, Claude Desktop, Cursor) via a local stdio connection.

## Architecture

This project implements a **forward-cache pattern**:
- It does **not** fetch historical messages retroactively.
- It uses the standard Telegram Bot API via long-polling to listen for incoming `channel_post` events in real-time.
- Messages are stored in a thread-safe, bounded in-memory ring buffer (`Arc<RwLock<VecDeque<ChannelMessage>>>`).
- When the buffer reaches capacity (`buffer_size`), the oldest messages are dropped.
- The server runs an MCP JSON-RPC protocol loop over `stdin` / `stdout`.
- **Strict Stdio Hygiene:** All internal application logs, warnings, and diagnostic info are written strictly to `stderr`, preserving `stdout` exclusively for clean JSON-RPC messaging.

```
┌────────────────────────────────┐
│   Telegram Cloud / Channels    │
└──────────────┬─────────────────┘
               │ Long-polling (channel_post)
               ▼
┌────────────────────────────────┐
│     Telegram Ingestion Task    │
└──────────────┬─────────────────┘
               │ push_back (evict oldest)
               ▼
┌────────────────────────────────┐
│    In-Memory Ring Buffer       │
│  (Arc<RwLock<VecDeque<...>>>)  │
└──────────────▲─────────────────┘
               │ read_recent
               │
┌──────────────┴─────────────────┐
│        MCP Server Task         │
└──────────────▲─────────────────┘
               │ stdio JSON-RPC (stdin / stdout)
               ▼
┌────────────────────────────────┐
│ LLM Client (e.g. Gemini Spark) │
└────────────────────────────────┘
```

---

## Prerequisites

- [Rust & Cargo](https://rustup.rs/) (edition 2021, Rust 1.75+)
- A Telegram Bot token from [@BotFather](https://t.me/botfather)
- Admin permissions for the bot in your monitored channel(s)

---

## Telegram Bot Setup

> [!IMPORTANT]
> To receive `channel_post` updates from a Telegram channel, the bot **must be added to the channel as an Administrator** (at minimum with permissions to read messages/view posts). Regular bot members cannot read posts from public or private channels.

1. Open Telegram and message [@BotFather](https://t.me/botfather).
2. Create a new bot using `/newbot` and copy the API token.
3. Open the Telegram channel you wish to monitor.
4. Go to **Channel Settings** > **Administrators** > **Add Admin**.
5. Search for your bot username and add it as an administrator.

---

## Configuration

Copy the example configuration file:

```bash
cp config.yaml.example config.yaml
```

Edit `config.yaml`:

```yaml
# Telegram Bot API Token obtained from @BotFather
telegram_bot_token: "123456789:ABCdefGhIJKlmNoPQRsTUVwxyZ"

# List of channel usernames (with or without '@'), channel IDs, or titles to monitor.
# Leave empty or use ["*"] to monitor all channels where the bot is added as Administrator.
monitored_channels:
  - "@example_channel"
  - "-1001234567890"

# Maximum number of messages to retain in memory (ring buffer capacity).
buffer_size: 200
```

---

## Building

Build the release binary:

```bash
cargo build --release
```

The compiled binary will be located at `target/release/telegram-mcp`.

---

## MCP Client Configuration

### Gemini Spark (`mcp_config.json`)

Add the server to your `mcp_config.json`:

```json
{
  "mcpServers": {
    "telegram": {
      "command": "/absolute/path/to/telegram-mcp/target/release/telegram-mcp",
      "args": ["--config", "/absolute/path/to/telegram-mcp/config.yaml"]
    }
  }
}
```

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "telegram": {
      "command": "/absolute/path/to/telegram-mcp/target/release/telegram-mcp",
      "args": ["--config", "/absolute/path/to/telegram-mcp/config.yaml"]
    }
  }
}
```

---

## MCP Tool Specification

### `get_recent_channel_messages`

Retrieves the most recently captured messages from the connected Telegram channels.

#### Arguments

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `limit` | integer | No | `10` | Number of messages to retrieve (1 to 1000). |
| `channel_name` | string | No | `null` | Filter by specific channel username, title, or ID. |

#### Example Tool Output

```
Retrieved 2 recent message(s) (ordered newest to oldest):

[2026-09-22 08:30:00 UTC] [Tech Alerts (@tech_alerts)] (msg_id: 104)
Deployment v2.4.1 completed successfully across all nodes.

---

[2026-09-22 08:25:12 UTC] [Tech Alerts (@tech_alerts)] (msg_id: 103)
Starting deployment v2.4.1 in production.
```

---

## Running Tests

Run all unit tests (ring buffer eviction, channel matching, config validation, MCP request handlers):

```bash
cargo test
```

## Testing Stdio MCP Protocol Manually

You can test the MCP server directly using `printf` or interactive terminal input:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_recent_channel_messages","arguments":{"limit":5}}}\n' | ./target/debug/telegram-mcp --config config.yaml
```
