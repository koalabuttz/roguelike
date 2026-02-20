# Acoustic Propagation & The Living Dungeon

> **Status:** Proposed. A new core system that adds sound as a parallel sense to vision, creating a dungeon that players *hear* as well as see.

## Summary

Traditional roguelikes are silent worlds perceived through a single sense: sight. This proposal introduces **acoustic propagation** as a first-class game mechanic — a sound map that runs alongside the existing FOV system, giving both the player and monsters the ability to hear through walls. Loud actions (combat, running) attract distant monsters. Quiet play (walking, waiting) keeps you hidden. The player receives audio cues about things they can't yet see: footsteps in the next corridor, a roar from deep in the dungeon.

On platforms with audio hardware — particularly the Commodore 64's SID chip — these sound events are rendered as **procedural, game-state-reactive audio**. On text-only platforms, they manifest as message log text and approximate-position glyphs (`?`). The core game logic produces abstract sound events; platform layers decide how to present them.

A companion system, **encounter escalation**, creates a global tension mechanic that uses the sound system to communicate the dungeon's rising hostility. Together, these systems transform the dungeon from a static map of rooms into a reactive, living environment that rewards careful play and punishes recklessness.

## Motivation

The [gameplay implementation plan](gameplay-implementation-plan.md) identified that the current game has no pacing mechanism beyond HP regen. The [roadmap](../roadmap.md) lists sound effects and music as polish items, treating audio as decoration. This proposal argues that **sound should be a game mechanic, not a cosmetic layer** — and that designing it this way creates a feature that is:

1. **Mechanically novel.** No traditional roguelike has a proper dual FOV/sound system. Brogue has stealth and DCSS has noise, but neither gives the *player* acoustic information as a parallel sense. The player hearing monsters through walls before seeing them is a new information channel.

2. **Platform-differential.** On the C64, the SID chip can render sound events as procedural audio — making the C64 version arguably *better* than the terminal version for this feature, not worse. This inverts the usual retro-port dynamic.

3. **Architecturally cheap.** Sound propagation is a bounded integer flood fill over the existing tile grid. It requires one `u8` per tile, zero heap allocations per turn, and a few hundred operations per sound event. It scales trivially to GBA and C64.

4. **Synergistic with planned features.** Creature mood (Phase 5 of the gameplay plan) already needs "awareness" beyond visual range. Sound gives it a natural trigger: a monster hears combat and becomes alert before it sees the player. The property bitfield system (Phase 6) can add sound-related properties (`SILENT`, `LOUD`, `ECHOLOCATING`).

## Core System: Sound Map

### Data Model

```rust
// In game.rs or a new sound.rs

/// A sound event emitted by an action.
pub struct SoundEvent {
    /// Where the sound originated.
    pub source: Pos,
    /// What kind of sound it is (affects monster response and audio rendering).
    pub kind: SoundKind,
    /// How far the sound carries (in tiles, before attenuation).
    pub intensity: u8,
}

/// Categories of sound. Each has different propagation characteristics
/// and different audio rendering on platforms that support it.
pub enum SoundKind {
    /// Melee combat — loud, sharp.
    Combat,
    /// Normal walking — moderate, rhythmic.
    Footstep,
    /// Running (autorun) — louder than walking.
    Run,
    /// Monster movement — varies by monster type.
    MonsterStep,
    /// Monster roar/alert — loud, carries far.
    Alert,
    /// Ambient drip/creak — quiet, atmospheric.
    Ambient,
}
```

### Propagation Algorithm

Sound propagates via **integer BFS with attenuation** — identical in structure to `pathfinding::nearest_by_cost()`, which already exists in the codebase.

```
fn propagate_sound(map: &Map, source: Pos, intensity: u8, sound_grid: &mut [u8]) {
    // BFS from source. Each step costs 1 through Floor, 3 through Door (future),
    // and is blocked entirely by Wall. Write max(existing, intensity - cost) at
    // each reached tile. Cap at intensity == 0.
}
```

Key design decisions:

- **Walls block sound.** No transmission through solid walls. This is a simplification (real sound passes through walls with heavy attenuation), but it creates clear tactical reasoning: corridors carry sound, rooms muffle it.
- **Corridors amplify sound.** 1-wide corridors carry sound at full intensity (no attenuation per step in narrow corridors — optional rule). This makes corridor topology mechanically significant: a fight in a narrow corridor alerts everything connected to it.
- **The sound grid is ephemeral.** It's recomputed per-turn from that turn's events, not accumulated. This keeps it a simple fixed-size array, never growing, never stale.
- **The grid is `u8`, not `HashSet`.** On an 80x40 map, that's 3,200 bytes. On a C64 (40x25), it's 1,000 bytes. On a GBA, it's trivial. Compare this to the `HashSet<Pos>` used for FOV/explored, which allocates on the heap and has per-entry overhead.

