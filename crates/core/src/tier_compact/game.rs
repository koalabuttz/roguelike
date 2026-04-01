//! Top-level compact-tier game state and step API (GBA).
//!
//! `CompactGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.

use super::ai;
use super::combat;
use super::entity::EntityStore;
use super::fov::CompactFov;
use super::item_store::ItemStore;
use super::map::{CompactMap, TILE_STAIRS_DOWN};
use super::msglog::CompactMessageLog;
use super::prng::LfsrRng32;
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
pub struct CompactStepResult {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

pub struct CompactGameState {
    pub map: CompactMap,
    pub fov: CompactFov,
    pub entities: EntityStore,
    pub items: ItemStore,
    pub equipment: Equipment,
    pub inventory: Inventory,
    pub log: CompactMessageLog,
    pub rng: LfsrRng32,
    pub seed: u32,
    pub turn_count: u16,
    pub kills: u8,
    pub depth: u8,
    pub game_over: bool,
    pub game_won: bool,
    pub idle_count: u8,
    pub wandering_spawned: u8,
    /// Countdown for wandering spawns (variable interval due to idle acceleration).
    pub(crate) wandering_counter: u8,
    pub auto_pickup: bool,
}

/// Result of an auto-fight sequence.
pub struct CompactAutoFightResult {
    pub rounds: u8,
    pub target_idx: u8,
    pub target_kind: Option<MonsterKind>,
    pub target_killed: bool,
    pub player_hp_lost: u8,
}

impl CompactGameState {
    /// Create a new game with the given seed and map dimensions.
    pub fn new(seed: u32, width: Coord, height: Coord) -> Self {
        let mut rng = LfsrRng32::new(seed);
        let mut map = CompactMap::new(width, height);
        let (sx, sy) = map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(sx, sy);

        let mut items = ItemStore::new();
        spawn::spawn_monsters(&mut entities, &map, &mut rng);
        spawn::spawn_items(&mut items, &map, 1, &mut rng);
        spawn::apply_depth_scaling(&mut entities, 1);

        let mut fov = CompactFov::new(width, height);
        fov.compute_fov(sx, sy, balance::FOV_RADIUS, &map);

        let mut log = CompactMessageLog::new();
        log.add(GameEvent::Welcome);

        Self {
            map,
            fov,
            entities,
            items,
            equipment: Equipment::default(),
            inventory: Inventory::new(),
            log,
            rng,
            seed,
            turn_count: 0,
            kills: 0,
            depth: 1,
            game_over: false,
            game_won: false,
            idle_count: 0,
            wandering_spawned: 0,
            wandering_counter: balance::WANDERING_GRACE_PERIOD - 1,
            auto_pickup: false,
        }
    }

    /// Construct a new game directly at `dst`, avoiding large stack allocations.
    ///
    /// Same as `new()` but writes each field directly to the destination pointer.
    /// Use this on constrained platforms (GBA, C64) where the ~8 KB struct
    /// would overflow the stack if returned by value.
    ///
    /// # Safety
    /// `dst` must point to valid, writable memory of at least `size_of::<Self>()` bytes.
    pub unsafe fn new_into(dst: *mut Self, seed: u32, width: Coord, height: Coord) {
        use core::ptr::addr_of_mut;

        // SAFETY: caller guarantees `dst` points to valid, writable memory of
        // at least `size_of::<Self>()` bytes. All field writes use addr_of_mut!
        // to avoid creating &mut references to uninitialized memory.
        unsafe {
            // Use raw pointer field access throughout — no &mut to uninitialized memory.
            addr_of_mut!((*dst).rng).write(LfsrRng32::new(seed));
            addr_of_mut!((*dst).seed).write(seed);

            // Initialize map in-place, then generate.
            addr_of_mut!((*dst).map).write(CompactMap::new(width, height));
            let (sx, sy) = (*dst).map.generate(&mut (*dst).rng);

            // Entities — spawn player + monsters.
            addr_of_mut!((*dst).entities).write(EntityStore::new());
            (*dst).entities.spawn_player(sx, sy);

            // Items.
            addr_of_mut!((*dst).items).write(ItemStore::new());
            spawn::spawn_monsters(&mut (*dst).entities, &(*dst).map, &mut (*dst).rng);
            spawn::spawn_items(&mut (*dst).items, &(*dst).map, 1, &mut (*dst).rng);
            spawn::apply_depth_scaling(&mut (*dst).entities, 1);

            // FOV.
            addr_of_mut!((*dst).fov).write(CompactFov::new(width, height));
            (*dst)
                .fov
                .compute_fov(sx, sy, balance::FOV_RADIUS, &(*dst).map);

            // Message log.
            addr_of_mut!((*dst).log).write(CompactMessageLog::new());
            (*dst).log.add(GameEvent::Welcome);

            // Equipment + inventory.
            addr_of_mut!((*dst).equipment).write(Equipment::default());
            addr_of_mut!((*dst).inventory).write(Inventory::new());

            // Scalars.
            addr_of_mut!((*dst).turn_count).write(0);
            addr_of_mut!((*dst).kills).write(0);
            addr_of_mut!((*dst).depth).write(1);
            addr_of_mut!((*dst).game_over).write(false);
            addr_of_mut!((*dst).game_won).write(false);
            addr_of_mut!((*dst).idle_count).write(0);
            addr_of_mut!((*dst).wandering_spawned).write(0);
            addr_of_mut!((*dst).wandering_counter).write(balance::WANDERING_GRACE_PERIOD - 1);
            addr_of_mut!((*dst).auto_pickup).write(false);
        }
    }

