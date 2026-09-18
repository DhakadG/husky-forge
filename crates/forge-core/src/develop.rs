//! Default "look" for developed RAW files. rawler hands back a technically correct but flat
//! rendering (no exposure, no tone curve); cameras ship a punchier JPEG. This applies
//! histogram-based levels plus a gentle S-curve so results sit next to the camera preview.
//! ponytail: works in the output's gamma space on a sampled histogram; a linear-light basecurve
//! with highlight reconstruction is the upgrade path.
use image::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawLook {
    /// rawler output as-is.
    Flat,
    /// Auto levels + mild contrast (default).
    #[default]
    Auto,
}

/// Black/white points from luminance percentiles of a subsampled image, then a soft S-curve.
pub fn apply(img: DynamicImage, look: RawLook) -> DynamicImage {
    if look == RawLook::Flat {
        return img;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let step = ((w * h) / 250_000).max(1);
    match img {
        DynamicImage::ImageRgb16(mut im) => {
            let lum: Vec<u16> = im.as_raw().as_chunks::<3>().0.iter().step_by(step).map(|p| ((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8) as u16).collect();
            let (lo, hi) = percentiles(lum, 65535);
            let lut: Vec<u16> = (0..=65535u32).map(|v| (curve((v as f32 - lo) / (hi - lo)) * 65535.0 + 0.5) as u16).collect();
            im.as_mut().iter_mut().for_each(|v| *v = lut[*v as usize]);
            DynamicImage::ImageRgb16(im)
        }
        other => {
            let mut im = other.to_rgb8();
            let lum: Vec<u16> = im.as_raw().as_chunks::<3>().0.iter().step_by(step).map(|p| ((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8) as u16).collect();
            let (lo, hi) = percentiles(lum, 255);
            let lut: Vec<u8> = (0..=255u32).map(|v| (curve((v as f32 - lo) / (hi - lo)) * 255.0 + 0.5) as u8).collect();
            im.as_mut().iter_mut().for_each(|v| *v = lut[*v as usize]);
            DynamicImage::ImageRgb8(im)
        }
    }
}

/// 0.2 % black point, 99.7 % white point, never stretching more than the histogram supports.
fn percentiles(mut lum: Vec<u16>, max: u32) -> (f32, f32) {
    if lum.is_empty() {
        return (0.0, max as f32);
    }
    lum.sort_unstable();
    let at = |q: f64| lum[((lum.len() - 1) as f64 * q) as usize] as f32;
    let (lo, hi) = (at(0.002), at(0.997));
    // Keep at least half the range so a genuinely flat scene is not blown out.
    let hi = hi.max(lo + max as f32 * 0.5);
    (lo, hi.min(max as f32))
}

/// Normalised input → gentle S-curve (contrast ≈ 1.15 around mid-grey), clamped.
fn curve(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let s = x - 0.5;
    (0.5 + s * (1.15 - 0.6 * s * s)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretches_dark_flat_frame_and_keeps_order() {
        let mut im = image::RgbImage::new(64, 64);
        for (x, _, p) in im.enumerate_pixels_mut() {
            let v = 20 + (x as u8) * 2; // 20..146: dark and flat
            *p = image::Rgb([v, v, v]);
        }
        let out = apply(DynamicImage::ImageRgb8(im), RawLook::Auto).to_rgb8();
        let (a, b) = (out.get_pixel(0, 0).0[0], out.get_pixel(63, 0).0[0]);
        assert!(a < 8, "black point lifted: {a}");
        assert!(b > 200, "white point not stretched: {b}");
        assert!(curve(0.5) > 0.49 && curve(0.5) < 0.51);
    }
}