### Memory Budget

| Platform | Map size | Sound grid | Total new memory |
|----------|----------|------------|------------------|
| Terminal | 80x40 | 3,200 B | ~3.5 KB |
| GBA | 30x20 | 600 B | ~0.8 KB |
| C64 | 40x25 | 1,000 B | ~1.2 KB |

The sound grid is the *only* new allocation. Sound events themselves are stack-allocated and processed immediately.

### Integration Point

In `GameState::step()`, after the player's action and before monster turns:

```rust
pub fn step(&mut self, cmd: GameCommand) -> StepResult {
    let action_taken = self.handle_command(cmd);

    if action_taken {
        self.update_fov();

        // NEW: Emit sound events from the player's action, propagate
        // through the map, and record results for monster AI + rendering.
        self.update_sound_map(cmd);

        if ai::run_monster_turns(&mut self.entities, &self.map, &mut self.log) {
            self.game_over = true;
        }

        self.turn_count += 1;
        self.apply_regen();
    }
    // ...
}
```

Monster turns also emit sounds (footsteps, attacks), which are propagated in a second pass after all monsters have acted. The player "hears" these on the *same turn* — enabling the experience of hearing a monster approach before your next action.

### What the Player Perceives

#### Text-based platforms (terminal, SSH, MCP)

- **Message log:** `"You hear footsteps to the east."`, `"A distant roar echoes from below."`, `"Something moves in the darkness."` Directional hints are based on the sound source's relative position to the player.
- **Map glyphs:** A `?` glyph appears at the approximate position of a heard-but-unseen sound source. The `?` is placed at the nearest tile along the propagation path that the player has explored, not at the monster's exact position — preserving fog-of-war integrity.
- **Look mode:** Examining a `?` tile shows `"Sound: footsteps (east corridor)"`.

#### C64 (SID chip)

