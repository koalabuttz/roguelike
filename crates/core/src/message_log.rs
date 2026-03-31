use crate::rules::health;
use crate::rules::items;
use crate::rules::message::{AutorunStopCause, Combatant, GameEvent, SoundDistance};

/// Entry in the message log: formatted string + optional structured event.
#[derive(serde::Serialize, serde::Deserialize)]
struct LogEntry {
    text: String,
    #[serde(skip)]
    event: Option<GameEvent>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MessageLog {
    entries: Vec<LogEntry>,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageLog {
    pub fn new() -> Self {
        MessageLog {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, msg: impl Into<String>) {
        self.entries.push(LogEntry {
            text: msg.into(),
            event: None,
        });
    }

    /// Add a structured game event, formatting it to a human-readable string.
    pub fn add_event(&mut self, event: GameEvent) {
        self.entries.push(LogEntry {
            text: format_event(event),
            event: Some(event),
        });
    }

    /// Return the last `n` messages as strings (newest last).
    pub fn recent(&self, n: usize) -> Vec<String> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter().map(|e| e.text.clone()).collect()
    }

    /// Return the nth most recent GameEvent (0 = most recent).
    /// Returns None if the entry was a raw string or index is out of range.
    pub fn recent_event(&self, n: usize) -> Option<GameEvent> {
        let len = self.entries.len();
        if n >= len {
            return None;
        }
        self.entries[len - 1 - n].event
    }

    /// Total number of messages ever added.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log contains no messages.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return all messages as strings (oldest first).
    pub fn all(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.text.clone()).collect()
    }

    /// Return all messages added since index `since` (exclusive).
    pub fn messages_since(&self, since: usize) -> Vec<String> {
        self.entries[since..].iter().map(|e| e.text.clone()).collect()
    }
}

/// Convert a `GameEvent` to a human-readable string for the standard-tier
/// message log. Format strings match the original `format!()` calls exactly
/// to preserve golden replay compatibility.
pub fn format_event(event: GameEvent) -> String {
    match event {
        GameEvent::Attack {
            attacker,
            defender,
            damage: _,
        } => format!("{} hits {}.", attacker.name(), defender.name()),
        GameEvent::NoDamage { attacker, defender } => format!(
            "{} attacks {} but deals no damage.",
            attacker.name(),
            defender.name()
        ),
        GameEvent::Kill { victim, .. } => format!("{} is dead!", victim.name()),
        GameEvent::HealthStatus { who, tier } => {
            let desc = health::health_description(tier);
            match who {
                Combatant::Player => format!("You are {}.", desc),
                _ => format!("The {} is {}.", who.name(), desc),
            }
        }
        GameEvent::EntityNotice { who } => match who {
            Combatant::UnknownMonster => format!("{} notices you!", who.name()),
            _ => format!("The {} notices you!", who.name()),
        },
        GameEvent::DrinkPotion { kind, healed } => {
            format!("You drink the {}. (+{} HP)", items::name(kind), healed)
        }
        GameEvent::EquipWeapon { kind, bonus } => {
            format!("You equip the {}. (+{} ATK)", items::name(kind), bonus)
        }
        GameEvent::EquipArmor { kind, bonus } => {
            format!("You equip the {}. (+{} DEF)", items::name(kind), bonus)
        }
        GameEvent::UnequipWeapon { kind } => {
            format!("You unequip the {}.", items::name(kind))
        }
        GameEvent::UnequipArmor { kind } => {
            format!("You unequip the {}.", items::name(kind))
        }
        GameEvent::NoStairs => "There are no stairs here.".into(),
        GameEvent::Descend { depth, target } => {
            format!("You descend to depth {}/{}...", depth, target)
        }
        GameEvent::Victory { depth } => format!(
            "You ascend from the dungeon victorious! You conquered all {} depths!",
            depth
        ),
        GameEvent::Welcome => "Welcome to the dungeon! Prepare yourself.".into(),
        GameEvent::SoundCue { distance } => match distance {
            SoundDistance::Near => "Something is moving very close!".into(),
            SoundDistance::Medium => "You hear footsteps nearby.".into(),
            SoundDistance::Far => "You hear a faint sound in the distance.".into(),
        },
        GameEvent::PlayerDeath => "You have died!".into(),
        GameEvent::PickupItem { kind } => {
            format!("You pick up the {}.", items::name(kind))
        }
        GameEvent::DropItem { kind } => {
            format!("You drop the {}.", items::name(kind))
        }
        GameEvent::InventoryFull => "Your inventory is full.".into(),
        GameEvent::ItemsHere { kind, count } => {
            if count <= 1 {
                format!("You see a {} here.", items::name(kind))
            } else {
                format!("You see {} {}s here.", count, items::name(kind))
            }
        }
        GameEvent::Autorun => "Running...".into(),
        GameEvent::UseStrengthPotion { bonus } => {
            format!("You drink the Potion of Strength. (+{} ATK)", bonus)
        }
        GameEvent::CombineItems { target, source } => {
            format!(
                "You combine the {} with the {}.",
                items::name(target),
                items::name(source)
            )
        }
        GameEvent::CombineNoEffect => "Nothing happens.".into(),
        GameEvent::ItemDestroyed { kind } => {
            format!("Your {} is destroyed!", items::name(kind))
        }
        GameEvent::AutorunStop { cause } => match cause {
            AutorunStopCause::WallReached => "Path blocked.".into(),
            AutorunStopCause::MonsterSpotted => "Monster spotted!".into(),
            AutorunStopCause::DamageTaken => "You take damage!".into(),
            AutorunStopCause::GameOver => "You have died!".into(),
            AutorunStopCause::CorridorBranches => "Path branches.".into(),
            AutorunStopCause::MaxSteps => "You stop running.".into(),
            AutorunStopCause::PathComplete => "Arrived.".into(),
            AutorunStopCause::StairsFound => "You see stairs here.".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::items::ItemKind;
    use crate::rules::monster_table::MonsterKind;

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
    fn all_returns_every_message() {
        let mut log = MessageLog::new();
        assert!(log.all().is_empty());
        log.add("first");
        log.add("second");
        log.add("third");
        assert_eq!(log.all(), &["first", "second", "third"]);
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

    // -----------------------------------------------------------------------
    // format_event tests — verify output matches original format!() strings
    // -----------------------------------------------------------------------

    #[test]
    fn format_attack_damage() {
        let msg = format_event(GameEvent::Attack {
            attacker: Combatant::Player,
            defender: Combatant::Monster(MonsterKind::Goblin),
            damage: 3,
        });
        assert_eq!(msg, "Player hits Goblin.");
    }

    #[test]
    fn format_no_damage() {
        let msg = format_event(GameEvent::NoDamage {
            attacker: Combatant::Monster(MonsterKind::Orc),
            defender: Combatant::Player,
        });
        assert_eq!(msg, "Orc attacks Player but deals no damage.");
    }

    #[test]
    fn format_kill() {
        let msg = format_event(GameEvent::Kill {
            attacker: Combatant::Player,
            victim: Combatant::Monster(MonsterKind::Troll),
        });
        assert_eq!(msg, "Troll is dead!");
    }

    #[test]
    fn format_entity_notice() {
        let msg = format_event(GameEvent::EntityNotice {
            who: Combatant::Monster(MonsterKind::Goblin),
        });
        assert_eq!(msg, "The Goblin notices you!");
    }

    #[test]
    fn format_entity_notice_unknown() {
        let msg = format_event(GameEvent::EntityNotice {
            who: Combatant::UnknownMonster,
        });
        assert_eq!(msg, "Something notices you!");
    }

    #[test]
    fn format_drink_potion() {
        let msg = format_event(GameEvent::DrinkPotion {
            kind: ItemKind::HealthPotion,
            healed: 7,
        });
        assert_eq!(msg, "You drink the Health Potion. (+7 HP)");
    }

    #[test]
    fn format_equip_weapon() {
        let msg = format_event(GameEvent::EquipWeapon {
            kind: ItemKind::ShortSword,
            bonus: 3,
        });
        assert_eq!(msg, "You equip the Short Sword. (+3 ATK)");
    }

    #[test]
    fn format_equip_armor() {
        let msg = format_event(GameEvent::EquipArmor {
            kind: ItemKind::LeatherArmor,
            bonus: 2,
        });
        assert_eq!(msg, "You equip the Leather Armor. (+2 DEF)");
    }

    #[test]
    fn format_unequip_weapon() {
        let msg = format_event(GameEvent::UnequipWeapon {
            kind: ItemKind::ShortSword,
        });
        assert_eq!(msg, "You unequip the Short Sword.");
    }

    #[test]
    fn format_unequip_armor() {
        let msg = format_event(GameEvent::UnequipArmor {
            kind: ItemKind::LeatherArmor,
        });
        assert_eq!(msg, "You unequip the Leather Armor.");
    }

    #[test]
    fn format_no_stairs() {
        assert_eq!(
            format_event(GameEvent::NoStairs),
            "There are no stairs here."
        );
    }

    #[test]
    fn format_descend() {
        let msg = format_event(GameEvent::Descend {
            depth: 2,
            target: 5,
        });
        assert_eq!(msg, "You descend to depth 2/5...");
    }

    #[test]
    fn format_victory() {
        let msg = format_event(GameEvent::Victory { depth: 5 });
        assert_eq!(
            msg,
            "You ascend from the dungeon victorious! You conquered all 5 depths!"
        );
    }

    #[test]
    fn format_welcome() {
        assert_eq!(
            format_event(GameEvent::Welcome),
            "Welcome to the dungeon! Prepare yourself."
        );
    }

    #[test]
    fn format_sound_cues() {
        assert_eq!(
            format_event(GameEvent::SoundCue {
                distance: SoundDistance::Near
            }),
            "Something is moving very close!"
        );
        assert_eq!(
            format_event(GameEvent::SoundCue {
                distance: SoundDistance::Medium
            }),
            "You hear footsteps nearby."
        );
        assert_eq!(
            format_event(GameEvent::SoundCue {
                distance: SoundDistance::Far
            }),
            "You hear a faint sound in the distance."
        );
    }

    #[test]
    fn format_pickup_item() {
        let msg = format_event(GameEvent::PickupItem {
            kind: ItemKind::HealthPotion,
        });
        assert_eq!(msg, "You pick up the Health Potion.");
    }

    #[test]
    fn format_drop_item() {
        let msg = format_event(GameEvent::DropItem {
            kind: ItemKind::ShortSword,
        });
        assert_eq!(msg, "You drop the Short Sword.");
    }

    #[test]
    fn format_inventory_full() {
        assert_eq!(
            format_event(GameEvent::InventoryFull),
            "Your inventory is full."
        );
    }

    #[test]
    fn format_items_here_single() {
        let msg = format_event(GameEvent::ItemsHere {
            kind: ItemKind::HealthPotion,
            count: 1,
        });
        assert_eq!(msg, "You see a Health Potion here.");
    }

    #[test]
    fn format_items_here_multiple() {
        let msg = format_event(GameEvent::ItemsHere {
            kind: ItemKind::HealthPotion,
            count: 3,
        });
        assert_eq!(msg, "You see 3 Health Potions here.");
    }

    #[test]
    fn add_event_stores_formatted_string() {
        let mut log = MessageLog::new();
        log.add_event(GameEvent::Welcome);
        assert_eq!(
            log.recent(1)[0],
            "Welcome to the dungeon! Prepare yourself."
        );
    }
}
