//! Image re-encoding (T-71, SR-27): the only place that decodes user images. Runs in a blocking
//! task with a pixel budget, a decoder memory cap and a wall-clock timeout (the caller enforces
//! the timeout), applies the EXIF orientation, and re-encodes from raw pixels, so no metadata
//! (EXIF, GPS, ICC, XMP, comments) and no trailing or polyglot bytes survive.

use std::io::Cursor;

use bytes::Bytes;
use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageReader, Limits, RgbaImage};

use crate::service::MediaKind;
use crate::sniff::{ImageFormat, MAX_DIMENSION};

/// Upper bound on decoder allocations (a 4096 x 4096 RGBA image is 64 MiB).
pub const MAX_DECODE_ALLOC: u64 = 192 * 1024 * 1024;

/// Recipient-facing sizes (ADR-0005). Longest side, never upscaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Feed1080,
    Widget512,
    Thumb256,
}

impl Variant {
    pub const ALL: [Self; 3] = [Self::Feed1080, Self::Widget512, Self::Thumb256];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feed1080 => "feed_1080",
            Self::Widget512 => "widget_512",
            Self::Thumb256 => "thumb_256",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == value)
    }

    fn max_side(self) -> u32 {
        match self {
            Self::Feed1080 => 1080,
            Self::Widget512 => 512,
            Self::Thumb256 => 256,
        }
    }

    /// Derivatives made for a kind. Originals get none (owner-only, AC-13.6).
    pub fn for_kind(kind: MediaKind) -> &'static [Self] {
        match kind {
            MediaKind::CardRender => &[Self::Feed1080, Self::Widget512, Self::Thumb256],
            MediaKind::Mask => &[Self::Feed1080],
            MediaKind::Avatar => &[Self::Thumb256],
            MediaKind::Original => &[],
        }
    }
}

/// One re-encoded image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub bytes: Bytes,
    pub width: u32,
    pub height: u32,
}

/// Output of [`process`]: the full-size re-encode, its derivatives and the decoded pixels (for
/// the content gates).
pub struct Processed {
    pub main: Encoded,
    pub derivatives: Vec<(Variant, Encoded)>,
    pub pixels: RgbaImage,
}

impl std::fmt::Debug for Processed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Processed")
            .field("main", &(self.main.width, self.main.height))
            .field("derivatives", &self.derivatives.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    /// The bytes are not a decodable image within the limits (corrupt, truncated, too large).
    #[error("image could not be decoded: {0}")]
    Decode(String),
    #[error("image could not be encoded: {0}")]
    Encode(String),
}

fn image_format(format: ImageFormat) -> image::ImageFormat {
    match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Webp => image::ImageFormat::WebP,
    }
}

/// Decodes `bytes` as `format` (the sniffed format, never a guess), applies the orientation and
/// re-encodes as lossless WebP plus the derivatives of `kind`. CPU-bound: call it from a
/// blocking task.
pub fn process(
    bytes: &[u8],
    format: ImageFormat,
    kind: MediaKind,
) -> Result<Processed, ProcessError> {
    let decode = |e: image::ImageError| ProcessError::Decode(e.to_string());
    let mut reader = ImageReader::with_format(Cursor::new(bytes), image_format(format));
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decode)?;
    let orientation = decoder.orientation().map_err(decode)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode)?;
    image.apply_orientation(orientation);
    let pixels = image.to_rgba8();
    drop(image);

    let main = encode(&pixels)?;
    let mut derivatives = Vec::new();
    for variant in Variant::for_kind(kind) {
        derivatives.push((*variant, encode(&fit(&pixels, variant.max_side()))?));
    }
    Ok(Processed {
        main,
        derivatives,
        pixels,
    })
}

/// Scales down to fit `max_side` (aspect ratio kept); smaller images are copied as they are.
fn fit(pixels: &RgbaImage, max_side: u32) -> RgbaImage {
    let (w, h) = pixels.dimensions();
    let longest = w.max(h);
    if longest <= max_side {
        return pixels.clone();
    }
    let scale = |side: u32| {
        let scaled = u64::from(side) * u64::from(max_side) / u64::from(longest);
        u32::try_from(scaled.max(1)).unwrap_or(1)
    };
    image::imageops::resize(pixels, scale(w), scale(h), FilterType::Triangle)
}

