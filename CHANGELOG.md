# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added
- Rewrite C64 crate as thin frontend over roguelike-core (#42)
- Add viewport scrolling to render_observation for maps larger than terminal (#40)
- Create tier_micro module — port C64 POC into core (#28)
- Move GameColor to rules/ with repr(u8) C64 color indices (#32)
- Add stairs and multi-level dungeons (#17)
- Replace XP/leveling with item-based progression in docs (#19)
- Regenerate golden replay files for item system changes (#13)
- Add item system to roguelike (#2)

### Fixed
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
