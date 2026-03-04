use crate::rules::items;
use crate::rules::message::{AutorunStopCause, Combatant, GameEvent, SoundDistance};

#[derive(serde::Serialize, serde::Deserialize)]
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

    /// Add a structured game event, formatting it to a human-readable string.
    pub fn add_event(&mut self, event: GameEvent) {
        self.messages.push(format_event(event));
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

    /// Return all messages (oldest first).
    pub fn all(&self) -> &[String] {
        &self.messages
    }

    /// Return all messages added since index `since` (exclusive).
    pub fn messages_since(&self, since: usize) -> Vec<String> {
        self.messages[since..].to_vec()
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
            damage,
        } => format!(
            "{} attacks {} for {} damage.",
            attacker.name(),
            defender.name(),
            damage
        ),
        GameEvent::NoDamage { attacker, defender } => format!(
            "{} attacks {} but does no damage.",
            attacker.name(),
            defender.name()
        ),
        GameEvent::Kill { victim, .. } => format!("{} is dead!", victim.name()),
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
        GameEvent::Autorun => "Running...".into(),
        GameEvent::AutorunStop { cause } => match cause {
            AutorunStopCause::WallReached => "Path blocked.".into(),
            AutorunStopCause::MonsterSpotted => "Monster spotted!".into(),
            AutorunStopCause::DamageTaken => "You take damage!".into(),
            AutorunStopCause::GameOver => "You have died!".into(),
            AutorunStopCause::CorridorBranches => "Path branches.".into(),
            AutorunStopCause::MaxSteps => "You stop running.".into(),
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
        assert_eq!(msg, "Player attacks Goblin for 3 damage.");
    }

    #[test]
    fn format_no_damage() {
        let msg = format_event(GameEvent::NoDamage {
            attacker: Combatant::Monster(MonsterKind::Orc),
            defender: Combatant::Player,
        });
        assert_eq!(msg, "Orc attacks Player but does no damage.");
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
    fn add_event_stores_formatted_string() {
        let mut log = MessageLog::new();
        log.add_event(GameEvent::Welcome);
        assert_eq!(
            log.recent(1)[0],
            "Welcome to the dungeon! Prepare yourself."
        );
    }
}
