# Contributing

Thanks for your interest in contributing to this project!

## License

This project is licensed under **GPL-3.0-or-later**. The author reserves the right to release platform-specific ports (e.g., console, mobile) under a commercial license.

By submitting a pull request, you agree that your contributions may be included in commercially licensed builds. The open source version will always remain fully functional and identical in gameplay.

## Getting Started

1. Fork the repository and clone your fork
2. Create a feature branch: `git checkout -b my-feature`
3. Make your changes

## Before Submitting

All of the following must pass:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

CI runs these automatically on every pull request.

## Development Workflow

### Running Checks Locally

Before pushing, run the validation commands above. You can run them individually during development or all at once before committing.

### Pre-commit Hook (Recommended but Optional)

To automatically run all checks before each commit:

```sh
# One-time setup
cp .github/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

**Why it's optional**: The hook helps catch issues early and saves CI minutes, but it's not required — GitHub Actions will run the same checks on every PR. Some developers prefer to commit freely and rely on CI feedback.

**Best practice**: If you're actively developing, enabling the hook prevents you from pushing broken code and provides faster feedback than waiting for CI.

## Edition 2024 Notes

This project uses Rust edition 2024. Key differences:

- **`gen` is a reserved keyword** — Use `r#gen()` when calling `rand::Rng::gen()` directly
- `gen_range()` and `gen_bool()` work normally (not exact match on `gen`)
- Requires Rust 1.85.0 or later

## Guidelines

- **One logical change per PR** — Don't mix features with refactors or bug fixes
- **Add tests** — New features and behavior changes should include unit tests
- **Keep it modular** — New systems should be self-contained modules with clear interfaces
- **Follow existing patterns** — Look at how current modules are structured before adding new ones

## Adding a Monster

The simplest way to contribute content — see the [README](README.md#adding-a-new-monster) for a step-by-step guide.

## Questions?

Open an issue to discuss before starting large changes.
