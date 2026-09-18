//! Geometry (cap / fit / fill / pad) and .cube LUTs, depth-preserving.
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::encode::{cap_megapixels, resize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Resize {
    None,
    /// Shrink until the pixel count is under `mp` megapixels.
    Cap { mp: f64 },
    /// Shrink to fit inside w×h, aspect kept.
    Fit { w: u32, h: u32 },
    /// Scale to cover w×h then centre-crop to exactly w×h.
    Fill { w: u32, h: u32 },
    /// Fit inside w×h, then pad to exactly w×h with `rgb`.
    Pad { w: u32, h: u32, rgb: [u8; 3] },
}

impl Default for Resize {
    fn default() -> Self {
        Resize::Cap { mp: 8.0 }
    }
}

impl Resize {
    pub fn describe(&self) -> Option<String> {
        Some(match self {
            Resize::None => return None,
            Resize::Cap { mp } => format!("Dimensions capped at {} MP", if mp.fract() == 0.0 { format!("{}", *mp as u64) } else { format!("{mp:.1}") }),
            Resize::Fit { w, h } => format!("Fit within {w}×{h}"),
            Resize::Fill { w, h } => format!("Filled and cropped to {w}×{h}"),
            Resize::Pad { w, h, .. } => format!("Fit and padded to {w}×{h}"),
        })
    }

    pub fn apply(&self, img: DynamicImage) -> Result<DynamicImage> {
        let (sw, sh) = (img.width() as f64, img.height() as f64);
        match *self {
            Resize::None => Ok(img),
            Resize::Cap { mp } => cap_megapixels(img, mp),
            Resize::Fit { w, h } => {
                let s = (w as f64 / sw).min(h as f64 / sh).min(1.0);
                if s >= 1.0 { Ok(img) } else { resize(img, (sw * s).round() as u32, (sh * s).round() as u32) }
            }
            Resize::Fill { w, h } => {
                let s = (w as f64 / sw).max(h as f64 / sh);
                let (rw, rh) = ((sw * s).round().max(w as f64) as u32, (sh * s).round().max(h as f64) as u32);
                let scaled = if (rw, rh) == (img.width(), img.height()) { img } else { resize(img, rw, rh)? };
                Ok(scaled.crop_imm((rw - w) / 2, (rh - h) / 2, w, h))
            }
            Resize::Pad { w, h, rgb } => {
                let inner = Resize::Fit { w, h }.apply(img)?;
                let (iw, ih) = (inner.width(), inner.height());
                if (iw, ih) == (w, h) {
                    return Ok(inner);
                }
                let (x, y) = ((w - iw) as i64 / 2, (h - ih) as i64 / 2);
                Ok(match inner {
                    DynamicImage::ImageRgb16(im) => {
                        let px = image::Rgb(rgb.map(|c| c as u16 * 257));
                        let mut canvas = image::ImageBuffer::from_pixel(w, h, px);
                        image::imageops::overlay(&mut canvas, &im, x, y);
                        DynamicImage::ImageRgb16(canvas)
                    }
                    other => {
                        let mut canvas = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
                        image::imageops::overlay(&mut canvas, &other.to_rgb8(), x, y);
                        DynamicImage::ImageRgb8(canvas)
                    }
                })
            }
        }
    }
}

/// A 3D (or 1D) LUT parsed from an Adobe/Resolve `.cube` file.
#[derive(Debug, Clone)]
pub struct Lut {
    pub size: usize,
    pub three_d: bool,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// RGB triples, red fastest for 3D.
    pub table: Vec<[f32; 3]>,
}

impl Lut {
    pub fn load(path: &Path) -> Result<Lut> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let mut lut = Lut { size: 0, three_d: true, min: [0.0; 3], max: [1.0; 3], table: Vec::new() };
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let mut it = line.split_whitespace();
            match it.next() {
                Some("TITLE") => {}
                Some("LUT_3D_SIZE") => {
                    lut.size = it.next().and_then(|s| s.parse().ok()).context("LUT_3D_SIZE")?;
                    lut.three_d = true;
                }
                Some("LUT_1D_SIZE") => {
                    lut.size = it.next().and_then(|s| s.parse().ok()).context("LUT_1D_SIZE")?;
                    lut.three_d = false;
                }
                Some("DOMAIN_MIN") => lut.min = triple(it).context("DOMAIN_MIN")?,
                Some("DOMAIN_MAX") => lut.max = triple(it).context("DOMAIN_MAX")?,
                Some(first) => {
                    let r: f32 = first.parse().with_context(|| format!("bad LUT line: {line}"))?;
                    let [g, b] = [it.next(), it.next()].map(|s| s.and_then(|s| s.parse::<f32>().ok()));
                    lut.table.push([r, g.context("LUT row")?, b.context("LUT row")?]);
                }
                None => {}
            }
        }
        let want = if lut.three_d { lut.size.pow(3) } else { lut.size };
        if lut.size < 2 || lut.table.len() != want {
            bail!("LUT has {} entries, expected {want}", lut.table.len());
        }
        Ok(lut)
    }

    fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let norm = |i: usize| ((rgb[i] - self.min[i]) / (self.max[i] - self.min[i])).clamp(0.0, 1.0) * (n - 1) as f32;
        if !self.three_d {
            let mut out = [0.0; 3];
            for (c, o) in out.iter_mut().enumerate() {
                let x = norm(c);
                let (i, t) = (x.floor() as usize, x.fract());
                let j = (i + 1).min(n - 1);
                *o = self.table[i][c] * (1.0 - t) + self.table[j][c] * t;
            }
            return out;
        }
        let p = [norm(0), norm(1), norm(2)];
        let i0 = p.map(|x| x.floor() as usize);
        let i1 = i0.map(|i| (i + 1).min(n - 1));
        let t = [p[0].fract(), p[1].fract(), p[2].fract()];
        let at = |r: usize, g: usize, b: usize| self.table[r + g * n + b * n * n];
        let mut out = [0.0; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let c00 = lerp(at(i0[0], i0[1], i0[2])[c], at(i1[0], i0[1], i0[2])[c], t[0]);
            let c10 = lerp(at(i0[0], i1[1], i0[2])[c], at(i1[0], i1[1], i0[2])[c], t[0]);
            let c01 = lerp(at(i0[0], i0[1], i1[2])[c], at(i1[0], i0[1], i1[2])[c], t[0]);
            let c11 = lerp(at(i0[0], i1[1], i1[2])[c], at(i1[0], i1[1], i1[2])[c], t[0]);
            *o = lerp(lerp(c00, c10, t[1]), lerp(c01, c11, t[1]), t[2]);
        }
        out
    }

    pub fn apply(&self, img: DynamicImage) -> DynamicImage {
        match img {
            DynamicImage::ImageRgb16(mut im) => {
                for p in im.pixels_mut() {
                    let o = self.sample(p.0.map(|v| v as f32 / 65535.0));
                    p.0 = o.map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
                }
                DynamicImage::ImageRgb16(im)
            }
            other => {
                let mut im = other.to_rgb8();
                for p in im.pixels_mut() {
                    let o = self.sample(p.0.map(|v| v as f32 / 255.0));
                    p.0 = o.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                }
                DynamicImage::ImageRgb8(im)
            }
        }
    }
}

