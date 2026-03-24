//! Top-level micro-tier game state and step API.
//!
//! `MicroGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.

use super::ai;
use super::combat;
use super::entity::EntityStore;
use super::fov::MicroFov;
use super::item_store::ItemStore;
use super::map::{MicroMap, TILE_STAIRS_DOWN};
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::spawn;
use super::types::*;
use crate::command::GameCommand;
use crate::rules::balance;
use crate::rules::damage;
use crate::rules::interactions;
use crate::rules::items::{self as rules_items, Equipment, Inventory};
use crate::rules::message::{GameEvent, SoundDistance};
use crate::rules::monster_table::{AiBehavior, MonsterKind};

/// Result of a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroStepResult {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

pub struct MicroGameState {
    pub map: MicroMap,
    pub fov: MicroFov,
    pub entities: EntityStore,
    pub items: ItemStore,
    pub equipment: Equipment,
    pub inventory: Inventory,
    pub log: MicroMessageLog,
    pub rng: LfsrRng16,
    /// Original seed used to create this game (for display/sharing).
    pub seed: u16,
    pub turn_count: u16,
    pub kills: u8,
    pub depth: u8,
    pub game_over: bool,
    pub game_won: bool,
    /// Consecutive Wait commands (resets on any non-wait action).
    pub idle_count: u8,
    /// Total wandering monsters spawned this game (for analytics).
    pub wandering_spawned: u8,
    /// Counts down each turn; triggers regen at zero and resets.
    /// Avoids modulo on 6502 where division is expensive.
    pub(crate) regen_counter: u8,
    /// Counts down each turn; triggers wandering spawn check at zero.
    /// Uses the same decrement pattern as regen_counter to avoid modulo.
    pub(crate) wandering_counter: u8,
    /// Auto-pickup consumable items when walking over them (runtime setting, not saved).
    pub auto_pickup: bool,
    /// Counts down each turn; triggers ambient sound check at zero.
    pub(crate) ambient_sound_counter: u8,
}

