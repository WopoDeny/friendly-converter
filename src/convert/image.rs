//! Raster image conversions.

use crate::formats::Format;
use anyhow::{bail, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage, RgbaImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Opens an image by content (not extension) and applies EXIF rotation,
/// so phone photos come out the right way up.
/// Returns the image and whether a rotation/flip was applied.
pub fn load_oriented(path: &Path) -> Result<(DynamicImage, bool)> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .with_guessed_format()?;
    let mut decoder = reader.into_decoder().with_context(|| format!("unsupported image: {}", path.display()))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).with_context(|| format!("cannot decode {}", path.display()))?;
    let transformed = orientation != Orientation::NoTransforms;
    if transformed {
        img.apply_orientation(orientation);
    }
    Ok((img, transformed))
}

/// Composites transparency over white and returns 8-bit RGB.
pub fn flatten_to_rgb(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}

fn image_format(f: Format) -> Option<ImageFormat> {
    Some(match f {
        Format::Png => ImageFormat::Png,
        Format::Jpg => ImageFormat::Jpeg,
        Format::Webp => ImageFormat::WebP,
        Format::Bmp => ImageFormat::Bmp,
        Format::Gif => ImageFormat::Gif,
        Format::Tiff => ImageFormat::Tiff,
        Format::Ico => ImageFormat::Ico,
        Format::Tga => ImageFormat::Tga,
        _ => return None,
    })
}

pub fn save_as(img: &DynamicImage, target: Format, out: &Path, jpeg_quality: u8) -> Result<()> {
    let Some(fmt) = image_format(target) else {
        bail!("{} is not an image format", target.label());
    };
    match target {
        Format::Jpg => {
            let rgb = flatten_to_rgb(img);
            let mut file = BufWriter::new(File::create(out)?);
            let encoder = JpegEncoder::new_with_quality(&mut file, jpeg_quality.clamp(1, 100));
            rgb.write_with_encoder(encoder)?;
        }
        Format::Ico => {
            let img = if img.width() > 256 || img.height() > 256 {
                img.resize(256, 256, FilterType::Lanczos3)
            } else {
                img.clone()
            };
            let rgba: RgbaImage = img.to_rgba8();
            rgba.save_with_format(out, fmt)?;
        }
        Format::Bmp | Format::Tga | Format::Tiff | Format::Png | Format::Webp | Format::Gif => {
            if img.color().has_alpha() || matches!(target, Format::Webp | Format::Gif) {
                img.to_rgba8().save_with_format(out, fmt)?;
            } else {
                img.to_rgb8().save_with_format(out, fmt)?;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn convert(input: &Path, target: Format, out: &Path, jpeg_quality: u8) -> Result<()> {
    let (img, _) = load_oriented(input)?;
    save_as(&img, target, out, jpeg_quality)
}
