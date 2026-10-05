//! Embedded TrueType fonts (DejaVu, full Cyrillic coverage) and the metrics
//! needed to place text in a PDF.

use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};
use ttf_parser::{Face, GlyphId};

pub static SANS: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");
pub static SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf");
pub static SANS_ITALIC: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Oblique.ttf");
pub static SANS_BOLD_ITALIC: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-BoldOblique.ttf");
pub static MONO: &[u8] = include_bytes!("../../assets/fonts/DejaVuSansMono.ttf");

pub struct PdfFont {
    data: &'static [u8],
    pub name: String,
    pub italic: bool,
    pub mono: bool,
    units_per_em: f32,
    pub ascent: f32,
    pub descent: f32,
    pub cap_height: f32,
    pub bbox: [f32; 4],
    cache: HashMap<char, (u16, u16)>,
    /// glyph id -> (advance in font units, first char mapped to it)
    used: BTreeMap<u16, (u16, char)>,
}

impl PdfFont {
    pub fn new(data: &'static [u8], name: &str, italic: bool, mono: bool) -> Result<PdfFont> {
        let face = Face::parse(data, 0).map_err(|e| anyhow!("bad font {name}: {e}"))?;
        let upem = face.units_per_em() as f32;
        let bb = face.global_bounding_box();
        let to1000 = |v: f32| v * 1000.0 / upem;
        Ok(PdfFont {
            data,
            name: name.to_string(),
            italic,
            mono,
            units_per_em: upem,
            ascent: to1000(face.ascender() as f32),
            descent: to1000(face.descender() as f32),
            cap_height: to1000(face.capital_height().unwrap_or(face.ascender()) as f32),
            bbox: [
                to1000(bb.x_min as f32),
                to1000(bb.y_min as f32),
                to1000(bb.x_max as f32),
                to1000(bb.y_max as f32),
            ],
            cache: HashMap::new(),
            used: BTreeMap::new(),
        })
    }

    /// The standard set used for documents: regular, bold, italic, bold italic, mono.
    pub fn standard_set() -> Result<Vec<PdfFont>> {
        Ok(vec![
            PdfFont::new(SANS, "DejaVuSans", false, false)?,
            PdfFont::new(SANS_BOLD, "DejaVuSans-Bold", false, false)?,
            PdfFont::new(SANS_ITALIC, "DejaVuSans-Oblique", true, false)?,
            PdfFont::new(SANS_BOLD_ITALIC, "DejaVuSans-BoldOblique", true, false)?,
            PdfFont::new(MONO, "DejaVuSansMono", false, true)?,
        ])
    }

    pub fn data(&self) -> &'static [u8] {
        self.data
    }

    pub fn units_per_em(&self) -> f32 {
        self.units_per_em
    }

    fn glyph(&mut self, c: char) -> (u16, u16) {
        if let Some(g) = self.cache.get(&c) {
            return *g;
        }
        let result = match Face::parse(self.data, 0) {
            Ok(face) => {
                let gid = face.glyph_index(c).unwrap_or(GlyphId(0));
                let adv = face.glyph_hor_advance(gid).unwrap_or(0);
                (gid.0, adv)
            }
            Err(_) => (0, 0),
        };
        self.cache.insert(c, result);
        result
    }

    /// Text width in points.
    pub fn width(&mut self, text: &str, size: f32) -> f32 {
        let mut units: u32 = 0;
        for c in text.chars() {
            units += self.glyph(c).1 as u32;
        }
        units as f32 * size / self.units_per_em
    }

    /// Glyph ids as big-endian u16 (Identity-H encoding), records used glyphs.
    pub fn encode(&mut self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() * 2);
        for c in text.chars() {
            let (gid, adv) = self.glyph(c);
            self.used.entry(gid).or_insert((adv, c));
            out.extend_from_slice(&gid.to_be_bytes());
        }
        out
    }

    pub fn used_glyphs(&self) -> &BTreeMap<u16, (u16, char)> {
        &self.used
    }

    pub fn is_used(&self) -> bool {
        !self.used.is_empty()
    }
}
