//! Server-side watermark and OG composition for public snapshots (AC-24.5, AC-24.7, SR-18).
//!
//! Pure, CPU-bound functions on decoded pixels (call them from a blocking task). The watermark is
//! a band with the product mark in the bottom-right corner plus a faint diagonal pattern over the
//! whole image, so cropping the band alone does not remove it. Text uses a built-in 5x7 bitmap
//! font (no font files, no extra dependency).

use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

/// The mark drawn on every public image.
pub const MARK: &str = "CARDSHARING";
/// Longest side of a public card derivative (AC-24.5: at most 1080 px).
pub const MAX_SIDE: u32 = 1080;
/// OG image size (AC-24.7).
pub const OG_WIDTH: u32 = 1200;
pub const OG_HEIGHT: u32 = 630;

/// Color of the band behind the mark.
const BAND: Rgba<u8> = Rgba([20, 20, 28, 150]);
const INK: Rgba<u8> = Rgba([255, 255, 255, 235]);
const PAPER: Rgba<u8> = Rgba([246, 241, 232, 255]);
const TILE: Rgba<u8> = Rgba([214, 210, 204, 255]);
const TILE_EDGE: Rgba<u8> = Rgba([186, 181, 174, 255]);

/// 5x7 glyphs (one byte per row, low 5 bits, MSB = left column).
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        _ => [0; 7],
    }
}

/// Alpha-blends `color` over the pixel at (x, y) when it is inside the image.
fn blend(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= image.width() || y >= image.height() {
        return;
    }
    let pixel = image.get_pixel_mut(x, y);
    let a = u32::from(color[3]);
    for i in 0..3 {
        let over = u32::from(color[i]) * a + u32::from(pixel[i]) * (255 - a);
        pixel[i] = u8::try_from(over / 255).unwrap_or(u8::MAX);
    }
    pixel[3] = pixel[3].max(color[3]);
}

fn fill_rect(image: &mut RgbaImage, x0: i64, y0: i64, w: i64, h: i64, color: Rgba<u8>) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            blend(image, x, y, color);
        }
    }
}

/// Width in pixels of `text` at `scale` (5 columns + 1 gap per glyph).
fn text_width(text: &str, scale: i64) -> i64 {
    i64::try_from(text.chars().count()).unwrap_or(0) * 6 * scale - scale
}

fn draw_text(image: &mut RgbaImage, text: &str, x0: i64, y0: i64, scale: i64, color: Rgba<u8>) {
    for (i, c) in text.chars().enumerate() {
        let gx = x0 + i64::try_from(i).unwrap_or(0) * 6 * scale;
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) != 0 {
                    let px = gx + i64::from(col) * scale;
                    let py = y0 + i64::try_from(row).unwrap_or(0) * scale;
                    fill_rect(image, px, py, scale, scale, color);
                }
            }
        }
    }
}

/// Draws the watermark in place: the faint diagonal pattern, then the band with the mark.
pub fn apply(image: &mut RgbaImage) {
    let (w, h) = (i64::from(image.width()), i64::from(image.height()));
    if w < 8 || h < 8 {
        return;
    }
    // Faint diagonal stripes every 48 px, 2 px wide, only over visible pixels.
    for y in 0..h {
        for x in 0..w {
            if (x + y).rem_euclid(48) < 2 {
                let (ux, uy) = (x as u32, y as u32);
                if image.get_pixel(ux, uy)[3] > 0 {
                    blend(image, x, y, Rgba([255, 255, 255, 40]));
                }
            }
        }
    }
    let scale = (w.min(h) / 160).clamp(1, 6);
    let text_w = text_width(MARK, scale);
    let pad = 2 * scale;
    let band_w = text_w + 2 * pad;
    let band_h = 7 * scale + 2 * pad;
    let x0 = (w - band_w - pad).max(0);
    let y0 = (h - band_h - pad).max(0);
    fill_rect(image, x0, y0, band_w, band_h, BAND);
    draw_text(image, MARK, x0 + pad, y0 + pad, scale, INK);
}

/// Applies a mask (luminance = coverage) to the render's alpha, when sizes match.
pub fn apply_mask(render: &mut RgbaImage, mask: &RgbaImage) {
    if render.dimensions() != mask.dimensions() {
        return;
    }
    for (pixel, m) in render.pixels_mut().zip(mask.pixels()) {
        let coverage = (u32::from(m[0]) + u32::from(m[1]) + u32::from(m[2])) / 3;
        let alpha = u32::from(pixel[3]) * coverage / 255;
        pixel[3] = u8::try_from(alpha).unwrap_or(u8::MAX);
    }
}

