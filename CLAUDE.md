# Husky Forge

Rust workspace: `crates/forge-core` (engine), `crates/forge-cli` (`forge` binary), `crates/forge-app` (Slint desktop app, `husky-forge` binary). Plan: `docs/PLAN.md`. Installers: `packaging/`.

- Build: `cargo build --release --workspace` (JXL needs cmake + clang-cl on Windows; `--no-default-features` without them)
- Test: `cargo test --workspace` · Lint: `cargo clippy --workspace -- -D warnings` (CI enforces)
- Release: push tag `v*` → `.github/workflows/release.yml` builds Windows/macOS/Linux binaries onto the GitHub Release
- Core stays platform-agnostic; OS specifics live only in `forge-app/src/platform.rs`, `forge-cli/src/schedule.rs` and `packaging/`
- Keep files under 500 lines; one runnable check per non-trivial module
