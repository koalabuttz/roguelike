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

## Guidelines

- **One logical change per PR** — Don't mix features with refactors or bug fixes
- **Add tests** — New features and behavior changes should include unit tests
- **Keep it modular** — New systems should be self-contained modules with clear interfaces
- **Follow existing patterns** — Look at how current modules are structured before adding new ones

## Adding a Monster

The simplest way to contribute content — see the [README](README.md#adding-a-new-monster) for a step-by-step guide.

## Questions?

Open an issue to discuss before starting large changes.
