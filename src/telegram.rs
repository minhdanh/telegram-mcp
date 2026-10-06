use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{AllowedUpdate, ChatId, Message, ParseMode, Recipient};
use tracing::{debug, info, warn};

use crate::buffer::MessageBuffer;
use crate::config::Config;
use crate::types::ChannelMessage;

/// Extract displayable text from a Telegram message.
/// Falls back to caption (for photos/videos/documents) if text is empty.
fn extract_message_content(msg: &Message) -> Option<String> {
    if let Some(text) = msg.text() {
        return Some(text.to_string());
    }

    if let Some(caption) = msg.caption() {
        return Some(format!("[Media] {}", caption));
    }

    // Identify media type if no text or caption is present
    if msg.photo().is_some() {
        return Some("[Photo without caption]".to_string());
    }
    if msg.video().is_some() {
        return Some("[Video without caption]".to_string());
    }
    if msg.document().is_some() {
        return Some("[Document without caption]".to_string());
    }
    if msg.audio().is_some() {
        return Some("[Audio without caption]".to_string());
    }
    if msg.voice().is_some() {
        return Some("[Voice note]".to_string());
    }
    if msg.sticker().is_some() {
        return Some("[Sticker]".to_string());
    }

    None
}

/// Convert a Telegram `Message` into our internal `ChannelMessage`.
fn parse_channel_post(msg: &Message) -> Option<ChannelMessage> {
    let chat = &msg.chat;
    let chat_id = chat.id.0;
    let username = chat.username().map(|s| s.to_string());
    let title = chat.title().map(|s| s.to_string());
    let text = extract_message_content(msg)?;

    Some(ChannelMessage {
        message_id: msg.id.0,
        channel_id: chat_id,
        channel_username: username,
        channel_title: title,
        timestamp: msg.date,
        text,
    })
}

/// Run the Telegram ingestion task using long-polling.
/// Listens exclusively for `channel_post` (and `edited_channel_post`) updates.
pub async fn run_telegram_listener(
    config: Arc<Config>,
    buffer: MessageBuffer,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bot = Bot::new(&config.telegram_bot_token);

    info!("Initializing Telegram bot client...");

    // Test bot token connectivity (logs to stderr)
    match bot.get_me().await {
        Ok(me) => {
            info!(
                bot_username = ?me.username(),
                bot_id = ?me.id.0,
                "Successfully connected to Telegram Bot API"
            );
        }
        Err(e) => {
            warn!(
                error = %e,
                "Could not verify Telegram bot credentials via getMe. Will proceed with polling anyway (may retry)."
            );
        }
    }

    let handler = dptree::entry()
        .branch(
            Update::filter_channel_post().endpoint(
                |msg: Message, buf: MessageBuffer, cfg: Arc<Config>| async move {
                    handle_incoming_message(msg, buf, cfg, false).await;
                    respond(())
                },
            ),
        )
        .branch(
            Update::filter_edited_channel_post().endpoint(
                |msg: Message, buf: MessageBuffer, cfg: Arc<Config>| async move {
                    handle_incoming_message(msg, buf, cfg, true).await;
                    respond(())
                },
            ),
        )
        .branch(
            Update::filter_message().endpoint(
                |msg: Message, buf: MessageBuffer, cfg: Arc<Config>| async move {
                    handle_incoming_message(msg, buf, cfg, false).await;
                    respond(())
                },
            ),
        )
        .branch(
            Update::filter_edited_message().endpoint(
                |msg: Message, buf: MessageBuffer, cfg: Arc<Config>| async move {
                    handle_incoming_message(msg, buf, cfg, true).await;
                    respond(())
                },
            ),
        );

    info!(
        monitored_channels = ?config.monitored_channels,
        buffer_size = config.buffer_size,
        "Starting Telegram long-polling for messages and channel posts..."
    );

    use teloxide::update_listeners::Polling;

    let listener = Polling::builder(bot.clone())
        .timeout(std::time::Duration::from_secs(30))
        .allowed_updates(vec![
            AllowedUpdate::ChannelPost,
            AllowedUpdate::EditedChannelPost,
            AllowedUpdate::Message,
            AllowedUpdate::EditedMessage,
        ])
        .build();

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![buffer, config])
        .enable_ctrlc_handler()
        .build();

    dispatcher.dispatch_with_listener(
        listener,
        LoggingErrorHandler::with_custom_text("An error from the update listener"),
    ).await;

    info!("Telegram ingestion task has terminated.");
    Ok(())
}

async fn handle_incoming_message(
    msg: Message,
    buffer: MessageBuffer,
    config: Arc<Config>,
    is_edit: bool,
) {
    let chat = &msg.chat;
    let chat_id = chat.id.0;
    let username = chat.username();
    let title = chat.title();

    if !config.is_channel_monitored(chat_id, username, title) {
        debug!(
            chat_id,
            username = ?username,
            title = ?title,
            "Ignoring channel post from unmonitored channel"
        );
        return;
    }

    if let Some(channel_msg) = parse_channel_post(&msg) {
        info!(
            channel_id = channel_msg.channel_id,
            channel = ?channel_msg.channel_username.as_deref().or(channel_msg.channel_title.as_deref()),
            message_id = channel_msg.message_id,
            is_edit,
            "Captured channel post into ring buffer"
        );
        buffer.push(channel_msg).await;
    } else {
        debug!(
            message_id = msg.id.0,
            "Received channel post with no extractable text or media caption"
        );
    }
}

/// Parse a string query (ID or username) into a teloxide `Recipient`.
pub fn parse_recipient(input: &str) -> Recipient {
    let trimmed = input.trim();
    if let Ok(id) = trimmed.parse::<i64>() {
        Recipient::Id(ChatId(id))
    } else {
        let channel_name = if trimmed.starts_with('@') {
            trimmed.to_string()
        } else {
            format!("@{}", trimmed)
        };
        Recipient::ChannelUsername(channel_name)
    }
}

/// Send a text message to a Telegram channel or chat.
pub async fn send_telegram_message(
    bot: &Bot,
    recipient: Recipient,
    text: &str,
    parse_mode: Option<ParseMode>,
) -> Result<Message, teloxide::RequestError> {
    let mut req = bot.send_message(recipient, text);
    if let Some(pm) = parse_mode {
        req = req.parse_mode(pm);
    }
    req.await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimpleTelegramUpdate {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: Option<String>,
}

/// Fetch recent updates from Telegram with an optional offset and limit.
pub async fn fetch_updates(
    bot: &Bot,
    offset: Option<i32>,
    limit: Option<u8>,
) -> Result<Vec<SimpleTelegramUpdate>, teloxide::RequestError> {
    let mut req = bot.get_updates();
    if let Some(off) = offset {
        req = req.offset(off);
    }
    if let Some(lim) = limit {
        req = req.limit(lim);
    }
    let updates = req.await?;
    let mut results = Vec::new();
    for u in updates {
        let update_id = u.id.0 as i64;
        let (chat_id, text) = match &u.kind {
            teloxide::types::UpdateKind::Message(m) => (m.chat.id.0, extract_message_content(m)),
            teloxide::types::UpdateKind::ChannelPost(m) => (m.chat.id.0, extract_message_content(m)),
            teloxide::types::UpdateKind::EditedMessage(m) => (m.chat.id.0, extract_message_content(m)),
            teloxide::types::UpdateKind::EditedChannelPost(m) => (m.chat.id.0, extract_message_content(m)),
            _ => continue,
        };
        results.push(SimpleTelegramUpdate {
            update_id,
            chat_id,
            text,
        });
    }
    Ok(results)
}
