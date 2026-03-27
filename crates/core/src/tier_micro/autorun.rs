//! No-std autorun stepper for the micro tier.
//!
//! Drives directional autorun on `MicroGameState` with the same stop
//! conditions as the standard tier. No heap allocation — suitable for
//! C64 and other constrained platforms.

use super::entity::EntityStore;
use super::fov::MicroFov;
use super::game::MicroGameState;
use super::map::TILE_STAIRS_DOWN;
use super::types::{MAX_ENTITIES, PLAYER_IDX};
use crate::command::{Direction, GameCommand};
use crate::rules::balance;
use crate::rules::message::AutorunStopCause;

/// Why autorun stopped (no_std, Copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MicroAutorunStop {
    /// Hit a wall or dead end.
    WallReached,
    /// A new living monster entered the field of view.
    MonsterSpotted,
    /// Player took damage from a monster.
    DamageTaken,
    /// Player died.
    GameOver,
    /// Forward path blocked with multiple alternative directions.
    CorridorBranches,
    /// Safety cap on steps reached.
    MaxSteps,
    /// Pathfinding reached the destination tile.
    PathComplete,
    /// Player stepped onto stairs.
    StairsFound,
}

impl MicroAutorunStop {
    /// Convert to the shared `AutorunStopCause` for message formatting.
    pub const fn to_cause(self) -> AutorunStopCause {
        match self {
            Self::WallReached => AutorunStopCause::WallReached,
            Self::MonsterSpotted => AutorunStopCause::MonsterSpotted,
            Self::DamageTaken => AutorunStopCause::DamageTaken,
            Self::GameOver => AutorunStopCause::GameOver,
            Self::CorridorBranches => AutorunStopCause::CorridorBranches,
            Self::MaxSteps => AutorunStopCause::MaxSteps,
            Self::PathComplete => AutorunStopCause::PathComplete,
            Self::StairsFound => AutorunStopCause::StairsFound,
        }
    }
}

/// Result of one autorun step (no_std).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroStepOutcome {
    /// Step succeeded, stepper can continue.
    Continue,
    /// Sequence is finished.
    Done(MicroAutorunStop),
}

/// How often to compute full FOV during autorun (every Nth step).
/// Intermediate steps use `step_skip_fov` for speed on slow CPUs.
const FOV_INTERVAL: u8 = 3;

/// Autorun stepper for the micro tier — directional movement only.
///
/// Call `next_step()` in a loop to drive the autorun sequence one step
/// at a time. Each call executes one `Move` command on the game state
/// and checks all stop conditions.
///
/// FOV is only computed every `FOV_INTERVAL` steps to reduce cost on
/// 6502. Adjacent monster and damage checks run every step regardless.
pub struct MicroAutorunStepper {
    dir: Direction,
    steps_taken: u8,
    max_steps: u8,
    visible_before: [bool; MAX_ENTITIES],
    stairs_visible_before: bool,
}

/// Check if any stairs tile is currently visible in the FOV.
pub fn stairs_in_fov(map: &super::map::MicroMap, fov: &MicroFov) -> bool {
    for y in 0..map.height {
        for x in 0..map.width {
            if fov.is_visible(x, y) && map.tile_at(x, y) == TILE_STAIRS_DOWN {
                return true;
            }
        }
    }
    false
}

/// Check if any alive monster is adjacent (Chebyshev distance <= 1) to the player.
pub fn has_adjacent_monster(entities: &EntityStore) -> bool {
    let pi = PLAYER_IDX as usize;
    let px = entities.x[pi];
    let py = entities.y[pi];
    let mut i: usize = 1;
    while i < entities.count as usize {
        if entities.alive[i] {
            let dx = entities.x[i].abs_diff(px);
            let dy = entities.y[i].abs_diff(py);
            if dx <= 1 && dy <= 1 {
                return true;
            }
        }
        i += 1;
    }
    false
}

impl MicroAutorunStepper {
    /// Create a new autorun stepper for the given direction.
    pub fn new(dir: Direction, stairs_already_visible: bool) -> Self {
        Self {
            dir,
            steps_taken: 0,
            max_steps: balance::MAX_AUTORUN_STEPS,
            visible_before: [false; MAX_ENTITIES],
            stairs_visible_before: stairs_already_visible,
        }
    }

    /// How many steps have been taken so far.
    pub fn steps_taken(&self) -> u8 {
        self.steps_taken
    }

