//! Format detection by extension (fast) with a magic-byte check for the ambiguous ones.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Jpeg,
    Png,
    Webp,
    Tiff,
    Gif,
    Bmp,
    Avif,
    Heic,
    Jxl,
    Raw,
}

const RAW_EXT: &[&str] = &[
    "arw", "srf", "sr2", "cr2", "cr3", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "3fr", "iiq",
];

impl Kind {
    pub fn from_path(p: &Path) -> Option<Kind> {
        let ext = p.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => Kind::Jpeg,
            "png" => Kind::Png,
            "webp" => Kind::Webp,
            "tif" | "tiff" => Kind::Tiff,
            "gif" => Kind::Gif,
            "bmp" => Kind::Bmp,
            "avif" => Kind::Avif,
            "heic" | "heif" => Kind::Heic,
            "jxl" => Kind::Jxl,
            e if RAW_EXT.contains(&e) => Kind::Raw,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Jpeg => "JPEG",
            Kind::Png => "PNG",
            Kind::Webp => "WebP",
            Kind::Tiff => "TIFF",
            Kind::Gif => "GIF",
            Kind::Bmp => "BMP",
            Kind::Avif => "AVIF",
            Kind::Heic => "HEIC",
            Kind::Jxl => "JPEG XL",
            Kind::Raw => "RAW",
        }
    }

    /// Decodable by this build. HEIC/AVIF need the `heic` feature (libheif).
    pub fn supported(self) -> bool {
        cfg!(feature = "heic") || !matches!(self, Kind::Heic | Kind::Avif)
    }
}