fn triple<'a>(mut it: impl Iterator<Item = &'a str>) -> Option<[f32; 3]> {
    Some([it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_fill_pad_geometry() {
        let img = DynamicImage::new_rgb8(4000, 2000);
        let fit = Resize::Fit { w: 1000, h: 1000 }.apply(img.clone()).unwrap();
        assert_eq!((fit.width(), fit.height()), (1000, 500));
        let fill = Resize::Fill { w: 1000, h: 1000 }.apply(img.clone()).unwrap();
        assert_eq!((fill.width(), fill.height()), (1000, 1000));
        let pad = Resize::Pad { w: 1000, h: 1000, rgb: [255, 0, 0] }.apply(img).unwrap();
        assert_eq!((pad.width(), pad.height()), (1000, 1000));
        assert_eq!(pad.to_rgb8().get_pixel(0, 0).0, [255, 0, 0]);
    }

    #[test]
    fn identity_cube_lut_is_identity() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("forge-id-{}.cube", std::process::id()));
        let mut s = String::from("TITLE \"id\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    s += &format!("{r} {g} {b}\n");
                }
            }
        }
        std::fs::write(&p, s).unwrap();
        let lut = Lut::load(&p).unwrap();
        let mut im = image::RgbImage::new(1, 1);
        im.put_pixel(0, 0, image::Rgb([10, 128, 250]));
        let out = lut.apply(DynamicImage::ImageRgb8(im)).to_rgb8();
        assert_eq!(out.get_pixel(0, 0).0, [10, 128, 250]);
        std::fs::remove_file(&p).unwrap();
    }
}
