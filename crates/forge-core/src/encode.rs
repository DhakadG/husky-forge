//! Resize cap + encoders. Quality 1-100 for all formats; AVIF/JXL map it internally.
use anyhow::{Result, anyhow};
use image::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Avif => "avif",
            Format::Jxl => "jxl",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
            Format::Avif => "AVIF",
            Format::Jxl => "JPEG XL",
        }
    }
    pub fn lossy(self) -> bool {
        self != Format::Png
    }
}

/// Cap to `max_mp` megapixels (0 = no cap), Lanczos3. Untouched if under the cap.
pub fn cap_megapixels(img: DynamicImage, max_mp: f64) -> Result<DynamicImage> {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if max_mp <= 0.0 || w * h <= max_mp * 1e6 {
        return Ok(img);
    }
    let s = (max_mp * 1e6 / (w * h)).sqrt();
    let (nw, nh) = ((w * s).round().max(1.0) as u32, (h * s).round().max(1.0) as u32);
    let src = DynamicImage::ImageRgb8(img.to_rgb8());
    let mut dst = fast_image_resize::images::Image::new(nw, nh, fast_image_resize::PixelType::U8x3);
    let opts = fast_image_resize::ResizeOptions::new()
        .resize_alg(fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3));
    fast_image_resize::Resizer::new().resize(&src, &mut dst, &opts)?;
    let out = image::RgbImage::from_raw(nw, nh, dst.into_vec()).ok_or_else(|| anyhow!("resize buffer"))?;
    Ok(DynamicImage::ImageRgb8(out))
}

/// ponytail: everything encodes from 8-bit RGB for now; 16-bit / HDR paths for AVIF and JXL are phase 2.
pub fn encode(img: &DynamicImage, f: Format, quality: u8, icc: Option<&[u8]>) -> Result<Vec<u8>> {
    let q = quality.clamp(1, 100);
    let (w, h) = (img.width() as usize, img.height() as usize);
    let rgb = img.to_rgb8();
    match f {
        Format::Jpeg => {
            let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
            c.set_size(w, h);
            c.set_quality(q as f32);
            c.set_progressive_mode();
            c.set_optimize_scans(true);
            if q >= 90 {
                c.set_chroma_sampling_pixel_sizes((1, 1), (1, 1));
            }
            let mut comp = c.start_compress(Vec::new())?;
            comp.write_scanlines(rgb.as_raw())?;
            Ok(comp.finish()?)
        }
        Format::Png => {
            let mut out = std::io::Cursor::new(Vec::new());
            let enc = image::codecs::png::PngEncoder::new_with_quality(
                &mut out,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            );
            img.write_with_encoder(enc)?;
            Ok(out.into_inner())
        }
        Format::Webp => Ok(webp::Encoder::from_rgb(rgb.as_raw(), w as u32, h as u32).encode(q as f32).to_vec()),
        Format::Avif => {
            let px: &[rgb::RGB8] = rgb::bytemuck::cast_slice(rgb.as_raw());
            let img = ravif::Img::new(px, w, h);
            let res = ravif::Encoder::new().with_quality(q as f32).with_speed(6).encode_rgb(img)?;
            let _ = icc; // ravif has no ICC hook; sRGB assumed. phase 2: convert via moxcms when profile != sRGB
            Ok(res.avif_file)
        }
        #[cfg(feature = "jxl")]
        Format::Jxl => {
            use jpegxl_rs::encode::{EncoderResult, EncoderSpeed};
            let mut enc = jpegxl_rs::encoder_builder()
                .quality(q as f32)
                .speed(EncoderSpeed::Squirrel)
                .use_container(true)
                .build()?;
            let res: EncoderResult<u8> = enc.encode(rgb.as_raw(), w as u32, h as u32)?;
            let _ = icc;
            Ok(res.data)
        }
        #[cfg(not(feature = "jxl"))]
        Format::Jxl => Err(anyhow!("built without jxl feature")),
    }
}

/// Encode toward a byte target: smooth frames land far under, busy ones far over,
/// so nudge quality a few times (same heuristic Husky Drop tuned on real archives).
pub fn encode_to_target(img: &DynamicImage, f: Format, start_q: u8, target: u64, icc: Option<&[u8]>) -> Result<(Vec<u8>, u8)> {
    let mut q = start_q;
    let mut bytes = encode(img, f, q, icc)?;
    if !f.lossy() || target == 0 {
        return Ok((bytes, q));
    }
    for _ in 0..4 {
        let ratio = bytes.len() as f64 / target as f64;
        if (0.7..1.3).contains(&ratio) {
            break;
        }
        let step: i16 = if ratio < 0.7 {
            if ratio < 0.35 { 8 } else { 4 }
        } else if ratio > 2.0 {
            -10
        } else {
            -5
        };
        let next = (q as i16 + step).clamp(50, 95) as u8;
        if next == q {
            break;
        }
        q = next;
        bytes = encode(img, f, q, icc)?;
    }
    Ok((bytes, q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_scales_down_keeps_aspect() {
        let out = cap_megapixels(DynamicImage::new_rgb8(4000, 2000), 2.0).unwrap();
        assert_eq!((out.width(), out.height()), (2000, 1000));
        let small = cap_megapixels(DynamicImage::new_rgb8(100, 100), 2.0).unwrap();
        assert_eq!(small.width(), 100);
    }

    #[test]
    fn jpeg_roundtrip() {
        let bytes = encode(&DynamicImage::new_rgb8(64, 48), Format::Jpeg, 80, None).unwrap();
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48));
    }
}
