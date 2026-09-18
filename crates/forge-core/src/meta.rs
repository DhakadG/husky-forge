//! Metadata carried from source to output: ICC, EXIF, XMP.
use anyhow::Result;
use exif::{Context, Field, In, Tag, experimental::Writer};
use img_parts::{Bytes, ImageEXIF, ImageICC};

use crate::encode::Format;

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub icc: Option<Vec<u8>>,
    /// Raw TIFF-structured EXIF (no "Exif\0\0" prefix).
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    pub has_gps: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetaMode {
    #[default]
    Keep,
    StripGps,
    Strip,
}

impl Meta {
    /// What survives into the output under `mode`: EXIF rewritten (orientation dropped, GPS optional), XMP kept or dropped.
    pub fn carried(&self, mode: MetaMode) -> Meta {
        let exif = match mode {
            MetaMode::Strip => None,
            MetaMode::StripGps => self.exif.as_deref().and_then(|e| rewrite_exif(e, true)),
            MetaMode::Keep => self.exif.as_deref().and_then(|e| rewrite_exif(e, false)),
        };
        let xmp = if mode == MetaMode::Strip { None } else { self.xmp.clone() };
        Meta { icc: self.icc.clone(), exif, xmp, has_gps: self.has_gps && mode == MetaMode::Keep }
    }
}

pub fn has_gps(exif: &[u8]) -> bool {
    exif::Reader::new()
        .read_raw(exif.to_vec())
        .map(|e| e.fields().any(|f| f.tag.context() == Context::Gps))
        .unwrap_or(false)
}

/// A JPEG APP1 segment holds at most this much EXIF (65535 - length bytes - "Exif\0\0").
pub const JPEG_EXIF_MAX: usize = 65_527;

/// Re-serialise EXIF: drop Orientation (pixels are upright after decode),
/// thumbnails (IFD1) and, when asked, the whole GPS IFD.
/// ponytail: MakerNote internal offsets are not relocated; most readers cope.
pub fn rewrite_exif(raw: &[u8], strip_gps: bool) -> Option<Vec<u8>> {
    rewrite_exif_within(raw, strip_gps, usize::MAX)
}

