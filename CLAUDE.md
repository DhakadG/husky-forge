# Husky Forge

Rust workspace: `crates/forge-core` (engine), `crates/forge-cli` (`forge` binary), `crates/forge-app` (Slint desktop app, `husky-forge` binary). Plan: `docs/PLAN.md`. Installers: `packaging/`.

- Build (Windows): `CMAKE=<VS cmake.exe> VCPKG_ROOT=C:/vcpkg VCPKGRS_TRIPLET=x64-windows-static cargo build --release --workspace --features heic` — libjxl needs clang-cl (VS component), libheif comes from `vcpkg install libheif:x64-windows-static` (/MT to match libjxl)
- Test: `cargo test --workspace` · Lint: `cargo clippy --workspace -- -D warnings` (CI enforces)
- Release: push tag `v*` → `.github/workflows/release.yml` builds Windows/macOS/Linux binaries onto the GitHub Release
- Core stays platform-agnostic; OS specifics live only in `forge-app/src/platform.rs`, `forge-cli/src/schedule.rs` and `packaging/`
- Keep files under 500 lines; one runnable check per non-trivial module
