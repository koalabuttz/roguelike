# Acoustic Propagation & The Living Dungeon

> **Status:** Proposed. A new core system that adds sound as a parallel sense to vision, creating a dungeon that players *hear* as well as see.

## Summary

Traditional roguelikes are silent worlds perceived through a single sense: sight. This proposal introduces **acoustic propagation** as a first-class game mechanic — a sound map that runs alongside the existing FOV system, giving both the player and monsters the ability to hear through walls. Loud actions (combat, running) attract distant monsters. Quiet play (walking, waiting) keeps you hidden. The player receives audio cues about things they can't yet see: footsteps in the next corridor, a roar from deep in the dungeon.

On all Rust-based platforms, these sound events are rendered as **procedural, game-state-reactive audio** through a cycle-accurate emulation of the Commodore 64's MOS 6581 SID chip, powered by [resid-rs](https://github.com/binaryfields/resid-rs). On the C64 itself, the same audio design drives real SID hardware. On text-only platforms (SSH, MCP), sound events manifest as message log text and approximate-position glyphs (`?`). The core game logic produces abstract sound events; platform layers decide how to present them.

A companion system, **encounter escalation**, creates a global tension mechanic that uses the sound system to communicate the dungeon's rising hostility. Together, these systems transform the dungeon from a static map of rooms into a reactive, living environment that rewards careful play and punishes recklessness.

## Motivation

The [gameplay implementation plan](gameplay-implementation-plan.md) identified that the current game has no pacing mechanism beyond HP regen. The [roadmap](../roadmap.md) lists sound effects and music as polish items, treating audio as decoration. This proposal argues that **sound should be a game mechanic, not a cosmetic layer** — and that designing it this way creates a feature that is:

1. **Mechanically novel.** No traditional roguelike has a proper dual FOV/sound system. Brogue has stealth and DCSS has noise, but neither gives the *player* acoustic information as a parallel sense. The player hearing monsters through walls before seeing them is a new information channel.

2. **Platform-unifying.** The SID chip's procedural audio can be emulated on every Rust-based platform via [resid-rs](https://github.com/binaryfields/resid-rs), a cycle-accurate MOS 6581 emulator. The same register writes that drive the C64's real SID hardware also drive an emulated SID on terminal, web, and future frontends. The audio design is written once; the C64 gets authentic analog output through silicon, and everyone else gets authentic digital output through emulation. The C64 version retains a unique edge — zero-latency analog path, true hardware filters — but the sensory dimension is no longer exclusive to it.

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

#### Terminal (SID emulation + text)