impl MicroGameState {
    /// Create a new game with the given seed and map dimensions.
    pub fn new(seed: u16, width: u8, height: u8) -> Self {
        // Safety: new_into initializes all fields.
        let mut state = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::new_into(state.as_mut_ptr(), seed, width, height);
            state.assume_init()
        }
    }

    /// Initialize a game directly at the given destination pointer.
    /// Avoids allocating a temporary `MicroGameState` on the static stack,
    /// which on rust-mos (6502) would cost ~4.5 KB of `.noinit`.
    ///
    /// # Safety
    /// `dest` must point to valid, writable memory for one `MicroGameState`.
    pub unsafe fn new_into(dest: *mut Self, seed: u16, width: u8, height: u8) {
        // Safety: caller guarantees dest points to valid writable memory.
        let s = unsafe { &mut *dest };
        s.rng = LfsrRng16::new(seed);
        s.map = MicroMap::new(width, height);
        let (sx, sy) = s.map.generate(&mut s.rng);

        s.entities = EntityStore::new();
        s.entities.spawn_player(sx, sy);
        spawn::spawn_monsters(&mut s.entities, &s.map, &mut s.rng);

        s.items = ItemStore::new();
        spawn::spawn_items(&mut s.items, &s.map, 1, &mut s.rng);

        s.fov = MicroFov::new(width, height);
        s.fov.compute_fov(sx, sy, &s.map);

        s.log = MicroMessageLog::new();
        s.log.add(GameEvent::Welcome);

        s.equipment = Equipment::default();
        s.inventory = Inventory::new();
        s.seed = seed;
        s.turn_count = 0;
        s.kills = 0;
        s.depth = 1;
        s.game_over = false;
        s.game_won = false;
        s.idle_count = 0;
        s.wandering_spawned = 0;
        s.regen_counter = balance::REGEN_INTERVAL;
        s.wandering_counter = balance::WANDERING_GRACE_PERIOD - 1;
        s.ambient_sound_counter = balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1;
        s.auto_pickup = false;
    }

    /// Returns true if the game has reached a terminal state (death or victory).
    pub fn is_terminal(&self) -> bool {
        self.game_over || self.game_won
    }

    /// Create a new game with C64 default dimensions (64×48).
    pub fn new_default(seed: u16) -> Self {
        Self::new(seed, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT)
    }

    /// Execute one player command + monster turns + regen, but skip FOV.
    ///
    /// Used by autorun for intermediate steps where full shadowcasting
    /// is unnecessary. Monster AI still runs (uses Bresenham LOS, not FOV).
    /// Caller must ensure `compute_fov` is called before reading visibility.
    pub fn step_skip_fov(&mut self, cmd: GameCommand) -> MicroStepResult {
        self.step_inner(cmd, false)
    }

    /// Execute one player command + monster turns + regen.
    ///
    /// Accepts the full `GameCommand` enum. Variants the micro tier doesn't
    /// support (Autorun, AutoExplore, Look, etc.) are silently ignored.
    pub fn step(&mut self, cmd: GameCommand) -> MicroStepResult {
        self.step_inner(cmd, true)
    }

    fn step_inner(&mut self, cmd: GameCommand, compute_fov: bool) -> MicroStepResult {
        if self.is_terminal() {
            return MicroStepResult {
                action_taken: false,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        // Descent is handled separately — it rebuilds the level and FOV.
        if matches!(cmd, GameCommand::Descend) {
            let descended = self.descend();
            return MicroStepResult {
                action_taken: descended,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        let old_px = self.entities.x[PLAYER_IDX as usize];
        let old_py = self.entities.y[PLAYER_IDX as usize];

        let is_wait = matches!(cmd, GameCommand::Wait);
        let action_taken = match cmd {
            GameCommand::Wait => true,
            GameCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                self.player_move_or_attack(dx as i8, dy as i8)
            }
            GameCommand::Pickup => self.pickup_item(),
            GameCommand::UseItem(slot) => self.use_item(slot),
            GameCommand::DropItem(slot) => self.drop_item(slot),
            GameCommand::EquipItem(slot) => self.equip_item(slot),
            GameCommand::UnequipWeapon => self.unequip_weapon(),
            GameCommand::UnequipArmor => self.unequip_armor(),
            GameCommand::DropEquippedWeapon => self.drop_equipped_weapon(),
            GameCommand::DropEquippedArmor => self.drop_equipped_armor(),
            GameCommand::Combine(target, source) => self.combine_items(target, source),
            // UI-only / unsupported variants — no action taken
            GameCommand::OpenInventory
            | GameCommand::Autorun(_)
            | GameCommand::AutoExplore
            | GameCommand::Look
            | GameCommand::Help
            | GameCommand::MessageHistory
            | GameCommand::Quit
            | GameCommand::Descend => false,
        };

        if action_taken {
            if is_wait {
                self.idle_count = self.idle_count.saturating_add(1);
            } else {
                self.idle_count = 0;
            }

            if compute_fov {
                let px = self.entities.x[PLAYER_IDX as usize];
                let py = self.entities.y[PLAYER_IDX as usize];
                if px != old_px || py != old_py {
                    self.fov.compute_fov(px, py, &self.map);
                }
            }

            let player_def = self.effective_defense();
            let player_died = ai::run_monster_turns(
                &mut self.entities,
                &self.map,
                &mut self.rng,
                &mut self.log,
                player_def,
            );
            if player_died {
                self.game_over = true;
                self.log.add(GameEvent::PlayerDeath);
            }

            self.turn_count += 1;
            if !self.game_won {
                self.try_spawn_wandering();
                self.emit_ambient_sound_cues();
            }
            self.apply_regen();
        }

        MicroStepResult {
            action_taken,
            game_over: self.game_over,
            game_won: self.game_won,
        }
    }

    /// Descend to the next dungeon level. Returns true if descent succeeded.
    fn descend(&mut self) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        // Must be standing on stairs
        if self.map.tile_at(px, py) != TILE_STAIRS_DOWN {
            self.log.add(GameEvent::NoStairs);
            return false;
        }

        // Victory condition: descending from the final floor
        if self.depth >= balance::TARGET_DEPTH {
            self.game_won = true;
            self.log.add(GameEvent::Victory {
                depth: balance::TARGET_DEPTH,
            });
            return true;
        }

        self.depth += 1;

        // Derive deterministic seed for this floor
        let floor_seed = self.seed ^ (self.depth as u16).wrapping_mul(0x9E37);
        self.rng = LfsrRng16::new(floor_seed);

        // Save player stats
        let hp = self.entities.hp[pi];
        let max_hp = self.entities.max_hp[pi];
        let atk = self.entities.atk[pi];
        let def = self.entities.def[pi];

        // Generate new map
        let w = self.map.width;
        let h = self.map.height;
        self.map = MicroMap::new(w, h);
        let (sx, sy) = self.map.generate(&mut self.rng);

        // Reset entities — player keeps stats
        self.entities = EntityStore::new();
        self.entities.spawn_player(sx, sy);
        self.entities.hp[pi] = hp;
        self.entities.max_hp[pi] = max_hp;
        self.entities.atk[pi] = atk;
        self.entities.def[pi] = def;

        // Spawn and scale monsters, spawn items
        spawn::spawn_monsters(&mut self.entities, &self.map, &mut self.rng);
        spawn::apply_depth_scaling(&mut self.entities, self.depth);
        self.items = ItemStore::new();
        spawn::spawn_items(&mut self.items, &self.map, self.depth, &mut self.rng);

        // Reset FOV
        self.fov = MicroFov::new(w, h);
        self.fov.compute_fov(sx, sy, &self.map);

        // Reset wandering state for new floor.
        self.idle_count = 0;
        self.wandering_spawned = 0;
        self.wandering_counter = balance::WANDERING_GRACE_PERIOD - 1;
        self.ambient_sound_counter = balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1;

        // Reset message log for new floor
        self.log.reset();
        self.log.add(GameEvent::Descend {
            depth: self.depth,
            target: balance::TARGET_DEPTH,
        });

        true
    }

    fn player_move_or_attack(&mut self, dx: i8, dy: i8) -> bool {
        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];
        let nx = (px as i8 + dx) as u8;
        let ny = (py as i8 + dy) as u8;

        // Check for monster at target position
        let target = self.entities.monster_at(nx, ny);
        if target != NO_ENTITY {
            let atk = self.effective_attack();
            let def = self.entities.def[target as usize];
            let killed = combat::melee_attack(
                PLAYER_IDX,
                target,
                atk,
                def,
                &mut self.entities,
                &mut self.log,
            );
            if killed {
                self.kills += 1;
            }
            return true;
        }

        // Try to move
        if self.map.is_walkable(nx, ny) {
            self.entities.x[PLAYER_IDX as usize] = nx;
            self.entities.y[PLAYER_IDX as usize] = ny;
            self.notify_items_here(nx, ny);
            return true;
        }

        false
    }

    /// Player's effective attack (base + weapon bonus).
    pub fn effective_attack(&self) -> u8 {
        let base = self.entities.atk[PLAYER_IDX as usize];
        damage::effective_attack(base, self.equipment.attack_bonus())
    }

    /// Player's effective defense (base + armor bonus).
    pub fn effective_defense(&self) -> u8 {
        let base = self.entities.def[PLAYER_IDX as usize];
        damage::effective_defense(base, self.equipment.defense_bonus())
    }

    /// Notify the player about items on the ground at their position.
    /// When auto_pickup is enabled, consumables are picked up first.
    fn notify_items_here(&mut self, x: u8, y: u8) {
        if self.auto_pickup {
            self.auto_pickup_items(x, y);
        }
        // Notify about remaining items (if inventory was full).
        let mut counts = [0u8; rules_items::KIND_COUNT];
        for i in 0..self.items.count as usize {
            if self.items.alive[i] && self.items.x[i] == x && self.items.y[i] == y {
                counts[self.items.kind[i] as usize] += 1;
            }
        }
        for (idx, &count) in counts.iter().enumerate() {
            if count > 0 {
                self.log.add(GameEvent::ItemsHere {
                    kind: rules_items::ALL_KINDS[idx],
                    count,
                });
            }
        }
    }

    /// Auto-pickup all items at (x, y).
    fn auto_pickup_items(&mut self, x: u8, y: u8) {
        loop {
            let mut found: Option<u8> = None;
            for i in 0..self.items.count as usize {
                if self.items.alive[i] && self.items.x[i] == x && self.items.y[i] == y {
                    found = Some(i as u8);
                    break;
                }
            }
            let Some(idx) = found else { break };
            let kind = self.items.kind[idx as usize];
            if !self.inventory.add(kind) {
                break; // inventory full
            }
            self.items.remove(idx);
            self.log.add(GameEvent::PickupItem { kind });
        }
    }

    /// Pick up the first item at the player's position.
    fn pickup_item(&mut self) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        for i in 0..self.items.count as usize {
            if self.items.alive[i] && self.items.x[i] == px && self.items.y[i] == py {
                let kind = self.items.kind[i];
                if !self.inventory.add(kind) {
                    self.log.add(GameEvent::InventoryFull);
                    return true; // turn consumed even on failure
                }
                self.items.remove(i as u8);
                self.log.add(GameEvent::PickupItem { kind });
                return true;
            }
        }
        false // nothing to pick up
    }

    /// Use an item from inventory (consumables only).
    fn use_item(&mut self, slot: u8) -> bool {
        let inv_slot = match self.inventory.get(slot as usize) {
            Some(s) => *s,
            None => return false,
        };

        if rules_items::is_consumable(inv_slot.kind) {
            let heal = rules_items::heal_amount(inv_slot.kind);
            if heal > 0 {
                let pi = PLAYER_IDX as usize;
                let hp = self.entities.hp[pi];
                let max_hp = self.entities.max_hp[pi];
                let healed = heal.min(max_hp.saturating_sub(hp));
                self.entities.hp[pi] = hp.saturating_add(healed);
                self.inventory.remove_one(slot as usize);
                self.log.add(GameEvent::DrinkPotion {
                    kind: inv_slot.kind,
                    healed,
                });
                return true;
            }
            let boost = rules_items::strength_boost(inv_slot.kind);
            if boost > 0 {
                let pi = PLAYER_IDX as usize;
                self.entities.atk[pi] = self.entities.atk[pi].saturating_add(boost);
                self.inventory.remove_one(slot as usize);
                self.log.add(GameEvent::UseStrengthPotion { bonus: boost });
                return true;
            }
        }
        false
    }

    /// Combine two inventory items: apply source's properties onto target.
    fn combine_items(&mut self, target_slot: u8, source_slot: u8) -> bool {
        if target_slot == source_slot {
            return false;
        }
        let target = match self.inventory.get(target_slot as usize) {
            Some(s) => *s,
            None => return false,
        };
        let source = match self.inventory.get(source_slot as usize) {
            Some(s) => *s,
            None => return false,
        };

        let mut a_props = target.props;
        let mut b_props = source.props;
        let mut effects = [interactions::Effect {
            effect_type: interactions::EffectType::Glow,
            intensity: 0,
        }; interactions::MAX_EFFECTS];

        let _effect_count = interactions::interact(&mut a_props, &mut b_props, &mut effects);

        if a_props == target.props && b_props == source.props {
            self.log.add(GameEvent::CombineNoEffect);
            return false;
        }

        // Consume source first (if consumable) so it can't interfere with
        // target re-insertion, and to free a slot for the split target.
        // Non-consumable source props are updated AFTER the target succeeds,
        // so the undo path doesn't need to revert them.
        if rules_items::is_consumable(source.kind) {
            self.inventory.remove_one(source_slot as usize);
        }

        // Remove target and re-add with modified props. This maintains the
        // stacking invariant: if the modified props match an existing stack,
        // it correctly merges instead of creating a duplicate.
        self.inventory.remove_one(target_slot as usize);
        if !self.inventory.add_with_props(target.kind, a_props) {
            // Inventory full — undo everything.
            // These re-inserts must succeed: we just freed the slot(s).
            let ok = self.inventory.add_with_props(target.kind, target.props);
            debug_assert!(ok, "undo target re-insert must succeed");
            if rules_items::is_consumable(source.kind) {
                let ok = self.inventory.add_with_props(source.kind, source.props);
                debug_assert!(ok, "undo source re-insert must succeed");
            }
            self.log.add(GameEvent::InventoryFull);
            return false;
        }

        // Update non-consumable source props after target succeeded.
        // source_slot is still valid: inventory uses fixed-position slots
        // (no compaction on remove), so absolute indices are stable.
        if !rules_items::is_consumable(source.kind) {
            self.inventory.set_props(source_slot as usize, b_props);
        }

        self.log.add(GameEvent::CombineItems {
            target: target.kind,
            source: source.kind,
        });

        true
    }

    /// Drop an item from inventory onto the ground.
    fn drop_item(&mut self, slot: u8) -> bool {
        if let Some(kind) = self.inventory.remove_one(slot as usize) {
            let pi = PLAYER_IDX as usize;
            let px = self.entities.x[pi];
            let py = self.entities.y[pi];
            self.items.spawn(px, py, kind);
            self.log.add(GameEvent::DropItem { kind });
            return true;
        }
        false
    }

    /// Equip an item from inventory (weapon or armor).
    fn equip_item(&mut self, slot: u8) -> bool {
        let inv_slot = match self.inventory.get(slot as usize) {
            Some(s) => *s,
            None => return false,
        };
        let kind = inv_slot.kind;
        let props = inv_slot.props;

        if rules_items::is_weapon(kind) {
            self.inventory.remove_one(slot as usize);
            // Swap old weapon into inventory if present.
            if let Some(old) = self.equipment.weapon {
                self.inventory
                    .add_with_props(old, self.equipment.weapon_props);
            }
            self.equipment.weapon = Some(kind);
            self.equipment.weapon_props = props;
            self.log.add(GameEvent::EquipWeapon {
                kind,
                bonus: rules_items::attack_from_bag(&props),
            });
            true
        } else if rules_items::is_armor(kind) {
            self.inventory.remove_one(slot as usize);
            if let Some(old) = self.equipment.armor {
                self.inventory
                    .add_with_props(old, self.equipment.armor_props);
            }
            self.equipment.armor = Some(kind);
            self.equipment.armor_props = props;
            self.log.add(GameEvent::EquipArmor {
                kind,
                bonus: rules_items::defense_from_bag(&props),
            });
            true
        } else {
            false
        }
    }

    /// Unequip the current weapon, returning it to inventory.
    fn unequip_weapon(&mut self) -> bool {
        if let Some(kind) = self.equipment.weapon {
            if !self
                .inventory
                .add_with_props(kind, self.equipment.weapon_props)
            {
                self.log.add(GameEvent::InventoryFull);
                return false;
            }
            self.equipment.weapon = None;
            self.equipment.weapon_props = crate::rules::properties::EMPTY;
            self.log.add(GameEvent::UnequipWeapon { kind });
            true
        } else {
            false
        }
    }

    /// Unequip the current armor, returning it to inventory.
    fn unequip_armor(&mut self) -> bool {
        if let Some(kind) = self.equipment.armor {
            if !self
                .inventory
                .add_with_props(kind, self.equipment.armor_props)
            {
                self.log.add(GameEvent::InventoryFull);
                return false;
            }
            self.equipment.armor = None;
            self.equipment.armor_props = crate::rules::properties::EMPTY;
            self.log.add(GameEvent::UnequipArmor { kind });
            true
        } else {
            false
        }
    }

    /// Drop an equipped weapon directly to the ground (bypasses inventory).
    /// Note: ground ItemStore doesn't carry PropertyBags yet — bag is lost on drop.
    fn drop_equipped_weapon(&mut self) -> bool {
        if let Some(kind) = self.equipment.weapon.take() {
            self.equipment.weapon_props = crate::rules::properties::EMPTY;
            let pi = PLAYER_IDX as usize;
            let px = self.entities.x[pi];
            let py = self.entities.y[pi];
            self.items.spawn(px, py, kind);
            self.log.add(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    /// Drop equipped armor directly to the ground (bypasses inventory).
    /// Note: ground ItemStore doesn't carry PropertyBags yet — bag is lost on drop.
    fn drop_equipped_armor(&mut self) -> bool {
        if let Some(kind) = self.equipment.armor.take() {
            self.equipment.armor_props = crate::rules::properties::EMPTY;
            let pi = PLAYER_IDX as usize;
            let px = self.entities.x[pi];
            let py = self.entities.y[pi];
            self.items.spawn(px, py, kind);
            self.log.add(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    fn apply_regen(&mut self) {
        if self.game_over {
            return;
        }
        self.regen_counter -= 1;
        if self.regen_counter == 0 {
            self.regen_counter = balance::REGEN_INTERVAL;
            let pi = PLAYER_IDX as usize;
            let hp = self.entities.hp[pi];
            let max_hp = self.entities.max_hp[pi];
            if hp < max_hp {
                self.entities.hp[pi] = hp + 1;
            }
        }
    }

    /// Attempt to spawn a wandering monster offscreen if conditions are met.
    ///
    /// Mirrors the standard tier's `try_spawn_wandering`: grace period, spawn
    /// interval (with idle acceleration), random chance, wandering cap, entity
    /// budget. Spawns in a random room the player isn't in, outside FOV.
    ///
    /// Uses a countdown counter (like `regen_counter`) to avoid modulo on 6502.
    /// The counter starts at `WANDERING_GRACE_PERIOD`, counts down each turn,
    /// then reloads at the spawn interval (halved when idle).
    fn try_spawn_wandering(&mut self) {
        if self.wandering_counter > 0 {
            self.wandering_counter -= 1;
            return;
        }

        // Reload counter: base interval, halved if idle.
        // Uses a right-shift instead of division — safe for 6502 as long as
        // WANDERING_IDLE_ACCELERATION is a power of 2 (enforced at compile time).
        let base = balance::WANDERING_SPAWN_INTERVAL;
        let interval = if self.idle_count >= balance::WANDERING_IDLE_THRESHOLD {
            (base >> balance::WANDERING_IDLE_ACCEL_SHIFT).max(1)
        } else {
            base
        };
        self.wandering_counter = interval - 1;

        if self.rng.range_u8(0, 99) >= balance::WANDERING_SPAWN_CHANCE {
            return;
        }

        if self.entities.count as usize >= MAX_ENTITIES {
            return;
        }

        // Cap alive wandering monsters.
        let mut wander_alive: u8 = 0;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] && self.entities.ai[i] == AiBehavior::Wander {
                wander_alive += 1;
            }
        }
        if wander_alive >= balance::WANDERING_MAX_ACTIVE {
            return;
        }

        if let Some((sx, sy)) = self.pick_offscreen_spawn_pos() {
            let kind = spawn::pick_monster_kind(&mut self.rng);
            if self
                .entities
                .spawn_monster(kind, sx, sy, AiBehavior::Wander)
            {
                let idx = (self.entities.count - 1) as usize;
                spawn::scale_monster(&mut self.entities, idx, self.depth);
                self.wandering_spawned += 1;
                self.emit_spawn_sound_cue(sx, sy);
            }
        }
    }

    /// Pick a random floor tile in a room the player isn't in,
    /// outside the player's FOV and not occupied by another entity.
    fn pick_offscreen_spawn_pos(&mut self) -> Option<(u8, u8)> {
        if self.map.room_count == 0 {
            return None;
        }

        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];

        for _ in 0..10 {
            let room_idx = self.rng.range_u8(0, self.map.room_count - 1) as usize;
            let room = self.map.rooms[room_idx];

            // Skip rooms the player is standing in.
            if room.contains_interior(px, py) {
                continue;
            }

            // Pick a random floor tile inside the room interior.
            if room.w < 3 || room.h < 3 {
                continue;
            }
            let sx = self.rng.range_u8(room.x + 1, room.x + room.w - 1);
            let sy = self.rng.range_u8(room.y + 1, room.y + room.h - 1);

            if !self.map.is_walkable(sx, sy) {
                continue;
            }
            if self.fov.is_visible(sx, sy) {
                continue;
            }
            if self.entities.entity_at(sx, sy) != NO_ENTITY {
                continue;
            }
            return Some((sx, sy));
        }
        None
    }

    /// Emit a distance-based sound cue when a wandering monster spawns.
    fn emit_spawn_sound_cue(&mut self, sx: u8, sy: u8) {
        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];
        let dist = px.abs_diff(sx) + py.abs_diff(sy);

        let distance = if dist <= balance::WANDERING_SOUND_NEAR {
            Some(SoundDistance::Near)
        } else if dist <= balance::WANDERING_SOUND_MEDIUM {
            Some(SoundDistance::Medium)
        } else if dist <= balance::WANDERING_SOUND_FAR {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add(GameEvent::SoundCue { distance });
        }
    }

    /// Emit ambient sound cues for nearby wandering monsters.
    ///
    /// Uses a countdown counter to avoid modulo. Bails early if a Near
    /// monster is found — no need to scan the rest.
    fn emit_ambient_sound_cues(&mut self) {
        if self.ambient_sound_counter > 0 {
            self.ambient_sound_counter -= 1;
            return;
        }
        self.ambient_sound_counter = balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1;

        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];

        let mut closest_dist: u8 = u8::MAX;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] && self.entities.ai[i] == AiBehavior::Wander {
                let dist = px.abs_diff(self.entities.x[i]) + py.abs_diff(self.entities.y[i]);
                if dist < closest_dist {
                    closest_dist = dist;
                    // Near is the tightest threshold — can't improve, bail.
                    if closest_dist <= balance::WANDERING_SOUND_NEAR {
                        break;
                    }
                }
            }
        }

        let distance = if closest_dist <= balance::WANDERING_SOUND_NEAR {
            Some(SoundDistance::Near)
        } else if closest_dist <= balance::WANDERING_SOUND_MEDIUM {
            Some(SoundDistance::Medium)
        } else if closest_dist <= balance::WANDERING_SOUND_FAR {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add(GameEvent::SoundCue { distance });
        }
    }

    /// Fight the weakest adjacent monster to the death.
    ///
    /// Returns `None` if no adjacent monster exists. Each round calls
    /// `step(Move(dir))`, so other monsters act and regen ticks.
    pub fn auto_fight(&mut self) -> Option<MicroAutoFightResult> {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        // Find weakest adjacent monster (lowest HP).
        let mut best_idx: u8 = NO_ENTITY;
        let mut best_hp: u8 = u8::MAX;
        let mut i: u8 = 1;
        while (i as usize) < self.entities.count as usize {
            let idx = i as usize;
            if self.entities.alive[idx] {
                let dx = self.entities.x[idx].abs_diff(px);
                let dy = self.entities.y[idx].abs_diff(py);
                if dx <= 1 && dy <= 1 && self.entities.hp[idx] < best_hp {
                    best_hp = self.entities.hp[idx];
                    best_idx = i;
                }
            }
            i += 1;
        }

        if best_idx == NO_ENTITY {
            return None;
        }

        let hp_before = self.entities.hp[pi];
        let target_kind = self.entities.kind[best_idx as usize];
        let mut rounds: u8 = 0;

        loop {
            if !self.entities.alive[best_idx as usize] {
                break;
            }

            // Recompute direction to target.
            let tx = self.entities.x[best_idx as usize];
            let ty = self.entities.y[best_idx as usize];
            if tx.abs_diff(self.entities.x[pi]) > 1 || ty.abs_diff(self.entities.y[pi]) > 1 {
                break; // Target moved out of melee range.
            }

            let ox = tx as i32 - self.entities.x[pi] as i32;
            let oy = ty as i32 - self.entities.y[pi] as i32;
            let cmd = GameCommand::move_or_wait(ox, oy);
            let result = self.step(cmd);
            rounds += 1;

            if result.game_over {
                break;
            }
        }

        Some(MicroAutoFightResult {
            rounds,
            target_idx: best_idx,
            target_kind,
            target_killed: !self.entities.alive[best_idx as usize],
            player_hp_lost: hp_before.saturating_sub(self.entities.hp[pi]),
        })
    }
}