/// A public card image: downscaled to at most [`MAX_SIDE`], masked, watermarked.
pub fn public_card(render: &RgbaImage, mask: Option<&RgbaImage>) -> RgbaImage {
    let mut image = render.clone();
    if let Some(mask) = mask {
        apply_mask(&mut image, mask);
    }
    let longest = image.width().max(image.height());
    if longest > MAX_SIDE {
        let ratio = f64::from(MAX_SIDE) / f64::from(longest);
        let w = ((f64::from(image.width()) * ratio).round() as u32).max(1);
        let h = ((f64::from(image.height()) * ratio).round() as u32).max(1);
        image = image::imageops::resize(&image, w, h, FilterType::Triangle);
    }
    apply(&mut image);
    image
}

/// One element of the OG composition, in board coordinates (0..1).
pub enum OgItem<'a> {
    Card {
        x: f64,
        y: f64,
        scale: f64,
        image: &'a RgbaImage,
    },
    PrivateTile {
        x: f64,
        y: f64,
        scale: f64,
    },
}

/// Size of a pin on the OG canvas at scale 1.
const OG_PIN: f64 = 220.0;

fn og_box(x: f64, y: f64, scale: f64, aspect: f64) -> (i64, i64, u32, u32) {
    let side = (OG_PIN * scale.clamp(0.25, 2.5)).round();
    let (w, h) = if aspect >= 1.0 {
        (side, (side / aspect).round())
    } else {
        ((side * aspect).round(), side)
    };
    let cx = x.clamp(0.0, 1.0) * f64::from(OG_WIDTH);
    let cy = y.clamp(0.0, 1.0) * f64::from(OG_HEIGHT);
    (
        (cx - w / 2.0) as i64,
        (cy - h / 2.0) as i64,
        w.max(1.0) as u32,
        h.max(1.0) as u32,
    )
}

/// The OG image: paper background, the owner's cards and neutral tiles at their pin positions
/// (z order = slice order), then the watermark.
pub fn og_image(items: &[OgItem<'_>]) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(OG_WIDTH, OG_HEIGHT, PAPER);
    for item in items {
        match item {
            OgItem::Card { x, y, scale, image } => {
                let aspect = f64::from(image.width()) / f64::from(image.height().max(1));
                let (x0, y0, w, h) = og_box(*x, *y, *scale, aspect);
                let thumb = image::imageops::resize(*image, w, h, FilterType::Triangle);
                for (tx, ty, pixel) in thumb.enumerate_pixels() {
                    blend(&mut canvas, x0 + i64::from(tx), y0 + i64::from(ty), *pixel);
                }
            }
            OgItem::PrivateTile { x, y, scale } => {
                let (x0, y0, w, h) = og_box(*x, *y, *scale, 0.75);
                let (w, h) = (i64::from(w), i64::from(h));
                fill_rect(&mut canvas, x0, y0, w, h, TILE_EDGE);
                fill_rect(&mut canvas, x0 + 3, y0 + 3, w - 6, h - 6, TILE);
            }
        }
    }
    apply(&mut canvas);
    canvas
}

/// Lossless WebP (the clean bucket serves `image/webp`).
pub fn encode_webp(image: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_watermark_changes_the_bottom_right_corner() {
        let plain = RgbaImage::from_pixel(800, 600, Rgba([10, 120, 200, 255]));
        let mut marked = plain.clone();
        apply(&mut marked);
        assert_ne!(plain, marked);
        // Band area is darker than the source color, somewhere near the corner.
        let corner = marked.get_pixel(790, 590);
        assert_ne!(corner, plain.get_pixel(790, 590));
    }

    #[test]
    fn public_cards_are_at_most_1080_px_and_masked() {
        let render = RgbaImage::from_pixel(2000, 1000, Rgba([200, 100, 50, 255]));
        let mut mask = RgbaImage::from_pixel(2000, 1000, Rgba([0, 0, 0, 255]));
        mask.put_pixel(1000, 500, Rgba([255, 255, 255, 255]));
        let out = public_card(&render, Some(&mask));
        assert_eq!(out.dimensions(), (1080, 540));
        assert_eq!(
            out.get_pixel(0, 0)[3],
            0,
            "masked-out pixels are transparent"
        );
    }

    #[test]
    fn og_images_are_1200_by_630_and_encode() {
        let card = RgbaImage::from_pixel(300, 400, Rgba([1, 2, 3, 255]));
        let items = [
            OgItem::Card {
                x: 0.3,
                y: 0.4,
                scale: 1.0,
                image: &card,
            },
            OgItem::PrivateTile {
                x: 0.7,
                y: 0.5,
                scale: 1.0,
            },
        ];
        let og = og_image(&items);
        assert_eq!(og.dimensions(), (OG_WIDTH, OG_HEIGHT));
        assert_eq!(*og.get_pixel(360, 252), Rgba([1, 2, 3, 255]));
        assert!(!encode_webp(&og).unwrap().is_empty());
    }
}
