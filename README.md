# Husky Forge

Image processing, perfected. Convert, optimize and archive large photo collections — RAW, JPEG,
PNG, WebP, TIFF, JPEG XL — with colour profiles and metadata intact, on Windows, macOS and Linux.

Rust core, no browser runtime. Desktop UI (Slint) is in progress; the `forge` CLI runs the same engine today.

## Install

Grab a binary from [Releases](https://github.com/DhakadG/husky-forge/releases), or build from source:

```bash
cargo install --path crates/forge-cli
```

## Use

```bash
# copy-mode: JPEG, q82, capped at 8 MP, results in _compressed/ next to the originals
forge ~/Photos/2025

# archive originals, JPEG XL tuned per file toward 2.5 MB, GPS stripped
forge ~/Photos/2025 --to jxl --target-mb 2.5 --meta strip-gps --mode archive

# see the Impact estimate without writing anything
forge ~/Photos --dry-run
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
