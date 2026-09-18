//! Colour management. Formats that can embed the source ICC keep it untouched;
//! formats whose encoders here cannot (AVIF, JXL) get pixels converted to sRGB first.
use anyhow::{Result, anyhow};
use image::DynamicImage;
use moxcms::{ColorProfile, Layout, TransformOptions};

/// Cheap check: matrix-shaper profiles with sRGB primaries need no conversion.
/// ponytail: TRC not compared; a gamma-2.2 "sRGB" variant passes as sRGB, which is visually fine.
pub fn is_srgb(icc: &[u8]) -> bool {
    let Ok(p) = ColorProfile::new_from_slice(icc) else { return true };
    if !p.is_matrix_shaper() {
        return false;
    }
    let s = ColorProfile::new_srgb();
    let close = |a: moxcms::Xyzd, b: moxcms::Xyzd| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.z - b.z).abs() < 0.01;
    close(p.red_colorant, s.red_colorant) && close(p.green_colorant, s.green_colorant) && close(p.blue_colorant, s.blue_colorant)
}

/// Convert to sRGB in the image's own bit depth (8 or 16). Non-RGB layouts go through RGB8.
pub fn to_srgb(img: DynamicImage, icc: &[u8]) -> Result<DynamicImage> {
    if is_srgb(icc) {
        return Ok(img);
    }
    let src = ColorProfile::new_from_slice(icc).map_err(|e| anyhow!("icc: {e:?}"))?;
    let dst = ColorProfile::new_srgb();
    let opts = TransformOptions::default();
    Ok(match img {
        DynamicImage::ImageRgb16(im) => {
            let (w, h) = im.dimensions();
            let t = src.create_transform_16bit(Layout::Rgb, &dst, Layout::Rgb, opts).map_err(|e| anyhow!("cms: {e:?}"))?;
            let mut out = vec![0u16; im.as_raw().len()];
            t.transform(im.as_raw(), &mut out).map_err(|e| anyhow!("cms: {e:?}"))?;
            DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(w, h, out).ok_or_else(|| anyhow!("cms buffer"))?)
        }
        other => {
            let im = other.to_rgb8();
            let (w, h) = im.dimensions();
            let t = src.create_transform_8bit(Layout::Rgb, &dst, Layout::Rgb, opts).map_err(|e| anyhow!("cms: {e:?}"))?;
            let mut out = vec![0u8; im.as_raw().len()];
            t.transform(im.as_raw(), &mut out).map_err(|e| anyhow!("cms: {e:?}"))?;
            DynamicImage::ImageRgb8(image::ImageBuffer::from_raw(w, h, out).ok_or_else(|| anyhow!("cms buffer"))?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_profile_is_identity() {
        let icc = ColorProfile::new_srgb().encode().unwrap();
        assert!(is_srgb(&icc));
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        assert!(!is_srgb(&p3));
        let img = DynamicImage::new_rgb8(4, 4);
        let out = to_srgb(img, &p3).unwrap();
        assert_eq!(out.width(), 4);
    }
}
