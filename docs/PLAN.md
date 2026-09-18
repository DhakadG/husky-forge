# Husky Forge — plan and status

Image processing, perfected. Cross-platform native desktop engine for converting, optimizing and
archiving photographic archives. Rust core, Slint UI, thin platform adapters. Builds ship only as
GitHub Releases on `v*` tags.

**Priority: Windows first.** macOS/Linux keep building in CI with the same engine; their platform
extras are ported after the Windows app is solid.

Legend: ✅ done and verified on real files · 🟡 partial · ❌ not started

## 1. Processing pipeline (the original diagram)

| Stage | Status | Where | Notes |
|---|---|---|---|
| Source file | ✅ | `job.rs` walk | files or folders, recursive, `_archive/_compressed` skipped |
| Format / metadata inspection | 🟡 | `inspect.rs`, `decode.rs` | by extension; EXIF/ICC/XMP read. ❌ magic-byte sniffing, ❌ `forge inspect` command |
| Raster decode | ✅ | `image` crate | JPEG PNG WebP TIFF GIF BMP; EXIF orientation applied |
| RAW decode (LibRaw/darktable) | ✅ | `rawler` (pure Rust) | NEF/DNG/ARW/CR2/CR3/RAF/ORF/RW2/PEF…; orientation from EXIF; default look = auto levels + soft S-curve (`--raw-look flat` opts out). ❌ WB/exposure/highlight controls |
| HEIC / AVIF decode | 🟡 | `heif.rs` via libheif | code done; Windows static build via vcpkg being wired (CRT triplet) |
| JPEG XL decode | ✅ | `jxl-oxide` | |
| High-precision image | ✅ | `DynamicImage` 8/16-bit/f32 | 16-bit kept through resize; RAW develops to 16-bit |
| Colour management / ICC | 🟡 | `color.rs` (`moxcms`) | ICC kept for JPEG/PNG/WebP; folded to sRGB for AVIF/JXL. ❌ output-profile choice (P3/AdobeRGB), ❌ ICC embedding in AVIF/JXL |
| HDR | 🟡 | | float → JXL 32-bit float. ❌ PQ/HLG AVIF, ❌ HDR preserve toggle |
| LUT / transforms | ✅ | `transform.rs` | `.cube` 1D/3D trilinear |
| Resize / crop / padding | ✅ | `transform.rs` | cap MP, fit, fill+crop, fit+pad. ❌ manual crop |
| Tone mapping | 🟡 | `develop.rs` | RAW look only. ❌ HDR→SDR tone map (float sources are clipped) |
| Output profile | 🟡 | | sRGB only |
| Encoder | ✅ | `encode.rs` | JPEG (mozjpeg progressive, 4:4:4 ≥ q90), PNG 8/16, WebP, AVIF 8/10-bit, JXL 8/16/float (quality→distance, threaded). ❌ JXL effort / AVIF speed knobs in UI |
| Target size | ✅ | | per-file quality search, Husky Drop heuristic |
| Metadata writer | ✅ | `meta.rs` | EXIF rewritten (orientation dropped, GPS optional, MakerNote shed when > 64 KB for JPEG), XMP, ICC. ❌ extended XMP, ❌ MakerNote offset relocation, ❌ XMP in AVIF |
| Validate | 🟡 | `job.rs verify` | dims re-read for JPEG/PNG/WebP, signature for AVIF/JXL. ❌ full decode check |
| Atomic filesystem commit | ✅ | `job.rs commit` | `.part` → verify → rename; copy / archive / replace |

## 2. Job management (the Husky Drop half)

| Item | Status |
|---|---|
| copy / archive / replace originals | ✅ |
| target-size quality iteration | ✅ |
| sidecar protection (RAW + .xmp) | ✅ |
| filters: min size, existing output, unsupported | ✅ |
| queueing + parallelism | ✅ rayon workers, pause / resume / cancel; ❌ multiple queued jobs |
| recurring rules | ✅ `forge rule add/list/rm/enable/disable/run-due`; ❌ rules UI |
| OS scheduler | ✅ Task Scheduler / launchd / systemd (`forge rule schedule`) |
| history + undo | ✅ SQLite, `forge history/files/undo`, app "Undo last"; ❌ history browser in app |
| custom output folder | ✅ mirrors dropped folder structure |
| logging | ✅ `<LocalAppData>/husky-forge/logs/husky-forge.log`, live Log panel, panics logged |
| crash isolation | ✅ a codec panic fails one file, never the job |