- **SID audio:** Sound events are rendered through the emulated MOS 6581 SID chip (see [SID Audio Engine](#sid-audio-engine) below). The player hears the same procedural audio as the C64 version — footsteps, combat hits, the ambient drone — through their speakers or headphones. Audio output uses `cpal` for cross-platform PCM playback. Audio can be disabled in settings for silent play.
- **Message log:** `"You hear footsteps to the east."`, `"A distant roar echoes from below."`, `"Something moves in the darkness."` Directional hints are based on the sound source's relative position to the player. These messages appear regardless of audio settings — they are part of the game mechanic, not decoration.
- **Map glyphs:** A `?` glyph appears at the approximate position of a heard-but-unseen sound source. The `?` is placed at the nearest tile along the propagation path that the player has explored, not at the monster's exact position — preserving fog-of-war integrity.
- **Look mode:** Examining a `?` tile shows `"Sound: footsteps (east corridor)"`.

#### SSH / MCP (text only)

- **SSH:** Same message log entries and map glyphs as terminal, but no audio output (remote terminal has no audio channel).
- **MCP:** Sound events included in JSON observations as a list of `{ direction, kind, intensity }` objects for LLM reasoning. Map glyphs include `?` positions.

#### C64 (native SID hardware)

See [Platform-Native Audio: C64](#c64-native-sid-hardware) below.

#### GBA (PSG channels)

See [Platform-Native Audio: GBA](#gba-psg) below.

#### Web / WASM (SID emulation + Web Audio)

See [Platform-Native Audio: Web](#web--wasm) below.

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
| 25 | **Distant rumble.** Message log only. | SID: low-frequency noise burst, filtered. Text: `"The dungeon groans..."` |
| 50 | **Alert pulse.** All dormant monsters within 15 tiles of the player become aware (as if they heard a loud sound). | SID: rising filter sweep. Text: `"Something stirs in the darkness."` |
| 75 | **Reinforcements.** Spawn 1-2 monsters from an unexplored room, using the existing spawn system. They start moving toward the sound of combat. | SID: heavy drum-like pulse. Text: `"You hear heavy footsteps approaching!"` |
| 100 | **Dungeon fury.** All living monsters on the map become aware of the player. Tension caps at 100 and decays normally from here. | SID: dissonant chord, all 3 oscillators. Text: `"The dungeon erupts with fury! Everything knows you're here."` |

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

This means the player *hears the reinforcements coming* — footstep sounds grow louder over several turns. On any platform with SID audio (terminal via resid-rs, C64 via real hardware, web via WASM), you hear the approaching threat before it appears on screen.

## SID Audio Engine

The core game produces `SoundEvent` values. Each platform's renderer decides what to do with them. The core never imports audio libraries.

The audio rendering layer is built on the **MOS 6581 SID chip** — either real silicon (C64) or cycle-accurate emulation via [resid-rs](https://github.com/binaryfields/resid-rs) (all other Rust platforms). The SID has 3 oscillators, each with waveform selection (triangle, sawtooth, pulse, noise), ADSR envelope, and a shared resonant multimode filter. This is a real-time synthesizer that produces rich procedural audio from a few register writes per turn.

Using the SID as the universal sound engine — rather than sample-based playback (Rodio) or platform-specific synthesis — provides:

1. **Zero audio assets.** No `.wav` or `.ogg` files to ship. All sound is procedurally generated from register writes. The entire sound design is ~200 lines of register mappings.
2. **Consistent aesthetic.** Every platform sounds like a C64. The SID's characteristic filter, waveforms, and ADSR envelopes give the game a distinctive retro voice that reinforces its identity.
3. **Reactive audio by construction.** Volume, filter cutoff, pitch, and waveform are parameters you modulate in real time based on game state. This is natural with a synthesizer — you're turning knobs per turn.
4. **WASM-compatible.** resid-rs compiles to `no_std` + `alloc`, so it runs in the browser. Web Audio API plays the PCM output.
5. **Tiny dependency footprint.** resid-rs depends only on `bit_field` and optionally `libm`. Compare to Rodio which pulls in `cpal`, `symphonia`, `dasp`, etc.
6. **The C64 port benefits directly.** The audio design work (which voices, waveforms, ADSR for each `SoundKind`) is done once. The C64 assembly uses the same register values — the emulated version is the prototype for the real hardware.

### resid-rs Library

[resid-rs](https://github.com/binaryfields/resid-rs) (crate: `resid`, v1.1.1) is a Rust port of the reSID emulation engine. Key properties:

| Property | Value |
|----------|-------|
| Chip models | `ChipModel::Mos6581` (1982, grittier filters) and `ChipModel::Mos8580` (1986, cleaner) |
| License | GPL-3.0+ (compatible with our GPL-3.0-or-later) |
| `no_std` support | Yes, since v1.0. Features: `std` → `alloc` → bare |
| Dependencies | `bit_field` 0.10, `libm` 0.2 (optional) |
| Core API | `Sid::new(model)`, `.write(reg, val)`, `.sample(delta, buf, interleave)` → `(count, remaining)` |
| Output format | PCM `i16` samples at configurable sample rate |
| Clock frequency | Configurable — 985,248 Hz (PAL) or 1,022,727 Hz (NTSC) |

The API is register-level: you write bytes to register offsets `0x00`–`0x18` — the same offsets as the SID's memory-mapped I/O at `$D400`–`$D418` on real hardware. Then you call `sample()` to clock the emulated chip forward and fill a buffer with PCM audio. This register-level compatibility is the key insight: the audio design is expressed as register writes, and those writes are valid for both real silicon and emulation.

**Chip model choice:** `Mos6581` is the correct choice for this project. The 6581 (original 1982 chip) has a grittier sound with characteristic filter distortion and combined-waveform artifacts that give it more character. The 8580 (1986 revision) has a cleaner filter but is less distinctive. For a dungeon crawler going for oppressive atmosphere, the 6581's rawer, more menacing sound is ideal.

### Architecture: `roguelike-audio` Crate

A new workspace crate, `crates/audio/` (`roguelike-audio`), contains the SID audio engine. It sits alongside `saves` as shared infrastructure used by some frontends but not all:

```
core (zero platform deps)
 ├── saves       (SaveBackend trait — connected platforms)
 ├── audio       (SidAudioEngine — platforms with audio output)  ← NEW
 ├── tui         (shared terminal rendering + game loop)
 │    ├── terminal  (imports audio + cpal for real-time output)
 │    └── ssh       (no audio — remote terminal)
 ├── mcp         (no audio — JSON observations)
 └── (future)
      ├── web    (imports audio — resid-rs compiles to WASM)
      ├── gba    (own PSG driver — no resid-rs)
      └── c64    (real SID hardware — no resid-rs)
```

**Dependencies:**

```toml
# crates/audio/Cargo.toml
[package]
name = "roguelike-audio"
version = "0.3.0"
edition = "2024"
license = "GPL-3.0-or-later"

[dependencies]
roguelike-core = { path = "../core" }
resid = { version = "1.1", default-features = false, features = ["alloc"] }
```

Using `default-features = false, features = ["alloc"]` keeps resid-rs lean and WASM-compatible. The `std` feature is not needed — `alloc` is sufficient for `Sid::new()` and `sample()`.

**Why a separate crate?** The same reasoning as `saves`: not every platform needs it. SSH and MCP don't produce audio. The C64 doesn't need emulation (it has the real chip). The GBA has its own PSG, not a SID. Keeping it out of `core` preserves the zero-platform-deps guarantee. Keeping it out of `tui` allows the future web crate to use it without depending on crossterm.

### SID Voice Allocation

The SID's 3 voices are allocated to distinct roles, ensuring sounds never collide:

| Voice | Role | Priority |
|-------|------|----------|
| Voice 1 | **Player actions** — footsteps, combat, damage taken, kills | Highest. Always reflects the player's immediate action. New events cut the current envelope. |
| Voice 2 | **Heard monsters** — monster footsteps, alert/roar | Medium. Volume attenuated by distance (sound grid value maps linearly to SID volume 0–15). |
| Voice 3 | **Atmosphere** — ambient drone, escalation rumbles, room transitions | Lowest. Continuous; never fully silent. Modulated by game state, not individual events. |

### Audio Design

| Sound event | SID voice | Technique |
|-------------|-----------|-----------|
| **Player footstep** | Voice 1 | Short noise burst, fast decay (AD: `$09`), low volume. Alternating pitch for left/right foot feel. |
| **Combat hit** | Voice 1 | Noise waveform, medium attack, fast decay. Higher volume than footsteps. |
| **Kill** | Voice 1 | Descending pitch sweep on pulse wave over ~10 frames. |
| **Monster footstep (heard)** | Voice 2 | Same as player footstep but volume attenuated by distance (sound grid value maps linearly to SID volume 0–15). |
| **Monster alert/roar** | Voice 2 | Low-frequency sawtooth, slow attack, medium sustain. Filter sweep from low to high cutoff. |
| **Ambient drone** | Voice 3 | Continuous triangle wave at very low volume. Filter cutoff modulated slowly by game state: low cutoff in corridors (muffled), higher in rooms (resonant). Pitch shifts with tension level (higher tension = slight detune = unease). |
| **Escalation rumble** | Voice 3 | Noise waveform, band-pass filtered at low frequency. Triggered at tension thresholds. Distinct from combat noise. |
| **Damage taken** | Voice 1 | Distorted pulse wave (narrow duty cycle), short burst. Immediately recognizable as "you got hit." |
| **Room transition** | Voice 3 | Filter sweep on the ambient drone. Different sweep direction for entering vs. leaving a room. |

### Implementation: `SidAudioEngine`

The `SidAudioEngine` struct maps `SoundEvent` values to SID register writes and produces PCM audio via the emulated chip:

```rust
// crates/audio/src/lib.rs

use resid::{Sid, ChipModel, SamplingMethod};
use roguelike_core::sound::{SoundEvent, SoundKind};

/// Cycle-accurate SID chip emulator that maps game sound events to
/// procedural audio. Wraps resid-rs and manages voice allocation,
/// envelope tracking, and ambient drone state.
pub struct SidAudioEngine {
    sid: Sid,
    /// Per-voice state: which sound owns the voice, envelope phase, etc.
    voices: [VoiceState; 3],
    /// Ambient drone state (voice 3 — continuous, modulated by game state).
    ambient: AmbientState,
    /// Master volume (0–15). Allows settings-driven volume control.
    master_volume: u8,
}

impl SidAudioEngine {
    pub fn new() -> Self {
        let mut sid = Sid::new(ChipModel::Mos6581);
        sid.set_sampling_parameters(
            SamplingMethod::Fast,
            985_248,    // PAL C64 clock — canonical SID tuning
            44_100,     // Standard PCM sample rate
        );
        sid.enable_filter(true);
        sid.enable_external_filter(true);
        Self {
            sid,
            voices: [VoiceState::Idle; 3],
            ambient: AmbientState::new(),
            master_volume: 15,
        }
    }

    /// Process a sound event from the current game turn.
    /// `intensity_at_player` is the sound grid value at the player's
    /// position (0 = inaudible, 255 = maximum).
    pub fn process_event(&mut self, event: &SoundEvent, intensity_at_player: u8) {
        let volume = (intensity_at_player as u16 * 15 / 255) as u8;
        match event.kind {
            SoundKind::Combat => {
                // Voice 1: noise waveform, medium attack, fast decay
                self.sid.write(0x05, 0x09); // AD: attack=0, decay=9
                self.sid.write(0x06, 0x00); // SR: sustain=0, release=0
                self.sid.write(0x04, 0x81); // Noise waveform + gate on
                self.voices[0] = VoiceState::Active(SoundKind::Combat);
            }
            SoundKind::Footstep => { /* Voice 1: short noise burst */ }
            SoundKind::MonsterStep => {
                // Voice 2: same as footstep, volume attenuated by distance
                self.sid.write(0x0C, 0x09);
                self.sid.write(0x0D, 0x00);
                self.sid.write(0x0B, 0x81);
                // Volume attenuation applied via voice 2 amplitude
                self.voices[1] = VoiceState::Active(SoundKind::MonsterStep);
            }
            SoundKind::Alert => { /* Voice 2: low sawtooth, filter sweep */ }
            SoundKind::Ambient => { /* Handled by update_ambient() */ }
            SoundKind::Run => { /* Voice 1: louder footstep variant */ }
        }
        // Update mode/volume register ($D418)
        let vol = self.master_volume.min(volume);
        self.sid.write(0x18, vol | (self.ambient.filter_mode << 4));
    }

    /// Update the ambient drone (voice 3) based on room geometry and tension.
    /// Called once per turn after all sound events are processed.
    pub fn update_ambient(&mut self, tension: u8, in_corridor: bool) {
        // Filter cutoff: low in corridors (muffled), higher in rooms (resonant)
        let base_cutoff: u16 = if in_corridor { 200 } else { 600 };
        // Pitch shifts with tension (higher tension = slight detune = unease)
        let detune = tension as u16 / 10;
        let freq = self.ambient.base_freq + detune;
        // Voice 3 frequency registers
        self.sid.write(0x0E, (freq & 0xFF) as u8);
        self.sid.write(0x0F, (freq >> 8) as u8);
        // Filter cutoff (registers $15-$16)
        self.sid.write(0x15, (base_cutoff & 0x07) as u8);
        self.sid.write(0x16, (base_cutoff >> 3) as u8);
    }

    /// Clock the SID emulator forward and fill `buffer` with PCM i16 samples.
    /// Called from the audio output thread's callback.
    pub fn render(&mut self, buffer: &mut [i16]) -> usize {
        let cycles_per_frame = 985_248 / 50; // PAL: 50 fps
        let (samples, _) = self.sid.sample(cycles_per_frame as u32, buffer, 1);
        samples
    }
}
```

Register offsets used here (`0x00`–`0x18`) correspond directly to the SID's memory-mapped registers at `$D400`–`$D418`. The voice layout is:

- `0x00`–`0x06`: Voice 1 (freq lo/hi, pulse width lo/hi, control, AD, SR)
- `0x07`–`0x0D`: Voice 2
- `0x0E`–`0x14`: Voice 3
- `0x15`–`0x18`: Filter cutoff, resonance/routing, mode/volume

The same register values written here can be written verbatim in 6502 assembly on the C64 — `STA $D405` instead of `sid.write(0x05, val)`. The audio design is portable at the register level.

### Audio Output Integration

Each platform with audio output wires `SidAudioEngine` to its audio subsystem:

#### Terminal (cpal)

The terminal crate uses [cpal](https://crates.io/crates/cpal) for cross-platform audio I/O:

```rust
// crates/terminal/src/audio.rs

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use roguelike_audio::SidAudioEngine;

pub fn start_audio(engine: Arc<Mutex<SidAudioEngine>>) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(44_100),
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [i16], _| {
            let mut engine = engine.lock().unwrap();
            engine.render(data);
        },
        |err| eprintln!("audio error: {err}"),
        None,
    ).ok()?;
    stream.play().ok()?;
    Some(stream)
}
```

The game loop feeds sound events to the engine after each `step()`. The audio thread runs independently, pulling PCM samples from the engine's `render()` method. The SID chip's ADSR envelopes handle timing naturally — a sound triggered on turn N decays smoothly over real-time milliseconds without the game loop needing to track it.

```toml
# crates/terminal/Cargo.toml additions
[dependencies]
roguelike-audio = { path = "../audio" }
cpal = { workspace = true }
```

Audio is optional — if no audio device is available (headless server, CI), `start_audio()` returns `None` and the game runs silently. The settings menu includes an audio toggle.

#### Web / WASM

resid-rs compiles to WASM (`no_std` + `alloc`). The web crate imports `roguelike-audio` and bridges PCM output to the Web Audio API via an `AudioWorkletProcessor`:

```javascript
// Web Audio callback (JS side, called from WASM)
class SidProcessor extends AudioWorkletProcessor {
    process(inputs, outputs) {
        const buffer = outputs[0][0]; // mono, Float32
        // Call into WASM: engine.render() fills an i16 buffer,
        // convert i16 → f32 (-1.0..1.0) for Web Audio
        wasmModule.render_audio(buffer);
        return true;
    }
}
```

No additional audio dependencies needed on the Rust side — just `roguelike-audio` (which brings `resid`) and the JS glue.

#### SSH / MCP

No audio output. Sound events produce text only:

1. **Message log entries** with directional hints (`"You hear footsteps to the north."`).
2. **Map glyphs:** `?` at approximate heard-but-unseen positions.
3. **Status bar indicator:** A `[!!]` or `[...]` noise level indicator showing how loud the player's recent actions were (helping the player understand their own noise footprint).

### Platform-Native Audio

Platforms with dedicated audio hardware bypass `roguelike-audio` and map `SoundEvent` values directly to hardware registers.

#### C64 (Native SID Hardware)

On the C64, the SID lives at `$D400`–`$D418` as memory-mapped I/O. The audio design table above translates directly to `STA` instructions:

```asm
; Combat hit — Voice 1: noise waveform, medium attack, fast decay
play_combat:
    lda #$09
    sta $D405           ; AD: attack=0, decay=9
    lda #$00
    sta $D406           ; SR: sustain=0, release=0
    lda #$81
    sta $D404           ; Noise waveform + gate on
    rts
```

The C64 renderer maintains a small `SidState` struct (~20 bytes) tracking voice ownership. Each frame (after game step), it:

1. Iterates the turn's sound events.
2. Maps each event to register writes using a lookup table — the same table as `SidAudioEngine`, expressed as 6502 data.
3. Writes 5-10 register bytes to `$D400`–`$D418`.
4. For the ambient drone (voice 3), updates filter cutoff based on room size and tension level.

This costs fewer than 500 CPU cycles per frame on the 6510 — negligible compared to the map rendering cost. No emulation overhead; the SID is clocked by the system bus.

#### Why SID Audio Matters for This Feature

The SID chip is the strongest hardware feature the C64 has. Using it for procedural, game-state-reactive audio — rather than a pre-composed background track — means the audio layer is *part of the game*, not a cosmetic addition. The player literally hears the dungeon respond to their actions: monster footsteps grow louder as they approach through corridors, the ambient drone shifts as tension rises, combat produces satisfying percussive feedback.

With resid-rs, every Rust-based platform shares this sensory dimension. The C64 retains a unique edge: zero-latency analog output through real silicon, authentic analog filter characteristics (the 6581's filter has component-level variation that no emulator can perfectly reproduce), and the cultural cachet of hearing a real SID chip. But the *design* of the audio — which waveforms, which ADSR, which filter sweeps — is developed and tested once in the `roguelike-audio` crate, then ported to 6502 assembly for the C64. No wasted work; the emulated version is the prototype for the real hardware.

#### GBA (PSG)

The GBA has 4 sound channels (2 pulse, 1 wave, 1 noise) plus 2 DMA channels. The approach mirrors the SID voice allocation but maps to different hardware:

- Channels 1-2: Action sounds (footsteps, combat) with volume attenuation by distance.
- Channel 3: Ambient wave channel, modulated by game state.
- Channel 4: Noise channel for escalation rumbles and alert sounds.

Register writes are similarly cheap. The GBA has more CPU headroom than the C64, so the audio system can be slightly more sophisticated (e.g., interpolated volume changes, longer envelopes). The GBA does not use resid-rs — its PSG is a different chip with different capabilities.

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

### Phase A–B (core mechanics, all platforms)

| File | Change |
|------|--------|
| New `crates/core/src/sound.rs` | `SoundEvent`, `SoundKind`, `propagate_sound()`, sound grid management. |
| `crates/core/src/game.rs` | Add `sound_grid: Vec<u8>`, `tension: Stat` to `GameState`. Add `update_sound_map()`, `update_tension()`. Call from `step()`. |
| `crates/core/src/ai.rs` | Extend `is_aware()` to check sound grid. Pass sound grid to `run_monster_turns()`. |
| `crates/core/src/data.rs` | Add `SoundConfig` and `EscalationConfig` structs. Add `sound_intensity` to `MonsterDef`. |
| `crates/core/data/game.toml` | Add `[sound]` and `[escalation]` sections. Add `sound_intensity` to each `[[monsters]]` entry. |
| `crates/core/src/entity.rs` | Add `sound_intensity: u8` field (with `#[serde(default)]` for save compat). |
| `crates/core/src/types.rs` | No changes — sound intensity is `u8`, positions are existing `Pos`. |
| Platform renderers | Each platform maps `SoundEvent` to text output: message log entries (terminal, SSH), JSON metadata (MCP). |

### Phase C (SID audio engine)

| File | Change |
|------|--------|
| New `crates/audio/Cargo.toml` | New crate. Depends on `roguelike-core`, `resid` (v1.1, `default-features = false, features = ["alloc"]`). |
| New `crates/audio/src/lib.rs` | `SidAudioEngine` struct: `SoundEvent` → SID register writes → PCM `i16` via resid-rs. Voice state tracking, ambient drone management. |
| `Cargo.toml` (workspace root) | Add `crates/audio` to `[workspace.members]`. Add `resid` and `cpal` to `[workspace.dependencies]`. |
| `crates/terminal/Cargo.toml` | Add `roguelike-audio` and `cpal` dependencies. |
| New `crates/terminal/src/audio.rs` | `start_audio()`: cpal stream setup, `Arc<Mutex<SidAudioEngine>>` shared with game loop. |
| `crates/terminal/src/main.rs` | Initialize `SidAudioEngine`, start audio stream, wire sound events from game loop. |
| `crates/tui/src/game_loop.rs` | Pass `SoundEvent` list from `StepResult` to an optional audio callback. |
| `crates/core/src/settings.rs` | Add `audio_enabled: bool` setting (default: `true`). |

## Relationship to Existing Roadmap

| Roadmap item | Relationship |
|--------------|-------------|
| Sound effects (Tier 4, Rodio + SoundEvent) | **Superseded.** resid-rs replaces Rodio as the audio engine. Instead of sample-based playback, all audio is procedurally synthesized through an emulated SID chip. The `SoundEvent` abstraction remains — only the rendering backend changes. |
| Music (Tier 5) | **Subsumed by ambient drone.** Voice 3's continuous triangle wave + filter modulation serves the atmospheric role that a separate music system would. A dedicated SID music player (e.g., playing `.sid` tune files) could coexist if desired, but the reactive ambient drone may be sufficient and is more interesting than a looping track. |
| Wandering monsters (Tier 2) | **Complementary.** Wandering monsters create time pressure; escalation creates *reactive* pressure. They serve different purposes. Wandering monsters also emit sound events, making them audible (both as game-mechanic text cues and as SID audio) before visible. |
| Creature mood (Phase 5) | **Synergistic.** Sound-triggered mood shifts ("heard ally die in next room") are more interesting than purely visual triggers. The sound system provides the awareness mechanism that mood needs. |
| Property bitfields (Phase 6) | **Synergistic.** Properties like `SILENT` (makes no footstep sound), `LOUD` (extra sound intensity), or `ECHOLOCATING` (uses sound map for awareness instead of visual FOV) become possible. |
| C64 port (Tier 5) | **Strongly synergistic.** The SID audio design is developed and tested in the `roguelike-audio` crate, then ported to 6502 assembly with identical register values. The C64 is no longer the only platform with audio — but it remains the only one with real analog SID output. |
| GBA port (Tier 5) | **Complementary.** The GBA uses its own PSG hardware, not SID emulation. The audio *design pattern* (voice allocation, game-state modulation) transfers, but the register values differ. |
| Web / WASM (Tier 3) | **Enabling.** resid-rs compiles to WASM, giving the web frontend SID audio via Web Audio API with no additional dependencies. |

## Implementation Phases

### Phase A: Sound Grid & Monster Awareness (Effort: S-M)

Add `sound.rs` with `propagate_sound()`. Add `sound_grid` to `GameState`. Player actions emit sound events. Monsters gain acoustic awareness. Message log reports heard sounds. No platform audio yet — text only.

**This phase is independently valuable.** Even without audio hardware, the sound system changes gameplay: combat alerts nearby monsters, quiet play is rewarded, and the message log gives the player information about unseen threats.

### Phase B: Encounter Escalation (Effort: S)

Add `tension` to `GameState`. Implement threshold effects. Sound events from escalation feed into the sound grid from Phase A. All text-based — the terminal version gets the full mechanical benefit.

### Phase C: SID Audio Engine (Effort: M)

Create the `roguelike-audio` crate with `SidAudioEngine`. Implement the `SoundEvent` → SID register mapping (the audio design table). Integrate resid-rs for cycle-accurate MOS 6581 emulation. Add `cpal` audio output to the terminal crate. Add `audio_enabled` setting.

This phase gives the terminal version full procedural SID audio — the same audio design that will later run on the C64's real hardware. The emulated version serves as a development and testing environment for the audio design: you can iterate on waveforms, ADSR values, and filter settings on your development machine and hear exactly what the C64 will sound like.

**Dependency additions:**
- `resid = { version = "1.1", default-features = false, features = ["alloc"] }` — SID emulation
- `cpal = "0.15"` — cross-platform audio I/O (terminal crate only)

### Phase D: C64 Native SID Audio (Effort: M)

Port the SID register mapping from `SidAudioEngine` to 6502 assembly. The register values are identical — this is a transliteration from `sid.write(0x05, 0x09)` to `LDA #$09 / STA $D405`. Add the dynamic color RAM lighting. Add the hardware sprite player animation. This phase is C64-specific.

### Phase E: GBA PSG Audio (Effort: M)

Design a separate audio mapping for the GBA's PSG channels. The voice allocation pattern mirrors the SID (action/monster/atmosphere), but the register values and capabilities differ. Developed independently of Phase D.

### Phase F: Web Audio (Effort: S)

Import `roguelike-audio` into the web/WASM crate. Bridge PCM output to Web Audio API via `AudioWorkletProcessor`. resid-rs compiles to WASM natively (`no_std` + `alloc`). No additional Rust dependencies beyond `roguelike-audio`.

## Testing

### Phase A–B (sound grid, escalation)

- **Unit test:** `propagate_sound()` attenuates correctly over distance, walls block, corridors carry.
- **Unit test:** Monsters become aware via sound when outside visual range.
- **Unit test:** Monsters do *not* become aware when sound intensity is zero at their position.
- **Unit test:** Tension rises on kills, decays on waits, caps at 100.
- **Unit test:** Escalation thresholds trigger at correct tension levels.
- **Scenario test:** Combat in a corridor alerts a monster 10 tiles away. Same combat in a room does not (walls block).
- **Scenario test:** Player waiting 50 turns with tension at 80 reduces tension below 50.
- **Golden replays:** Regenerate after Phase A (monster awareness changes will alter outcomes).
- **Invariant tests:** Sound grid values are always 0–255. Tension is always 0–100.

### Phase C (SID audio engine)

- **Unit test:** `SidAudioEngine::process_event()` writes expected register values for each `SoundKind`. Verify specific registers against the audio design table (e.g., `Combat` → noise waveform at voice 1, AD=`$09`).
- **Unit test:** Volume attenuation maps distance correctly — `intensity_at_player = 255` → SID volume 15, `intensity_at_player = 0` → SID volume 0.
- **Unit test:** `update_ambient()` adjusts filter cutoff based on corridor/room state.
- **Unit test:** `render()` produces non-zero PCM samples after a sound event (SID is actually generating audio, not silent).
- **Unit test:** `render()` produces near-zero samples after sufficient time with no events (envelopes decay to silence).
- **Integration test:** Full round-trip — create `SidAudioEngine`, process a combat event, render 4410 samples (100ms at 44.1kHz), verify peak amplitude is above noise floor.
- **No golden audio tests.** PCM output depends on resid-rs internals and may change between versions. Test behavior (non-silence, decay, volume mapping), not exact sample values.

## Open Questions

1. **Should sound propagate through doors?** The game doesn't have doors yet. When doors are added, they could attenuate sound (cost 3-5 per door tile) rather than blocking it entirely. This makes doors tactically interesting: close a door behind you to muffle your combat noise.

2. **Should the player be able to deliberately make noise?** A "shout" command that emits a high-intensity sound event could be a tactical tool: lure monsters into an ambush, or use noise to distract while sneaking past. This is a natural extension but not necessary for the initial implementation.

3. **Interaction with auto-explore and autorun.** Autorun currently stops for monsters and damage. Should it also stop when a loud sound is detected? Probably yes — the player would want to know that something heard them. The `AutorunStopReason` enum could gain a `SoundDetected` variant.

4. **MCP representation.** Should the MCP observation include the raw sound grid, a list of heard sound events, or just the `?` glyph positions? A list of `{ direction, kind, intensity }` objects is probably most useful for LLM agents — they can reason about "I hear footsteps to the north" better than a grid of numbers.

5. **SID chip model: 6581 vs 8580.** This proposal specifies the MOS 6581 for its grittier, more characteristic sound. Should `ChipModel` be a user setting? The 8580 has a cleaner filter that some players might prefer. Since resid-rs supports both models, this is trivial to expose — but it adds a settings option and makes the audio output non-deterministic across players. Recommendation: hardcode 6581, revisit if players request it.

6. **Audio latency and turn-based timing.** The SID's ADSR envelopes operate in real time (milliseconds), but the game is turn-based (arbitrary time between turns). A sound triggered on turn N plays its attack-decay-sustain in real time, then sustains or decays to silence. If the player takes another action before the previous sound finishes, should the new sound cut the old envelope (retriggering the voice) or wait? Current design: retrigger immediately — each turn's sounds override the previous voice state. This keeps audio responsive but may clip long envelopes. The ambient drone (voice 3) is exempt — it sustains continuously.

7. **cpal as a dependency.** cpal is the standard Rust audio output library, but it has a non-trivial dependency tree (ALSA bindings on Linux, CoreAudio on macOS, WASAPI on Windows). Should audio output be behind a feature flag (`audio` feature on the terminal crate) to keep the default build dependency-free? This mirrors the `gamepad` feature pattern already used for gilrs. Recommendation: yes, feature-gate it — `cargo run` works without audio, `cargo run --features audio` enables SID output.

8. **SSH audio forwarding.** SSH supports audio forwarding (X11 forwarding with PulseAudio, or more exotic setups). Should the SSH frontend optionally support SID audio if audio forwarding is detected? This is low priority and complex, but theoretically the SSH crate could import `roguelike-audio` + `cpal` behind a feature flag. Recommendation: defer — text cues are sufficient for SSH.
