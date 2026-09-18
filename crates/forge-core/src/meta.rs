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

pub fn has_gps(exif: &[u8]) -> bool {
    exif::Reader::new()
        .read_raw(exif.to_vec())
        .map(|e| e.fields().any(|f| f.tag.context() == Context::Gps))
        .unwrap_or(false)
}

/// Re-serialise EXIF: drop Orientation (pixels are upright after decode),
/// thumbnails (IFD1) and, when asked, the whole GPS IFD.
/// ponytail: MakerNote internal offsets are not relocated; most readers cope.
pub fn rewrite_exif(raw: &[u8], strip_gps: bool) -> Option<Vec<u8>> {
    let parsed = exif::Reader::new().read_raw(raw.to_vec()).ok()?;
    let keep: Vec<&Field> = parsed
        .fields()
        .filter(|f| f.ifd_num == In::PRIMARY)
        .filter(|f| f.tag != Tag::Orientation)
        .filter(|f| !(strip_gps && f.tag.context() == Context::Gps))
        .collect();
    if keep.is_empty() {
        return None;
    }
    let mut w = Writer::new();
    for f in keep {
        w.push_field(f);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    w.write(&mut out, parsed.little_endian()).ok()?;
    Some(out.into_inner())
}

/// Attach metadata to freshly encoded bytes. AVIF/JXL take ICC at encode time;
/// EXIF/XMP for those containers is phase 2.
pub fn inject(bytes: Vec<u8>, f: Format, m: &Meta, mode: MetaMode) -> Result<Vec<u8>> {
    let exif = match mode {
        MetaMode::Strip => None,
        MetaMode::StripGps => m.exif.as_deref().and_then(|e| rewrite_exif(e, true)),
        MetaMode::Keep => m.exif.as_deref().and_then(|e| rewrite_exif(e, false)),
    };
    let icc = m.icc.clone().map(Bytes::from);
    let exif = exif.map(Bytes::from);
    let xmp = if mode == MetaMode::Strip { None } else { m.xmp.clone() };
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
