# Husky Forge — plan

Image processing, perfected. Cross-platform desktop engine for converting, optimizing and archiving
photographic archives. Rust core, Slint UI, native platform adapters. Builds ship as GitHub Releases.

## Priority
Windows first (installer, Mica, Explorer integration, HEIC). macOS and Linux keep building in CI and
ship the same engine; their platform extras (HEIC bundling, signing, Quick Actions) are ported after
the Windows release is solid.

## Pipeline

```
source → inspect → decode (raster | RAW) → cap / resize → encode → metadata → verify → atomic commit
```

## Crates

| crate        | role                                                       | status |
|--------------|------------------------------------------------------------|--------|
| `forge-core` | engine: inspect, decode, encode, meta, job, impact         | v0.1   |
| `forge-cli`  | `forge` binary; CI smoke test + scripting                  | v0.1   |
| `forge-app`  | Slint desktop UI, simple + advanced mode                   | v0.2   |
| platform code | `forge-app/src/platform.rs` (Mica, vibrancy, toasts) · `forge-cli/src/schedule.rs` (Task Scheduler, launchd, systemd) · `packaging/` | v0.2 |

Split further only when a file passes ~500 lines.

## Phases

### 0 — repo & CI ✅
- workspace, MIT, `.github/workflows/ci.yml` (3 OS test + clippy), `release.yml` (tag `v*` → binaries)

### 1 — engine ✅ (v0.1)
- inspect by extension; JPEG/PNG/WebP/TIFF/GIF/BMP via `image`, JXL via `jxl-oxide`, RAW via `rawler`
- EXIF orientation applied; RAW orientation applied
- megapixel cap, Lanczos3 (`fast_image_resize`)
- encoders: JPEG (mozjpeg, progressive, 4:4:4 at q≥90), PNG, WebP, AVIF (ravif), JXL (libjxl, feature `jxl`)
- target-size quality search (ported from Husky Drop)
- metadata: ICC + EXIF + XMP re-attached for JPEG/PNG/WebP; EXIF rewritten (orientation dropped, GPS optional)
- job engine: walk, filter (small / sidecar / existing copy), rayon workers, copy | archive | replace,
  `.part` write → verify → rename
- Impact card: estimate before, real numbers after

### 2 — engine depth
- [x] ICC → sRGB conversion (`moxcms`) when output cannot carry the profile; keep profile otherwise
- [x] 16-bit path: PNG/JXL 16-bit, AVIF 10-bit from Rgb16 sources (RAW develops to 16-bit)
- [x] EXIF (+XMP) boxes for AVIF + JXL containers
- [x] LUT (.cube 1D/3D) · fit / fill / pad resize modes
- [x] SQLite history (`rusqlite` bundled): every job, every file, undo for copy/archive
- [x] recurring rules (paths + options + cadence), `forge rule run-due` for the OS scheduler
- [x] HEIC/HEIF + AVIF decode via libheif (`heic` feature): Windows static via vcpkg in CI
- [ ] HEIC on macOS (brew libheif + dylib bundling) and Linux (AppImage bundling) — after the Windows release
- [x] HDR float sources pass into JXL as 32-bit float
- [ ] PQ/HLG 10-bit AVIF output with CICP; tone-map float/HDR sources to SDR for JPEG/WebP (today: clipped)
- [ ] ICC embedding for AVIF (`avif-serialize` colr box) and JXL (`JxlEncoderSetICCProfile`) — drop the sRGB fold
- [ ] MakerNote offset relocation on EXIF rewrite
- [ ] rawler: expose white balance / exposure / highlight recovery as advanced options

### 3 — UI (Slint) ✅ (v0.2)
- [x] simple mode: drop zone (native file drop) → format → quality → originals → START
- [x] advanced mode: target size, fit/fill/pad, LUT, metadata, filters, workers, presets
- [x] Impact card first-class (estimate while planning, real numbers after); per-file rows with before/after, q, bits, decoder, errors
- [x] cancel (in-flight files finish cleanly), progress from `Event`, Undo last
- [x] presets as TOML in the per-user config dir
- [x] paths on argv → Explorer/Finder/desktop "Forge with Husky"
- [x] pause / resume / cancel
- [ ] per-file preview thumbnails and before/after compare
- [ ] history browser inside the app (today: `forge history` / `forge undo`)

### 4 — platform
- [x] Windows: Mica (`window-vibrancy`), toast (`notify-rust`), Task Scheduler (`forge rule schedule`)
- [x] Windows Explorer: `forge shell install` writes a "Husky Forge" cascade (open / JPEG copy / JXL / AVIF / archive 2.5 MB / strip GPS / every saved preset) on folders, folder backgrounds and image files; one-click entries pass `--start`; multi-select funnels into the running window
- [ ] Windows 11 top-level menu entry (needs a sparse MSIX + IExplorerCommand; today the cascade sits under "Show more options")
- [x] macOS: vibrancy, launchd agent, notifications, `.app` accepts folders/images
- [x] Linux: XDG dirs (`dirs`), systemd user timer, desktop notifications, `.desktop` with MimeTypes
- [x] native file dialogs (`rfd`)
- [ ] macOS Finder Quick Action (Automator workflow in the dmg)
- [ ] Windows: WinUI-style title bar integration (Slint draws its own chrome today)

### 5 — packaging (`release.yml`, on every `v*` tag)
- [x] Windows: Inno Setup installer (Start Menu, optional context menu + PATH) + portable zip
- [x] macOS: `.app` in a dmg (arm64 + x64), ad-hoc signed — right-click → Open on first launch until a Developer ID exists
- [x] Linux: AppImage + tar.gz
- [ ] Developer ID signing + notarization (needs certificates in repo secrets)
- [ ] winget / Homebrew cask / Flatpak manifests
- [x] in-app "new version" check against GitHub Releases API

## Dependencies policy
Everything is statically linked or pure Rust: users install nothing. The one exception today is JXL
encoding, which needs cmake + a C++ compiler at *build* time (CI has them; local builds can pass
`--no-default-features`). HEIC/AVIF decode will follow the same rule (static libheif/dav1d in CI).

## Non-goals (for now)
- video, documents — name leaves room, code does not
- cloud storage backends — Husky Drop owns that side
