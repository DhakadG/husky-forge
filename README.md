# Husky Forge

Image processing, perfected. Convert, optimize and archive large photo collections — RAW, JPEG,
PNG, WebP, TIFF, JPEG XL — with colour profiles and metadata intact, on Windows, macOS and Linux.

Rust core, native Slint UI, no browser runtime. Every download bundles its codecs — nothing else to install.

## Install

From [Releases](https://github.com/DhakadG/husky-forge/releases):

| Platform | Download |
|----------|----------|
| Windows 10/11 | `husky-forge-<v>-windows-x64-setup.exe` (installer, adds "Forge with Husky" to Explorer) or the portable zip |
| macOS 11+ | `husky-forge-<v>-macos-arm64.dmg` / `-x64.dmg` — unsigned for now: right-click → Open the first time |
| Linux | `husky-forge-<v>-linux-x64.AppImage` (`chmod +x`, run) or the tar.gz |

Each package ships the desktop app (`husky-forge`) and the CLI (`forge`). From source: `cargo build --release --workspace`.

## Desktop app

Drop folders or photos, pick a format and quality, press START. On Windows, right-click any folder or image → **Husky Forge** for one-click actions (the installer sets this up; portable users run `forge shell install`). Advanced mode adds per-file size
targets, fit/fill/pad resizing, .cube LUTs, metadata policy, worker count and presets. The Impact card
shows the estimate before and the real numbers after. Undo reverts the last copy/archive job.

## CLI

## Use

```bash
# copy-mode: JPEG, q82, capped at 8 MP, results in _compressed/ next to the originals
forge ~/Photos/2025

# archive originals, JPEG XL tuned per file toward 2.5 MB, GPS stripped
forge ~/Photos/2025 --to jxl --target-mb 2.5 --meta strip-gps --mode archive

# see the Impact estimate without writing anything
forge ~/Photos --dry-run

# recurring: every 7 days, and let the OS run it nightly at 03:00
forge rule add --name inbox --every-days 7 ~/Photos/Inbox --to jxl --mode archive
forge rule schedule --hour 3

# history and undo
forge history
forge undo 12
```

Every run ends with the Impact card:

```
  3.8 GB → 481 MB
  88% smaller
  525 JPEG → 525 JPEG XL
  8-bit SDR
  ICC preserved
  EXIF preserved
  GPS removed (525 files)
  Dimensions capped at 8 MP
  Quality individually tuned to 2.5 MB target
```

## Roadmap

See [docs/PLAN.md](docs/PLAN.md).
