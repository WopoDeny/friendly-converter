//! Images into PDF pages.

use super::Canvas;
use crate::convert::image::{flatten_to_rgb, load_oriented};
use anyhow::Result;
use image::DynamicImage;
use std::path::{Path, PathBuf};

const A4_W: f32 = 595.28;
const A4_H: f32 = 841.89;

/// Reads the number of color components from a baseline/progressive JPEG header.
pub fn jpeg_components(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // SOF markers (excluding DHT C4, JPG C8, DAC CC)
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            return bytes.get(i + 9).copied();
        }
        if marker == 0xDA {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// Adds an image to the canvas, embedding JPEG data directly when possible.
/// Returns the image resource name.
pub fn add_image(canvas: &mut Canvas, img: &DynamicImage, original_jpeg: Option<&[u8]>) -> Result<String> {
    if let Some(bytes) = original_jpeg {
        if let Some(c @ (1 | 3)) = jpeg_components(bytes) {
            return Ok(canvas.add_image_jpeg(img.width(), img.height(), c, bytes.to_vec()));
        }
    }
    match img {
        DynamicImage::ImageLuma8(gray) => canvas.add_image_gray(gray.width(), gray.height(), gray.as_raw()),
        _ => {
            let rgb = flatten_to_rgb(img);
            canvas.add_image_rgb(rgb.width(), rgb.height(), rgb.as_raw())
        }
    }
}

/// One image per A4 page, orientation chosen per image.
pub fn images_to_pdf(paths: &[PathBuf], out: &Path) -> Result<()> {
    let mut canvas = Canvas::new(Vec::new());
    for path in paths {
        let (img, transformed) = load_oriented(path)?;
        let bytes = std::fs::read(path)?;
        let is_jpeg = matches!(image::guess_format(&bytes), Ok(image::ImageFormat::Jpeg));
        let original = if is_jpeg && !transformed { Some(bytes.as_slice()) } else { None };
        let name = add_image(&mut canvas, &img, original)?;

        let (w, h) = (img.width() as f32, img.height() as f32);
        let (pw, ph) = if w > h { (A4_H, A4_W) } else { (A4_W, A4_H) };
        let margin = 24.0;
        // 96 dpi pixels -> points, allow up to 2x enlargement for small images.
        let fit = ((pw - 2.0 * margin) / w).min((ph - 2.0 * margin) / h);
        let scale = fit.min(0.75 * 2.0);
        let (dw, dh) = (w * scale, h * scale);
        canvas.add_page(pw, ph);
        canvas.draw_image(&name, (pw - dw) / 2.0, (ph - dh) / 2.0, dw, dh);
    }
    let title = if paths.len() == 1 {
        paths[0].file_stem().map(|s| s.to_string_lossy().into_owned())
    } else {
        None
    };
    canvas.save(out, title.as_deref())
}
