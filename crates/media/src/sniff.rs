//! Upload sniffing (SR-27): the format comes from magic bytes, never from the file name or the
//! declared content type, and the pixel size comes from the container header, read **before**
//! anything is decoded. A 50k x 50k header is refused from its first few dozen bytes.

/// Largest accepted width and height (ADR-0005: header dimension check before accepting).
pub const MAX_DIMENSION: u32 = 4096;

/// How many leading bytes may be searched for the size header before giving up (JPEG files can
/// carry large metadata segments before the frame header).
pub const MAX_HEADER_SCAN: usize = 1024 * 1024;

/// Formats the pipeline accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn from_content_type(value: &str) -> Option<Self> {
        match value {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// What the header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sniffed {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SniffError {
    /// Not a supported image (HTML, scripts, archives, HEIC until a memory-safe decoder exists).
    #[error("unsupported media type")]
    Unsupported,
    /// A supported container whose header is broken or never states the size.
    #[error("malformed image header")]
    Malformed,
    /// Width or height is zero or above [`MAX_DIMENSION`].
    #[error("image dimensions out of range")]
    Dimensions,
}

/// Result of looking at a prefix of the upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SniffOutcome {
    Done(Sniffed),
    /// The prefix is a plausible start, but the size header is further in.
    NeedMore,
}

/// Sniffs `prefix` (the first bytes of the upload). `complete` = the whole upload is in `prefix`.
pub fn sniff(prefix: &[u8], complete: bool) -> Result<SniffOutcome, SniffError> {
    let outcome = match detect(prefix) {
        Detected::NeedMore => SniffOutcome::NeedMore,
        Detected::Unsupported => return Err(SniffError::Unsupported),
        Detected::Format(ImageFormat::Png) => png(prefix)?,
        Detected::Format(ImageFormat::Jpeg) => jpeg(prefix)?,
        Detected::Format(ImageFormat::Webp) => webp(prefix)?,
    };
    match outcome {
        SniffOutcome::Done(sniffed) => {
            check_dimensions(sniffed.width, sniffed.height)?;
            Ok(SniffOutcome::Done(sniffed))
        }
        SniffOutcome::NeedMore if complete || prefix.len() >= MAX_HEADER_SCAN => {
            Err(SniffError::Malformed)
        }
        SniffOutcome::NeedMore => Ok(SniffOutcome::NeedMore),
    }
}

/// Width and height within `1..=MAX_DIMENSION`.
pub fn check_dimensions(width: u32, height: u32) -> Result<(), SniffError> {
    let ok = |side: u32| (1..=MAX_DIMENSION).contains(&side);
    if ok(width) && ok(height) {
        Ok(())
    } else {
        Err(SniffError::Dimensions)
    }
}

