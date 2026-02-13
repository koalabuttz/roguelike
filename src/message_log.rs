pub struct MessageLog {
    messages: Vec<String>,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageLog {
    pub fn new() -> Self {
        MessageLog {
            messages: Vec::new(),
        }
    }

    pub fn add(&mut self, msg: impl Into<String>) {
        self.messages.push(msg.into());
    }

    /// Return the last `n` messages (newest last).
    pub fn recent(&self, n: usize) -> &[String] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    /// Total number of messages ever added.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the log contains no messages.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Return all messages added since index `since` (exclusive).
    pub fn messages_since(&self, since: usize) -> Vec<String> {
        self.messages[since..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_log_is_empty() {
        let log = MessageLog::new();
        assert_eq!(log.recent(10).len(), 0);
    }

    #[test]
    fn add_and_recent_preserves_order() {
        let mut log = MessageLog::new();
        log.add("first");
        log.add("second");
        log.add("third");
        let msgs = log.recent(3);
        assert_eq!(msgs, &["first", "second", "third"]);
    }

    #[test]
    fn recent_returns_last_n() {
        let mut log = MessageLog::new();
        log.add("a");
        log.add("b");
        log.add("c");
        let msgs = log.recent(2);
        assert_eq!(msgs, &["b", "c"]);
    }

    #[test]
    fn recent_with_n_greater_than_count_returns_all() {
        let mut log = MessageLog::new();
        log.add("only");
        let msgs = log.recent(100);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], "only");
    }

    #[test]
    fn len_tracks_message_count() {
        let mut log = MessageLog::new();
        assert_eq!(log.len(), 0);
        assert!(log.is_empty());
        log.add("hello");
        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
    }

    #[test]
    fn messages_since_returns_new_messages() {
        let mut log = MessageLog::new();
        log.add("old");
        let before = log.len();
        log.add("new1");
        log.add("new2");
        let new = log.messages_since(before);
        assert_eq!(new, vec!["new1", "new2"]);
    }

    #[test]
    fn messages_since_returns_empty_when_nothing_new() {
        let mut log = MessageLog::new();
        log.add("existing");
        let before = log.len();
        let new = log.messages_since(before);
        assert!(new.is_empty());
    }
}
