# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.5.0] - 2026-03-18

### Added
- Update compact tier Coord type from i16 to i32 (ARM7-native) (#210)
- GBA: Update docs and memory with all resolved decisions (#209)
- GBA: Define compact tier launch feature scope (#205)
- GBA: Decide code sharing strategy between micro and compact tiers (#208)
- GBA: Decide allocation strategy — no_std fixed arrays vs allocator vs hybrid (#206)
- GBA: Update docs with session decisions and open questions (#207)
- Increase dungeon to 22 levels with rebalanced depth scaling (#192)
- Add item death when material property reaches zero (#175)
- Add property visibility in look mode and MCP observe (#171)
- Add dev text console for debug commands (#156)
- Emergent item interaction engine (property system step 2) (#155)
- Add more item types for deeper dungeon progression (#16)
- Add more item types for deeper dungeon progression (#16)
- Add input repeat/delay system to C64 frontend (#51)
- DCSS-style monster health indicators in look mode (#105)
- Update auto-pickup to grab all items, not just consumables (#154)
- Auto-pickup toggle for consumables (#86)
- Optimize C64 keyboard scanning with early exit and in-place edge detection (#147)
- Optimize micro tier FOV slope comparison with lookup table (#146)

#### Gameplay
- 26-slot Brogue-style inventory system with stacking consumables, equipment slots, pickup/drop/use/equip actions across all tiers (#78-#85)
- Stop autorun when stairs enter FOV, not just when stepped on — matches DCSS/Brogue behavior (#142)
- StairsFound autorun stop condition across all tiers and steppers
- Explored stairs coordinates in observe() response — spatial memory analogue for LLM players (#138)
- Unequip and drop-equipped actions for both tiers (#99)
- Kill and turn counters in standard tier status bar (#106)
- Kills, turns, and seed on standard tier game-over screen (#108)

#### Micro Tier (no_std)
- BFS pathfinding with fixed-size buffers (1.1 KB) — enables auto_explore and pathfind_to on micro tier (#137)
- auto_fight for micro tier — weakest-adjacent fight-to-death loop
- MicroBfsStepper for BFS-guided autorun with full stop conditions

#### C64 Port
- Save and load game to 1541 floppy disk via KERNAL SAVE/LOAD with inline asm (#134)
- SID music: intermittent playback, fade in/out, combat SFX (#61, #65, #66)
- Screen shake effect on combat via VIC-II raster IRQ (#63)
- Sprite-based loading spinner (#64)
- Two-phase inventory with action bar (#90)
- Help screen overlay with multi-page navigation (#104, #110)
- Message history overlay (#109)
- ATK/DEF stats in status bar (#107)
- Seed code display on end screens (#74)
- I/O banking overlay — frees 3.6 KB by placing computation under $D000 (#72)
- HIRAM expansion — game state at $E000, BASIC ROM unmapped (#71)
- Corpse rendering and look mode descriptions (#69)
- Per-item colors in inventory rendering (#88)

#### MCP / LLM Playtesting
- Route auto_explore, pathfind_to, auto_fight to both standard and micro tiers
- Error message lists all valid actions including item commands (#139)
- Playtest chat TUI with broadcast mode for injecting messages into running games

#### CI / Tooling
- .d64 disk image in nightly and release builds (#140)
- Multi-arch Docker image (arm64 + amd64) for C64 builds
- Cargo audit ignore for unfixable RUSTSEC-2023-0071 (rsa crate)

### Changed
- Add compact tier autorun with directional and BFS steppers (#218)
- Add compact tier BFS pathfinding with fixed-size buffers (#217)
- Implement compact tier item_store and spawn modules (#216)
- Implement compact tier combat, msglog, and AI modules (#215)
- Implement compact tier FOV module (tier_compact/fov.rs) (#214)
- Implement compact tier entity module (tier_compact/entity.rs) (#213)
- Implement compact tier map module (tier_compact/map.rs) (#212)
- Extract combat resolution algorithm to rules/combat.rs (#211)
- Fix compact tier and GBA spec documentation (#202)
- Token-templated message system for C64 format_event (#190)
- C64: Replace RangeInclusive iterators and inventory iterator adaptors with manual loops (#180)
- C64: Overlap SAVE_BUF and DiffState with union to save 809 bytes .noinit (#182)
- Centralize nibble access: move get/set_by_index to properties.rs (#160)
- Fix tautological fuzz test assertions and use .contains() in test code (#158)
- Step 3: Combat reads from property bags (#157)
- Remove explored % from player-facing UI (#116)
- Eliminate __udivhi3 calls and reduce scan_octant stack init overhead (#152)
- C64: Lazy clear_visible in FOV — track dirty bytes instead of full memset (#151)
- Eliminate remaining __ashlqi3 from keyboard scanner and pathfinding (#150)
- C64 profiler: multi-run averaging and benchmark mode (#149)
- C64 FOV & render micro-optimizations (#148)
- Comprehensive documentation sweep (#145)
- C64: switch Docker image to koalabuttz/rust-mos on GHCR with MachineOutliner PH/PL fix — ~5-6 KB code size savings
- C64: in-place game state init eliminates 7.8 KB static stack temporaries
- C64: eliminate __mulsi3 via subtraction-based u16 decimal formatting (#126)
- C64: compress spinner sprite data — derive 3 frames via vflip at runtime
- Inventory UX polish: equip bonus display, equipped indicators, item coloring (#89, #92, #93)

### Fixed
- Check is_material_dead for non-consumable source items in combine_items (#186)
- Fix combine undo not reverting non-consumable source mutation (#167)
- Fix combine stacking invariant: consume source before re-adding target (#166)
- Fix Cancel rule reading snapshot instead of working copy in interact() (#165)
- Fix combine-stack item loss and remove dead save migration code (#162)
- Fix review findings: combine-stack, EMPTY-bag fallback, dev console off-by-one, spawn guard (#159)
- Fix MCP error message omitting item actions from valid action list (#139)
- Fix gamepad analog_to_direction tests to use Direction enum (#141)
- Fix code review issues: CHANGELOG dupe, bestiary stats, status bar layout (#111)
- Fix dropping equipped items on C64 inventory (#102)
- Fix C64 freeze when equipping item via keyboard in inventory (#98)
- Fix C64 equip bonus not shown in messages (#91)
- Fix structural walls not rendering in terminal game (#75)
- Fix C64 .noinit RAM overflow from corpse and look mode additions (#70)
- Fix raster IRQ on C64 — spinner corrupts game state during map generation (#60)
- Fix joystick edge detection with auto-repeat to prevent phantom input (#55)
- Fix C64 I/O banking — CPU port value $0C unmaps I/O area causing frozen input (#54)
- Fix combat event detection cap missing events in rare multi-monster scenarios (#68)

## [0.4.0] - 2026-02-27

### Added
- Improve spinner animation with line-drawing characters (#62)
- Unmap KERNAL ROM on C64 for 8KB extra RAM headroom (#52)
- Add Tile::to_kind() and use MICRO_LOG_CAPACITY constant (#44)
- Unify render() across tiers via RenderSource trait (#41)
- Rewrite C64 crate as thin frontend over roguelike-core (#42)
- Add viewport scrolling to render_observation for maps larger than terminal (#40)
- Create tier_micro module — port C64 POC into core (#28)
- Move GameColor to rules/ with repr(u8) C64 color indices (#32)
- Add stairs and multi-level dungeons (#17)
- Replace XP/leveling with item-based progression in docs (#19)
- Regenerate golden replay files for item system changes (#13)
- Add item system to roguelike (#2)

### Fixed
- Replace division with right-shift for idle acceleration in micro tier (#50)
- Fix wandering spawn timing drift, extract depth scaling helper, add ambient sound constant (#49)
- Fix nightly C64 Docker mount to include roguelike-core (#46)
- Fix raw-usb feature build: update check_hid_stick call sites for Direction return type (#45)
- Extract run_error_dialog helper to deduplicate factory failure menus (#39)
- Clamp msg_lines in render_observation to prevent underflow (#38)
- Wandering spawn table lost on save/load (#18)
- Fix equipment stats not applied during monster attacks (#12)

### Changed
- Refactor TUI game loop to use dyn GameStep (#37)
- Refactor MCP server to use dyn GameStep for tier-agnostic play (#36)
- Refactor FrameSink to accept dyn GameStep instead of GameState (#35)
- Prepare roguelike-core for capability tier hierarchy (#20)
- Define GameStep cross-tier trait (#31)
- Gate standard-tier code behind std feature (#30)
- Remove MicroCommand, accept GameCommand in tier_micro (#34)
- Create tier_micro module — port C64 POC into core (#28)
- Create tier_compact stubs (#29)
- Extract rules/seed_code.rs — no_std seed encode/decode (#27)
- Extract rules/message.rs — GameEvent structured message enum (#25)
- Add Direction enum to GameCommand (#26)
- Extract rules/monster_table.rs — MonsterKind enum and stat lookups (#24)
- Extract rules/damage.rs — pure damage formula (#22)
- Extract rules/items.rs — pure item definitions and lookups (#23)
- Extract rules/balance.rs — shared balance constants (#21)
- Add comprehensive tests for item system (#11)
- Update spectate frame output for items (#10)
- Update MCP server observations and rules for items (#9)
- Update TUI rendering for items and equipment display (#8)
- Update look mode to show items on tiles (#7)
- Update combat to use effective stats from equipment (#6)
- Integrate items into GameState (pickup, equipment, effective stats) (#5)
- Add item spawning to spawn.rs (#4)
- Create item.rs module with ItemKind, Item, Equipment structs and pure functions (#3)
