//! Source file → upright DynamicImage + carried metadata.
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::inspect::Kind;
use crate::meta::{Meta, has_gps};

pub struct Source {
    pub img: DynamicImage,
    pub meta: Meta,
    pub kind: Kind,
    /// Which decoder produced the pixels.
    pub via: &'static str,
}

pub fn decode(path: &Path) -> Result<Source> {
    let kind = Kind::from_path(path).context("unknown format")?;
    if !kind.supported() {
        bail!("{} decode not supported yet", kind.label());
    }
    let (img, mut meta, via) = match kind {
        Kind::Raw => (develop_raw(path)?, Meta::default(), "rawler"),
        Kind::Jxl => {
            let dec = jxl_oxide::integration::JxlDecoder::new(BufReader::new(File::open(path)?))?;
            let (img, meta) = from_decoder(dec)?;
            (img, meta, "jxl-oxide")
        }
        _ => {
            let dec = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
            let (img, meta) = from_decoder(dec)?;
            (img, meta, "image")
        }
    };
    // RAW containers are TIFF/ISOBMFF; pull EXIF from the original file itself.
    if kind == Kind::Raw {
        meta.exif = exif_from_container(path);
    }
    if kind == Kind::Jpeg {
        meta.xmp = xmp_from_jpeg(path);
    }
    meta.has_gps = meta.exif.as_deref().map(has_gps).unwrap_or(false);
    Ok(Source { img, meta, kind, via })
}

fn from_decoder<D: ImageDecoder>(mut dec: D) -> Result<(DynamicImage, Meta)> {
    let icc = dec.icc_profile().ok().flatten();
    let exif = dec.exif_metadata().ok().flatten();
    let orientation = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec)?;
    img.apply_orientation(orientation);
    Ok((img, Meta { icc, exif, xmp: None, has_gps: false }))
}

fn develop_raw(path: &Path) -> Result<DynamicImage> {
    let raw = rawler::decode_file(path).map_err(|e| anyhow::anyhow!("rawler: {e}"))?;
    let dev = rawler::imgop::develop::RawDevelop::default();
    let inter = dev.develop_intermediate(&raw).map_err(|e| anyhow::anyhow!("develop: {e}"))?;
    let img = inter.to_dynamic_image().context("rawler produced no image")?;
    Ok(apply_raw_orientation(img, raw.orientation))
}

fn apply_raw_orientation(img: DynamicImage, o: rawler::Orientation) -> DynamicImage {
    use rawler::Orientation as O;
    match o {
        O::Normal | O::Unknown => img,
        O::HorizontalFlip => img.fliph(),
        O::Rotate180 => img.rotate180(),
        O::VerticalFlip => img.flipv(),
        O::Transpose => img.rotate90().fliph(),
        O::Rotate90 => img.rotate90(),
        O::Transverse => img.rotate270().fliph(),
        O::Rotate270 => img.rotate270(),
    }
}

fn exif_from_container(path: &Path) -> Option<Vec<u8>> {
    let mut r = BufReader::new(File::open(path).ok()?);
    let e = exif::Reader::new().read_from_container(&mut r).ok()?;
    Some(e.buf().to_vec())
}

fn xmp_from_jpeg(path: &Path) -> Option<Vec<u8>> {
    const HDR: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    let j = img_parts::jpeg::Jpeg::from_bytes(std::fs::read(path).ok()?.into()).ok()?;
    j.segments()
        .iter()
        .filter(|s| s.marker() == img_parts::jpeg::markers::APP1)
        .find(|s| s.contents().starts_with(HDR))
        .map(|s| s.contents()[HDR.len()..].to_vec())
}
