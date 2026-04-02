//! Circular message buffer for the micro tier.
//!
//! Stores `GameEvent` values directly — no string formatting needed.
//! Rendering backends format events to their platform's display format.

use crate::rules::message::GameEvent;

/// Circular buffer capacity — number of recent events stored.
/// Must be a power of 2 (used for bitwise masking).
pub const MSG_COUNT: usize = 16;

pub struct MicroMessageLog {
    events: [Option<GameEvent>; MSG_COUNT],
    head: u8,
    total: u16,
}

impl Default for MicroMessageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl MicroMessageLog {
    pub fn new() -> Self {
        Self {
            events: [None; MSG_COUNT],
            head: 0,
            total: 0,
        }
    }

    pub fn reset(&mut self) {
        self.events = [None; MSG_COUNT];
        self.head = 0;
        self.total = 0;
    }

    pub fn add(&mut self, event: GameEvent) {
        self.events[self.head as usize] = Some(event);
        self.head = (self.head + 1) & (MSG_COUNT as u8 - 1);
        self.total = self.total.wrapping_add(1);
    }

    /// Get a recent event. `n=0` is the newest, `n=1` is second newest, etc.
    pub fn recent(&self, n: u8) -> Option<GameEvent> {
        if n as usize >= MSG_COUNT {
            return None;
        }
        let idx = (self.head as usize + MSG_COUNT - 1 - n as usize) & (MSG_COUNT - 1);
        self.events[idx]
    }

    pub fn total(&self) -> u16 {
        self.total
    }

    // -- Save/load accessors --

    /// Direct array access for serialization.
    pub fn event_at(&self, idx: usize) -> Option<GameEvent> {
        self.events[idx]
    }

    /// Current write head position.
    pub fn head(&self) -> u8 {
        self.head
    }

    /// Restore from saved state.
    pub fn from_saved(events: [Option<GameEvent>; MSG_COUNT], head: u8, total: u16) -> Self {
        Self {
            events,
            head,
            total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_retrieve() {
        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);
        assert_eq!(log.recent(0), Some(GameEvent::Welcome));
    }

    #[test]
    fn circular_wraps() {
        let mut log = MicroMessageLog::new();
        for i in 0..10u8 {
            log.add(GameEvent::Descend {
                depth: i,
                target: 5,
            });
        }
        // Most recent should be i=9
        assert_eq!(
            log.recent(0),
            Some(GameEvent::Descend {
                depth: 9,
                target: 5,
            })
        );
        assert_eq!(log.total(), 10);
    }

    #[test]
    fn recent_out_of_range() {
        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);
        assert_eq!(log.recent(MSG_COUNT as u8), None);
    }

    #[test]
    fn reset_clears() {
        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);
        log.reset();
        assert_eq!(log.recent(0), None);
        assert_eq!(log.total(), 0);
    }
}
