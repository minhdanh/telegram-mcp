use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod buffer;
mod config;
mod mcp;
mod telegram;
mod types;

use buffer::MessageBuffer;
use config::Config;

#[derive(Parser, Debug)]
#[command(
    name = "telegram-mcp",
    about = "Model Context Protocol (MCP) server and CLI for Telegram integration",
    version
)]
struct Args {
    /// Path to YAML configuration file [default: ~/.config/telegram-mcp/config.yaml or ./config.yaml]
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Run as MCP server over stdio (default if no subcommand specified)
    Serve,
    /// Send a message to a Telegram chat or channel
    Send {
        /// Message text content to send
        #[arg(short, long)]
        text: String,
        /// Target chat ID or username (defaults to default_chat_id in config)
        #[arg(long, allow_hyphen_values = true)]
        chat_id: Option<String>,
        /// Parse mode: Markdown, MarkdownV2, HTML, or None
        #[arg(long, default_value = "Markdown")]
        parse_mode: String,
    },
    /// Fetch recent updates from Telegram (long-polling or offset-based)
    Updates {
        /// Update ID offset to start from
        #[arg(long)]
        offset: Option<i64>,
        /// Maximum number of updates to fetch
        #[arg(long, default_value_t = 10)]
        limit: u8,
    },
}

fn resolve_config_path(cli_path: Option<PathBuf>) -> PathBuf {
    // 1. Explicit CLI argument flag: --config <path>
    if let Some(p) = cli_path {
        return p;
    }

    // 2. Explicit environment variable: TELEGRAM_MCP_CONFIG
    if let Ok(env_path) = std::env::var("TELEGRAM_MCP_CONFIG") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return p;
        }
    }

    // 3. Prefer standard XDG user configuration: ~/.config/telegram-mcp/config.yaml
    if let Ok(xdg_home) = std::env::var("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg_home).join("telegram-mcp/config.yaml");
        if xdg.exists() {
            return xdg;
        }
    } else if let Ok(home) = std::env::var("HOME") {
        let xdg = PathBuf::from(home).join(".config/telegram-mcp/config.yaml");
        if xdg.exists() {
            return xdg;
        }
    }

    // 4. Fallback to current working directory: ./config.yaml
    let local = PathBuf::from("config.yaml");
    if local.exists() {
        return local;
    }

    // 5. Default fallback to standard XDG path
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/telegram-mcp/config.yaml")
    } else {
        local
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // CRITICAL: MCP communication uses stdout for JSON-RPC messages.
    // ALL logs, info, warnings, and errors MUST be directed strictly to stderr.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("telegram_mcp=info,teloxide=info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    let args = Args::parse();
    let config_path = resolve_config_path(args.config);
    info!(config_file = ?config_path, "Loading configuration...");

    let config = match Config::load_from_file(&config_path) {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            error!(
                error = %e,
                config_path = ?config_path,
                "Failed to load configuration. Please ensure the config file exists and is valid YAML."
            );
            std::process::exit(1);
        }
    };

    info!(
        buffer_size = config.buffer_size,
        monitored_channels = ?config.monitored_channels,
        "Configuration successfully loaded"
    );

    // Dispatch CLI subcommands if specified
    if let Some(cmd) = args.command {
        match cmd {
            Commands::Send { text, chat_id, parse_mode } => {
                let bot = teloxide::Bot::new(&config.telegram_bot_token);
                let target_chat = chat_id.as_deref()
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
                        eprintln!("Error: No target chat_id provided and no default channel configured in config.yaml.");
                        std::process::exit(1);
                    }
                };

                let recipient = telegram::parse_recipient(target);
                #[allow(deprecated)]
                let pm = match parse_mode.as_str() {
                    "HTML" | "html" => Some(teloxide::types::ParseMode::Html),
                    "MarkdownV2" | "markdownv2" => Some(teloxide::types::ParseMode::MarkdownV2),
                    "Markdown" | "markdown" => Some(teloxide::types::ParseMode::Markdown),
                    _ => None,
                };

                match telegram::send_telegram_message(&bot, recipient, &text, pm).await {
                    Ok(msg) => {
                        println!("{}", serde_json::json!({
                            "ok": true,
                            "message_id": msg.id.0,
                            "chat_id": target,
                        }));
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("Error sending Telegram message: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Commands::Updates { offset, limit } => {
                let bot = teloxide::Bot::new(&config.telegram_bot_token);
                let offset_i32 = offset.map(|o| o as i32);
                match telegram::fetch_updates(&bot, offset_i32, Some(limit)).await {
                    Ok(updates) => {
                        println!("{}", serde_json::to_string(&updates).unwrap_or_else(|_| "[]".to_string()));
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("Error fetching Telegram updates: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Commands::Serve => {
                // Proceed to standard MCP server below
            }
        }
    }

    // Initialize shared in-memory ring buffer
    let buffer = MessageBuffer::new(config.buffer_size);

    // Initialize Telegram bot client
    let bot = teloxide::Bot::new(&config.telegram_bot_token);

    // Spawn Telegram ingestion task and MCP server task concurrently
    let telegram_config = Arc::clone(&config);
    let telegram_buffer = buffer.clone();
    let mcp_buffer = buffer.clone();
    let mcp_bot = bot.clone();
    let mcp_config = Arc::clone(&config);

    info!("Spawning Telegram ingestion and MCP server tasks...");

    tokio::select! {
        // Run Telegram long-polling ingestion task in background
        telegram_res = telegram::run_telegram_listener(telegram_config, telegram_buffer) => {
            if let Err(e) = telegram_res {
                error!(error = %e, "Telegram ingestion task exited with error");
            } else {
                info!("Telegram ingestion task completed");
            }
        }

        // Run MCP server task handling stdio JSON-RPC
        mcp_res = mcp::run_mcp_server(mcp_buffer, mcp_bot, mcp_config) => {
            if let Err(e) = mcp_res {
                error!(error = %e, "MCP server task exited with error");
            } else {
                info!("MCP server task completed");
            }
        }

        // Listen for termination signal (Ctrl+C)
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal (Ctrl+C). Shutting down telegram-mcp...");
        }
    }

    Ok(())
}