/// `rewrite_exif` that also fits the result under `max_bytes`, shedding the least valuable
/// data first: MakerNote (camera-private, often 100 KB+ in RAW files), then any blob over 4 KB.
pub fn rewrite_exif_within(raw: &[u8], strip_gps: bool, max_bytes: usize) -> Option<Vec<u8>> {
    let parsed = exif::Reader::new().read_raw(raw.to_vec()).ok()?;
    let base: Vec<&Field> = parsed
        .fields()
        .filter(|f| f.ifd_num == In::PRIMARY)
        .filter(|f| f.tag != Tag::Orientation)
        .filter(|f| !(strip_gps && f.tag.context() == Context::Gps))
        .collect();
    let blob_len = |f: &Field| match &f.value {
        exif::Value::Undefined(b, _) => b.len(),
        exif::Value::Byte(b) => b.len(),
        _ => 0,
    };
    let sheds: [&dyn Fn(&Field) -> bool; 3] = [&|_| true, &|f| f.tag != Tag::MakerNote, &|f| f.tag != Tag::MakerNote && blob_len(f) <= 4096];
    for keep_if in sheds {
        let keep: Vec<&Field> = base.iter().copied().filter(|f| keep_if(f)).collect();
        if keep.is_empty() {
            return None;
        }
        let mut w = Writer::new();
        for f in &keep {
            w.push_field(f);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        w.write(&mut out, parsed.little_endian()).ok()?;
        let out = out.into_inner();
        if out.len() <= max_bytes {
            return Some(out);
        }
    }
    log::warn!("EXIF block cannot be made to fit {max_bytes} bytes; dropping it");
    None
}

/// Attach already-`carried` metadata to freshly encoded JPEG/PNG/WebP bytes.
/// AVIF/JXL took theirs at encode time.
pub fn inject(bytes: Vec<u8>, f: Format, m: &Meta) -> Result<Vec<u8>> {
    let icc = m.icc.clone().map(Bytes::from);
    let mut exif = m.exif.clone();
    let mut xmp = m.xmp.clone();
    if f == Format::Jpeg {
        // JPEG segments cap at 64 KB: shrink the EXIF, drop oversize XMP (extended XMP is phase 3).
        if exif.as_ref().is_some_and(|e| e.len() > JPEG_EXIF_MAX) {
            exif = exif.as_deref().and_then(|e| rewrite_exif_within(e, false, JPEG_EXIF_MAX));
        }
        if xmp.as_ref().is_some_and(|x| x.len() > 65_000) {
            log::warn!("XMP block too large for a JPEG segment; dropping it");
            xmp = None;
        }
    }
    let exif = exif.map(Bytes::from);
    let mut out = Vec::new();
    match f {
        Format::Jpeg => {
            let mut j = img_parts::jpeg::Jpeg::from_bytes(bytes.into())?;
            j.set_icc_profile(icc);
            j.set_exif(exif);
            if let Some(x) = xmp {
                let mut seg = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
                seg.extend_from_slice(&x);
                j.segments_mut()
                    .push(img_parts::jpeg::JpegSegment::new_with_contents(img_parts::jpeg::markers::APP1, seg.into()));
            }
            j.encoder().write_to(&mut out)?;
        }
        Format::Png => {
            let mut p = img_parts::png::Png::from_bytes(bytes.into())?;
            p.set_icc_profile(icc);
            p.set_exif(exif);
            p.encoder().write_to(&mut out)?;
        }
        Format::Webp => {
            let mut w = img_parts::webp::WebP::from_bytes(bytes.into())?;
            w.set_icc_profile(icc);
            w.set_exif(exif);
            w.encoder().write_to(&mut out)?;
        }
        Format::Avif | Format::Jxl => return Ok(bytes),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use exif::{Rational, Value};

    fn sample() -> Vec<u8> {
        let mut w = Writer::new();
        let make = Field { tag: Tag::Make, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"Forge".to_vec()]) };
        let orient = Field { tag: Tag::Orientation, ifd_num: In::PRIMARY, value: Value::Short(vec![6]) };
        let lat = Field { tag: Tag::GPSLatitude, ifd_num: In::PRIMARY, value: Value::Rational(vec![Rational { num: 12, denom: 1 }; 3]) };
        w.push_field(&make);
        w.push_field(&orient);
        w.push_field(&lat);
        let mut out = std::io::Cursor::new(Vec::new());
        w.write(&mut out, false).unwrap();
        out.into_inner()
    }

    #[test]
    fn oversize_makernote_is_shed_for_jpeg() {
        let mut w = Writer::new();
        let make = Field { tag: Tag::Make, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"Forge".to_vec()]) };
        let note = Field { tag: Tag::MakerNote, ifd_num: In::PRIMARY, value: Value::Undefined(vec![7u8; 120_000], 0) };
        w.push_field(&make);
        w.push_field(&note);
        let mut out = std::io::Cursor::new(Vec::new());
        w.write(&mut out, false).unwrap();
        let raw = out.into_inner();
        assert!(raw.len() > JPEG_EXIF_MAX);
        let small = rewrite_exif_within(&raw, false, JPEG_EXIF_MAX).unwrap();
        assert!(small.len() <= JPEG_EXIF_MAX);
        let e = exif::Reader::new().read_raw(small).unwrap();
        assert!(e.get_field(Tag::Make, In::PRIMARY).is_some());
        assert!(e.get_field(Tag::MakerNote, In::PRIMARY).is_none());
    }

    #[test]
    fn strips_gps_and_orientation_keeps_rest() {
        let raw = sample();
        assert!(has_gps(&raw));
        let kept = rewrite_exif(&raw, false).unwrap();
        assert!(has_gps(&kept));
        let stripped = rewrite_exif(&raw, true).unwrap();
        assert!(!has_gps(&stripped));
        let e = exif::Reader::new().read_raw(stripped).unwrap();
        assert!(e.get_field(Tag::Make, In::PRIMARY).is_some());
        assert!(e.get_field(Tag::Orientation, In::PRIMARY).is_none());
    }
}
