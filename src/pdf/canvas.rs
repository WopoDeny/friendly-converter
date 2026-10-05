//! A tiny PDF drawing surface: pages, Unicode text with embedded TrueType
//! fonts, lines, rectangles and raster images.

use super::fonts::PdfFont;
use anyhow::Result;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::io::Write;
use std::path::Path;

pub type Rgb = [f32; 3];

struct PageBuf {
    width: f32,
    height: f32,
    ops: Vec<Operation>,
}

pub struct Canvas {
    doc: Document,
    fonts: Vec<PdfFont>,
    images: Vec<(String, ObjectId)>,
    pages: Vec<PageBuf>,
    current: usize,
}

fn real(v: f32) -> Object {
    Object::Real(v as _)
}

fn int(v: i64) -> Object {
    Object::Integer(v)
}

fn name(n: &str) -> Object {
    Object::Name(n.as_bytes().to_vec())
}

/// PDF text string (UTF-16BE with BOM), safe for any language.
pub fn text_string(s: &str) -> Object {
    let mut bytes = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        bytes.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Hexadecimal)
}

fn zlib(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

impl Canvas {
    pub fn new(fonts: Vec<PdfFont>) -> Canvas {
        Canvas { doc: Document::with_version("1.7"), fonts, images: Vec::new(), pages: Vec::new(), current: 0 }
    }

    pub fn add_page(&mut self, width: f32, height: f32) -> usize {
        self.pages.push(PageBuf { width, height, ops: Vec::new() });
        self.current = self.pages.len() - 1;
        self.current
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn select_page(&mut self, index: usize) {
        if index < self.pages.len() {
            self.current = index;
        }
    }

    fn ops(&mut self) -> &mut Vec<Operation> {
        if self.pages.is_empty() {
            self.add_page(595.28, 841.89);
        }
        &mut self.pages[self.current].ops
    }

    pub fn measure(&mut self, font: usize, size: f32, text: &str) -> f32 {
        self.fonts[font].width(text, size)
    }

    pub fn font(&self, font: usize) -> &PdfFont {
        &self.fonts[font]
    }

    /// Draws text with its baseline at `y`.
    pub fn text(&mut self, font: usize, size: f32, x: f32, y: f32, text: &str, color: Rgb) {
        if text.is_empty() {
            return;
        }
        let encoded = self.fonts[font].encode(text);
        let font_name = format!("F{font}");
        let ops = self.ops();
        ops.push(Operation::new("BT", vec![]));
        ops.push(Operation::new("rg", vec![real(color[0]), real(color[1]), real(color[2])]));
        ops.push(Operation::new("Tf", vec![name(&font_name), real(size)]));
        ops.push(Operation::new("Tm", vec![real(1.0), real(0.0), real(0.0), real(1.0), real(x), real(y)]));
        ops.push(Operation::new("Tj", vec![Object::String(encoded, StringFormat::Hexadecimal)]));
        ops.push(Operation::new("ET", vec![]));
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgb) {
        let ops = self.ops();
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new("w", vec![real(width)]));
        ops.push(Operation::new("RG", vec![real(color[0]), real(color[1]), real(color[2])]));
        ops.push(Operation::new("m", vec![real(x1), real(y1)]));
        ops.push(Operation::new("l", vec![real(x2), real(y2)]));
        ops.push(Operation::new("S", vec![]));
        ops.push(Operation::new("Q", vec![]));
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        let ops = self.ops();
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new("rg", vec![real(color[0]), real(color[1]), real(color[2])]));
        ops.push(Operation::new("re", vec![real(x), real(y), real(w), real(h)]));
        ops.push(Operation::new("f", vec![]));
        ops.push(Operation::new("Q", vec![]));
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: Rgb) {
        let ops = self.ops();
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new("w", vec![real(width)]));
        ops.push(Operation::new("RG", vec![real(color[0]), real(color[1]), real(color[2])]));
        ops.push(Operation::new("re", vec![real(x), real(y), real(w), real(h)]));
        ops.push(Operation::new("S", vec![]));
        ops.push(Operation::new("Q", vec![]));
    }

    /// Places a previously added image. (x, y) is the lower-left corner.
    pub fn draw_image(&mut self, image_name: &str, x: f32, y: f32, w: f32, h: f32) {
        let ops = self.ops();
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new("cm", vec![real(w), real(0.0), real(0.0), real(h), real(x), real(y)]));
        ops.push(Operation::new("Do", vec![name(image_name)]));
        ops.push(Operation::new("Q", vec![]));
    }

    fn register_image(&mut self, stream: Stream) -> String {
        let id = self.doc.add_object(Object::Stream(stream));
        let image_name = format!("Im{}", self.images.len());
        self.images.push((image_name.clone(), id));
        image_name
    }

    /// Raw 8-bit RGB pixels, stored Flate-compressed (lossless).
    pub fn add_image_rgb(&mut self, width: u32, height: u32, rgb: &[u8]) -> Result<String> {
        self.add_raw_image(width, height, rgb, "DeviceRGB")
    }

    /// Raw 8-bit grayscale pixels.
    pub fn add_image_gray(&mut self, width: u32, height: u32, gray: &[u8]) -> Result<String> {
        self.add_raw_image(width, height, gray, "DeviceGray")
    }

    fn add_raw_image(&mut self, width: u32, height: u32, data: &[u8], color_space: &str) -> Result<String> {
        let compressed = zlib(data)?;
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => int(width as i64),
            "Height" => int(height as i64),
            "ColorSpace" => name(color_space),
            "BitsPerComponent" => int(8),
            "Filter" => "FlateDecode",
        };
        let mut stream = Stream::new(dict, compressed);
        stream.allows_compression = false;
        Ok(self.register_image(stream))
    }

    /// Embeds JPEG bytes as they are (no recompression).
    pub fn add_image_jpeg(&mut self, width: u32, height: u32, components: u8, jpeg: Vec<u8>) -> String {
        let color_space = if components == 1 { "DeviceGray" } else { "DeviceRGB" };
        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => int(width as i64),
            "Height" => int(height as i64),
            "ColorSpace" => name(color_space),
            "BitsPerComponent" => int(8),
            "Filter" => "DCTDecode",
        };
        let mut stream = Stream::new(dict, jpeg);
        stream.allows_compression = false;
        self.register_image(stream)
    }

    fn embed_font(doc: &mut Document, font: &PdfFont) -> ObjectId {
        let scale = 1000.0 / font.units_per_em();
        let data = font.data();

        let file_id = doc.add_object(Stream::new(dictionary! { "Length1" => int(data.len() as i64) }, data.to_vec()));

        let mut flags: i64 = 32; // nonsymbolic
        if font.mono {
            flags |= 1;
        }
        if font.italic {
            flags |= 64;
        }
        let descriptor_id = doc.add_object(dictionary! {
            "Type" => "FontDescriptor",
            "FontName" => name(&font.name),
            "Flags" => int(flags),
            "FontBBox" => vec![real(font.bbox[0]), real(font.bbox[1]), real(font.bbox[2]), real(font.bbox[3])],
            "ItalicAngle" => int(if font.italic { -11 } else { 0 }),
            "Ascent" => real(font.ascent),
            "Descent" => real(font.descent),
            "CapHeight" => real(font.cap_height),
            "StemV" => int(80),
            "FontFile2" => file_id,
        });

        let mut widths: Vec<Object> = Vec::new();
        for (gid, (adv, _)) in font.used_glyphs() {
            widths.push(int(*gid as i64));
            widths.push(Object::Array(vec![int((*adv as f32 * scale).round() as i64)]));
        }

        let cid_font = dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "BaseFont" => name(&font.name),
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"),
                "Supplement" => int(0),
            },
            "FontDescriptor" => descriptor_id,
            "DW" => int(1000),
            "W" => Object::Array(widths),
            "CIDToGIDMap" => "Identity",
        };
        let cid_id = doc.add_object(cid_font);

        let to_unicode_id = doc.add_object(Stream::new(Dictionary::new(), to_unicode_cmap(font).into_bytes()));

        doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => name(&font.name),
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(cid_id)],
            "ToUnicode" => to_unicode_id,
        })
    }

    pub fn save(self, path: &Path, title: Option<&str>) -> Result<()> {
        let bytes = self.into_bytes(title)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    pub fn into_bytes(mut self, title: Option<&str>) -> Result<Vec<u8>> {
        if self.pages.is_empty() {
            self.add_page(595.28, 841.89);
        }
        let mut doc = std::mem::replace(&mut self.doc, Document::with_version("1.7"));

        let mut font_dict = Dictionary::new();
        for (i, font) in self.fonts.iter().enumerate() {
            if font.is_used() {
                let id = Self::embed_font(&mut doc, font);
                font_dict.set(format!("F{i}"), Object::Reference(id));
            }
        }
        let mut xobjects = Dictionary::new();
        for (image_name, id) in &self.images {
            xobjects.set(image_name.clone(), Object::Reference(*id));
        }
        let resources_id = doc.add_object(dictionary! {
            "Font" => font_dict,
            "XObject" => xobjects,
            "ProcSet" => vec![name("PDF"), name("Text"), name("ImageB"), name("ImageC")],
        });

        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();
        for page in self.pages.drain(..) {
            let content = Content { operations: page.ops };
            let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode()?));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![real(0.0), real(0.0), real(page.width), real(page.height)],
                "Contents" => content_id,
                "Resources" => resources_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => int(count),
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut info = dictionary! {
            "Producer" => text_string("Friendly Converter"),
            "Creator" => text_string("Friendly Converter"),
        };
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            info.set("Title", text_string(t));
        }
        let info_id = doc.add_object(info);
        doc.trailer.set("Info", Object::Reference(info_id));

        doc.compress();
        let mut out = Vec::new();
        doc.save_to(&mut out)?;
        Ok(out)
    }
}

fn to_unicode_cmap(font: &PdfFont) -> String {
    let mut entries: Vec<String> = Vec::new();
    for (gid, (_, c)) in font.used_glyphs() {
        let mut hex = String::new();
        let mut buf = [0u16; 2];
        for unit in c.encode_utf16(&mut buf) {
            hex.push_str(&format!("{:04X}", unit));
        }
        entries.push(format!("<{:04X}> <{}>", gid, hex));
    }
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for e in chunk {
            s.push_str(e);
            s.push('\n');
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}
