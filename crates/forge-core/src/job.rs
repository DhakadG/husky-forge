//! Job engine: walk → filter → plan → parallel process → verify → atomic commit.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::decode::decode;
use crate::encode::{Format, cap_megapixels, encode_to_target};
use crate::inspect::Kind;
use crate::meta::{MetaMode, inject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Output beside the original in `_compressed/`.
    Copy,
    /// Output takes the original's place; original moved to `_archive/`.
    Archive,
    /// Output takes the original's place; original deleted.
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    /// None = same family (RAW/TIFF/BMP/HEIC → JPEG, GIF → PNG, others keep their format).
    pub format: Option<Format>,
    pub quality: u8,
    /// 0 = no cap.
    pub max_mp: f64,
    /// 0 = no target.
    pub target_bytes: u64,
    pub meta: MetaMode,
    pub mode: Mode,
    pub recursive: bool,
    /// Skip files already under this size.
    pub min_bytes: u64,
    /// RAW with an .xmp sidecar was edited in a RAW editor — leave it alone.
    pub skip_sidecar: bool,
    /// Discard the result unless it is at least 10% smaller than the source.
    pub only_if_smaller: bool,
    /// 0 = all cores.
    pub workers: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format: None,
            quality: 82,
            max_mp: 8.0,
            target_bytes: 0,
            meta: MetaMode::Keep,
            mode: Mode::Copy,
            recursive: true,
            min_bytes: 300 * 1024,
            skip_sidecar: true,
            only_if_smaller: true,
            workers: 0,
        }
    }
}

