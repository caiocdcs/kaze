//! Recognise image formats from their first bytes.
//!
//! Servers lie: WeebCentral serves JPEGs named `.png`, and a broken CDN
//! answers with an HTML error page and status 200. Sniffing the bytes
//! catches both.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    Webp,
    Avif,
}

impl ImageFormat {
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0xFF, 0xD8, 0xFF, ..] => Some(Self::Jpeg),
            [0x89, b'P', b'N', b'G', ..] => Some(Self::Png),
            [b'G', b'I', b'F', b'8', ..] => Some(Self::Gif),
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'E',
                b'B',
                b'P',
                ..,
            ] => Some(Self::Webp),
            [
                _,
                _,
                _,
                _,
                b'f',
                b't',
                b'y',
                b'p',
                b'a',
                b'v',
                b'i',
                b'f' | b's',
                ..,
            ] => Some(Self::Avif),
            _ => None,
        }
    }

    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Avif => "avif",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_formats() {
        assert_eq!(
            ImageFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::sniff(b"\x89PNG\r\n\x1a\n"),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            ImageFormat::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(
            ImageFormat::sniff(b"\0\0\0\x1cftypavif"),
            Some(ImageFormat::Avif)
        );
        assert_eq!(ImageFormat::sniff(b"<!DOCTYPE html>"), None);
        assert_eq!(ImageFormat::sniff(b""), None);
    }
}
