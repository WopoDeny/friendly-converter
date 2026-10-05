//! Extracting embedded raster images from a PDF.

use anyhow::{bail, Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
use lopdf::{Document, Object, Stream};
use std::path::{Path, PathBuf};

pub struct ExtractReport {
    pub saved: Vec<PathBuf>,
    pub skipped: usize,
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    let mut current = obj;
    for _ in 0..8 {
        match current {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(o) => current = o,
                Err(_) => return current,
            },
            _ => return current,
        }
    }
    current
}

fn filters(doc: &Document, stream: &Stream) -> Vec<Vec<u8>> {
    match stream.dict.get(b"Filter").map(|o| resolve(doc, o)) {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(items)) => items
            .iter()
            .filter_map(|i| match resolve(doc, i) {
                Object::Name(n) => Some(n.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Number of color components for the image color space, if supported.
fn components(doc: &Document, stream: &Stream) -> Option<u32> {
    let cs = resolve(doc, stream.dict.get(b"ColorSpace").ok()?);
    match cs {
        Object::Name(n) => match n.as_slice() {
            b"DeviceRGB" | b"CalRGB" => Some(3),
            b"DeviceGray" | b"CalGray" => Some(1),
            b"DeviceCMYK" => Some(4),
            _ => None,
        },
        Object::Array(items) => {
            let kind = match items.first().map(|o| resolve(doc, o)) {
                Some(Object::Name(n)) => n.clone(),
                _ => return None,
            };
            match kind.as_slice() {
                b"ICCBased" => {
                    let icc = resolve(doc, items.get(1)?);
                    if let Object::Stream(s) = icc {
                        s.dict.get(b"N").ok().and_then(|n| n.as_i64().ok()).map(|n| n as u32)
                    } else {
                        None
                    }
                }
                b"CalRGB" => Some(3),
                b"CalGray" => Some(1),
                _ => None,
            }
        }
        _ => None,
    }
}

fn int_of(doc: &Document, stream: &Stream, key: &[u8]) -> Option<i64> {
    stream.dict.get(key).ok().map(|o| resolve(doc, o)).and_then(|o| o.as_i64().ok())
}

pub fn extract_images(input: &Path, out_dir: &Path, stem: &str) -> Result<ExtractReport> {
    let doc = Document::load(input).with_context(|| format!("cannot open {}", input.display()))?;
    if doc.trailer.get(b"Encrypt").is_ok() {
        bail!("the PDF is password protected");
    }
    let target_dir = crate::util::unique_dir(out_dir, &format!("{stem}_images"));
    std::fs::create_dir_all(&target_dir)?;

    let mut saved = Vec::new();
    let mut skipped = 0usize;
    let mut index = 0usize;

    for obj in doc.objects.values() {
        let stream = match obj {
            Object::Stream(s) => s,
            _ => continue,
        };
        let is_image = matches!(stream.dict.get(b"Subtype").map(|o| resolve(&doc, o)), Ok(Object::Name(n)) if n == b"Image");
        if !is_image {
            continue;
        }
        let width = int_of(&doc, stream, b"Width").unwrap_or(0);
        let height = int_of(&doc, stream, b"Height").unwrap_or(0);
        // Skip tiny masks / decorations.
        if width < 8 || height < 8 {
            continue;
        }
        index += 1;
        let f = filters(&doc, stream);
        let last = f.last().map(|v| v.as_slice());

        if last == Some(b"DCTDecode".as_slice()) && f.len() == 1 {
            let path = target_dir.join(format!("image_{index:03}.jpg"));
            std::fs::write(&path, &stream.content)?;
            saved.push(path);
            continue;
        }
        if last == Some(b"JPXDecode".as_slice()) && f.len() == 1 {
            let path = target_dir.join(format!("image_{index:03}.jp2"));
            std::fs::write(&path, &stream.content)?;
            saved.push(path);
            continue;
        }

        let bpc = int_of(&doc, stream, b"BitsPerComponent").unwrap_or(8);
        let comps = components(&doc, stream);
        let data = if f.is_empty() { Ok(stream.content.clone()) } else { stream.decompressed_content() };
        let (Ok(data), Some(comps), 8) = (data, comps, bpc) else {
            skipped += 1;
            continue;
        };
        let (w, h) = (width as u32, height as u32);
        let pixels = (w as usize) * (h as usize);
        let img: Option<DynamicImage> = match comps {
            1 if data.len() >= pixels => GrayImage::from_raw(w, h, data[..pixels].to_vec()).map(DynamicImage::ImageLuma8),
            3 if data.len() >= pixels * 3 => RgbImage::from_raw(w, h, data[..pixels * 3].to_vec()).map(DynamicImage::ImageRgb8),
            4 if data.len() >= pixels * 4 => {
                let mut rgb = Vec::with_capacity(pixels * 3);
                for px in data[..pixels * 4].chunks_exact(4) {
                    let k = px[3] as f32 / 255.0;
                    for &c in &px[..3] {
                        let v = 255.0 * (1.0 - c as f32 / 255.0) * (1.0 - k);
                        rgb.push(v.round().clamp(0.0, 255.0) as u8);
                    }
                }
                RgbImage::from_raw(w, h, rgb).map(DynamicImage::ImageRgb8)
            }
            _ => None,
        };
        match img {
            Some(img) => {
                let path = target_dir.join(format!("image_{index:03}.png"));
                img.save_with_format(&path, image::ImageFormat::Png)?;
                saved.push(path);
            }
            None => skipped += 1,
        }
    }
    if saved.is_empty() {
        let _ = std::fs::remove_dir(&target_dir);
        if skipped > 0 {
            bail!("found {skipped} image(s) in unsupported formats, nothing extracted");
        }
        bail!("no images found in this PDF");
    }
    Ok(ExtractReport { saved, skipped })
}
