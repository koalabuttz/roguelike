# Documentation

Project documentation for the roguelike dungeon crawler.

## Document Index

### Core

| Document | Purpose | Status |
|----------|---------|--------|
| [roadmap.md](roadmap.md) | Prioritized feature roadmap with dependencies and critical path | Current |
| [testing-strategy.md](testing-strategy.md) | Project-wide testing: core tests, `no_std` CI verification, tier determinism, property tests | Reference |

### Architecture

| Document | Purpose | Status |
|----------|---------|--------|
| [architecture/cross-platform.md](architecture/cross-platform.md) | Workspace layout, crate responsibilities, development workflow | Current |
| [architecture/simulation.md](architecture/simulation.md) | Future design for emergent simulation (properties, events, CA) | Future design |
| [architecture/capability-tier-reference.md](architecture/capability-tier-reference.md) | Cross-platform capability tier hierarchy: per-tier types, algorithms, sharing matrix, seed system, tier divergence | Reference (all tiers complete; AI/spawn/dungeon unified via rules/) |

### Platforms

| Document | Purpose | Status |
|----------|---------|--------|
| [platforms/c64-port-proposal.md](platforms/c64-port-proposal.md) | C64 port proposal: rust-mos toolchain, shared crate design, implementation plan | Implemented — C64 is production frontend over core |
| [platforms/c64-platform-guide.md](platforms/c64-platform-guide.md) | C64-specific hardware guidance: module mapping, CIA multiplexing, static stack allocation, cycle budgets, 6502 code style | Reference |
| [platforms/c64-demo-techniques-for-roguelike.md](platforms/c64-demo-techniques-for-roguelike.md) | VIC-II demo scene techniques evaluated for the roguelike C64 port | Reference |
| [platforms/c64-display-mode-analysis.md](platforms/c64-display-mode-analysis.md) | VIC-II display mode evaluation: why standard character mode is optimal | Reference |
| [platforms/c64-atproto-bridge.md](platforms/c64-atproto-bridge.md) | Self-hostable bridge server connecting C64 (via Ultimate64 Ethernet) to AT Protocol for PDS saves and spectating | Future design |
| [platforms/gba-port.md](platforms/gba-port.md) | GBA frontend: rendering, input, save, audio, no_std core adaptations | Implemented |
| [platforms/vita-port.md](platforms/vita-port.md) | PS Vita frontend: vita2d rendering, dual touch, spatial audio, WiFi networking | Phase 1 complete |
| NDS frontend (no dedicated doc) | NDS frontend: hardware 3D, 2D automap, touchscreen, compact tier, bare-metal from GBATEK. See [CLAUDE.md](../CLAUDE.md) for crate details. | Phase 3 complete |

### Game & Feature Design

| Document | Purpose | Status |
|----------|---------|--------|
| [design/gameplay-implementation-plan.md](design/gameplay-implementation-plan.md) | Implementation plan for high-leverage gameplay features (6 phases) | In progress (Phases 1-3 complete) |
| [design/cross-tier-content-foundation.md](design/cross-tier-content-foundation.md) | Canonical portable content pipeline, stable IDs, live reconciliation, and next-agent direction | Current |
| [design/procgen-exploration.md](design/procgen-exploration.md) | Survey of procedural generation techniques with evaluation against project constraints | Exploration |
| [design/procgen-terrain-and-themed-floors.md](design/procgen-terrain-and-themed-floors.md) | Terrain variety, themed procedural floors, and prefab integration | Exploration |
| [design/acoustic-propagation.md](design/acoustic-propagation.md) | Sound as a game mechanic: acoustic propagation, SID/PSG integration, spatialized audio | Exploration |
| [design/spectator-mode.md](design/spectator-mode.md) | Spectator mode options (file, stderr, TCP, Unix socket) | Current |
| [design/atproto.md](design/atproto.md) | AT Protocol integration: OAuth, PDS saves, WASM frontend | Future design |
| [design/atproto-spectating.md](design/atproto-spectating.md) | Federated spectating via AT Protocol: Jetstream frame delivery, discovery | Future design |

### Tooling

| Document | Purpose | Status |
|----------|---------|--------|
| [tooling/mcp-optimizations.md](tooling/mcp-optimizations.md) | MCP tool design principles and optimization ideas for AI play | Current |
| [tooling/headless-runner.md](tooling/headless-runner.md) | Headless runner: CLI flags, parameter sweeps, visualization tools | Current |
| [tooling/llm-playtesting.md](tooling/llm-playtesting.md) | LLM playtesting: dual backends, /playtest skill, token optimization | Current |

### Reports

| Document | Purpose | Status |
|----------|---------|--------|
| [reports/llm-playtest-summary.md](reports/llm-playtest-summary.md) | Consolidated findings from all 6 LLM playtest sessions | Historical |
| [reports/llm-playtest-session-1.md](reports/llm-playtest-session-1.md) | Session 1: baseline, no healing, identified core MCP pain points | Historical |
| [reports/llm-playtest-session-2.md](reports/llm-playtest-session-2.md) | Session 2: HP regen added, autorun/navigation issues persisted | Historical |
| [reports/llm-playtest-session-3.md](reports/llm-playtest-session-3.md) | Session 3: pathfind_to + frontiers, efficiency nearly doubled | Historical |
| [reports/llm-playtest-session-4.md](reports/llm-playtest-session-4.md) | Session 4: auto_explore, MCP interface reached maturity | Historical |
| [reports/llm-playtest-session-5.md](reports/llm-playtest-session-5.md) | Session 5: Item system verification (potions, swords, armor) | Historical |
| [reports/llm-playtest-session-6.md](reports/llm-playtest-session-6.md) | Session 6: Micro tier (BFS, StairsFound, auto_fight) | Historical |
| [reports/dev-tools-session.md](reports/dev-tools-session.md) | Dev tooling session: debug console, map presets, replay, headless | Historical |

### Archive

| Document | Purpose | Status |
|----------|---------|--------|
| [archive/plan-platform-abstraction-and-menus.md](archive/plan-platform-abstraction-and-menus.md) | Original plan for platform traits + menu system (fully implemented) | Archived |

## Suggested Reading Order

1. **[roadmap.md](roadmap.md)** — Start here for the big picture: what's done, what's next, and how features depend on each other.
2. **[architecture/cross-platform.md](architecture/cross-platform.md)** — Understand the workspace layout and how the six crates relate.
3. **[tooling/mcp-optimizations.md](tooling/mcp-optimizations.md)** — Design principles for the MCP server and AI play interface.
4. **[reports/llm-playtest-summary.md](reports/llm-playtest-summary.md)** — How the MCP tools were validated through playtesting.
5. **[design/atproto.md](design/atproto.md)** — The most detailed future design: AT Protocol identity, PDS saves, and WASM.

## Related Top-Level Docs

- [README.md](../README.md) — Project overview, feature list, and roadmap summary
- [CONTRIBUTING.md](../CONTRIBUTING.md) — How to contribute
- [CLAUDE.md](../CLAUDE.md) — AI assistant context for working on this codebase