    /// Effective attack: base + weapon bonus.
    pub fn effective_attack(&self) -> u8 {
        let base = self.entities.atk[PLAYER_IDX as usize];
        damage::effective_attack(base, self.equipment.attack_bonus())
    }

    /// Effective defense: base + armor bonus.
    pub fn effective_defense(&self) -> u8 {
        let base = self.entities.def[PLAYER_IDX as usize];
        damage::effective_defense(base, self.equipment.defense_bonus())
    }

    /// Process one player command. Returns step result.
    pub fn step(&mut self, cmd: GameCommand) -> CompactStepResult {
        self.step_inner(cmd, true)
    }

    /// Process one command without recomputing FOV (for autorun intermediate steps).
    pub fn step_skip_fov(&mut self, cmd: GameCommand) -> CompactStepResult {
        self.step_inner(cmd, false)
    }

    /// Resolve Interact to a concrete command by checking tile state.
    fn resolve_interact(&self) -> Option<GameCommand> {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];
        for i in 0..self.items.count as usize {
            if self.items.alive[i] && self.items.x[i] == px && self.items.y[i] == py {
                return Some(GameCommand::Pickup);
            }
        }
        if self.map.tile_at(px, py) == TILE_STAIRS_DOWN {
            return Some(GameCommand::Descend);
        }
        None
    }