    /// Execute one step of the autorun sequence.
    ///
    /// FOV is computed every `FOV_INTERVAL` steps. On intermediate steps
    /// the "new monster spotted" check is skipped (but adjacent monster,
    /// damage, and game-over checks still run every step).
    pub fn next_step(&mut self, state: &mut MicroGameState) -> MicroStepOutcome {
        let pi = PLAYER_IDX as usize;

        // Check 1: max steps cap.
        if self.steps_taken >= self.max_steps {
            return MicroStepOutcome::Done(MicroAutorunStop::MaxSteps);
        }

        // Check 2: adjacent monster before stepping.
        if has_adjacent_monster(&state.entities) {
            return MicroStepOutcome::Done(MicroAutorunStop::MonsterSpotted);
        }

        // Decide whether this step computes FOV.
        let do_fov = self.steps_taken.is_multiple_of(FOV_INTERVAL);

        // Snapshot HP (always) and visible monsters (only on FOV steps).
        let hp_before = state.entities.hp[pi];
        if do_fov {
            self.snapshot_visible(&state.entities, &state.fov);
        }

        // Execute the move — skip FOV on intermediate steps.
        let result = if do_fov {
            state.step(GameCommand::Move(self.dir))
        } else {
            state.step_skip_fov(GameCommand::Move(self.dir))
        };

        // Check 3: wall hit.
        if !result.action_taken {
            return MicroStepOutcome::Done(MicroAutorunStop::WallReached);
        }

        self.steps_taken += 1;

        // Check 4: game over.
        if result.game_over {
            return MicroStepOutcome::Done(MicroAutorunStop::GameOver);
        }

        // Check 5: damage taken.
        if state.entities.hp[pi] < hp_before {
            return MicroStepOutcome::Done(MicroAutorunStop::DamageTaken);
        }

        // Check 6: new monster spotted (only on FOV steps).
        if do_fov && self.has_new_visible_monster(&state.entities, &state.fov) {
            return MicroStepOutcome::Done(MicroAutorunStop::MonsterSpotted);
        }

        // Check 6b: stairs newly visible in FOV (only on FOV steps).
        if do_fov && !self.stairs_visible_before && stairs_in_fov(&state.map, &state.fov) {
            return MicroStepOutcome::Done(MicroAutorunStop::StairsFound);
        }

        // Check 7: corridor branches — wall ahead with 2+ alternatives.
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        let (dx, dy) = self.dir.to_offset();
        let ahead_x = (px as i8 + dx as i8) as u8;
        let ahead_y = (py as i8 + dy as i8) as u8;
        if !state.map.is_walkable(ahead_x, ahead_y) {
            let alternatives = state
                .map
                .open_neighbors_excluding(px, py, -(dx as i8), -(dy as i8));
            if alternatives >= 2 {
                return MicroStepOutcome::Done(MicroAutorunStop::CorridorBranches);
            }
            return MicroStepOutcome::Done(MicroAutorunStop::WallReached);
        }

        // Check 8: stepped onto stairs.
        if state.map.tile_at(px, py) == TILE_STAIRS_DOWN {
            return MicroStepOutcome::Done(MicroAutorunStop::StairsFound);
        }

        MicroStepOutcome::Continue
    }

