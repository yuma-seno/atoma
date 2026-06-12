# Contributing to Atoma

Thank you for your interest in contributing! This document covers how to report bugs, suggest features, and submit pull requests.

## Reporting Issues

1. Search [existing issues](https://github.com/yuma-seno/atoma/issues) first to avoid duplicates.
2. Open a new issue with:
   - A clear, descriptive title
   - Steps to reproduce (for bugs)
   - Expected vs. actual behavior
   - Atoma version (`atoma --version`) and OS

## Development Setup

```bash
# Clone the repo
git clone https://github.com/yuma-seno/atoma
cd atoma

# Build
cargo build

# Run tests
cargo test

# Check formatting and lints
cargo fmt --check
cargo clippy -- -D warnings
```

The project uses Rust edition 2021 with toolchain `1.95.0` (pinned via `rust-toolchain.toml`).

## Submitting a Pull Request

1. Fork the repository and create a branch from `main`:
   ```bash
   git checkout -b feat/my-feature
   ```
2. Make your changes. Keep commits focused — one logical change per commit.
3. Run the full check suite before pushing:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
4. Open a PR against `main`. Describe:
   - What the change does
   - Why it is needed
   - Any breaking changes

PRs that fail CI checks will not be merged.

## Versioning

Atoma follows [Semantic Versioning](https://semver.org). Bump the version in `Cargo.toml` to trigger an automated release:
- **Patch** (`0.1.x`): bug fixes, internal improvements
- **Minor** (`0.x.0`): new features, backward-compatible
- **Major** (`x.0.0`): breaking changes to CLI or config format

## Code Style

- Use `cargo fmt` (default settings)
- No `unwrap()` in non-test code; prefer `?` with `anyhow`
- Keep modules focused: one clear responsibility per file
- All public APIs should be documented with `///` comments

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
