# Documentation

Project documentation for the roguelike dungeon crawler.

## Document Index

| Document | Purpose | Status |
|----------|---------|--------|
| [roadmap.md](roadmap.md) | Prioritized feature roadmap with dependencies and critical path | Current |
| [mcp-optimizations.md](mcp-optimizations.md) | MCP tool design principles and optimization ideas for AI play | Current |
| [architecture/cross-platform.md](architecture/cross-platform.md) | Workspace structure, platform abstraction, crate responsibilities | Current |
| [architecture/simulation.md](architecture/simulation.md) | Future design for emergent simulation (properties, events, CA) | Future design |
| [design/atproto.md](design/atproto.md) | AT Protocol integration: OAuth, PDS saves, WASM frontend | Future design |
| [design/atproto-spectating.md](design/atproto-spectating.md) | Federated spectating via AT Protocol: Jetstream frame delivery, discovery | Future design |
| [design/c64-atproto-bridge.md](design/c64-atproto-bridge.md) | Self-hostable bridge server connecting C64 (via Ultimate64 Ethernet) to AT Protocol for PDS saves and spectating | Future design |
| [design/gba-port.md](design/gba-port.md) | GBA frontend: rendering, input, save, audio, no_std core adaptations | Proposed |
| [design/procgen-exploration.md](design/procgen-exploration.md) | Survey of procedural generation techniques with evaluation against project constraints | Exploration |
| [design/gameplay-implementation-plan.md](design/gameplay-implementation-plan.md) | Implementation plan for high-leverage gameplay features (6 phases) | Proposed |
| [c64-port-proposal.md](c64-port-proposal.md) | C64 port proposal: rust-mos toolchain, shared crate design, implementation plan | Proposal — POC validated |
| [c64-technical-reference.md](c64-technical-reference.md) | Implementation details, code listings, and C64 platform guidance for the port proposal | Reference |
| [design/spectator-mode.md](design/spectator-mode.md) | Spectator mode options (file, stderr, TCP, Unix socket) | Current |
| [headless-runner.md](headless-runner.md) | Headless runner: CLI flags, parameter sweeps, visualization tools | Current |
| [llm-playtesting.md](llm-playtesting.md) | LLM playtesting: dual backends, /playtest skill, token optimization | Current |
| [reports/llm-playtest-summary.md](reports/llm-playtest-summary.md) | Consolidated findings from all 4 LLM playtest sessions | Historical |
| [reports/llm-playtest-session-1.md](reports/llm-playtest-session-1.md) | Session 1: baseline, no healing, identified core MCP pain points | Historical |
| [reports/llm-playtest-session-2.md](reports/llm-playtest-session-2.md) | Session 2: HP regen added, autorun/navigation issues persisted | Historical |
| [reports/llm-playtest-session-3.md](reports/llm-playtest-session-3.md) | Session 3: pathfind_to + frontiers, efficiency nearly doubled | Historical |
| [reports/llm-playtest-session-4.md](reports/llm-playtest-session-4.md) | Session 4: auto_explore, MCP interface reached maturity | Historical |
| [reports/dev-tools-session.md](reports/dev-tools-session.md) | Dev tooling session: debug console, map presets, replay, headless | Historical |
| [archive/plan-platform-abstraction-and-menus.md](archive/plan-platform-abstraction-and-menus.md) | Original plan for platform traits + menu system (fully implemented) | Archived |

## Suggested Reading Order

1. **[roadmap.md](roadmap.md)** — Start here for the big picture: what's done, what's next, and how features depend on each other.
2. **[architecture/cross-platform.md](architecture/cross-platform.md)** — Understand the workspace layout and how the five crates relate.
3. **[mcp-optimizations.md](mcp-optimizations.md)** — Design principles for the MCP server and AI play interface.
4. **[reports/llm-playtest-summary.md](reports/llm-playtest-summary.md)** — How the MCP tools were validated through playtesting.
5. **[design/atproto.md](design/atproto.md)** — The most detailed future design: AT Protocol identity, PDS saves, and WASM.

## Related Top-Level Docs

- [README.md](../README.md) — Project overview, feature list, and roadmap summary
- [CONTRIBUTING.md](../CONTRIBUTING.md) — How to contribute
- [claude.md](../claude.md) — AI assistant context for working on this codebase
