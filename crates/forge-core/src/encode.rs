//! Resize cap + encoders. Quality 1-100 for all formats; AVIF/JXL map it internally.
use anyhow::{Result, anyhow};
use image::DynamicImage;

use crate::meta::Meta;

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
    /// Can the container carry the source ICC profile as-is with the encoders used here?
    pub fn carries_icc(self) -> bool {
        matches!(self, Format::Jpeg | Format::Png | Format::Webp)
    }
    /// Highest bit depth this path writes: PNG/JXL 16, AVIF 10, JPEG/WebP 8.
    pub fn max_bits(self) -> u8 {
        match self {
            Format::Png | Format::Jxl => 16,
            Format::Avif => 10,
            Format::Jpeg | Format::Webp => 8,
        }
    }
}

/// Cap to `max_mp` megapixels (0 = no cap), Lanczos3, keeping 8- or 16-bit depth.
pub fn cap_megapixels(img: DynamicImage, max_mp: f64) -> Result<DynamicImage> {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if max_mp <= 0.0 || w * h <= max_mp * 1e6 {
        return Ok(img);
    }
    let s = (max_mp * 1e6 / (w * h)).sqrt();
    resize(img, (w * s).round().max(1.0) as u32, (h * s).round().max(1.0) as u32)
}

pub fn resize(img: DynamicImage, nw: u32, nh: u32) -> Result<DynamicImage> {
    use fast_image_resize as fr;
    let opts = fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3));
    let deep = matches!(img, DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) | DynamicImage::ImageLuma16(_) | DynamicImage::ImageLumaA16(_));
    let (src, pt) = if deep {
        (DynamicImage::ImageRgb16(img.to_rgb16()), fr::PixelType::U16x3)
    } else {
        (DynamicImage::ImageRgb8(img.to_rgb8()), fr::PixelType::U8x3)
    };
    let mut dst = fr::images::Image::new(nw, nh, pt);
    fr::Resizer::new().resize(&src, &mut dst, &opts)?;
    let buf = dst.into_vec();
    Ok(if deep {
        let px: Vec<u16> = buf.as_chunks::<2>().0.iter().map(|c| u16::from_ne_bytes(*c)).collect();
        DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(nw, nh, px).ok_or_else(|| anyhow!("resize buffer"))?)
    } else {
        DynamicImage::ImageRgb8(image::RgbImage::from_raw(nw, nh, buf).ok_or_else(|| anyhow!("resize buffer"))?)
    })
}

/// Bits per sample the encoder will actually write for this image.
pub fn output_bits(img: &DynamicImage, f: Format) -> u8 {
    let deep = matches!(img, DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) | DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_));
    if deep { f.max_bits() } else { 8 }
}

/// `meta` is what gets embedded where the container supports it at encode time (AVIF, JXL);
/// JPEG/PNG/WebP get theirs injected afterwards by `meta::inject`.
pub fn encode(img: &DynamicImage, f: Format, quality: u8, meta: &Meta) -> Result<Vec<u8>> {
    let q = quality.clamp(1, 100);
    let (w, h) = (img.width() as usize, img.height() as usize);
    let bits = output_bits(img, f);
    match f {
        Format::Jpeg => {
            let rgb = img.to_rgb8();
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
            let img = if bits == 16 { DynamicImage::ImageRgb16(img.to_rgb16()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
            img.write_with_encoder(enc)?;
            Ok(out.into_inner())
        }
        Format::Webp => {
            let rgb = img.to_rgb8();
            Ok(webp::Encoder::from_rgb(rgb.as_raw(), w as u32, h as u32).encode(q as f32).to_vec())
        }
        Format::Avif => {
            let mut enc = ravif::Encoder::new().with_quality(q as f32).with_speed(6);
            if let Some(e) = &meta.exif {
                enc = enc.with_exif(e.clone());
            }
            let res = if bits == 10 {
                // True 10-bit from 16-bit sources: GBR identity planes, no chroma matrix loss.
                let rgb = img.to_rgb16();
                let planes = rgb.as_raw().as_chunks::<3>().0.iter().map(|p| [p[1] >> 6, p[2] >> 6, p[0] >> 6]);
                enc.with_bit_depth(ravif::BitDepth::Ten).with_internal_color_model(ravif::ColorModel::RGB).encode_raw_planes_10_bit(
                    w,
                    h,
                    planes,
                    None::<[u16; 0]>,
                    ravif::PixelRange::Full,
                    ravif::MatrixCoefficients::Identity,
                )?
            } else {
                let rgb = img.to_rgb8();
                let px: &[rgb::RGB8] = rgb::bytemuck::cast_slice(rgb.as_raw());
                enc.encode_rgb(ravif::Img::new(px, w, h))?
            };
            Ok(res.avif_file)
        }
        #[cfg(feature = "jxl")]
        Format::Jxl => {
            use jpegxl_rs::encode::{EncoderSpeed, Metadata};
            let mut enc = jpegxl_rs::encoder_builder().quality(q as f32).speed(EncoderSpeed::Squirrel).use_container(true).build()?;
            if let Some(e) = &meta.exif {
                let mut boxed = vec![0u8; 4];
                boxed.extend_from_slice(e);
                enc.add_metadata(&Metadata::Exif(&boxed), true)?;
            }
            if let Some(x) = &meta.xmp {
                enc.add_metadata(&Metadata::Xmp(x), true)?;
            }
            let data = if bits == 16 {
                let rgb = img.to_rgb16();
                enc.encode::<u16, u8>(rgb.as_raw(), w as u32, h as u32)?.data
            } else {
                let rgb = img.to_rgb8();
                enc.encode::<u8, u8>(rgb.as_raw(), w as u32, h as u32)?.data
            };
            Ok(data)
        }
        #[cfg(not(feature = "jxl"))]
        Format::Jxl => Err(anyhow!("built without jxl feature")),
    }
}

/// Encode toward a byte target: smooth frames land far under, busy ones far over,
/// so nudge quality a few times (same heuristic Husky Drop tuned on real archives).
pub fn encode_to_target(img: &DynamicImage, f: Format, start_q: u8, target: u64, meta: &Meta) -> Result<(Vec<u8>, u8)> {
    let mut q = start_q;
    let mut bytes = encode(img, f, q, meta)?;
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
        bytes = encode(img, f, q, meta)?;
    }
    Ok((bytes, q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_scales_down_keeps_aspect_and_depth() {
        let out = cap_megapixels(DynamicImage::new_rgb8(4000, 2000), 2.0).unwrap();
        assert_eq!((out.width(), out.height()), (2000, 1000));
        let deep = cap_megapixels(DynamicImage::new_rgb16(4000, 2000), 2.0).unwrap();
        assert!(matches!(deep, DynamicImage::ImageRgb16(_)));
        let small = cap_megapixels(DynamicImage::new_rgb8(100, 100), 2.0).unwrap();
        assert_eq!(small.width(), 100);
    }

    #[test]
    fn jpeg_and_png16_roundtrip() {
        let m = Meta::default();
        let bytes = encode(&DynamicImage::new_rgb8(64, 48), Format::Jpeg, 80, &m).unwrap();
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48));
        let bytes = encode(&DynamicImage::new_rgb16(8, 8), Format::Png, 100, &m).unwrap();
        assert!(matches!(image::load_from_memory(&bytes).unwrap(), DynamicImage::ImageRgb16(_)));
    }
}
