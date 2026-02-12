pub struct MessageLog {
    messages: Vec<String>,
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