## 3. Impact card
✅ first-class: estimate while planning, real numbers after; lines for size, %, per-format counts,
bit depth, ICC/EXIF/GPS handling, resize, LUT, target. ❌ per-file before/after preview.

## 4. UI (Slint)

| Item | Status |
|---|---|
| Simple mode: drop → format → quality → originals → START | ✅ |
| Advanced: target size, output folder, resize modes, LUT, metadata, RAW look, filters, workers, presets | ✅ |
| ❌ Advanced: source-type filter, ICC/HDR policy, JXL effort/distance, separate XMP toggle | |
| drag and drop from Explorer | ✅ via winit hook (Slint's winit backend does not forward OS drops) |
| progress: bar, done/failed counters, elapsed, ETA, per-file time and status | ✅ |
| log panel + Open log file | ✅ |
| Open output folder, output path shown before run | ✅ |
| typography: bundled Inter, larger sizes | ✅ (first pass; real design pass later) |
| pause / resume / cancel | ✅ |
| presets (TOML) | ✅ |
| single instance: second launch funnels into the open window | ✅ |
| ❌ thumbnails / before-after compare, ❌ history browser, ❌ rules editor | |

## 5. Platform

### Windows
| Item | Status |
|---|---|
| Mica backdrop | ✅ (`window-vibrancy`) — ❌ Mica Alt option, ❌ title-bar integration |
| native file dialogs | ✅ rfd |
| notifications | ✅ toast on completion |
| Task Scheduler | ✅ |
| Explorer context menu | 🟡 `forge shell install` writes a "Husky Forge" cascade (open / JPEG / JXL / AVIF / archive-2.5MB / strip GPS / presets) to HKCU. **Not visible on this machine** — HKCU `SystemFileAssociations` verbs appear to be ignored while HKLM ones (File Converter) show. Fix in progress: installer writes HKLM (elevated); portable fallback via `*\shell` + `AppliesTo` |
| Windows 11 top-level menu | ❌ needs sparse MSIX + IExplorerCommand |
| installer | ✅ Inno Setup: Start Menu, optional PATH, context menu task |

### macOS (after Windows)
❌ vibrancy verified · ❌ launchd verified · ❌ Finder Quick Action · ✅ `.app` + dmg (ad-hoc signed) built in CI · ❌ HEIC bundling · ❌ Developer ID signing/notarization

### Linux (after Windows)
✅ AppImage + `.desktop` with MimeTypes built in CI · ✅ systemd user timer code · ❌ verified on a desktop · ❌ HEIC bundling

## 6. Packaging / release
✅ `release.yml` on `v*`: Windows installer + portable zip, mac dmg ×2, Linux AppImage + tar.gz ·
✅ in-app update check · ❌ winget / Homebrew / Flatpak · ❌ code signing

## 7. Crate layout vs the proposed one
The proposal listed forge-core / engine / codecs / color / metadata / raw / queue / history /
scheduler / ui / platform. Today these are modules inside `forge-core` (`decode`, `develop`,
`encode`, `color`, `meta`, `transform`, `job`, `history`, `rules`, `logging`, `heif`) plus
`forge-cli` (incl. `schedule`, `shell`) and `forge-app` (incl. `platform`, `launch`). Split into
crates only when a module passes ~500 lines or needs its own feature gates.

## Next up (in order)
1. HEIC on Windows: static libheif with the `/MT` triplet to match libjxl, CI + installer
2. Explorer menu visible (HKLM via installer, AppliesTo fallback), verified on this machine
3. Inspect command + magic-byte sniffing; full-decode validation
4. JXL effort / AVIF speed / output profile / HDR toggles in Advanced
5. HDR→SDR tone mapping; ICC in AVIF/JXL
6. Rules editor + history browser in the app
7. UI design pass (thumbnails, before/after)
8. macOS / Linux port verification
