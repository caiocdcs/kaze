pub mod cover_cache;

use image::{DynamicImage, GrayImage, Luma};
use tracing::error;

/// Process image: optional B&W conversion and JPEG compression.
/// Returns original bytes if no processing needed or if format is unsupported.
pub fn process_image(data: &[u8], black_white: bool, compression: i32) -> Vec<u8> {
    if !black_white && compression == 100 {
        return data.to_vec();
    }

    match image::load_from_memory(data) {
        Ok(mut img) => {
            if black_white {
                img = DynamicImage::ImageLuma8(adaptive_threshold(&img.to_luma8()));
            }
            let mut output = Vec::with_capacity(data.len());
            let quality = compression.clamp(1, 100) as u8;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality);
            if let Err(e) = img.write_with_encoder(encoder) {
                error!("Image encode failed: {e}");
                return data.to_vec();
            }
            output
        }
        Err(_) => data.to_vec(),
    }
}

/// Adaptive threshold binarization (matches C# AdaptiveThresholdProcessor behavior).
/// For each pixel, computes the mean of a surrounding block and thresholds:
///   pixel < (local_mean - C) => black, else white.
fn adaptive_threshold(gray: &GrayImage) -> GrayImage {
    let (width, height) = gray.dimensions();
    let block = 11u32;
    let c_value: i32 = 10;

    let integral = build_integral_image(gray);
    let mut result = GrayImage::new(width, height);

    let half = block / 2;
    for y in 0..height {
        for x in 0..width {
            let x1 = x.saturating_sub(half);
            let y1 = y.saturating_sub(half);
            let x2 = (x + half).min(width - 1);
            let y2 = (y + half).min(height - 1);

            let count = ((x2 - x1 + 1) * (y2 - y1 + 1)) as i64;
            let sum = integral_sum(&integral, width, x1, y1, x2, y2);
            let mean = sum / count;

            let pixel = gray.get_pixel(x, y).0[0] as i64;
            let lum = if pixel < mean - c_value as i64 { 0 } else { 255 };
            result.put_pixel(x, y, Luma([lum]));
        }
    }

    result
}

fn build_integral_image(gray: &GrayImage) -> Vec<i64> {
    let (width, height) = gray.dimensions();
    let mut integral = vec![0i64; (width as usize) * (height as usize)];

    for y in 0..height {
        let mut row_sum = 0i64;
        for x in 0..width {
            row_sum += gray.get_pixel(x, y).0[0] as i64;
            let idx = (y as usize) * (width as usize) + (x as usize);
            let above = if y > 0 { integral[idx - width as usize] } else { 0 };
            integral[idx] = row_sum + above;
        }
    }

    integral
}

fn integral_sum(integral: &[i64], width: u32, x1: u32, y1: u32, x2: u32, y2: u32) -> i64 {
    let w = width as usize;
    let a = integral[(y2 as usize) * w + (x2 as usize)];
    let b = if x1 > 0 { integral[(y2 as usize) * w + (x1 as usize - 1)] } else { 0 };
    let c = if y1 > 0 { integral[(y1 as usize - 1) * w + (x2 as usize)] } else { 0 };
    let d = if x1 > 0 && y1 > 0 { integral[(y1 as usize - 1) * w + (x1 as usize - 1)] } else { 0 };
    a - b - c + d
}

/// Resize image to fit within max dimensions, maintaining aspect ratio.
pub fn resize_cover(data: &[u8], max_w: u32, max_h: u32, quality: u8) -> Option<Vec<u8>> {
    let img = image::load_from_memory(data).ok()?;
    let (w, h) = (img.width(), img.height());
    let scale = if w as f64 / max_w as f64 > h as f64 / max_h as f64 {
        max_w as f64 / w as f64
    } else {
        max_h as f64 / h as f64
    };

    let resized = if scale < 1.0 {
        let nw = (w as f64 * scale) as u32;
        let nh = (h as f64 * scale) as u32;
        img.resize(nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let mut output = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality);
    resized.write_with_encoder(encoder).ok()?;
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(100, 150, image::Rgb([128, 64, 32]));
        let mut buf = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90))
            .expect("encode");
        buf
    }

    #[test]
    fn process_image_passthrough_when_no_processing() {
        let data = create_test_jpeg();
        let result = process_image(&data, false, 100);
        assert_eq!(result, data, "should pass through unchanged");
    }

    #[test]
    fn process_image_compresses() {
        let data = create_test_jpeg();
        let result = process_image(&data, false, 50);
        assert!(!result.is_empty(), "should produce output");
        assert_ne!(result.len(), data.len(), "compressed size should differ");
    }

    #[test]
    fn process_image_bw() {
        let data = create_test_jpeg();
        let result = process_image(&data, true, 90);
        assert!(!result.is_empty());
    }

    #[test]
    fn process_image_handles_garbage() {
        let garbage = b"not an image at all";
        let result = process_image(garbage, false, 50);
        assert_eq!(result, garbage.to_vec(), "should pass through unprocessable data");
    }

    #[test]
    fn resize_cover_downscales() {
        let data = create_test_jpeg(); // 100x150
        let result = resize_cover(&data, 50, 75, 40);
        assert!(result.is_some(), "should produce resized image");
    }

    #[test]
    fn resize_cover_no_upscale() {
        let data = create_test_jpeg(); // 100x150
        let result = resize_cover(&data, 200, 300, 40);
        assert!(result.is_some(), "should produce image without upscaling");
    }
}
