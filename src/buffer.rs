use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::types::ChannelMessage;

/// A thread-safe bounded in-memory ring buffer for storing recent Telegram channel messages.
#[derive(Clone, Debug)]
pub struct MessageBuffer {
    inner: Arc<RwLock<VecDeque<ChannelMessage>>>,
    capacity: usize,
}

impl MessageBuffer {
    pub fn new(capacity: usize) -> Self {
        let actual_capacity = capacity.max(1);
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(actual_capacity))),
            capacity: actual_capacity,
        }
    }

    /// Push a new message into the ring buffer.
    /// If the buffer has reached its capacity, the oldest message is evicted.
    pub async fn push(&self, message: ChannelMessage) {
        let mut buffer = self.inner.write().await;
        if buffer.len() >= self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(message);
    }

    /// Retrieve the most recent messages, optionally filtered by channel.
    /// Results are returned in reverse-chronological order (newest first).
    pub async fn get_recent(
        &self,
        limit: usize,
        channel_filter: Option<&str>,
    ) -> Vec<ChannelMessage> {
        let buffer = self.inner.read().await;
        let mut results = Vec::new();

        for msg in buffer.iter().rev() {
            if let Some(filter) = channel_filter {
                if !msg.matches_channel(filter) {
                    continue;
                }
            }

            results.push(msg.clone());
            if results.len() >= limit {
                break;
            }
        }

        results
    }

    /// Return the current number of messages stored in the buffer.
    pub async fn len(&self) -> usize {
        let buffer = self.inner.read().await;
        buffer.len()
    }

    /// Return true if the buffer is empty.
    #[allow(dead_code)]
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Return the capacity of the buffer.
    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_test_msg(id: i32, channel_id: i64, username: &str, text: &str) -> ChannelMessage {
        ChannelMessage {
            message_id: id,
            channel_id,
            channel_username: Some(username.to_string()),
            channel_title: Some(format!("Title {}", username)),
            timestamp: Utc::now(),
            text: text.to_string(),
        }
    }

    #[tokio::test]
    async fn test_ring_buffer_eviction() {
        let buffer = MessageBuffer::new(3);

        buffer.push(make_test_msg(1, 100, "chan1", "Msg 1")).await;
        buffer.push(make_test_msg(2, 100, "chan1", "Msg 2")).await;
        buffer.push(make_test_msg(3, 100, "chan1", "Msg 3")).await;

        assert_eq!(buffer.len().await, 3);

        // Push 4th message, 1st should be evicted
        buffer.push(make_test_msg(4, 100, "chan1", "Msg 4")).await;
        assert_eq!(buffer.len().await, 3);

        let recent = buffer.get_recent(10, None).await;
        assert_eq!(recent.len(), 3);
        // Newest first
        assert_eq!(recent[0].message_id, 4);
        assert_eq!(recent[1].message_id, 3);
        assert_eq!(recent[2].message_id, 2);
    }

    #[tokio::test]
    async fn test_get_recent_limit_and_filter() {
        let buffer = MessageBuffer::new(10);

        buffer.push(make_test_msg(1, 101, "alpha", "Alpha 1")).await;
        buffer.push(make_test_msg(2, 102, "beta", "Beta 1")).await;
        buffer.push(make_test_msg(3, 101, "alpha", "Alpha 2")).await;
        buffer.push(make_test_msg(4, 102, "beta", "Beta 2")).await;
        buffer.push(make_test_msg(5, 101, "alpha", "Alpha 3")).await;

        // Filter by alpha with limit 2
        let alpha_recent = buffer.get_recent(2, Some("alpha")).await;
        assert_eq!(alpha_recent.len(), 2);
        assert_eq!(alpha_recent[0].message_id, 5);
        assert_eq!(alpha_recent[1].message_id, 3);

        // Filter with leading '@'
        let beta_recent = buffer.get_recent(5, Some("@beta")).await;
        assert_eq!(beta_recent.len(), 2);
        assert_eq!(beta_recent[0].message_id, 4);
        assert_eq!(beta_recent[1].message_id, 2);

        // Filter by ID
        let by_id = buffer.get_recent(5, Some("101")).await;
        assert_eq!(by_id.len(), 3);
    }
}