fn encode(pixels: &RgbaImage) -> Result<Encoded, ProcessError> {
    let (width, height) = pixels.dimensions();
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out)
        .encode(pixels.as_raw(), width, height, ExtendedColorType::Rgba8)
        .map_err(|e| ProcessError::Encode(e.to_string()))?;
    Ok(Encoded {
        bytes: Bytes::from(out),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::{ImageEncoder, Rgba};

    fn gradient(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        })
    }

    fn png(pixels: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        out
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    fn is_webp(bytes: &[u8]) -> bool {
        bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
    }

    #[test]
    fn polyglot_png_becomes_pure_webp() {
        let mut bytes = png(&gradient(40, 30));
        bytes.extend_from_slice(b"<html><script>alert('polyglot')</script></html>PK\x03\x04");
        let out = process(&bytes, ImageFormat::Png, MediaKind::CardRender).unwrap();
        assert!(is_webp(&out.main.bytes));
        assert!(!contains(&out.main.bytes, b"script"));
        assert!(!contains(&out.main.bytes, b"PK\x03\x04"));
        assert_eq!((out.main.width, out.main.height), (40, 30));
        // Derivatives of a small render are copies at the same size, all WebP.
        assert_eq!(out.derivatives.len(), 3);
        for (_, d) in &out.derivatives {
            assert!(is_webp(&d.bytes));
            assert_eq!((d.width, d.height), (40, 30));
        }
    }

    #[test]
    fn exif_gps_is_stripped_from_jpeg() {
        let pixels = image::DynamicImage::ImageRgba8(gradient(64, 48)).to_rgb8();
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .write_image(pixels.as_raw(), 64, 48, ExtendedColorType::Rgb8)
            .unwrap();
        // Insert an APP1 Exif segment carrying GPS tags right after SOI.
        let mut payload = b"Exif\0\0MM\0*\0\0\0\x08".to_vec();
        payload.extend_from_slice(b"GPSLatitude=48.8584;GPSLongitude=2.2945");
        let length = u16::try_from(payload.len() + 2).unwrap().to_be_bytes();
        let mut with_exif = vec![0xFF, 0xD8, 0xFF, 0xE1, length[0], length[1]];
        with_exif.extend_from_slice(&payload);
        with_exif.extend_from_slice(&jpeg[2..]);
        assert!(contains(&with_exif, b"GPSLatitude"));

        let out = process(&with_exif, ImageFormat::Jpeg, MediaKind::Original).unwrap();
        assert!(is_webp(&out.main.bytes));
        for needle in [&b"GPS"[..], b"Exif", b"48.8584"] {
            assert!(!contains(&out.main.bytes, needle));
        }
        // Originals get no recipient derivatives (AC-13.6).
        assert!(out.derivatives.is_empty());
    }

    #[test]
    fn derivatives_fit_their_box_without_upscaling() {
        let out = process(
            &png(&gradient(2000, 1000)),
            ImageFormat::Png,
            MediaKind::CardRender,
        )
        .unwrap();
        let sizes: Vec<_> = out
            .derivatives
            .iter()
            .map(|(v, d)| (v.as_str(), d.width, d.height))
            .collect();
        assert_eq!(
            sizes,
            vec![
                ("feed_1080", 1080, 540),
                ("widget_512", 512, 256),
                ("thumb_256", 256, 128)
            ]
        );
        assert_eq!(Variant::for_kind(MediaKind::Mask), &[Variant::Feed1080]);
        assert_eq!(Variant::for_kind(MediaKind::Avatar), &[Variant::Thumb256]);
    }

    #[test]
    fn truncated_or_lying_images_fail_to_decode() {
        let bytes = png(&gradient(32, 32));
        assert!(matches!(
            process(
                &bytes[..bytes.len() / 2],
                ImageFormat::Png,
                MediaKind::CardRender
            ),
            Err(ProcessError::Decode(_))
        ));
        // A PNG sent as "JPEG" is decoded as JPEG only (never guessed), so it fails.
        assert!(matches!(
            process(&bytes, ImageFormat::Jpeg, MediaKind::CardRender),
            Err(ProcessError::Decode(_))
        ));
    }
}
