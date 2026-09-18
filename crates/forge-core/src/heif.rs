//! HEIC / AVIF decode through libheif (feature `heic`). Static on Windows (vcpkg), bundled in the
//! macOS .app and the Linux AppImage; end users never install codecs.
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use image::DynamicImage;
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

use crate::meta::Meta;

pub fn decode(path: &Path) -> Result<(DynamicImage, Meta)> {
    let ctx = HeifContext::read_from_file(path.to_str().context("non-UTF-8 path")?).map_err(|e| anyhow!("libheif: {e}"))?;
    let handle = ctx.primary_image_handle().map_err(|e| anyhow!("libheif: {e}"))?;
    let deep = handle.luma_bits_per_pixel() > 8;
    let chroma = if deep { RgbChroma::HdrRgbLe } else { RgbChroma::Rgb };
    let img = LibHeif::new().decode(&handle, ColorSpace::Rgb(chroma), None).map_err(|e| anyhow!("libheif decode: {e}"))?;
    let planes = img.planes();
    let p = planes.interleaved.context("libheif: no interleaved plane")?;
    let (w, h) = (p.width, p.height);
    let img = if deep {
        let bpp = 6; // 3 × u16
        let mut out = Vec::with_capacity(w as usize * h as usize * 3);
        for row in p.data.chunks(p.stride).take(h as usize) {
            out.extend(row[..w as usize * bpp].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)));
        }
        // libheif hands back 10/12-bit values in the low bits; scale to full 16-bit range.
        let shift = 16 - handle.luma_bits_per_pixel();
        out.iter_mut().for_each(|v| *v <<= shift);
        DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(w, h, out).context("heif buffer")?)
    } else {
        let mut out = Vec::with_capacity(w as usize * h as usize * 3);
        for row in p.data.chunks(p.stride).take(h as usize) {
            out.extend_from_slice(&row[..w as usize * 3]);
        }
        DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, out).context("heif buffer")?)
    };
    let icc = handle.color_profile_raw().map(|p| p.data).filter(|d| !d.is_empty());
    // EXIF blocks carry a 4-byte offset to the TIFF header first.
    let exif = handle.all_metadata().into_iter().find(|m| m.item_type == "Exif").and_then(|m| {
        let off = u32::from_be_bytes(m.raw_data.get(0..4)?.try_into().ok()?) as usize;
        m.raw_data.get(4 + off..).map(|s| s.to_vec())
    });
    let xmp = handle.all_metadata().into_iter().find(|m| m.item_type == "mime").map(|m| m.raw_data);
    Ok((img, Meta { icc, exif, xmp, has_gps: false }))
}
