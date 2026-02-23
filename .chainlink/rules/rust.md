### Rust Best Practices

#### Edition 2024
- This project uses Rust edition **2024** (requires rustc 1.85.0+).
- `gen` is a reserved keyword. Use `r#gen()` for `rand::Rng::gen()`.
- `gen_range()` and `gen_bool()` are NOT affected and work normally.

#### Code Style
- Use `rustfmt` for formatting: `cargo fmt --all --check`
- Use `clippy` for linting: `cargo clippy --workspace -- -D warnings`
- Prefer `?` operator over `.unwrap()` for error handling
- Avoid `.clone()` unless necessary — prefer references
- Use `&str` for function parameters, `String` for owned data

#### Error Handling
- This project does NOT use `anyhow` or `thiserror` — use standard `Result` types and custom error handling
- Propagate errors with `?` operator
- Never use `.unwrap()` in production code — handle errors explicitly

#### Architecture Conventions
- Player is always `entities[0]`
- **Method placement**: put helpers where the data lives (map queries → `map.rs`, entity queries → `game.rs`). Only orchestration on `GameState`.
- Tests go in `#[cfg(test)] mod tests` blocks at the bottom of each source file
- **Enums over strings** for game concepts (item types, effects, equipment slots)
- **Pure functions for rules** — if a calculation doesn't need `&self`, make it a free function
- **Const for limits** — use named constants for caps, not hardcoded literals
- **Keep balance data in `game.toml`** — `crates/core/data/game.toml` is compiled into the binary via `build.rs`. Edit balance constants there (player HP, monster stats, spawn weights, FOV radius, room sizes), NOT in Rust source files. The `data.rs` module loads these at compile time.

#### Memory Safety
- Never use `unsafe` without explicit justification and review
- Prefer `Vec` over raw pointers
- Use `Arc<Mutex<T>>` for shared mutable state across threads
- Avoid `static mut` — use `lazy_static` or `once_cell` instead

#### Testing
- Run `cargo test --workspace` before committing (~600 unit + integration tests)
- Golden replay tests: `cargo test -p roguelike-core --test golden_replays`
- Balance scenarios: `cargo test -p roguelike-core --test scenarios`
- Invariant property tests: `cargo test -p roguelike-core --test invariants`
- MCP integration tests: `cargo test -p roguelike-mcp`
- Use `tempfile` for tests involving filesystem
- If golden replay tests fail after an intentional gameplay change, regenerate:
  `cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/`

#### SQL Injection Prevention
Always use parameterized queries with `rusqlite::params![]`:
```rust
// GOOD
conn.execute("INSERT INTO users (name) VALUES (?1)", params![name])?;

// BAD - SQL injection vulnerability
conn.execute(&format!("INSERT INTO users (name) VALUES ('{}')", name), [])?;
```
