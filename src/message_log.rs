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
}
