use clap::Parser;
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
    about = "Model Context Protocol (MCP) server for real-time Telegram channel monitoring",
    version
)]
struct Args {
    /// Path to YAML configuration file [default: ./config.yaml or ~/.config/telegram-mcp/config.yaml]
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn resolve_config_path(cli_path: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_path {
        return p;
    }
    let local = PathBuf::from("config.yaml");
    if local.exists() {
        return local;
    }
    if let Ok(home) = std::env::var("HOME") {
        let xdg = PathBuf::from(home).join(".config/telegram-mcp/config.yaml");
        if xdg.exists() {
            return xdg;
        }
    }
    local
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