impl Options {
    pub fn output_format(&self, kind: Kind) -> Format {
        self.format.unwrap_or(match kind {
            Kind::Png | Kind::Gif => Format::Png,
            Kind::Webp => Format::Webp,
            Kind::Avif => Format::Avif,
            Kind::Jxl => Format::Jxl,
            _ => Format::Jpeg,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub items: Vec<Item>,
    pub skipped: Vec<(PathBuf, String)>,
}

impl Plan {
    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }
}

/// Walk the given files/folders and decide what gets processed.
pub fn plan(paths: &[PathBuf], o: &Options) -> Plan {
    let mut p = Plan::default();
    for root in paths {
        let depth = if o.recursive || root.is_file() { usize::MAX } else { 1 };
        let walk = walkdir::WalkDir::new(root).max_depth(depth).into_iter().filter_entry(|e| {
            !e.file_type().is_dir() || !matches!(e.file_name().to_str(), Some("_archive" | "_compressed"))
        });
        for e in walk.filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
            let path = e.path().to_path_buf();
            let Some(kind) = Kind::from_path(&path) else { continue };
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if let Some(why) = skip_reason(&path, kind, size, o) {
                p.skipped.push((path, why));
            } else {
                p.items.push(Item { path, kind, size });
            }
        }
    }
    p.items.sort_by_key(|i| std::cmp::Reverse(i.size));
    p
}

fn skip_reason(path: &Path, kind: Kind, size: u64, o: &Options) -> Option<String> {
    if !kind.supported() {
        return Some(format!("{} not supported yet", kind.label()));
    }
    if size < o.min_bytes {
        return Some("already small".into());
    }
    if o.skip_sidecar && kind == Kind::Raw && path.with_extension("xmp").exists() {
        return Some("RAW has an .xmp sidecar (edited)".into());
    }
    let out = output_path(path, o.output_format(kind), o.mode);
    if o.mode == Mode::Copy && out.exists() {
        return Some("copy already exists".into());
    }
    None
}

pub fn output_path(src: &Path, f: Format, mode: Mode) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let name = format!("{stem}.{}", f.ext());
    match mode {
        Mode::Copy => dir.join("_compressed").join(name),
        Mode::Archive | Mode::Replace => dir.join(name),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub path: PathBuf,
    pub out: Option<PathBuf>,
    pub kind: Kind,
    pub format: Format,
    pub before: u64,
    pub after: u64,
    pub quality: u8,
    pub width: u32,
    pub height: u32,
    pub via: &'static str,
    pub had_icc: bool,
    pub had_exif: bool,
    pub had_gps: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum Event<'a> {
    Started(&'a Item),
    Finished(&'a Outcome),
}

/// Process every planned item on a rayon pool; `on` is called from worker threads.
pub fn run(plan: &Plan, o: &Options, on: &(dyn Fn(Event) + Sync)) -> Vec<Outcome> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(o.workers).build().expect("thread pool");
    pool.install(|| {
        plan.items
            .par_iter()
            .map(|item| {
                on(Event::Started(item));
                let out = process(item, o).unwrap_or_else(|e| Outcome {
                    path: item.path.clone(),
                    out: None,
                    kind: item.kind,
                    format: o.output_format(item.kind),
                    before: item.size,
                    after: 0,
                    quality: 0,
                    width: 0,
                    height: 0,
                    via: "",
                    had_icc: false,
                    had_exif: false,
                    had_gps: false,
                    error: Some(format!("{e:#}")),
                });
                on(Event::Finished(&out));
                out
            })
            .collect()
    })
}

fn process(item: &Item, o: &Options) -> Result<Outcome> {
    let format = o.output_format(item.kind);
    let src = decode(&item.path)?;
    let (had_icc, had_exif, had_gps) = (src.meta.icc.is_some(), src.meta.exif.is_some(), src.meta.has_gps);
    let img = cap_megapixels(src.img, o.max_mp)?;
    let (w, h) = (img.width(), img.height());
    let icc = src.meta.icc.as_deref();
    let (bytes, quality) = encode_to_target(&img, format, o.quality, o.target_bytes, icc)?;
    drop(img);
    let bytes = inject(bytes, format, &src.meta, o.meta)?;
    let mut outcome = Outcome {
        path: item.path.clone(),
        out: None,
        kind: item.kind,
        format,
        before: item.size,
        after: bytes.len() as u64,
        quality,
        width: w,
        height: h,
        via: src.via,
        had_icc,
        had_exif,
        had_gps,
        error: None,
    };
    if o.only_if_smaller && (bytes.len() as u64) >= item.size * 9 / 10 {
        outcome.after = item.size;
        outcome.error = Some("no worthwhile saving".into());
        return Ok(outcome);
    }
    outcome.out = Some(commit(&item.path, output_path(&item.path, format, o.mode), &bytes, format, o.mode, (w, h))?);
    Ok(outcome)
}

/// Write beside the destination, verify the bytes decode, then rename into place.
fn commit(src: &Path, dst: PathBuf, bytes: &[u8], f: Format, mode: Mode, dims: (u32, u32)) -> Result<PathBuf> {
    let dir = dst.parent().ok_or_else(|| anyhow!("no parent"))?;
    fs::create_dir_all(dir)?;
    let tmp = dst.with_extension(format!("{}.part", f.ext()));
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    if let Err(e) = verify(&tmp, f, dims) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    let same_name = src == dst;
    match mode {
        Mode::Copy => {}
        Mode::Archive => {
            let arch = src.parent().unwrap_or(Path::new(".")).join("_archive");
            fs::create_dir_all(&arch)?;
            let target = arch.join(src.file_name().unwrap_or_default());
            if target.exists() {
                bail!("archive already has {}", target.display());
            }
            fs::rename(src, &target)?;
        }
        Mode::Replace if !same_name => fs::remove_file(src)?,
        Mode::Replace => {}
    }
    fs::rename(&tmp, &dst).with_context(|| format!("rename to {}", dst.display()))?;
    Ok(dst)
}

/// ponytail: JPEG/PNG/WebP are re-read for dimensions; AVIF/JXL get a container-signature check.
fn verify(path: &Path, f: Format, dims: (u32, u32)) -> Result<()> {
    let len = fs::metadata(path)?.len();
    if len == 0 {
        bail!("encoder produced nothing");
    }
    match f {
        Format::Jpeg | Format::Png | Format::Webp => {
            let got = image::ImageReader::open(path)?.with_guessed_format()?.into_dimensions()?;
            if got != dims {
                bail!("wrote {}x{}, expected {}x{}", got.0, got.1, dims.0, dims.1);
            }
        }
        Format::Avif => {
            let head = fs::read(path)?;
            if head.len() < 12 || &head[4..8] != b"ftyp" {
                bail!("not an ISOBMFF file");
            }
        }
        Format::Jxl => {
            let head = fs::read(path)?;
            let bare = head.starts_with(&[0xFF, 0x0A]);
            let boxed = head.starts_with(&[0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ']);
            if !bare && !boxed {
                bail!("not a JXL file");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode;
    use image::DynamicImage;

    #[test]
    fn copy_mode_writes_into_compressed_and_keeps_source() {
        let dir = std::env::temp_dir().join(format!("forge-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.png");
        let mut img = image::RgbImage::new(300, 200);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 7 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8]);
        }
        let png = encode(&DynamicImage::ImageRgb8(img), Format::Png, 100, None).unwrap();
        fs::write(&src, png).unwrap();
        let o = Options { format: Some(Format::Jpeg), min_bytes: 0, only_if_smaller: false, ..Options::default() };
        let p = plan(&[dir.clone()], &o);
        assert_eq!(p.items.len(), 1);
        let res = run(&p, &o, &|_| {});
        assert!(res[0].error.is_none(), "{:?}", res[0].error);
        assert!(dir.join("_compressed/a.jpg").exists());
        assert!(src.exists());
        assert_eq!(plan(&[dir.clone()], &o).skipped[0].1, "copy already exists");
        fs::remove_dir_all(&dir).unwrap();
    }
}
