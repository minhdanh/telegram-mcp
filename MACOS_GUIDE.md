# Running Telegram MCP Server on macOS

This guide provides instructions and troubleshooting tips for installing, configuring, and integrating the **Telegram MCP Server** with AI clients (such as Claude Desktop, Cursor, and Gemini / Antigravity IDE) on macOS (both Apple Silicon M1/M2/M3/M4 and Intel).

> [!TIP]
> **Standard macOS / Unix Locations:**
> - **Binary:** `~/.local/bin/telegram-mcp`
> - **Configuration:** `~/.config/telegram-mcp/config.yaml`
> - **No Rust Required:** Pre-compiled native binaries are built automatically via GitHub Actions and installed directly by our setup script.

---

## 1. Quick Start (Recommended)

Run the automated setup script in your terminal:

```bash
./scripts/setup-mac.sh
```

### Customizing Install Directory (Optional)
By default, the binary is installed to `~/.local/bin/telegram-mcp`. If you prefer `/usr/local/bin` instead:
```bash
INSTALL_DIR=/usr/local/bin ./scripts/setup-mac.sh
```

### What this script does:
1. Detects your Mac hardware architecture (`arm64` Apple Silicon or `x86_64` Intel).
2. Sets up configuration at `~/.config/telegram-mcp/config.yaml` (copied from `config.yaml` or `config.yaml.example`).
3. Automatically downloads and installs the pre-compiled `telegram-mcp` binary into `~/.local/bin/telegram-mcp`.
4. Removes macOS Gatekeeper quarantine restrictions (`xattr -d com.apple.quarantine`).
5. Prints ready-to-copy configuration snippets formatted for **Claude Desktop**, **Cursor**, and **Gemini IDE** with your Mac's exact absolute paths.

---

## 2. Configure Your Telegram Bot

Edit your configuration in `~/.config/telegram-mcp/config.yaml`:

```yaml
# Telegram Bot API Token obtained from @BotFather
telegram_bot_token: "123456789:ABCdefGhIJKlmNoPQRsTUVwxyZ"

# Channels to monitor: username (with '@'), chat ID (e.g. -100...), or title
# Use ["*"] or leave empty to monitor all channels where the bot is Admin
monitored_channels:
  - "@example_channel"
  - "-1001234567890"

# Number of messages to retain in memory ring buffer
buffer_size: 200
```

