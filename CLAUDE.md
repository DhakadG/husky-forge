# Husky Forge

Rust workspace: `crates/forge-core` (engine), `crates/forge-cli` (`forge` binary). Plan: `docs/PLAN.md`.

- Build: `cargo build --release -p forge-cli` (JXL needs cmake + clang-cl on Windows; `--no-default-features` without them)
- Test: `cargo test --workspace` · Lint: `cargo clippy --workspace -- -D warnings` (CI enforces)
- Release: push tag `v*` → `.github/workflows/release.yml` builds Windows/macOS/Linux binaries onto the GitHub Release
- Core stays platform-agnostic; OS specifics go in `forge-platform` only
- Keep files under 500 lines; one runnable check per non-trivial module
