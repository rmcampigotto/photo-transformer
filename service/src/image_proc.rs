//! Contain-fit to 1200×1200 JPEG on white background (same as index.html).

use crate::error::AppResult;
use image::{DynamicImage, ImageEncoder, ImageFormat, Rgb, RgbImage, imageops::FilterType};
use std::io::Cursor;

pub const SIZE: u32 = 1200;

/// Scale so the full image fits inside SIZE×SIZE (contain), center on #FFFFFF.
pub fn contain_to_square_jpeg(input: &[u8], quality: u8) -> AppResult<Vec<u8>> {
    let img = image::load_from_memory(input)?;
    let square = contain_to_square(&img);
    encode_jpeg(&square, quality)
}

pub fn contain_to_square(source: &DynamicImage) -> RgbImage {
    let rgb = source.to_rgb8();
    let (sw, sh) = rgb.dimensions();
    let scale = (SIZE as f64 / sw as f64).min(SIZE as f64 / sh as f64);
    let dw = ((sw as f64) * scale).round().max(1.0) as u32;
    let dh = ((sh as f64) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&rgb, dw, dh, FilterType::Lanczos3);

    let mut canvas = RgbImage::from_pixel(SIZE, SIZE, Rgb([255, 255, 255]));
    let dx = ((SIZE - dw) / 2) as i64;
    let dy = ((SIZE - dh) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, dx, dy);
    canvas
}

fn encode_jpeg(img: &RgbImage, quality: u8) -> AppResult<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    let _ = ImageFormat::Jpeg; // keep codec feature usage clear
    Ok(buf.into_inner())
}

/// Filename stem without extension — used as Bling `codigo` / SKU.
pub fn sku_from_filename(name: &str) -> String {
    let base = std::path::Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    // strip common "-1200x1200" suffix if already processed
    let base = base
        .strip_suffix("-1200x1200")
        .or_else(|| base.strip_suffix("_1200x1200"))
        .unwrap_or(base);
    base.trim().to_string()
}

pub fn is_image_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.starts_with("image/") && !m.contains("svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sku_strips_extension() {
        assert_eq!(sku_from_filename("ABC-123.jpg"), "ABC-123");
        assert_eq!(sku_from_filename("XYZ-1200x1200.jpeg"), "XYZ");
    }
}
