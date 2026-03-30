//! No-std autorun stepper for the compact tier (GBA).
//!
//! Drives directional and BFS-guided autorun on `CompactGameState` with
//! the same stop conditions as all other tiers.

use super::entity::EntityStore;
use super::fov::CompactFov;
use super::game::CompactGameState;
use super::map::TILE_STAIRS_DOWN;
use super::pathfinding::{self, BfsBuffers};
use super::types::{Coord, MAX_ENTITIES, PLAYER_IDX};
use crate::command::{Direction, GameCommand};
use crate::rules::balance;
use crate::rules::message::AutorunStopCause;

/// Why autorun stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompactAutorunStop {
    WallReached,
    MonsterSpotted,
    DamageTaken,
    GameOver,
    CorridorBranches,
    MaxSteps,
    PathComplete,
    StairsFound,
}

impl CompactAutorunStop {
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

/// Result of one autorun step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactStepOutcome {
    Continue,
    Done(CompactAutorunStop),
}

/// How often to compute full FOV during autorun (every Nth step).
const FOV_INTERVAL: u8 = 3;

/// Check if any stairs tile is currently visible in the FOV.
pub fn stairs_in_fov(map: &super::map::CompactMap, fov: &CompactFov) -> bool {
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
    for i in 1..entities.count as usize {
        if entities.alive[i] {
            let dx = (entities.x[i] - px).abs();
            let dy = (entities.y[i] - py).abs();
            if dx <= 1 && dy <= 1 {
                return true;
            }
        }
    }
    false
}

// ── Directional autorun stepper ──────────────────────────────────────

/// Autorun stepper for directional movement.
pub struct CompactAutorunStepper {
    dir: Direction,
    steps_taken: u8,
    max_steps: u8,
    visible_before: [bool; MAX_ENTITIES],
    stairs_visible_before: bool,
}

impl CompactAutorunStepper {
    pub fn new(dir: Direction, stairs_already_visible: bool) -> Self {
        Self {
            dir,
            steps_taken: 0,
            max_steps: balance::MAX_AUTORUN_STEPS,
            visible_before: [false; MAX_ENTITIES],
            stairs_visible_before: stairs_already_visible,
        }
    }

    pub fn steps_taken(&self) -> u8 {
        self.steps_taken
    }

    pub fn next_step(&mut self, state: &mut CompactGameState) -> CompactStepOutcome {
        let pi = PLAYER_IDX as usize;

        if self.steps_taken >= self.max_steps {
            return CompactStepOutcome::Done(CompactAutorunStop::MaxSteps);
        }

        if has_adjacent_monster(&state.entities) {
            return CompactStepOutcome::Done(CompactAutorunStop::MonsterSpotted);
        }

        let do_fov = self.steps_taken.is_multiple_of(FOV_INTERVAL);

        let hp_before = state.entities.hp[pi];
        if do_fov {
            self.snapshot_visible(&state.entities, &state.fov);
        }

        let result = if do_fov {
            state.step(GameCommand::Move(self.dir))
        } else {
            state.step_skip_fov(GameCommand::Move(self.dir))
        };

        if !result.action_taken {
            return CompactStepOutcome::Done(CompactAutorunStop::WallReached);
        }

        self.steps_taken += 1;

        if result.game_over {
            return CompactStepOutcome::Done(CompactAutorunStop::GameOver);
        }

        if state.entities.hp[pi] < hp_before {
            return CompactStepOutcome::Done(CompactAutorunStop::DamageTaken);
        }

        if do_fov && self.has_new_visible_monster(&state.entities, &state.fov) {
            return CompactStepOutcome::Done(CompactAutorunStop::MonsterSpotted);
        }

        if do_fov && !self.stairs_visible_before && stairs_in_fov(&state.map, &state.fov) {
            return CompactStepOutcome::Done(CompactAutorunStop::StairsFound);
        }

        let px = state.entities.x[pi];
        let py = state.entities.y[pi];
        let (dx, dy) = self.dir.to_offset();
        let ahead_x = px + dx as Coord;
        let ahead_y = py + dy as Coord;
        if !state.map.is_walkable(ahead_x, ahead_y) {
            let alternatives = state.map.open_neighbors_excluding(px, py, -dx, -dy);
            if alternatives >= 2 {
                return CompactStepOutcome::Done(CompactAutorunStop::CorridorBranches);
            }
            return CompactStepOutcome::Done(CompactAutorunStop::WallReached);
        }

        if state.map.tile_at(px, py) == TILE_STAIRS_DOWN {
            return CompactStepOutcome::Done(CompactAutorunStop::StairsFound);
        }

        CompactStepOutcome::Continue
    }

    fn snapshot_visible(&mut self, entities: &EntityStore, fov: &CompactFov) {
        for i in 1..entities.count as usize {
            self.visible_before[i] =
                entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]);
        }
    }

    fn has_new_visible_monster(&self, entities: &EntityStore, fov: &CompactFov) -> bool {
        for i in 1..entities.count as usize {
            if entities.alive[i]
                && fov.is_visible(entities.x[i], entities.y[i])
                && !self.visible_before[i]
            {
                return true;
            }
        }
        false
    }
}

