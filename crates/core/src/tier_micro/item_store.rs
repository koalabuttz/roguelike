//! Fixed-size parallel array item storage for the micro tier.
//!
//! Items on the ground occupy slots 0..count. Removed items are marked
//! `alive = false` and skipped during queries (same pattern as EntityStore).

use crate::rules::balance;
use crate::rules::items::ItemKind;

pub const MAX_ITEMS: usize = balance::MICRO_MAX_ITEMS as usize;
pub const NO_ITEM: u8 = 0xFF;

pub struct ItemStore {
    pub x: [u8; MAX_ITEMS],
    pub y: [u8; MAX_ITEMS],
    pub kind: [ItemKind; MAX_ITEMS],
    pub alive: [bool; MAX_ITEMS],
    pub count: u8,
}

impl Default for ItemStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemStore {
    pub fn new() -> Self {
        Self {
            x: [0; MAX_ITEMS],
            y: [0; MAX_ITEMS],
            kind: [ItemKind::HealthPotion; MAX_ITEMS],
            alive: [false; MAX_ITEMS],
            count: 0,
        }
    }

    /// Place an item on the ground. Returns false if the store is full.
    ///
    /// Reuses dead slots before appending, so removing and re-spawning
    /// items within a floor won't exhaust the store prematurely.
    pub fn spawn(&mut self, ix: u8, iy: u8, item_kind: ItemKind) -> bool {
        // Scan for a dead slot to reuse.
        for i in 0..self.count as usize {
            if !self.alive[i] {
                self.x[i] = ix;
                self.y[i] = iy;
                self.kind[i] = item_kind;
                self.alive[i] = true;
                return true;
            }
        }
        // No dead slots — append if there's room.
        if (self.count as usize) < MAX_ITEMS {
            let i = self.count as usize;
            self.x[i] = ix;
            self.y[i] = iy;
            self.kind[i] = item_kind;
            self.alive[i] = true;
            self.count += 1;
            return true;
        }
        false
    }

    /// Find the first alive item at position. Returns slot index or NO_ITEM.
    pub fn item_at(&self, ix: u8, iy: u8) -> u8 {
        for i in 0..self.count {
            let idx = i as usize;
            if self.alive[idx] && self.x[idx] == ix && self.y[idx] == iy {
                return i;
            }
        }
        NO_ITEM
    }

    /// Remove an item by marking it dead.
    pub fn remove(&mut self, i: u8) {
        self.alive[i as usize] = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_retrieve() {
        let mut items = ItemStore::new();
        assert!(items.spawn(10, 20, ItemKind::ShortSword));
        assert_eq!(items.count, 1);
        let idx = items.item_at(10, 20);
        assert_ne!(idx, NO_ITEM);
        assert_eq!(items.kind[idx as usize], ItemKind::ShortSword);
    }

    #[test]
    fn remove_marks_dead() {
        let mut items = ItemStore::new();
        items.spawn(5, 5, ItemKind::HealthPotion);
        items.remove(0);
        assert!(!items.alive[0]);
        assert_eq!(items.item_at(5, 5), NO_ITEM);
    }

    #[test]
    fn spawn_beyond_max_returns_false() {
        let mut items = ItemStore::new();
        for i in 0..MAX_ITEMS {
            assert!(items.spawn(i as u8, 0, ItemKind::HealthPotion));
        }
        assert!(!items.spawn(99, 99, ItemKind::HealthPotion));
    }

    #[test]
    fn multiple_items_at_same_position() {
        let mut items = ItemStore::new();
        items.spawn(5, 5, ItemKind::HealthPotion);
        items.spawn(5, 5, ItemKind::ShortSword);
        // item_at returns first alive match
        let first = items.item_at(5, 5);
        assert_eq!(items.kind[first as usize], ItemKind::HealthPotion);
        // Remove first, second becomes visible
        items.remove(first);
        let second = items.item_at(5, 5);
        assert_eq!(items.kind[second as usize], ItemKind::ShortSword);
    }

    #[test]
    fn item_at_empty_returns_no_item() {
        let items = ItemStore::new();
        assert_eq!(items.item_at(0, 0), NO_ITEM);
    }

    #[test]
    fn spawn_reuses_dead_slots() {
        let mut items = ItemStore::new();
        items.spawn(1, 1, ItemKind::HealthPotion);
        items.spawn(2, 2, ItemKind::ShortSword);
        let count_before = items.count;

        // Kill slot 0, then spawn a new item — should reuse slot 0.
        items.remove(0);
        assert!(items.spawn(9, 9, ItemKind::LeatherArmor));
        assert_eq!(items.count, count_before, "count should not grow");
        assert!(items.alive[0]);
        assert_eq!(items.kind[0], ItemKind::LeatherArmor);
        assert_eq!(items.x[0], 9);
    }
}