See [Platform-Native Audio: C64](#c64-sid-chip) below.

#### GBA (PSG channels)

See [Platform-Native Audio: GBA](#gba-psg) below.

### Monster AI Integration

Sound gives monsters a second awareness channel beyond visual LOS. In `ai.rs`, the awareness check becomes:

```rust
fn is_aware(entities: &[Entity], idx: usize, px: Coord, py: Coord,
            map: &Map, sound_grid: &[u8]) -> bool {
    // Visual awareness (existing)
    let visual = match entities[idx].ai {
        AiBehavior::Chase => fov::can_see(map, ex, ey, px, py, sight_radius),
        _ => false,
    };

    // Acoustic awareness (new)
    let acoustic = {
        let idx = map.idx(entities[idx].x, entities[idx].y);
        sound_grid[idx] > 0  // Monster's tile has nonzero sound level
    };

    visual || acoustic
}
```

Sound-aware monsters don't know the player's exact position — they pathfind toward the sound source tile, which might be around a corner or at the end of a corridor. This creates a natural "investigate" behavior without adding a new AI state: the Chase AI already moves toward a target position, and the target can be "where the sound came from" instead of "where the player is."

For creature mood integration (gameplay plan Phase 5): hearing combat in adjacent rooms shifts mood. A monster that *hears* its allies dying gets a morale penalty, even if it can't see the fight. This creates cascading fear through corridors.

### Sound Levels by Action

All values are tuning parameters in `game.toml`:

```toml
[sound]
combat_intensity = 12       # Melee combat — very loud
footstep_intensity = 4      # Normal movement
autorun_intensity = 8       # Running — louder than walking
wait_intensity = 0          # Waiting is silent
monster_step_intensity = 5  # Monster footstep (varies by monster)
alert_intensity = 15        # Monster roar/alert call
wall_attenuation = 255      # Walls fully block sound (impassable)
corridor_bonus = 0          # Extra carry distance in 1-wide corridors
```

Adding `sound_intensity` to `MonsterDef` allows per-monster-type noise levels: trolls are loud, goblins are quiet, a future "shade" monster is silent.

```toml
[[monsters]]
name = "Troll"
# ... existing fields ...
sound_intensity = 8         # Heavy footsteps, audible from far away

[[monsters]]
name = "Goblin"
sound_intensity = 3         # Light footsteps, harder to detect
```

## Companion System: Encounter Escalation

### Concept

A global **tension counter** that rises as the player explores and kills, and drops when the player waits or rests. At thresholds, the dungeon responds with escalating consequences. The sound system delivers these consequences to the player as audible warnings — rumbles, distant roars, approaching footsteps — before the mechanical effects arrive.

This is distinct from wandering monsters (gameplay plan Phase 1), which spawn on a fixed timer. Escalation is *reactive* — it responds to how aggressively the player is playing. A cautious player who clears rooms slowly and waits to heal keeps tension low. A speedrunner who blitzes through rooms and kills everything fast pushes tension high and faces consequences.

### Data Model

```rust
// In game.rs
pub struct GameState {
    // ... existing fields ...

    /// Global tension level (0–100). Rises on kills and exploration,
    /// decays on wait turns.
    pub tension: Stat,
}
```

### Tension Rules

| Event | Tension change | Notes |
|-------|---------------|-------|
| Kill a monster | +5 to +15 (scaled by monster difficulty) | Stronger monsters generate more tension |
| Explore a new room | +3 | Entering a room whose center was previously unexplored |
| Wait / rest | -1 per turn | Natural decay, but waiting also means wandering monsters approach |
| Take damage | +2 | Pain makes the dungeon more hostile |
| Per-turn passive decay | -0.5 (rounded, applied every 2 turns) | Prevents permanent high tension from a single burst |

### Escalation Thresholds

| Tension | Effect | Sound cue |
|---------|--------|-----------|
| 25 | **Distant rumble.** Message log only. | SID: low-frequency noise burst, filtered. Terminal: `"The dungeon groans..."` |
| 50 | **Alert pulse.** All dormant monsters within 15 tiles of the player become aware (as if they heard a loud sound). | SID: rising filter sweep. Terminal: `"Something stirs in the darkness."` |
| 75 | **Reinforcements.** Spawn 1-2 monsters from an unexplored room, using the existing spawn system. They start moving toward the sound of combat. | SID: heavy drum-like pulse. Terminal: `"You hear heavy footsteps approaching!"` |
| 100 | **Dungeon fury.** All living monsters on the map become aware of the player. Tension caps at 100 and decays normally from here. | SID: dissonant chord, all 3 oscillators. Terminal: `"The dungeon erupts with fury! Everything knows you're here."` |

### Configuration

```toml
[escalation]
enabled = true
kill_tension_base = 5           # Per kill, before difficulty scaling
room_tension = 3                # Per new room explored
wait_decay = 1                  # Tension lost per wait turn
passive_decay_interval = 2      # Passive decay every N turns
threshold_rumble = 25
threshold_alert = 50
threshold_reinforce = 75
threshold_fury = 100
alert_radius = 15               # Tiles within which dormant monsters wake
reinforce_count_min = 1
reinforce_count_max = 2
```

### Interaction with Sound Map

Escalation thresholds emit synthetic sound events:

- At threshold 50, a `SoundEvent { kind: Alert, intensity: 15 }` is emitted from the player's position, propagating through the entire connected corridor network. Monsters that receive this sound become aware.
- At threshold 75, reinforcement monsters spawn at a distant room and immediately begin pathfinding toward the player's last known sound position.

This means the player *hears the reinforcements coming* — footstep sounds grow louder over several turns. On the C64, you hear the approaching threat through the SID before it appears on screen.

## Platform-Native Audio

The core game produces `SoundEvent` values. Each platform's renderer decides what to do with them. The core never imports audio libraries.

### Terminal / SSH

Text only. Sound events become:

1. **Message log entries** with directional hints (`"You hear footsteps to the north."`).
2. **Map glyphs:** `?` at approximate heard-but-unseen positions.
3. **Status bar indicator:** A `[!!]` or `[...]` noise level indicator showing how loud the player's recent actions were (helping the player understand their own noise footprint).

### C64 (SID Chip)

The SID (`$D400`–`$D418`) has 3 oscillators, each with waveform selection (triangle, sawtooth, pulse, noise), ADSR envelope, and a shared resonant filter. This is a real-time synthesizer that can produce rich procedural audio from a few register writes per frame.

#### Audio Design

| Sound event | SID voice | Technique |
|-------------|-----------|-----------|
| **Player footstep** | Voice 1 | Short noise burst, fast decay (AD: $09), low volume. Alternating pitch for left/right foot feel. |
| **Combat hit** | Voice 1 | Noise waveform, medium attack, fast decay. Higher volume than footsteps. |
| **Kill** | Voice 1 | Descending pitch sweep on pulse wave over ~10 frames. |
| **Monster footstep (heard)** | Voice 2 | Same as player footstep but volume attenuated by distance (sound grid value maps linearly to SID volume 0–15). |
| **Monster alert/roar** | Voice 2 | Low-frequency sawtooth, slow attack, medium sustain. Filter sweep from low to high cutoff. |
| **Ambient drone** | Voice 3 | Continuous triangle wave at very low volume. Filter cutoff modulated slowly by game state: low cutoff in corridors (muffled), higher in rooms (resonant). Pitch shifts with tension level (higher tension = slight detune = unease). |
| **Escalation rumble** | Voice 3 | Noise waveform, band-pass filtered at low frequency. Triggered at tension thresholds. Distinct from combat noise. |
| **Damage taken** | Voice 1 | Distorted pulse wave (narrow duty cycle), short burst. Immediately recognizable as "you got hit." |
| **Room transition** | Voice 3 | Filter sweep on the ambient drone. Different sweep direction for entering vs. leaving a room. |

#### Implementation

The C64 renderer maintains a small `SidState` struct (~20 bytes) tracking the current register values. Each frame (after game step), it:

1. Iterates the turn's sound events.
2. Maps each event to register writes using a lookup table.
3. Writes 5-10 register bytes to `$D400`–`$D418`.
4. For the ambient drone (voice 3), updates filter cutoff based on room size and tension level.

This costs fewer than 500 CPU cycles per frame on the 6510 — negligible compared to the map rendering cost.

#### Why SID Audio Matters for This Feature

The SID chip is the strongest hardware feature the C64 has. Using it for procedural, game-state-reactive audio — rather than a pre-composed background track — means the audio layer is *part of the game*, not a cosmetic addition. The player literally hears the dungeon respond to their actions: monster footsteps grow louder as they approach through corridors, the ambient drone shifts as tension rises, combat produces satisfying percussive feedback.

This also means the C64 version offers something the terminal version cannot. Rather than being a compromised downport, it becomes a platform with a unique sensory dimension.

### GBA (PSG)

The GBA has 4 sound channels (2 pulse, 1 wave, 1 noise) plus 2 DMA channels. The approach mirrors the C64:

- Channels 1-2: Action sounds (footsteps, combat) with volume attenuation by distance.
- Channel 3: Ambient wave channel, modulated by game state.
- Channel 4: Noise channel for escalation rumbles and alert sounds.

Register writes are similarly cheap. The GBA has more CPU headroom than the C64, so the audio system can be slightly more sophisticated (e.g., interpolated volume changes, longer envelopes).

## Visual Complement: Dynamic FOV Lighting (C64)

To pair with the acoustic system, the C64's per-character-cell color RAM (`$D800`) maps naturally onto the FOV system. Instead of the binary visible/not-visible rendering, use the C64's 16-color palette to create a gradient:

| FOV zone | C64 color | Effect |
|----------|-----------|--------|
| Player's tile | White (1) | Brightest point |
| Inner FOV (radius 1-3) | Light grey (15) | Well-lit area |
| Mid FOV (radius 4-6) | Grey (12) | Normal visibility |
| Outer FOV (radius 7-8) | Dark grey (11) | Edge of vision |
| Explored, not visible | Blue (6) | Remembered, dim |
| Unexplored | Black (0) | Total darkness |
| Sound source (`?`) | Yellow (7), flashing | Something heard but not seen |

**Torchlight flicker:** On raster interrupt, randomly shift 2-3 tiles at the FOV boundary between their zone color and one shade dimmer, at ~3Hz. This costs a few bytes of color RAM writes per frame and creates a convincing "flickering light" effect. The small 40x25 screen makes the darkness feel oppressive and the light feel precious.

**Hardware sprite for player:** Instead of a character-cell `@`, render the player as a hardware sprite that glides smoothly between tile positions over 4-6 frames. The rest of the map stays in text mode. This costs 2 sprite register writes (X, Y) per animation frame and adds a level of polish that distinguishes the C64 version.

## Files Touched

| File | Change |
|------|--------|
| New `sound.rs` | `SoundEvent`, `SoundKind`, `propagate_sound()`, sound grid management. |
| `game.rs` | Add `sound_grid: Vec<u8>`, `tension: Stat` to `GameState`. Add `update_sound_map()`, `update_tension()`. Call from `step()`. |
| `ai.rs` | Extend `is_aware()` to check sound grid. Pass sound grid to `run_monster_turns()`. |
| `data.rs` | Add `SoundConfig` and `EscalationConfig` structs. Add `sound_intensity` to `MonsterDef`. |
| `game.toml` | Add `[sound]` and `[escalation]` sections. Add `sound_intensity` to each `[[monsters]]` entry. |
| `entity.rs` | Add `sound_intensity: u8` field (with `#[serde(default)]` for save compat). |
| `types.rs` | No changes — sound intensity is `u8`, positions are existing `Pos`. |
| Platform renderers | Each platform maps `SoundEvent` to its output: message log text (terminal), SID registers (C64), PSG registers (GBA). |

## Relationship to Existing Roadmap

| Roadmap item | Relationship |
|--------------|-------------|
| Sound effects (Polish) | **Superseded.** This proposal replaces "add sound effects" with "sound as a core mechanic." The platform-native audio sections cover the rendering side. |
| Music (Polish) | **Complementary.** The ambient drone (SID voice 3 / GBA wave channel) serves the atmospheric role that music would. A separate background music system could coexist, but the ambient drone may be sufficient. |
| Wandering monsters (Phase 1) | **Complementary.** Wandering monsters create time pressure; escalation creates *reactive* pressure. They serve different purposes. Wandering monsters could also emit sound events, making them detectable before visible. |
| Creature mood (Phase 5) | **Synergistic.** Sound-triggered mood shifts ("heard ally die in next room") are more interesting than purely visual triggers. The sound system provides the awareness mechanism that mood needs. |
| Property bitfields (Phase 6) | **Synergistic.** Properties like `SILENT` (makes no footstep sound), `LOUD` (extra sound intensity), or `ECHOLOCATING` (uses sound map for awareness instead of visual FOV) become possible. |
| C64 / GBA ports (Tier 5) | **Motivating.** This proposal gives those ports a flagship feature that justifies the porting effort. |

## Implementation Phases

### Phase A: Sound Grid & Monster Awareness (Effort: S-M)

Add `sound.rs` with `propagate_sound()`. Add `sound_grid` to `GameState`. Player actions emit sound events. Monsters gain acoustic awareness. Message log reports heard sounds. No platform audio yet — text only.

**This phase is independently valuable.** Even without audio hardware, the sound system changes gameplay: combat alerts nearby monsters, quiet play is rewarded, and the message log gives the player information about unseen threats.

### Phase B: Encounter Escalation (Effort: S)

Add `tension` to `GameState`. Implement threshold effects. Sound events from escalation feed into the sound grid from Phase A. All text-based — the terminal version gets the full mechanical benefit.

### Phase C: C64 SID Audio (Effort: M)

Implement the SID driver that maps `SoundEvent` to register writes. Add the ambient drone. Add the dynamic color RAM lighting. Add the hardware sprite player animation. This phase is C64-specific and doesn't affect other platforms.

### Phase D: GBA PSG Audio (Effort: M)

Same approach as Phase C, adapted for the GBA's PSG channels. Developed independently of Phase C.

## Testing

- **Unit test:** `propagate_sound()` attenuates correctly over distance, walls block, corridors carry.
- **Unit test:** Monsters become aware via sound when outside visual range.
- **Unit test:** Monsters do *not* become aware when sound intensity is zero at their position.
- **Unit test:** Tension rises on kills, decays on waits, caps at 100.
- **Unit test:** Escalation thresholds trigger at correct tension levels.
- **Scenario test:** Combat in a corridor alerts a monster 10 tiles away. Same combat in a room does not (walls block).
- **Scenario test:** Player waiting 50 turns with tension at 80 reduces tension below 50.
- **Golden replays:** Regenerate after Phase A (monster awareness changes will alter outcomes).
- **Invariant tests:** Sound grid values are always 0–255. Tension is always 0–100.

## Open Questions

1. **Should sound propagate through doors?** The game doesn't have doors yet. When doors are added, they could attenuate sound (cost 3-5 per door tile) rather than blocking it entirely. This makes doors tactically interesting: close a door behind you to muffle your combat noise.

2. **Should the player be able to deliberately make noise?** A "shout" command that emits a high-intensity sound event could be a tactical tool: lure monsters into an ambush, or use noise to distract while sneaking past. This is a natural extension but not necessary for the initial implementation.

3. **Interaction with auto-explore and autorun.** Autorun currently stops for monsters and damage. Should it also stop when a loud sound is detected? Probably yes — the player would want to know that something heard them. The `AutorunStopReason` enum could gain a `SoundDetected` variant.

4. **MCP representation.** Should the MCP observation include the raw sound grid, a list of heard sound events, or just the `?` glyph positions? A list of `{ direction, kind, intensity }` objects is probably most useful for LLM agents — they can reason about "I hear footsteps to the north" better than a grid of numbers.
