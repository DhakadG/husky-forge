# Husky Forge — plan

Image processing, perfected. Cross-platform desktop engine for converting, optimizing and archiving
photographic archives. Rust core, Slint UI, native platform adapters. Builds ship as GitHub Releases.

## Pipeline

```
source → inspect → decode (raster | RAW) → cap / resize → encode → metadata → verify → atomic commit
```

## Crates

| crate        | role                                                       | status |
|--------------|------------------------------------------------------------|--------|
| `forge-core` | engine: inspect, decode, encode, meta, job, impact         | v0.1   |
| `forge-cli`  | `forge` binary; CI smoke test + scripting                  | v0.1   |
| `forge-app`  | Slint desktop UI, simple + advanced mode                   | phase 3 |
| `forge-platform` | Mica / vibrancy / notifications / scheduler adapters   | phase 4 |

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
- [ ] HEIC decode (libheif + libde265, static) · AVIF decode (dav1d, static) — bundled in CI, never a user install
- [ ] HDR: Rgb32F / PQ / HLG passthrough into AVIF + JXL; tone-map to SDR for JPEG/WebP
- [ ] ICC embedding for AVIF (`avif-serialize` colr box) and JXL (`JxlEncoderSetICCProfile`) — drop the sRGB fold
- [ ] MakerNote offset relocation on EXIF rewrite
- [ ] rawler: expose white balance / exposure / highlight recovery as advanced options

### 3 — UI (Slint)
- [ ] simple mode: drop zone → format → quality → keep originals → Convert
- [ ] advanced mode: source / color / transform / output / metadata / originals / workers panels
- [ ] Impact card first-class; per-file table with before/after, q, via, errors
- [ ] queue: pause / resume / cancel; progress from `Event`
- [ ] presets saved as TOML

### 4 — platform
- [ ] Windows: Mica via `window-vibrancy`, toast (`notify-rust`), Task Scheduler for rules, Explorer "Forge here"
- [ ] macOS: NSVisualEffectView, launchd agent, Finder service, notifications
- [ ] Linux: XDG dirs, systemd user timer, desktop notifications, `.desktop` file
- [ ] native file dialogs (`rfd`) everywhere

### 5 — packaging
- [ ] Windows: portable zip + MSI (cargo-wix) · macOS: .app + dmg, signed/notarized when certs exist
- [ ] Linux: AppImage + tar.gz · all attached to the GitHub Release by `release.yml`
- [ ] in-app "new version" check against GitHub Releases API

## Non-goals (for now)
- video, documents — name leaves room, code does not
- cloud storage backends — Husky Drop owns that side
