//! The Impact card: what changed and why, from a plan (estimate) or from real outcomes.
use std::collections::BTreeMap;

use crate::encode::Format;
use crate::inspect::Kind;
use crate::job::{Options, Outcome, Plan};
use crate::meta::MetaMode;
use crate::transform::Resize;

/// Bytes per output pixel at quality 82, measured on real archives by Husky Drop.
fn bpp(f: Format) -> f64 {
    match f {
        Format::Jpeg => 0.12,
        Format::Webp => 0.09,
        Format::Avif => 0.06,
        Format::Jxl => 0.07,
        Format::Png => 1.2,
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Impact {
    pub before: u64,
    pub after: u64,
    pub estimate: bool,
    pub files: usize,
    pub failed: usize,
    /// (source kind, output format) → count
    pub conversions: BTreeMap<(Kind, Format), usize>,
    pub gps_files: usize,
    pub icc_files: usize,
    pub exif_files: usize,
    pub icc_converted: usize,
    /// Distinct output bit depths seen.
    pub bits: std::collections::BTreeSet<u8>,
    pub resize: Resize,
    pub lut: bool,
    pub quality: u8,
    pub target_bytes: u64,
    pub meta: MetaMode,
}

impl Impact {
    /// Before any work: sizes estimated from pixel-count heuristics.
    pub fn estimate(plan: &Plan, o: &Options) -> Impact {
        let mut i = Impact { estimate: true, ..Self::base(o) };
        for it in &plan.items {
            let f = o.output_format(it.kind);
            // Unknown dimensions until decode; assume the cap (or 12 MP) like Husky Drop does.
            let px = match o.resize {
                Resize::Cap { mp } => mp * 1e6,
                Resize::Fit { w, h } | Resize::Fill { w, h } | Resize::Pad { w, h, .. } => (w * h) as f64,
                Resize::None => 12e6,
            };
            let guess = (px * bpp(f) * (o.quality as f64 / 82.0)) as u64;
            let guess = if o.target_bytes > 0 && f.lossy() { guess.min(o.target_bytes) } else { guess };
            i.before += it.size;
            i.after += guess.min(it.size * 9 / 10);
            i.files += 1;
            *i.conversions.entry((it.kind, f)).or_default() += 1;
        }
        i
    }

    pub fn from_outcomes(outs: &[Outcome], o: &Options) -> Impact {
        let mut i = Self::base(o);
        for r in outs {
            i.before += r.before;
            i.after += if r.out.is_some() { r.after } else { r.before };
            i.files += 1;
            if r.error.is_some() {
                i.failed += 1;
                continue;
            }
            *i.conversions.entry((r.kind, r.format)).or_default() += 1;
            i.gps_files += r.had_gps as usize;
            i.icc_files += r.had_icc as usize;
            i.exif_files += r.had_exif as usize;
            i.icc_converted += r.icc_converted as usize;
            i.bits.insert(r.bits);
        }
        i
    }

    fn base(o: &Options) -> Impact {
        Impact { resize: o.resize, lut: o.lut.is_some(), quality: o.quality, target_bytes: o.target_bytes, meta: o.meta, ..Default::default() }
    }

    pub fn percent_smaller(&self) -> u32 {
        (self.after * 100).checked_div(self.before).map_or(0, |p| 100u64.saturating_sub(p) as u32)
    }

    /// The card, one line per row.
    pub fn lines(&self) -> Vec<String> {
        let tilde = if self.estimate { "~" } else { "" };
        let mut v = vec![
            format!("{} → {tilde}{}", human(self.before), human(self.after)),
            format!("{}% smaller", self.percent_smaller()),
        ];
        for ((k, f), n) in &self.conversions {
            v.push(format!("{n} {} → {n} {}", k.label(), f.label()));
        }
        let bits: Vec<String> = self.bits.iter().map(|b| format!("{b}-bit")).collect();
        v.push(if bits.is_empty() { "8-bit SDR".into() } else { format!("{} SDR", bits.join(" / ")) });
        if !self.estimate {
            let icc = if self.icc_files == 0 {
                "no ICC in sources".to_string()
            } else if self.icc_converted > 0 {
                format!("ICC folded into sRGB ({} files)", self.icc_converted)
            } else {
                "ICC preserved".to_string()
            };
            match self.meta {
                MetaMode::Strip => v.push(format!("{icc}, EXIF removed")),
                _ => {
                    v.push(icc);
                    v.push(if self.exif_files > 0 { "EXIF preserved".into() } else { "no EXIF in sources".into() });
                }
            }
            if self.gps_files > 0 {
                v.push(match self.meta {
                    MetaMode::Keep => format!("GPS preserved ({} files)", self.gps_files),
                    _ => format!("GPS removed ({} files)", self.gps_files),
                });
            }
        } else {
            v.push(match self.meta {
                MetaMode::Keep => "ICC, EXIF, GPS preserved".into(),
                MetaMode::StripGps => "ICC, EXIF preserved · GPS removed".into(),
                MetaMode::Strip => "ICC preserved · EXIF removed".into(),
            });
        }
        if let Some(d) = self.resize.describe() {
            v.push(d);
        }
        if self.lut {
            v.push("LUT applied".into());
        }
        if self.target_bytes > 0 {
            v.push(format!("Quality individually tuned to {} target", human(self.target_bytes)));
        } else {
            v.push(format!("Quality {}", self.quality));
        }
        if self.failed > 0 {
            v.push(format!("{} files skipped or failed", self.failed));
        }
        v
    }
}


pub fn human(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else if v >= 100.0 { format!("{v:.0} {}", U[i]) } else { format!("{v:.1} {}", U[i]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(2_621_440), "2.5 MB");
        assert_eq!(human(4_080_218_931), "3.8 GB");
    }
}