/// Result of a micro-tier auto-fight (no_std, Copy).
pub struct MicroAutoFightResult {
    pub rounds: u8,
    pub target_idx: u8,
    pub target_kind: Option<MonsterKind>,
    pub target_killed: bool,
    pub player_hp_lost: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Direction;
    use crate::rules::items::ItemKind;

    #[test]
    fn new_game_is_playable() {
        let g = MicroGameState::new_default(42);
        assert!(!g.game_over);
        assert!(g.entities.alive[PLAYER_IDX as usize]);
        assert!(g.entities.hp[PLAYER_IDX as usize] > 0);
        assert!(g.entities.count > 1, "should have monsters");
    }

    #[test]
    fn move_changes_position() {
        let mut g = MicroGameState::new_default(42);
        let px = g.entities.x[0];
        let py = g.entities.y[0];

        // Try all 8 directions until one succeeds
        let dirs = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
            Direction::NorthEast,
            Direction::NorthWest,
            Direction::SouthEast,
            Direction::SouthWest,
        ];
        let mut moved = false;
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                let result = g.step(GameCommand::Move(dir));
                assert!(result.action_taken);
                assert_ne!((g.entities.x[0], g.entities.y[0]), (px, py));
                moved = true;
                break;
            }
        }
        assert!(
            moved,
            "player should be able to move in at least one direction"
        );
    }

    #[test]
    fn wait_passes_turn() {
        let mut g = MicroGameState::new_default(42);
        let result = g.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert_eq!(g.turn_count, 1);
    }

    #[test]
    fn game_over_blocks_step() {
        let mut g = MicroGameState::new_default(42);
        g.game_over = true;
        let result = g.step(GameCommand::Wait);
        assert!(!result.action_taken);
        assert!(result.game_over);
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut a = MicroGameState::new_default(1234);
        let mut b = MicroGameState::new_default(1234);

        // Same initial state
        assert_eq!(a.entities.count, b.entities.count);
        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_eq!(a.map.tiles[..packed_size], b.map.tiles[..packed_size]);

        // Run same commands
        for _ in 0..10 {
            a.step(GameCommand::Wait);
            b.step(GameCommand::Wait);
        }

        assert_eq!(a.turn_count, b.turn_count);
        assert_eq!(a.kills, b.kills);
        assert_eq!(a.rng.state(), b.rng.state());
        assert_eq!(a.entities.hp[0], b.entities.hp[0]);
    }

    #[test]
    fn regen_heals_player() {
        let mut g = MicroGameState::new_default(42);
        // Damage the player
        let pi = PLAYER_IDX as usize;
        g.entities.hp[pi] = g.entities.max_hp[pi] - 5;
        let hp_after_damage = g.entities.hp[pi];

        // Step enough turns for regen to kick in
        for _ in 0..(balance::REGEN_INTERVAL as u16 * 3) {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        if !g.game_over {
            assert!(
                g.entities.hp[pi] > hp_after_damage,
                "player should have regenerated HP"
            );
        }
    }

    #[test]
    fn custom_dimensions() {
        let g = MicroGameState::new(42, 80, 40);
        assert_eq!(g.map.width, 80);
        assert_eq!(g.map.height, 40);
        assert!(!g.game_over);
        assert!(g.entities.count > 1);
    }

    #[test]
    fn descend_on_stairs_succeeds() {
        let mut g = MicroGameState::new_default(42);
        // Teleport player to stairs (last room center)
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        let result = g.step(GameCommand::Descend);
        assert!(result.action_taken);
        assert_eq!(g.depth, 2);
        assert!(!result.game_won);
    }

    #[test]
    fn descend_not_on_stairs_fails() {
        let mut g = MicroGameState::new_default(42);
        // Player starts on floor in room 0, not on stairs
        let result = g.step(GameCommand::Descend);
        assert!(!result.action_taken);
        assert_eq!(g.depth, 1);
        // Should have logged NoStairs
        assert_eq!(g.log.recent(0), Some(GameEvent::NoStairs));
    }

    #[test]
    fn victory_after_target_depth() {
        let mut g = MicroGameState::new_default(42);
        for _ in 0..balance::TARGET_DEPTH {
            // Teleport to stairs and descend
            let last = g.map.rooms[(g.map.room_count - 1) as usize];
            g.entities.x[0] = last.cx();
            g.entities.y[0] = last.cy();
            g.step(GameCommand::Descend);
        }
        assert!(g.game_won);
        assert_eq!(g.depth, balance::TARGET_DEPTH);
    }

    #[test]
    fn player_hp_carries_over() {
        let mut g = MicroGameState::new_default(42);
        g.entities.hp[0] = 15; // damage player
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        g.step(GameCommand::Descend);
        assert_eq!(g.entities.hp[0], 15, "HP should carry over");
        assert_eq!(g.entities.max_hp[0], balance::PLAYER_HP);
    }

    #[test]
    fn deterministic_floor_generation() {
        // Two games with same seed should produce identical level 2
        let mut a = MicroGameState::new_default(100);
        let mut b = MicroGameState::new_default(100);

        // Descend both to level 2
        for g in [&mut a, &mut b] {
            let last = g.map.rooms[(g.map.room_count - 1) as usize];
            g.entities.x[0] = last.cx();
            g.entities.y[0] = last.cy();
            g.step(GameCommand::Descend);
        }

        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_eq!(a.map.tiles[..packed_size], b.map.tiles[..packed_size]);
        assert_eq!(a.entities.count, b.entities.count);
        assert_eq!(a.depth, b.depth);
    }

    #[test]
    fn floor_seeds_decorrelated() {
        // Seed N at depth 2 must NOT produce the same map as seed N+1 at depth 1
        let mut a = MicroGameState::new_default(100);
        let b = MicroGameState::new_default(101);

        // Descend game A to depth 2
        let last = a.map.rooms[(a.map.room_count - 1) as usize];
        a.entities.x[0] = last.cx();
        a.entities.y[0] = last.cy();
        a.step(GameCommand::Descend);
        assert_eq!(a.depth, 2);

        // Game B stays at depth 1 — compare maps
        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_ne!(
            a.map.tiles[..packed_size],
            b.map.tiles[..packed_size],
            "seed 100 depth 2 should differ from seed 101 depth 1"
        );
    }

    #[test]
    fn monsters_scaled_on_deeper_floors() {
        let mut g = MicroGameState::new_default(42);
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        g.step(GameCommand::Descend);
        assert_eq!(g.depth, 2);

        // Check that at least one monster has scaled stats
        if g.entities.count > 1 {
            let kind = g.entities.kind[1].unwrap();
            let base_hp = crate::rules::monster_table::max_hp(kind);
            assert_eq!(
                g.entities.hp[1],
                base_hp + balance::MONSTER_HP_PER_FLOOR,
                "monster HP should be scaled for depth 2"
            );
        }
    }

    // ── Item and equipment tests ──────────────────────────────────────

    /// Place an item under the player and step onto it.
    fn place_item_at_player(g: &mut MicroGameState, kind: ItemKind) {
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.items.spawn(px, py, kind);
    }

    #[test]
    fn items_spawn_on_new_game() {
        let g = MicroGameState::new_default(42);
        assert!(g.items.count > 0, "should have spawned items");
    }

    #[test]
    fn pickup_adds_to_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        place_item_at_player(&mut g, ItemKind::HealthPotion);
        let result = g.step(GameCommand::Pickup);
        assert!(result.action_taken);
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
    }

    #[test]
    fn pickup_stacks_consumables() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        place_item_at_player(&mut g, ItemKind::HealthPotion);
        place_item_at_player(&mut g, ItemKind::HealthPotion);
        g.step(GameCommand::Pickup);
        g.step(GameCommand::Pickup);
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().count, 2);
    }

    #[test]
    fn pickup_full_inventory_rejected() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        // Fill inventory with swords (non-stackable).
        for _ in 0..rules_items::MAX_INVENTORY {
            g.inventory.add(ItemKind::ShortSword);
        }
        place_item_at_player(&mut g, ItemKind::ShortSword);
        let result = g.step(GameCommand::Pickup);
        assert!(result.action_taken); // turn consumed
        // Item should still be on ground.
        assert!(g.items.alive[0], "item should remain on ground");
    }

    // --- auto-pickup tests ---

    /// Find a walkable direction from the player with no monster, spawn an item there.
    /// Returns the direction to move and the (x, y) of the target tile.
    fn place_item_adjacent(g: &mut MicroGameState, kind: ItemKind) -> Direction {
        let dirs = [
            Direction::East,
            Direction::West,
            Direction::North,
            Direction::South,
        ];
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let px = g.entities.x[0];
            let py = g.entities.y[0];
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                g.items.spawn(nx, ny, kind);
                return dir;
            }
        }
        panic!("no walkable adjacent tile found");
    }

    #[test]
    fn auto_pickup_grabs_consumable() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.auto_pickup = true;
        let dir = place_item_adjacent(&mut g, ItemKind::HealthPotion);
        g.step(GameCommand::Move(dir));
        assert!(g.inventory.len() == 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
    }

    #[test]
    fn auto_pickup_grabs_equipment() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.auto_pickup = true;
        let dir = place_item_adjacent(&mut g, ItemKind::ShortSword);
        g.step(GameCommand::Move(dir));
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn auto_pickup_multiple_consumables() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.auto_pickup = true;
        // Place 3 potions at the same adjacent tile.
        let dirs = [
            Direction::East,
            Direction::West,
            Direction::North,
            Direction::South,
        ];
        let mut chosen_dir = Direction::East;
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let px = g.entities.x[0];
            let py = g.entities.y[0];
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                g.items.spawn(nx, ny, ItemKind::HealthPotion);
                g.items.spawn(nx, ny, ItemKind::HealthPotion);
                g.items.spawn(nx, ny, ItemKind::HealthPotion);
                chosen_dir = dir;
                break;
            }
        }
        g.step(GameCommand::Move(chosen_dir));
        assert_eq!(g.inventory.len(), 1); // stacked
        assert_eq!(g.inventory.get(0).unwrap().count, 3);
    }

    #[test]
    fn auto_pickup_stops_when_inventory_full() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.auto_pickup = true;
        for _ in 0..rules_items::MAX_INVENTORY {
            g.inventory.add(ItemKind::ShortSword);
        }
        let dir = place_item_adjacent(&mut g, ItemKind::HealthPotion);
        g.step(GameCommand::Move(dir));
        assert!(g.items.alive[0], "potion should remain on ground");
    }

    #[test]
    fn auto_pickup_off_by_default() {
        let g = MicroGameState::new_default(42);
        assert!(!g.auto_pickup);
    }

    #[test]
    fn use_potion_heals_from_inventory() {
        let mut g = MicroGameState::new_default(42);
        let pi = PLAYER_IDX as usize;
        g.entities.hp[pi] = g.entities.max_hp[pi] - 5;
        let hp_before = g.entities.hp[pi];
        g.inventory.add(ItemKind::HealthPotion);

        let result = g.step(GameCommand::UseItem(0));
        assert!(result.action_taken);
        assert!(g.entities.hp[pi] > hp_before, "potion should heal");
        assert!(g.inventory.is_empty(), "potion should be consumed");
    }

    #[test]
    fn use_on_empty_slot_no_action() {
        let mut g = MicroGameState::new_default(42);
        let result = g.step(GameCommand::UseItem(0));
        assert!(!result.action_taken);
    }

    #[test]
    fn drop_puts_item_on_ground() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.inventory.add(ItemKind::ShortSword);
        let result = g.step(GameCommand::DropItem(0));
        assert!(result.action_taken);
        assert!(g.inventory.is_empty());
        // Item should be on ground at player position.
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        assert!(g.items.alive[0]);
        assert_eq!(g.items.x[0], px);
        assert_eq!(g.items.y[0], py);
        assert_eq!(g.items.kind[0], ItemKind::ShortSword);
    }

    #[test]
    fn equip_from_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::ShortSword);
        assert_eq!(g.equipment.weapon, None);

        let result = g.step(GameCommand::EquipItem(0));
        assert!(result.action_taken);
        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
        assert!(g.inventory.is_empty());
    }

    #[test]
    fn equip_swaps_old_to_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        g.inventory.add(ItemKind::ShortSword); // a second sword

        let result = g.step(GameCommand::EquipItem(0));
        assert!(result.action_taken);
        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
        // Old weapon should be in inventory.
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn unequip_weapon_returns_to_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        assert!(g.inventory.is_empty());
        let result = g.step(GameCommand::UnequipWeapon);
        assert!(result.action_taken);
        assert!(g.equipment.weapon.is_none());
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn unequip_armor_returns_to_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.armor = Some(ItemKind::LeatherArmor);
        let result = g.step(GameCommand::UnequipArmor);
        assert!(result.action_taken);
        assert!(g.equipment.armor.is_none());
        assert_eq!(g.inventory.len(), 1);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::LeatherArmor);
    }

    #[test]
    fn unequip_nothing_no_action() {
        let mut g = MicroGameState::new_default(42);
        let result = g.step(GameCommand::UnequipWeapon);
        assert!(!result.action_taken);
        let result = g.step(GameCommand::UnequipArmor);
        assert!(!result.action_taken);
    }

    #[test]
    fn unequip_full_inventory_fails() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        for _ in 0..26 {
            g.inventory.add(ItemKind::ShortSword);
        }
        let result = g.step(GameCommand::UnequipWeapon);
        assert!(!result.action_taken);
        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
    }

    #[test]
    fn drop_equipped_weapon_to_ground() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        let result = g.step(GameCommand::DropEquippedWeapon);
        assert!(result.action_taken);
        assert!(g.equipment.weapon.is_none());
        assert!(g.inventory.is_empty());
    }

    #[test]
    fn drop_equipped_with_full_inventory() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        for _ in 0..26 {
            g.inventory.add(ItemKind::ShortSword);
        }
        let result = g.step(GameCommand::DropEquippedWeapon);
        assert!(result.action_taken);
        assert!(g.equipment.weapon.is_none());
    }

    #[test]
    fn walk_over_item_notifies_only() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        // Find a walkable direction.
        let dirs = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ];
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let px = g.entities.x[0];
            let py = g.entities.y[0];
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                // Place item at target tile.
                g.items.spawn(nx, ny, ItemKind::HealthPotion);
                g.step(GameCommand::Move(dir));
                // Item should still be on ground (no auto-pickup).
                assert!(g.items.alive[0], "item should remain on ground");
                // Item should be in no inventory.
                assert!(g.inventory.is_empty(), "no auto-pickup");
                break;
            }
        }
    }

    #[test]
    fn inventory_persists_across_descent() {
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::HealthPotion);
        g.inventory.add(ItemKind::ShortSword);

        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert_eq!(g.inventory.len(), 2);
        assert_eq!(g.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
        assert_eq!(g.inventory.get(1).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn effective_attack_with_weapon() {
        let mut g = MicroGameState::new_default(42);
        let base = g.entities.atk[PLAYER_IDX as usize];
        assert_eq!(g.effective_attack(), base);

        g.equipment.weapon = Some(ItemKind::ShortSword);
        g.equipment.weapon_props = rules_items::default_properties(ItemKind::ShortSword);
        assert_eq!(
            g.effective_attack(),
            base + rules_items::attack_bonus(ItemKind::ShortSword)
        );
    }

    #[test]
    fn effective_defense_with_armor() {
        let mut g = MicroGameState::new_default(42);
        let base = g.entities.def[PLAYER_IDX as usize];
        assert_eq!(g.effective_defense(), base);

        g.equipment.armor = Some(ItemKind::LeatherArmor);
        g.equipment.armor_props = rules_items::default_properties(ItemKind::LeatherArmor);
        assert_eq!(
            g.effective_defense(),
            base + rules_items::defense_bonus(ItemKind::LeatherArmor)
        );
    }

    #[test]
    fn equipment_persists_across_descent() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        g.equipment.armor = Some(ItemKind::LeatherArmor);

        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
        assert_eq!(g.equipment.armor, Some(ItemKind::LeatherArmor));
    }

    #[test]
    fn items_spawn_on_new_floor() {
        let mut g = MicroGameState::new_default(42);
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert!(g.items.count > 0, "new floor should have items");
    }

    // ── Wandering monster tests ──────────────────────────────────────

    /// Helper: run Wait commands until a wandering monster spawns or limit is hit.
    /// Returns the count of alive entities with AiBehavior::Wander.
    fn count_wanderers(g: &MicroGameState) -> u8 {
        let mut count = 0u8;
        for i in 1..g.entities.count as usize {
            if g.entities.alive[i] && g.entities.ai[i] == AiBehavior::Wander {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn no_wandering_before_grace_period() {
        let mut g = MicroGameState::new_default(42);
        let initial_count = g.entities.count;

        // Run turns within the grace period. The wandering counter starts at
        // GRACE_PERIOD - 1 and decrements each turn, so after GRACE_PERIOD - 1
        // turns the counter reaches 0 but the last decrement still returns early.
        // The GRACE_PERIOD-th turn would enter spawn logic, so we stop before it.
        for _ in 0..balance::WANDERING_GRACE_PERIOD - 1 {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        // No wandering spawns should have occurred — entity count should only
        // decrease (monster kills) or stay the same, never increase beyond
        // what initial room-based spawning created.
        assert!(
            count_wanderers(&g) == 0,
            "no wanderers should spawn before grace period"
        );
        assert!(
            g.wandering_spawned == 0,
            "wandering_spawned counter should be 0"
        );
        // Entity count can't have grown (only killed, never spawned).
        assert!(
            g.entities.count <= initial_count,
            "entity count should not grow before grace period"
        );
    }

    #[test]
    fn wandering_spawns_after_grace_period() {
        let mut g = MicroGameState::new_default(42);
        // Kill all initial monsters to make room and avoid interference.
        for i in 1..g.entities.count {
            g.entities.kill(i);
        }

        // Run well past grace period — enough turns for spawns to happen.
        let target_turns = (balance::WANDERING_GRACE_PERIOD as u16) * 4;
        for _ in 0..target_turns {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        assert!(
            g.wandering_spawned > 0,
            "at least one wandering monster should have spawned after {} turns",
            target_turns
        );
    }

    #[test]
    fn wandering_cap_respected() {
        let mut g = MicroGameState::new_default(42);
        // Kill all initial monsters.
        for i in 1..g.entities.count {
            g.entities.kill(i);
        }

        // Run a large number of turns.
        for _ in 0..500 {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        let wanderers = count_wanderers(&g);
        assert!(
            wanderers <= balance::WANDERING_MAX_ACTIVE,
            "wanderer count {} exceeds cap {}",
            wanderers,
            balance::WANDERING_MAX_ACTIVE
        );
    }

    #[test]
    fn idle_count_tracks_waits() {
        let mut g = MicroGameState::new_default(42);
        assert_eq!(g.idle_count, 0);

        g.step(GameCommand::Wait);
        assert_eq!(g.idle_count, 1);
        g.step(GameCommand::Wait);
        assert_eq!(g.idle_count, 2);

        // Move resets idle count (find a walkable direction).
        let dirs = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ];
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let px = g.entities.x[0];
            let py = g.entities.y[0];
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                g.step(GameCommand::Move(dir));
                break;
            }
        }
        assert_eq!(g.idle_count, 0, "move should reset idle count");
    }

    #[test]
    fn wandering_state_resets_on_descent() {
        let mut g = MicroGameState::new_default(42);
        g.idle_count = 10;
        g.wandering_spawned = 3;

        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert_eq!(g.idle_count, 0, "idle_count should reset on descent");
        assert_eq!(
            g.wandering_spawned, 0,
            "wandering_spawned should reset on descent"
        );
    }

    #[test]
    fn auto_fight_kills_adjacent_monster() {
        use crate::rules::monster_table::MonsterKind;
        let mut g = MicroGameState::new_default(42);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        // Spawn a goblin adjacent.
        g.entities
            .spawn_monster(MonsterKind::Goblin, px + 1, py, AiBehavior::Chase);
        let result = g.auto_fight();
        assert!(result.is_some(), "should find adjacent monster");
        let r = result.unwrap();
        assert!(r.rounds > 0);
        assert!(r.target_killed);
        assert_eq!(r.target_kind, Some(MonsterKind::Goblin));
    }

    #[test]
    fn auto_fight_no_adjacent_returns_none() {
        let mut g = MicroGameState::new_default(42);
        // Clear all monsters far from player.
        for i in 1..g.entities.count as usize {
            g.entities.alive[i] = false;
        }
        assert!(g.auto_fight().is_none());
    }

    #[test]
    fn auto_fight_picks_weakest() {
        use crate::rules::monster_table::MonsterKind;
        let mut g = MicroGameState::new_default(42);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        // Clear existing monsters.
        for i in 1..g.entities.count as usize {
            g.entities.alive[i] = false;
        }
        // Spawn an orc (higher HP) and a goblin (lower HP) adjacent.
        g.entities
            .spawn_monster(MonsterKind::Orc, px + 1, py, AiBehavior::Chase);
        g.entities.spawn_monster(
            MonsterKind::Goblin,
            px.wrapping_sub(1),
            py,
            AiBehavior::Chase,
        );
        let result = g.auto_fight().unwrap();
        // Should have targeted the goblin (lower HP).
        assert_eq!(result.target_kind, Some(MonsterKind::Goblin));
        assert!(result.target_killed);
    }

    // ── Combine tests ─────────────────────────────────────────────────

    #[test]
    fn combine_self_rejected() {
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::ShortSword);
        let result = g.step(GameCommand::Combine(0, 0));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_empty_slot_rejected() {
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::ShortSword);
        let result = g.step(GameCommand::Combine(0, 5));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_no_effect_when_no_rules_match() {
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::ShortSword);
        g.inventory.add(ItemKind::ShortSword);
        let result = g.step(GameCommand::Combine(0, 1));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_consumes_consumable_source() {
        use crate::rules::properties::{self, Property};
        let mut g = MicroGameState::new_default(42);
        g.inventory.add(ItemKind::ShortSword);
        g.inventory.add(ItemKind::StrengthPotion);
        let hard_before = properties::get(&g.inventory.get(0).unwrap().props, Property::Hard);
        let result = g.step(GameCommand::Combine(0, 1));
        assert!(result.action_taken);
        assert!(g.inventory.get(1).is_none());
        let hard_after = properties::get(&g.inventory.get(0).unwrap().props, Property::Hard);
        assert!(
            hard_after > hard_before,
            "HARD should increase: before={}, after={}",
            hard_before,
            hard_after,
        );
    }
}