// ── BFS-based pathfinding stepper ────────────────────────────────────

/// Autorun stepper that follows a BFS path to a target tile.
pub struct CompactBfsStepper {
    pub tx: Coord,
    pub ty: Coord,
    steps_taken: u8,
    max_steps: u8,
    visible_before: [bool; MAX_ENTITIES],
    stairs_visible_before: bool,
}

impl CompactBfsStepper {
    pub fn new(tx: Coord, ty: Coord, stairs_already_visible: bool) -> Self {
        Self {
            tx,
            ty,
            steps_taken: 0,
            max_steps: balance::MAX_AUTORUN_STEPS,
            visible_before: [false; MAX_ENTITIES],
            stairs_visible_before: stairs_already_visible,
        }
    }

    pub fn steps_taken(&self) -> u8 {
        self.steps_taken
    }

    pub fn next_step(
        &mut self,
        state: &mut CompactGameState,
        buf: &mut BfsBuffers,
    ) -> CompactStepOutcome {
        let pi = PLAYER_IDX as usize;

        if self.steps_taken >= self.max_steps {
            return CompactStepOutcome::Done(CompactAutorunStop::MaxSteps);
        }

        if state.entities.x[pi] == self.tx && state.entities.y[pi] == self.ty {
            return CompactStepOutcome::Done(CompactAutorunStop::PathComplete);
        }

        if has_adjacent_monster(&state.entities) {
            return CompactStepOutcome::Done(CompactAutorunStop::MonsterSpotted);
        }

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
            None => return CompactStepOutcome::Done(CompactAutorunStop::PathComplete),
        };

        let hp_before = state.entities.hp[pi];
        self.snapshot_visible(&state.entities, &state.fov);

        let result = state.step(GameCommand::Move(dir));

        if !result.action_taken {
            return CompactStepOutcome::Done(CompactAutorunStop::WallReached);
        }

        self.steps_taken += 1;

        if result.game_over {
            return CompactStepOutcome::Done(CompactAutorunStop::GameOver);
        }

        if state.entities.hp[pi] < hp_before {
            return CompactStepOutcome::Done(CompactAutorunStop::DamageTaken);
        }

        if self.has_new_visible_monster(&state.entities, &state.fov) {
            return CompactStepOutcome::Done(CompactAutorunStop::MonsterSpotted);
        }

        if !self.stairs_visible_before && stairs_in_fov(&state.map, &state.fov) {
            return CompactStepOutcome::Done(CompactAutorunStop::StairsFound);
        }

        if state.entities.x[pi] == self.tx && state.entities.y[pi] == self.ty {
            return CompactStepOutcome::Done(CompactAutorunStop::PathComplete);
        }

        if state
            .map
            .tile_at(state.entities.x[pi], state.entities.y[pi])
            == TILE_STAIRS_DOWN
        {
            return CompactStepOutcome::Done(CompactAutorunStop::StairsFound);
        }

        CompactStepOutcome::Continue
    }

    fn snapshot_visible(&mut self, entities: &EntityStore, fov: &CompactFov) {
        for i in 1..entities.count as usize {
            self.visible_before[i] =
                entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]);
        }
    }

    fn has_new_visible_monster(&self, entities: &EntityStore, fov: &CompactFov) -> bool {
        for i in 1..entities.count as usize {
            if entities.alive[i]
                && fov.is_visible(entities.x[i], entities.y[i])
                && !self.visible_before[i]
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::balance;
    use crate::rules::monster_table::{AiBehavior, MonsterKind};
    use crate::tier_compact::map::{CompactMap, TILE_FLOOR};
    use crate::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH};

    #[test]
    fn has_adjacent_monster_detects_neighbor() {
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        assert!(has_adjacent_monster(&entities));
    }

    #[test]
    fn has_adjacent_monster_ignores_distant() {
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 13, 10, AiBehavior::Chase);
        assert!(!has_adjacent_monster(&entities));
    }

    #[test]
    fn has_adjacent_monster_ignores_dead() {
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        entities.kill(1);
        assert!(!has_adjacent_monster(&entities));
    }

    #[test]
    fn stairs_in_fov_detection() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 5..15 {
            for x in 5..15 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        map.set_tile(10, 10, TILE_STAIRS_DOWN);

        let mut fov = CompactFov::new(MAP_WIDTH, MAP_HEIGHT);
        fov.compute_fov(10, 10, balance::FOV_RADIUS, &map);
        assert!(stairs_in_fov(&map, &fov));
    }

    #[test]
    fn stairs_not_in_fov_when_distant() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 1..MAP_HEIGHT - 1 {
            for x in 1..MAP_WIDTH - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        map.set_tile(MAP_WIDTH - 2, MAP_HEIGHT - 2, TILE_STAIRS_DOWN);

        let mut fov = CompactFov::new(MAP_WIDTH, MAP_HEIGHT);
        fov.compute_fov(5, 5, balance::FOV_RADIUS, &map);
        assert!(!stairs_in_fov(&map, &fov));
    }
}