    fn snapshot_visible(&mut self, entities: &EntityStore, fov: &MicroFov) {
        let mut i: usize = 1;
        while i < entities.count as usize {
            self.visible_before[i] =
                entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]);
            i += 1;
        }
    }

    fn has_new_visible_monster(&self, entities: &EntityStore, fov: &MicroFov) -> bool {
        let mut i: usize = 1;
        while i < entities.count as usize {
            if entities.alive[i]
                && fov.is_visible(entities.x[i], entities.y[i])
                && !self.visible_before[i]
            {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ── BFS-based pathfinding stepper ──────────────────────────────────

use super::pathfinding::{self, BfsBuffers};

/// Autorun stepper that follows a BFS path to a target tile.
///
/// Re-pathfinds each step (handles map changes from combat, spawns, etc.).
/// Uses the same stop conditions as directional autorun: monster spotted,
/// damage taken, game over, max steps.
pub struct MicroBfsStepper {
    pub tx: u8,
    pub ty: u8,
    steps_taken: u8,
    max_steps: u8,
    visible_before: [bool; MAX_ENTITIES],
    stairs_visible_before: bool,
}

impl MicroBfsStepper {
    /// Create a new BFS stepper targeting (tx, ty).
    pub fn new(tx: u8, ty: u8, stairs_already_visible: bool) -> Self {
        Self {
            tx,
            ty,
            steps_taken: 0,
            max_steps: balance::MAX_AUTORUN_STEPS,
            visible_before: [false; MAX_ENTITIES],
            stairs_visible_before: stairs_already_visible,
        }
    }

    /// How many steps have been taken so far.
    pub fn steps_taken(&self) -> u8 {
        self.steps_taken
    }

    /// Execute one step toward the target.
    pub fn next_step(
        &mut self,
        state: &mut MicroGameState,
        buf: &mut BfsBuffers,
    ) -> MicroStepOutcome {
        let pi = PLAYER_IDX as usize;

        // Check 1: max steps cap.
        if self.steps_taken >= self.max_steps {
            return MicroStepOutcome::Done(MicroAutorunStop::MaxSteps);
        }

        // Check 2: already at target.
        if state.entities.x[pi] == self.tx && state.entities.y[pi] == self.ty {
            return MicroStepOutcome::Done(MicroAutorunStop::PathComplete);
        }

        // Check 3: adjacent monster before stepping.
        if has_adjacent_monster(&state.entities) {
            return MicroStepOutcome::Done(MicroAutorunStop::MonsterSpotted);
        }

        // Find direction toward target via backward BFS.
        let dir = match pathfinding::find_first_step(
            state.entities.x[pi],
            state.entities.y[pi],
            self.tx,
            self.ty,
            &state.map,
            &state.fov,
            buf,
        ) {
            Some(d) => d,
            None => return MicroStepOutcome::Done(MicroAutorunStop::PathComplete),
        };

        // Snapshot HP and visible monsters.
        let hp_before = state.entities.hp[pi];
        self.snapshot_visible(&state.entities, &state.fov);

        // Execute the move.
        let result = state.step(GameCommand::Move(dir));

        if !result.action_taken {
            return MicroStepOutcome::Done(MicroAutorunStop::WallReached);
        }

        self.steps_taken += 1;

        // Check: game over.
        if result.game_over {
            return MicroStepOutcome::Done(MicroAutorunStop::GameOver);
        }

        // Check: damage taken.
        if state.entities.hp[pi] < hp_before {
            return MicroStepOutcome::Done(MicroAutorunStop::DamageTaken);
        }

        // Check: new monster spotted.
        if self.has_new_visible_monster(&state.entities, &state.fov) {
            return MicroStepOutcome::Done(MicroAutorunStop::MonsterSpotted);
        }

        // Check: stairs newly visible in FOV.
        if !self.stairs_visible_before && stairs_in_fov(&state.map, &state.fov) {
            return MicroStepOutcome::Done(MicroAutorunStop::StairsFound);
        }

        // Check: reached target.
        if state.entities.x[pi] == self.tx && state.entities.y[pi] == self.ty {
            return MicroStepOutcome::Done(MicroAutorunStop::PathComplete);
        }

        // Check: stepped onto stairs.
        if state
            .map
            .tile_at(state.entities.x[pi], state.entities.y[pi])
            == TILE_STAIRS_DOWN
        {
            return MicroStepOutcome::Done(MicroAutorunStop::StairsFound);
        }

        MicroStepOutcome::Continue
    }

    fn snapshot_visible(&mut self, entities: &EntityStore, fov: &MicroFov) {
        let mut i: usize = 1;
        while i < entities.count as usize {
            self.visible_before[i] =
                entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]);
            i += 1;
        }
    }

    fn has_new_visible_monster(&self, entities: &EntityStore, fov: &MicroFov) -> bool {
        let mut i: usize = 1;
        while i < entities.count as usize {
            if entities.alive[i]
                && fov.is_visible(entities.x[i], entities.y[i])
                && !self.visible_before[i]
            {
                return true;
            }
            i += 1;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier_micro::game::MicroGameState;
    use crate::tier_micro::types::{DEFAULT_MAP_HEIGHT, DEFAULT_MAP_WIDTH};

    /// Helper: create a game and run autorun to completion, returning the stop reason.
    fn run_autorun(state: &mut MicroGameState, dir: Direction) -> MicroAutorunStop {
        let mut stepper = MicroAutorunStepper::new(dir, stairs_in_fov(&state.map, &state.fov));
        loop {
            match stepper.next_step(state) {
                MicroStepOutcome::Continue => continue,
                MicroStepOutcome::Done(reason) => return reason,
            }
        }
    }

    #[test]
    fn autorun_stops_at_wall() {
        let mut state = MicroGameState::new(42, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
        // Run east — will eventually hit a wall.
        let reason = run_autorun(&mut state, Direction::East);
        assert!(
            matches!(
                reason,
                MicroAutorunStop::WallReached | MicroAutorunStop::CorridorBranches
            ),
            "Expected wall/corridor stop, got {:?}",
            reason
        );
    }

    #[test]
    fn autorun_respects_max_steps() {
        // Use a large open map to maximize steps before wall.
        let mut state = MicroGameState::new(42, 80, 60);
        let mut stepper =
            MicroAutorunStepper::new(Direction::East, stairs_in_fov(&state.map, &state.fov));
        let mut steps = 0u8;
        loop {
            match stepper.next_step(&mut state) {
                MicroStepOutcome::Continue => steps += 1,
                MicroStepOutcome::Done(reason) => {
                    if matches!(reason, MicroAutorunStop::MaxSteps) {
                        assert_eq!(steps, balance::MAX_AUTORUN_STEPS);
                    }
                    // Other reasons are valid too (wall, monster, etc.)
                    break;
                }
            }
        }
    }

    #[test]
    fn has_adjacent_monster_detects_neighbor() {
        use crate::rules::monster_table::{AiBehavior, MonsterKind};

        let mut state = MicroGameState::new(42, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];

        // No adjacent monster initially (rooms are separated from spawn).
        // Spawn one adjacent.
        state
            .entities
            .spawn_monster(MonsterKind::Goblin, px + 1, py, AiBehavior::Chase);
        assert!(has_adjacent_monster(&state.entities));
    }

    #[test]
    fn has_adjacent_monster_ignores_distant() {
        use crate::rules::monster_table::{AiBehavior, MonsterKind};

        let mut state = MicroGameState::new(42, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];

        // Spawn monster 3 tiles away.
        state
            .entities
            .spawn_monster(MonsterKind::Goblin, px + 3, py, AiBehavior::Chase);
        assert!(!has_adjacent_monster(&state.entities));
    }

    #[test]
    fn autorun_stops_on_adjacent_monster_at_start() {
        use crate::rules::monster_table::{AiBehavior, MonsterKind};

        let mut state = MicroGameState::new(42, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];

        // Place monster adjacent — autorun should refuse to start.
        state
            .entities
            .spawn_monster(MonsterKind::Goblin, px + 1, py, AiBehavior::Chase);

        let mut stepper =
            MicroAutorunStepper::new(Direction::East, stairs_in_fov(&state.map, &state.fov));
        match stepper.next_step(&mut state) {
            MicroStepOutcome::Done(MicroAutorunStop::MonsterSpotted) => {}
            other => panic!("Expected MonsterSpotted, got {:?}", other),
        }
    }

    #[test]
    fn autorun_stops_when_stairs_enter_fov() {
        use super::super::fov::MicroFov;
        use super::super::map::{MicroMap, TILE_FLOOR, TILE_STAIRS_DOWN};

        // Layout: long east-west corridor at y=10, with a room at x=15
        // containing stairs. Player starts far west, runs east.
        // When player gets close enough, stairs enter FOV.
        let w: u8 = 30;
        let h: u8 = 20;
        let mut state = MicroGameState::new(42, w, h);
        state.map = MicroMap::new(w, h);

        // Carve east-west corridor at y=10.
        for x in 1..w - 1 {
            state.map.set_tile(x, 10, TILE_FLOOR);
        }
        // Carve a room at (14,7)-(17,13) — visible from corridor.
        for y in 7..14 {
            for x in 14..18 {
                state.map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Stairs inside the room at (16,8) — 2 tiles north of corridor.
        state.map.set_tile(16, 8, TILE_STAIRS_DOWN);

        // Player starts at (2,10), running east.
        state.entities.x[0] = 2;
        state.entities.y[0] = 10;
        for i in 1..state.entities.count as usize {
            state.entities.alive[i] = false;
        }

        state.fov = MicroFov::new(w, h);
        state.fov.compute_fov(2, 10, &state.map);

        // Stairs should NOT be visible from (2,10) — 14 tiles east.
        assert!(
            !state.fov.is_visible(16, 8),
            "stairs should not be visible from starting position"
        );

        let reason = run_autorun(&mut state, Direction::East);
        assert_eq!(
            reason,
            MicroAutorunStop::StairsFound,
            "autorun should stop when stairs enter FOV"
        );

        // Player should be east of start but NOT on the stairs tile.
        let px = state.entities.x[0];
        let py = state.entities.y[0];
        assert!(px > 2, "player should have moved east");
        assert_eq!(py, 10, "player should still be in corridor");
        assert_ne!((px, py), (16, 8), "player should not be on stairs tile");
    }
}