    fn step_inner(&mut self, cmd: GameCommand, compute_fov: bool) -> CompactStepResult {
        if self.game_over || self.game_won {
            return CompactStepResult {
                action_taken: false,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        // Resolve Interact to a concrete command before dispatch.
        let cmd = if matches!(cmd, GameCommand::Interact) {
            match self.resolve_interact() {
                Some(c) => c,
                None => {
                    return CompactStepResult {
                        action_taken: false,
                        game_over: false,
                        game_won: false,
                    };
                }
            }
        } else {
            cmd
        };

        // Descent is handled separately — it rebuilds the level and FOV.
        if matches!(cmd, GameCommand::Descend) {
            let descended = self.descend();
            return CompactStepResult {
                action_taken: descended,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        let pi = PLAYER_IDX as usize;
        let old_px = self.entities.x[pi];
        let old_py = self.entities.y[pi];

        let is_wait = matches!(cmd, GameCommand::Wait);
        let action_taken = match cmd {
            GameCommand::Wait => true,
            GameCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                self.player_move_or_attack(dx, dy)
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
            | GameCommand::Descend
            | GameCommand::Interact => false,
        };

        if action_taken {
            if is_wait {
                self.idle_count = self.idle_count.saturating_add(1);
            } else {
                self.idle_count = 0;
            }

            if compute_fov {
                let px = self.entities.x[pi];
                let py = self.entities.y[pi];
                if px != old_px || py != old_py {
                    self.fov.compute_fov(px, py, balance::FOV_RADIUS, &self.map);
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

        CompactStepResult {
            action_taken,
            game_over: self.game_over,
            game_won: self.game_won,
        }
    }

    // ── Descent ──────────────────────────────────────────────────────

    fn descend(&mut self) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        if self.map.tile_at(px, py) != TILE_STAIRS_DOWN {
            self.log.add(GameEvent::NoStairs);
            return false;
        }

        if self.depth >= balance::TARGET_DEPTH {
            self.game_won = true;
            self.log.add(GameEvent::Victory {
                depth: balance::TARGET_DEPTH,
            });
            return true;
        }

        self.depth += 1;

        let floor_seed = self.seed ^ (self.depth as u32).wrapping_mul(0x9E37);
        self.rng = LfsrRng32::new(floor_seed);

        // Save player stats
        let hp = self.entities.hp[pi];
        let max_hp = self.entities.max_hp[pi];
        let atk = self.entities.atk[pi];
        let def = self.entities.def[pi];

        // Generate new map (preserve dimensions from current map)
        let w = self.map.width;
        let h = self.map.height;
        self.map = CompactMap::new(w, h);
        let (sx, sy) = self.map.generate(&mut self.rng);

        // Reset entities — player keeps stats
        self.entities = EntityStore::new();
        self.entities.spawn_player(sx, sy);
        self.entities.hp[pi] = hp;
        self.entities.max_hp[pi] = max_hp;
        self.entities.atk[pi] = atk;
        self.entities.def[pi] = def;

        spawn::spawn_monsters(&mut self.entities, &self.map, &mut self.rng);
        spawn::apply_depth_scaling(&mut self.entities, self.depth);
        self.items = ItemStore::new();
        spawn::spawn_items(&mut self.items, &self.map, self.depth, &mut self.rng);

        self.fov = CompactFov::new(w, h);
        self.fov.compute_fov(sx, sy, balance::FOV_RADIUS, &self.map);

        self.idle_count = 0;
        self.wandering_spawned = 0;
        self.wandering_counter = balance::WANDERING_GRACE_PERIOD - 1;

        self.log.reset();
        self.log.add(GameEvent::Descend {
            depth: self.depth,
            target: balance::TARGET_DEPTH,
        });

        true
    }

    // ── Movement & combat ────────────────────────────────────────────

    fn player_move_or_attack(&mut self, dx: i32, dy: i32) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];
        let nx = px + dx;
        let ny = py + dy;

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

        if self.map.is_walkable(nx, ny) {
            self.entities.x[pi] = nx;
            self.entities.y[pi] = ny;
            self.notify_items_here(nx, ny);
            return true;
        }

        false
    }

    // ── Items ────────────────────────────────────────────────────────

    fn notify_items_here(&mut self, x: Coord, y: Coord) {
        if self.auto_pickup {
            self.auto_pickup_items(x, y);
        }
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

    fn auto_pickup_items(&mut self, x: Coord, y: Coord) {
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
                break;
            }
            self.items.remove(idx);
            self.log.add(GameEvent::PickupItem { kind });
        }
    }

    fn pickup_item(&mut self) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        for i in 0..self.items.count as usize {
            if self.items.alive[i] && self.items.x[i] == px && self.items.y[i] == py {
                let kind = self.items.kind[i];
                if !self.inventory.add(kind) {
                    self.log.add(GameEvent::InventoryFull);
                    return true;
                }
                self.items.remove(i as u8);
                self.log.add(GameEvent::PickupItem { kind });
                return true;
            }
        }
        false
    }

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

    // ── Equipment ────────────────────────────────────────────────────

    fn equip_item(&mut self, slot: u8) -> bool {
        let inv_slot = match self.inventory.get(slot as usize) {
            Some(s) => *s,
            None => return false,
        };
        let kind = inv_slot.kind;
        let props = inv_slot.props;

        if rules_items::is_weapon(kind) {
            self.inventory.remove_one(slot as usize);
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

    fn drop_equipped_weapon(&mut self) -> bool {
        if let Some(kind) = self.equipment.weapon.take() {
            self.equipment.weapon_props = crate::rules::properties::EMPTY;
            let pi = PLAYER_IDX as usize;
            self.items
                .spawn(self.entities.x[pi], self.entities.y[pi], kind);
            self.log.add(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    fn drop_equipped_armor(&mut self) -> bool {
        if let Some(kind) = self.equipment.armor.take() {
            self.equipment.armor_props = crate::rules::properties::EMPTY;
            let pi = PLAYER_IDX as usize;
            self.items
                .spawn(self.entities.x[pi], self.entities.y[pi], kind);
            self.log.add(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    // ── Combine items ────────────────────────────────────────────────

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

        let source_consumed = rules_items::is_consumable(source.kind);
        let source_destroyed =
            !source_consumed && rules_items::is_material_dead(source.kind, &b_props);
        let source_removed = source_consumed || source_destroyed;
        if source_removed {
            self.inventory.remove_one(source_slot as usize);
        }

        let target_destroyed = rules_items::is_material_dead(target.kind, &a_props);

        self.inventory.remove_one(target_slot as usize);
        if !target_destroyed && !self.inventory.add_with_props(target.kind, a_props) {
            let ok = self.inventory.add_with_props(target.kind, target.props);
            debug_assert!(ok, "undo target re-insert must succeed");
            if source_removed {
                let ok = self.inventory.add_with_props(source.kind, source.props);
                debug_assert!(ok, "undo source re-insert must succeed");
            }
            self.log.add(GameEvent::InventoryFull);
            return false;
        }

        if !source_removed {
            self.inventory.set_props(source_slot as usize, b_props);
        }

        self.log.add(GameEvent::CombineItems {
            target: target.kind,
            source: source.kind,
        });
        if target_destroyed {
            self.log.add(GameEvent::ItemDestroyed { kind: target.kind });
        }
        if source_destroyed {
            self.log.add(GameEvent::ItemDestroyed { kind: source.kind });
        }

        true
    }

    // ── Post-action systems ──────────────────────────────────────────

    fn apply_regen(&mut self) {
        if self.game_over {
            return;
        }
        if self
            .turn_count
            .is_multiple_of(balance::REGEN_INTERVAL as u16)
        {
            let pi = PLAYER_IDX as usize;
            let hp = self.entities.hp[pi];
            let max_hp = self.entities.max_hp[pi];
            if hp < max_hp {
                self.entities.hp[pi] = hp + 1;
            }
        }
    }

    fn try_spawn_wandering(&mut self) {
        if self.wandering_counter > 0 {
            self.wandering_counter -= 1;
            return;
        }

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

    fn pick_offscreen_spawn_pos(&mut self) -> Option<(Coord, Coord)> {
        if self.map.room_count == 0 {
            return None;
        }

        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        for _ in 0..10 {
            let room_idx = self.rng.range_u8(0, self.map.room_count - 1) as usize;
            let room = self.map.rooms[room_idx];

            if room.contains_interior(px, py) {
                continue;
            }

            if room.w < 3 || room.h < 3 {
                continue;
            }
            let sx = self
                .rng
                .range_u8(room.x as u8 + 1, (room.x + room.w - 1) as u8)
                as Coord;
            let sy = self
                .rng
                .range_u8(room.y as u8 + 1, (room.y + room.h - 1) as u8)
                as Coord;

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

    fn emit_spawn_sound_cue(&mut self, sx: Coord, sy: Coord) {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];
        let dist = ((px - sx).abs() + (py - sy).abs()) as u8;

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

    fn emit_ambient_sound_cues(&mut self) {
        if !self
            .turn_count
            .is_multiple_of(balance::WANDERING_AMBIENT_SOUND_INTERVAL as u16)
        {
            return;
        }

        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        let mut closest_dist: u8 = u8::MAX;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] && self.entities.ai[i] == AiBehavior::Wander {
                let dist =
                    ((px - self.entities.x[i]).abs() + (py - self.entities.y[i]).abs()) as u8;
                if dist < closest_dist {
                    closest_dist = dist;
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

    // ── Auto-fight ───────────────────────────────────────────────────

    /// Fight the weakest adjacent monster to the death.
    pub fn auto_fight(&mut self) -> Option<CompactAutoFightResult> {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        let mut best_idx: u8 = NO_ENTITY;
        let mut best_hp: u8 = u8::MAX;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] {
                let dx = (self.entities.x[i] - px).abs();
                let dy = (self.entities.y[i] - py).abs();
                if dx <= 1 && dy <= 1 && self.entities.hp[i] < best_hp {
                    best_hp = self.entities.hp[i];
                    best_idx = i as u8;
                }
            }
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

            let tx = self.entities.x[best_idx as usize];
            let ty = self.entities.y[best_idx as usize];
            if (tx - self.entities.x[pi]).abs() > 1 || (ty - self.entities.y[pi]).abs() > 1 {
                break;
            }

            let ox = tx - self.entities.x[pi];
            let oy = ty - self.entities.y[pi];
            let cmd = GameCommand::move_or_wait(ox, oy);
            let result = self.step(cmd);
            rounds += 1;

            if result.game_over {
                break;
            }
        }

        Some(CompactAutoFightResult {
            rounds,
            target_idx: best_idx,
            target_kind,
            target_killed: !self.entities.alive[best_idx as usize],
            player_hp_lost: hp_before.saturating_sub(self.entities.hp[pi]),
        })
    }
}

// ---------------------------------------------------------------------------
// GameView implementation
// ---------------------------------------------------------------------------

impl crate::rules::game_view::GameView for CompactGameState {
    fn map_dims(&self) -> (i32, i32) {
        (self.map.width, self.map.height)
    }
    fn map_in_bounds(&self, x: i32, y: i32) -> bool {
        self.map.in_bounds(x, y)
    }
    fn tile_at(&self, x: i32, y: i32) -> u8 {
        self.map.tile_at(x, y)
    }
    fn is_visible(&self, x: i32, y: i32) -> bool {
        self.fov.is_visible(x, y)
    }
    fn is_explored(&self, x: i32, y: i32) -> bool {
        self.fov.is_explored(x, y)
    }
    fn player_xy(&self) -> (i32, i32) {
        (
            self.entities.x[PLAYER_IDX as usize],
            self.entities.y[PLAYER_IDX as usize],
        )
    }
    fn player_hp(&self) -> (u8, u8) {
        (
            self.entities.hp[PLAYER_IDX as usize],
            self.entities.max_hp[PLAYER_IDX as usize],
        )
    }
    fn effective_attack(&self) -> u8 {
        self.effective_attack()
    }
    fn effective_defense(&self) -> u8 {
        self.effective_defense()
    }
    fn entity_count(&self) -> usize {
        self.entities.count as usize
    }
    fn entity_xy(&self, i: usize) -> (i32, i32) {
        (self.entities.x[i], self.entities.y[i])
    }
    fn entity_alive(&self, i: usize) -> bool {
        self.entities.alive[i]
    }
    fn entity_kind(&self, i: usize) -> Option<MonsterKind> {
        self.entities.kind[i]
    }
    fn entity_hp(&self, i: usize) -> (u8, u8) {
        (self.entities.hp[i], self.entities.max_hp[i])
    }
    fn entity_at(&self, x: i32, y: i32) -> Option<u8> {
        let idx = self.entities.entity_at(x, y);
        if idx == super::types::NO_ENTITY {
            None
        } else {
            Some(idx)
        }
    }
    fn item_count(&self) -> usize {
        self.items.count as usize
    }
    fn item_xy(&self, i: usize) -> (i32, i32) {
        (self.items.x[i], self.items.y[i])
    }
    fn item_alive(&self, i: usize) -> bool {
        self.items.alive[i]
    }
    fn item_kind_at(&self, i: usize) -> crate::rules::items::ItemKind {
        self.items.kind[i]
    }
    fn item_at(&self, x: i32, y: i32) -> Option<u8> {
        let idx = self.items.item_at(x, y);
        if idx == super::types::NO_ITEM {
            None
        } else {
            Some(idx)
        }
    }
    fn equipment(&self) -> &crate::rules::items::Equipment {
        &self.equipment
    }
    fn inventory(&self) -> &crate::rules::items::Inventory {
        &self.inventory
    }
    fn depth(&self) -> u8 {
        self.depth
    }
    fn kills(&self) -> u8 {
        self.kills
    }
    fn turn_count(&self) -> u16 {
        self.turn_count
    }
    fn game_over(&self) -> bool {
        self.game_over
    }
    fn game_won(&self) -> bool {
        self.game_won
    }
    fn seed_u32(&self) -> u32 {
        self.seed
    }
    fn explored_pct(&self) -> u8 {
        let total = self.map.floor_count();
        if total == 0 {
            return 0;
        }
        let explored = self.fov.explored_floor_count(&self.map);
        ((explored as u32 * 100) / total as u32) as u8
    }
    fn target_depth(&self) -> u8 {
        crate::rules::balance::TARGET_DEPTH
    }
    fn recent_message(&self, n: u8) -> Option<GameEvent> {
        self.log.recent(n)
    }
    fn step_view(&mut self, cmd: GameCommand) -> crate::rules::game_view::GameViewStep {
        let r = CompactGameState::step(self, cmd);
        crate::rules::game_view::GameViewStep {
            action_taken: r.action_taken,
            game_over: r.game_over,
            game_won: r.game_won,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk_to_stairs(state: &mut CompactGameState) {
        let last = state.map.rooms[(state.map.room_count - 1) as usize];
        let tx = last.cx();
        let ty = last.cy();
        // Teleport player to stairs
        let pi = PLAYER_IDX as usize;
        state.entities.x[pi] = tx;
        state.entities.y[pi] = ty;
        state
            .fov
            .compute_fov(tx, ty, balance::FOV_RADIUS, &state.map);
    }

    #[test]
    fn new_game_is_playable() {
        let state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        assert!(!state.game_over);
        assert!(!state.game_won);
        assert!(state.entities.count > 1, "should have monsters");
        assert!(state.items.count > 0, "should have items");
    }

    #[test]
    fn move_changes_position() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];

        // Try all 8 directions — at least one should succeed in room 0.
        let mut moved = false;
        for dir in crate::rules::direction::ALL_DIRECTIONS {
            let r = state.step(GameCommand::Move(dir));
            if r.action_taken {
                moved = true;
                break;
            }
        }
        assert!(moved, "should be able to move in at least one direction");
        let nx = state.entities.x[pi];
        let ny = state.entities.y[pi];
        assert!(nx != px || ny != py, "position should change");
    }

    #[test]
    fn wait_passes_turn() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let tc = state.turn_count;
        let r = state.step(GameCommand::Wait);
        assert!(r.action_taken);
        assert_eq!(state.turn_count, tc + 1);
    }

    #[test]
    fn game_over_blocks_step() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.game_over = true;
        let r = state.step(GameCommand::Wait);
        assert!(!r.action_taken);
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut a = CompactGameState::new(1234, MAP_WIDTH, MAP_HEIGHT);
        let mut b = CompactGameState::new(1234, MAP_WIDTH, MAP_HEIGHT);
        for _ in 0..5 {
            a.step(GameCommand::Wait);
            b.step(GameCommand::Wait);
        }
        assert_eq!(a.rng.state(), b.rng.state());
        assert_eq!(a.turn_count, b.turn_count);
    }

    #[test]
    fn regen_heals_player() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        state.entities.hp[pi] = 1;
        let max_hp = state.entities.max_hp[pi];
        assert!(max_hp > 1);

        for _ in 0..(balance::REGEN_INTERVAL as u16 * 3) {
            state.step(GameCommand::Wait);
            if state.game_over {
                break;
            }
        }
        if !state.game_over {
            assert!(state.entities.hp[pi] > 1, "should have regenerated HP");
        }
    }

    // ── Descent tests ────────────────────────────────────────────────

    #[test]
    fn descend_on_stairs_succeeds() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        walk_to_stairs(&mut state);
        let r = state.step(GameCommand::Descend);
        assert!(r.action_taken);
        assert_eq!(state.depth, 2);
    }

    #[test]
    fn descend_not_on_stairs_fails() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let r = state.step(GameCommand::Descend);
        assert!(!r.action_taken);
        assert_eq!(state.depth, 1);
    }

    #[test]
    fn victory_after_target_depth() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.depth = balance::TARGET_DEPTH;
        walk_to_stairs(&mut state);
        let r = state.step(GameCommand::Descend);
        assert!(r.action_taken);
        assert!(r.game_won);
    }

    #[test]
    fn player_hp_carries_over() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        state.entities.hp[pi] = 5;
        walk_to_stairs(&mut state);
        state.step(GameCommand::Descend);
        assert_eq!(state.entities.hp[pi], 5);
    }

    #[test]
    fn deterministic_floor_generation() {
        let mut a = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let mut b = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        walk_to_stairs(&mut a);
        walk_to_stairs(&mut b);
        a.step(GameCommand::Descend);
        b.step(GameCommand::Descend);
        assert_eq!(a.entities.count, b.entities.count);
        assert_eq!(a.map.room_count, b.map.room_count);
    }

    #[test]
    fn monsters_scaled_on_deeper_floors() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        // Descend enough floors to trigger scaling (depth > DEPTH_SCALE_INTERVAL)
        for _ in 0..5 {
            walk_to_stairs(&mut state);
            state.step(GameCommand::Descend);
            if state.game_won {
                return; // Reached target depth, test N/A
            }
        }
        // Check that at least one monster has boosted stats
        let has_boosted = (1..state.entities.count as usize).any(|i| {
            if !state.entities.alive[i] {
                return false;
            }
            if let Some(kind) = state.entities.kind[i] {
                let base_hp = crate::rules::monster_table::max_hp(kind);
                state.entities.hp[i] > base_hp || state.entities.max_hp[i] > base_hp
            } else {
                false
            }
        });
        assert!(
            has_boosted,
            "monsters on deep floors should have boosted stats"
        );
    }

    // ── Item tests ───────────────────────────────────────────────────

    #[test]
    fn pickup_adds_to_inventory() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        state
            .items
            .spawn(px, py, rules_items::ItemKind::HealthPotion);
        let r = state.step(GameCommand::Pickup);
        assert!(r.action_taken);
        assert!(state.inventory.get(0).is_some());
    }

    #[test]
    fn use_potion_heals() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        state.entities.hp[pi] = 1;
        state.inventory.add(rules_items::ItemKind::HealthPotion);
        let r = state.step(GameCommand::UseItem(0));
        assert!(r.action_taken);
        assert!(state.entities.hp[pi] > 1);
    }

    #[test]
    fn drop_puts_item_on_ground() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.inventory.add(rules_items::ItemKind::ShortSword);
        let items_before = state.items.count;
        let r = state.step(GameCommand::DropItem(0));
        assert!(r.action_taken);
        assert!(state.items.count > items_before);
    }

    #[test]
    fn equip_from_inventory() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.inventory.add(rules_items::ItemKind::ShortSword);
        let r = state.step(GameCommand::EquipItem(0));
        assert!(r.action_taken);
        assert!(state.equipment.weapon.is_some());
    }