enum Detected {
    Format(ImageFormat),
    Unsupported,
    NeedMore,
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn detect(prefix: &[u8]) -> Detected {
    let starts = |magic: &[u8]| {
        let n = magic.len().min(prefix.len());
        prefix[..n] == magic[..n]
    };
    if starts(PNG_SIGNATURE) {
        return complete_or_more(prefix, PNG_SIGNATURE.len(), ImageFormat::Png);
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return complete_or_more(prefix, 3, ImageFormat::Jpeg);
    }
    if starts(b"RIFF") {
        if prefix.len() < 12 {
            return Detected::NeedMore;
        }
        return if &prefix[8..12] == b"WEBP" {
            Detected::Format(ImageFormat::Webp)
        } else {
            Detected::Unsupported
        };
    }
    // Too short to rule anything out yet (but never for an empty upload's first chunk of text).
    if prefix.len() < 3 {
        return Detected::NeedMore;
    }
    Detected::Unsupported
}

fn complete_or_more(prefix: &[u8], magic_len: usize, format: ImageFormat) -> Detected {
    if prefix.len() < magic_len {
        Detected::NeedMore
    } else {
        Detected::Format(format)
    }
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn le_u24(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at + 3)?;
    Some(u32::from(s[0]) | (u32::from(s[1]) << 8) | (u32::from(s[2]) << 16))
}

/// PNG: the first chunk must be IHDR (width, height as big-endian u32).
fn png(prefix: &[u8]) -> Result<SniffOutcome, SniffError> {
    if prefix.len() < 24 {
        return Ok(SniffOutcome::NeedMore);
    }
    if &prefix[12..16] != b"IHDR" {
        return Err(SniffError::Malformed);
    }
    let (Some(width), Some(height)) = (be_u32(prefix, 16), be_u32(prefix, 20)) else {
        return Ok(SniffOutcome::NeedMore);
    };
    Ok(SniffOutcome::Done(Sniffed {
        format: ImageFormat::Png,
        width,
        height,
    }))
}

/// JPEG: walk the marker segments to the first start-of-frame (SOFn) header.
fn jpeg(prefix: &[u8]) -> Result<SniffOutcome, SniffError> {
    let mut at = 2;
    loop {
        let Some(&byte) = prefix.get(at) else {
            return Ok(SniffOutcome::NeedMore);
        };
        if byte != 0xFF {
            return Err(SniffError::Malformed);
        }
        // Fill bytes: any number of 0xFF before the marker code.
        let mut code_at = at + 1;
        while prefix.get(code_at) == Some(&0xFF) {
            code_at += 1;
        }
        let Some(&code) = prefix.get(code_at) else {
            return Ok(SniffOutcome::NeedMore);
        };
        match code {
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => at = code_at + 1,
            // Start of scan or end of image before any frame header.
            0xD8..=0xDA => return Err(SniffError::Malformed),
            _ => {
                let Some(length) = be_u16(prefix, code_at + 1) else {
                    return Ok(SniffOutcome::NeedMore);
                };
                if length < 2 {
                    return Err(SniffError::Malformed);
                }
                let is_frame = matches!(code, 0xC0..=0xCF) && !matches!(code, 0xC4 | 0xC8 | 0xCC);
                if is_frame {
                    // Length (2), precision (1), height (2), width (2).
                    let (Some(height), Some(width)) =
                        (be_u16(prefix, code_at + 4), be_u16(prefix, code_at + 6))
                    else {
                        return Ok(SniffOutcome::NeedMore);
                    };
                    return Ok(SniffOutcome::Done(Sniffed {
                        format: ImageFormat::Jpeg,
                        width: u32::from(width),
                        height: u32::from(height),
                    }));
                }
                at = code_at + 1 + usize::from(length);
            }
        }
    }
}

/// WebP: the first chunk after the RIFF header is `VP8 ` (lossy), `VP8L` (lossless) or `VP8X`
/// (extended, with the canvas size).
fn webp(prefix: &[u8]) -> Result<SniffOutcome, SniffError> {
    if prefix.len() < 30 {
        return Ok(SniffOutcome::NeedMore);
    }
    let (width, height) = match &prefix[12..16] {
        b"VP8 " => {
            if prefix[23..26] != [0x9D, 0x01, 0x2A] {
                return Err(SniffError::Malformed);
            }
            let w = u16::from_le_bytes([prefix[26], prefix[27]]) & 0x3FFF;
            let h = u16::from_le_bytes([prefix[28], prefix[29]]) & 0x3FFF;
            (u32::from(w), u32::from(h))
        }
        b"VP8L" => {
            if prefix[20] != 0x2F {
                return Err(SniffError::Malformed);
            }
            let bits = u32::from_le_bytes([prefix[21], prefix[22], prefix[23], prefix[24]]);
            ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
        }
        b"VP8X" => {
            let (Some(w), Some(h)) = (le_u24(prefix, 24), le_u24(prefix, 27)) else {
                return Ok(SniffOutcome::NeedMore);
            };
            (w + 1, h + 1)
        }
        _ => return Err(SniffError::Malformed),
    };
    Ok(SniffOutcome::Done(Sniffed {
        format: ImageFormat::Webp,
        width,
        height,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// A PNG signature plus an IHDR header claiming `width` x `height` (no pixel data).
    pub fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn done(bytes: &[u8]) -> Sniffed {
        match sniff(bytes, true).unwrap() {
            SniffOutcome::Done(s) => s,
            SniffOutcome::NeedMore => panic!("expected a result"),
        }
    }

    #[test]
    fn reads_png_size() {
        let s = done(&png_header(1080, 1350));
        assert_eq!(
            (s.format, s.width, s.height),
            (ImageFormat::Png, 1080, 1350)
        );
    }

    #[test]
    fn refuses_huge_headers_before_decoding() {
        let started = std::time::Instant::now();
        assert_eq!(
            sniff(&png_header(50_000, 50_000), false),
            Err(SniffError::Dimensions)
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
        assert_eq!(sniff(&png_header(0, 10), true), Err(SniffError::Dimensions));
        assert_eq!(
            sniff(&png_header(4096, 4097), true),
            Err(SniffError::Dimensions)
        );
        assert!(sniff(&png_header(4096, 4096), true).is_ok());
    }

    #[test]
    fn html_named_png_is_unsupported() {
        assert_eq!(
            sniff(b"<!doctype html><script>alert(1)</script>", true),
            Err(SniffError::Unsupported)
        );
        assert_eq!(sniff(b"GIF89a....", true), Err(SniffError::Unsupported));
        // HEIC (ISO BMFF `ftyp`) waits for a memory-safe decoder.
        assert_eq!(
            sniff(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic", true),
            Err(SniffError::Unsupported)
        );
    }

    #[test]
    fn short_prefixes_ask_for_more() {
        assert_eq!(sniff(&[0x89, b'P'], false), Ok(SniffOutcome::NeedMore));
        assert_eq!(
            sniff(&png_header(10, 10)[..20], false),
            Ok(SniffOutcome::NeedMore)
        );
        // ...unless that was the whole upload.
        assert_eq!(
            sniff(&png_header(10, 10)[..20], true),
            Err(SniffError::Malformed)
        );
        assert_eq!(sniff(&[], true), Err(SniffError::Malformed));
    }

    #[test]
    fn reads_jpeg_frame_header_after_metadata() {
        let mut bytes = vec![0xFF, 0xD8];
        // APP1 segment of 100 bytes (EXIF-like filler).
        bytes.extend_from_slice(&[0xFF, 0xE1, 0x00, 100]);
        bytes.extend_from_slice(&[0u8; 98]);
        // SOF0: length 17, precision 8, height 480, width 640.
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80]);
        let s = done(&bytes);
        assert_eq!((s.format, s.width, s.height), (ImageFormat::Jpeg, 640, 480));
        // Cut inside the APP1 segment: more is needed.
        assert_eq!(sniff(&bytes[..50], false), Ok(SniffOutcome::NeedMore));
        // A scan before any frame header is malformed.
        assert_eq!(
            sniff(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2], true),
            Err(SniffError::Malformed)
        );
    }

    #[test]
    fn reads_webp_headers() {
        let riff = |chunk: &[u8; 4], body: &[u8]| {
            let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
            bytes.extend_from_slice(chunk);
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(body);
            bytes.resize(40, 0);
            bytes
        };
        // VP8X: canvas 1080 x 1350 (stored minus one, 24-bit little endian).
        let mut vp8x = vec![0u8; 4];
        vp8x.extend_from_slice(&[0x37, 0x04, 0x00, 0x45, 0x05, 0x00]);
        let s = done(&riff(b"VP8X", &vp8x));
        assert_eq!((s.width, s.height), (1080, 1350));
        // VP8L: 14-bit width-1 and height-1 after the 0x2F signature.
        let bits: u32 = (99) | (49 << 14);
        let mut vp8l = vec![0x2F];
        vp8l.extend_from_slice(&bits.to_le_bytes());
        let s = done(&riff(b"VP8L", &vp8l));
        assert_eq!((s.width, s.height), (100, 50));
        // VP8 (lossy): frame tag, start code, 14-bit sizes.
        let vp8 = [0, 0, 0, 0x9D, 0x01, 0x2A, 0x40, 0x01, 0xF0, 0x00];
        let s = done(&riff(b"VP8 ", &vp8));
        assert_eq!((s.width, s.height), (320, 240));
        // Another RIFF type (WAV) is not an image.
        assert_eq!(
            sniff(b"RIFF\0\0\0\0WAVEfmt ", true),
            Err(SniffError::Unsupported)
        );
    }
}