> [!IMPORTANT]
> **Telegram Bot Requirements:**
> 1. Obtain a bot token from [@BotFather](https://t.me/botfather).
> 2. Add your bot as an **Administrator** in the target channel(s). Standard channel members cannot receive channel posts through the Telegram Bot API.

---

## 3. MCP Client Configuration on macOS

> [!CAUTION]
> **Use Absolute Paths:**
> macOS GUI applications launched from Spotlight, Finder, or the Dock do not run inside your interactive shell environment. They do **not** inherit custom shell variables from `~/.zshrc`, nor do they consistently expand `~` (tilde) in JSON configuration paths.
> 
> Always specify complete absolute paths starting with `/Users/<your-username>/...`.

### Claude Desktop for Mac

1. Open or create the Claude Desktop configuration file:
   ```bash
   code ~/Library/Application\ Support/Claude/claude_desktop_config.json
   # or with nano:
   nano ~/Library/Application\ Support/Claude/claude_desktop_config.json
   ```

2. Add the `telegram` server entry under `mcpServers`:
   ```json
   {
     "mcpServers": {
       "telegram": {
         "command": "/Users/YOUR_USERNAME/.local/bin/telegram-mcp",
         "args": [
           "--config",
           "/Users/YOUR_USERNAME/.config/telegram-mcp/config.yaml"
         ]
       }
     }
   }
   ```
   *(Run `./scripts/setup-mac.sh` to get your exact path).*

3. Completely quit Claude Desktop (`Cmd + Q`) and relaunch it.
4. Click the 🔨 (hammer) icon in Claude to verify that `get_recent_channel_messages` is active.

---

### Cursor on Mac

1. Open Cursor Settings (`Cmd + ,`) and navigate to **Features > MCP**, or create/edit your MCP configuration:
   - User level: `~/.cursor/mcp.json`
   - Workspace level: `.cursor/mcp.json` in your project root.

2. Add the configuration:
   ```json
   {
     "mcpServers": {
       "telegram": {
         "command": "/Users/YOUR_USERNAME/.local/bin/telegram-mcp",
         "args": [
           "--config",
           "/Users/YOUR_USERNAME/.config/telegram-mcp/config.yaml"
         ]
       }
     }
   }
   ```

---

### Gemini / Antigravity IDE

Add to your `mcp_config.json` (typically at `~/.gemini/antigravity-ide/mcp_config.json`):

```json
{
  "mcpServers": {
    "telegram": {
      "command": "/Users/YOUR_USERNAME/.local/bin/telegram-mcp",
      "args": [
        "--config",
        "/Users/YOUR_USERNAME/.config/telegram-mcp/config.yaml"
      ]
    }
  }
}
```

---

## 4. Testing & Verification

### Manual stdio JSON-RPC Verification
You can test the installed binary directly in your macOS Terminal:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n' | ~/.local/bin/telegram-mcp
```

*(Notice: `telegram-mcp` automatically detects `~/.config/telegram-mcp/config.yaml` even when `--config` is omitted).*

Expected output:
- Standard output (`stdout`) returns valid MCP JSON-RPC responses for `initialize` and `tools/list`.
- Standard error (`stderr`) shows diagnostic logs (e.g. `Loading configuration...`, `Spawning Telegram ingestion...`) without corrupting the protocol.

---

## 5. Developer Guide: Building from Source & CI/CD

If you are developing or modifying the Rust code:

### Prerequisites
- [Rust & Cargo](https://rustup.rs/): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Xcode Command Line Tools: `xcode-select --install`

### Local Compilation
```bash
cargo build --release
cargo test
```
To install your locally-built binary into `~/.local/bin/telegram-mcp`:
```bash
./scripts/setup-mac.sh --build
```

### GitHub Actions Release Workflow
This repository includes an automated GitHub Actions workflow (`.github/workflows/release.yml`):
- Whenever a tag starting with `v*` (e.g. `v0.1.0`) is pushed to GitHub, or when triggered manually via `workflow_dispatch`, GitHub Actions:
  1. Compiles optimized native release binaries for:
     - `aarch64-apple-darwin` (Apple Silicon M1/M2/M3/M4)
     - `x86_64-apple-darwin` (Intel Mac)
     - `x86_64-unknown-linux-gnu` (Linux)
  2. Generates SHA256 checksums.
  3. Publishes archives directly to the GitHub Release.

To release a new version:
```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## 6. Troubleshooting on macOS

| Issue | Cause | Solution |
|---|---|---|
| Claude Desktop says **"Server disconnected"** or cannot spawn process | Relative path or tilde (`~`) used in `claude_desktop_config.json` | Use full absolute path starting with `/Users/...`. Run `pwd` or `./scripts/setup-mac.sh` to get the exact path. |
| macOS Gatekeeper blocks downloaded binary | Quarantine extended attribute attached by browser or curl | Run `xattr -d com.apple.quarantine ~/.local/bin/telegram-mcp` (handled automatically by `./scripts/setup-mac.sh`). |
| `command not found: telegram-mcp` | `~/.local/bin` is not in your shell `$PATH` | Add `export PATH="$HOME/.local/bin:$PATH"` to your `~/.zshrc`, or install to `/usr/local/bin` using `INSTALL_DIR=/usr/local/bin ./scripts/setup-mac.sh`. |
| Telegram updates are not showing up in buffer | Bot is not an Administrator in the channel | Open channel settings on Telegram > Administrators > Add your bot as Admin with message-viewing permissions. |
| Port or Connection issues | Telegram Bot API is blocked by a proxy/firewall | Ensure `api.telegram.org:443` is reachable from your network. |