    #[test]
    fn effective_attack_with_weapon() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let base = state.effective_attack();
        state.inventory.add(rules_items::ItemKind::ShortSword);
        state.step(GameCommand::EquipItem(0));
        assert!(state.effective_attack() > base);
    }

    #[test]
    fn auto_pickup_when_enabled() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.auto_pickup = true;
        let pi = PLAYER_IDX as usize;
        // Find a walkable neighbor to place item and move onto
        for dir in crate::rules::direction::ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = state.entities.x[pi] + dx;
            let ny = state.entities.y[pi] + dy;
            if state.map.is_walkable(nx, ny) && state.entities.monster_at(nx, ny) == NO_ENTITY {
                state
                    .items
                    .spawn(nx, ny, rules_items::ItemKind::HealthPotion);
                state.step(GameCommand::Move(dir));
                assert!(
                    state.inventory.get(0).is_some(),
                    "auto-pickup should have grabbed item"
                );
                return;
            }
        }
    }

    #[test]
    fn inventory_persists_across_descent() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.inventory.add(rules_items::ItemKind::HealthPotion);
        walk_to_stairs(&mut state);
        state.step(GameCommand::Descend);
        assert!(state.inventory.get(0).is_some());
    }

    #[test]
    fn equipment_persists_across_descent() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.inventory.add(rules_items::ItemKind::ShortSword);
        state.step(GameCommand::EquipItem(0));
        walk_to_stairs(&mut state);
        state.step(GameCommand::Descend);
        assert!(state.equipment.weapon.is_some());
    }

    // ── Wandering & idle tests ───────────────────────────────────────

    #[test]
    fn idle_count_tracks_waits() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.step(GameCommand::Wait);
        assert_eq!(state.idle_count, 1);
        state.step(GameCommand::Wait);
        assert_eq!(state.idle_count, 2);
        // Move resets idle
        for dir in crate::rules::direction::ALL_DIRECTIONS {
            let r = state.step(GameCommand::Move(dir));
            if r.action_taken {
                assert_eq!(state.idle_count, 0);
                return;
            }
        }
    }

    #[test]
    fn combine_self_rejected() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.inventory.add(rules_items::ItemKind::HealthPotion);
        let r = state.step(GameCommand::Combine(0, 0));
        assert!(!r.action_taken);
    }

    // ── Auto-fight tests ─────────────────────────────────────────────

    #[test]
    fn auto_fight_no_adjacent_returns_none() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        // Kill all adjacent monsters
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        for i in 1..state.entities.count as usize {
            let dx = (state.entities.x[i] - px).abs();
            let dy = (state.entities.y[i] - py).abs();
            if dx <= 1 && dy <= 1 {
                state.entities.kill(i as u8);
            }
        }
        assert!(state.auto_fight().is_none());
    }

    #[test]
    fn auto_fight_kills_adjacent() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        // Spawn weak monster adjacent
        state
            .entities
            .spawn_monster(MonsterKind::Goblin, px + 1, py, AiBehavior::Chase);
        let target_idx = state.entities.count - 1;
        state.entities.hp[target_idx as usize] = 1;

        let result = state.auto_fight();
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.target_killed);
    }

    // ── Interact tests ───────────────────────────────────────────────

    fn place_item_at_player(state: &mut CompactGameState, kind: rules_items::ItemKind) {
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        state.items.spawn(px, py, kind);
    }

    #[test]
    fn interact_picks_up_item() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.items = ItemStore::new();
        place_item_at_player(&mut state, rules_items::ItemKind::HealthPotion);
        let r = state.step(GameCommand::Interact);
        assert!(r.action_taken);
        assert!(state.inventory.get(0).is_some());
    }

    #[test]
    fn interact_descends_stairs() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.items = ItemStore::new();
        walk_to_stairs(&mut state);
        let r = state.step(GameCommand::Interact);
        assert!(r.action_taken);
        assert_eq!(state.depth, 2);
    }

    #[test]
    fn interact_prefers_pickup_over_descend() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.items = ItemStore::new();
        walk_to_stairs(&mut state);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        state
            .items
            .spawn(px, py, rules_items::ItemKind::HealthPotion);
        let r = state.step(GameCommand::Interact);
        assert!(r.action_taken);
        assert_eq!(state.depth, 1, "should not have descended");
        assert!(
            state.inventory.get(0).is_some(),
            "should have picked up item"
        );
    }

    #[test]
    fn interact_on_empty_floor() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.items = ItemStore::new();
        let msg_before = state.log.total();
        let r = state.step(GameCommand::Interact);
        assert!(!r.action_taken);
        assert_eq!(
            state.log.total(),
            msg_before,
            "should not have logged any message"
        );
    }

    #[test]
    fn interact_full_inventory_on_item() {
        let mut state = CompactGameState::new(42, MAP_WIDTH, MAP_HEIGHT);
        state.items = ItemStore::new();
        for _ in 0..rules_items::MAX_INVENTORY {
            state.inventory.add(rules_items::ItemKind::ShortSword);
        }
        walk_to_stairs(&mut state);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        state.items.spawn(px, py, rules_items::ItemKind::ShortSword);
        let r = state.step(GameCommand::Interact);
        assert!(r.action_taken, "InventoryFull consumes turn");
        assert_eq!(state.depth, 1, "should not have descended");
    }
}
