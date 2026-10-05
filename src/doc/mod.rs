//! Format independent document model.
//!
//! Every document reader produces a [`Document`], every writer consumes one,
//! so any reader can be combined with any writer.

pub mod read_docx;
pub mod read_html;
pub mod read_md;
pub mod read_pdf;
pub mod read_txt;
pub mod write_docx;
pub mod write_html;
pub mod write_md;
pub mod write_pdf;
pub mod write_txt;

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub enum Block {
    Heading { level: u8, runs: Vec<Run> },
    Paragraph(Vec<Run>),
    ListItem { ordered: bool, number: usize, level: u8, runs: Vec<Run> },
    Code(String),
    Table(Table),
    Image(ImageData),
    Rule,
    PageBreak,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub header: bool,
}

/// An image stored as PNG or JPEG bytes.
#[derive(Debug, Clone)]
pub struct ImageData {
    pub bytes: Vec<u8>,
    pub jpeg: bool,
    pub width: u32,
    pub height: u32,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Run {
        Run { text: text.into(), ..Default::default() }
    }

    pub fn bold(text: impl Into<String>) -> Run {
        Run { text: text.into(), bold: true, ..Default::default() }
    }
}

impl Table {
    pub fn columns(&self) -> usize {
        self.rows.iter().map(|r| r.len()).max().unwrap_or(0)
    }
}

impl ImageData {
    /// Accepts any image the `image` crate can decode. PNG and JPEG bytes are
    /// kept as they are, anything else is re-encoded as PNG.
    pub fn from_bytes(bytes: Vec<u8>) -> Option<ImageData> {
        let format = image::guess_format(&bytes).ok()?;
        let img = image::load_from_memory(&bytes).ok()?;
        let (width, height) = (img.width(), img.height());
        if width == 0 || height == 0 {
            return None;
        }
        match format {
            image::ImageFormat::Png => Some(ImageData { bytes, jpeg: false, width, height }),
            image::ImageFormat::Jpeg => Some(ImageData { bytes, jpeg: true, width, height }),
            _ => {
                let mut out = std::io::Cursor::new(Vec::new());
                img.to_rgba8().write_to(&mut out, image::ImageFormat::Png).ok()?;
                Some(ImageData { bytes: out.into_inner(), jpeg: false, width, height })
            }
        }
    }

    pub fn extension(&self) -> &'static str {
        if self.jpeg {
            "jpeg"
        } else {
            "png"
        }
    }

    pub fn mime(&self) -> &'static str {
        if self.jpeg {
            "image/jpeg"
        } else {
            "image/png"
        }
    }
}

/// Concatenated text of a run list.
pub fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// Merges neighbouring runs that share the same style and drops empty ones.
pub fn normalize_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.bold == run.bold && last.italic == run.italic && last.code == run.code {
                last.text.push_str(&run.text);
                continue;
            }
        }
        out.push(run);
    }
    out
}

/// Removes characters that are not allowed in XML / break layout.
pub fn clean_text(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .map(|c| if c == '\u{00A0}' { ' ' } else { c })
        .collect()
}

impl Document {
    /// Plain text of the whole document, used by tests and as a fallback.
    pub fn plain_text(&self) -> String {
        let mut s = String::new();
        for b in &self.blocks {
            match b {
                Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
                    s.push_str(&runs_text(runs));
                    s.push('\n');
                }
                Block::Code(c) => {
                    s.push_str(c);
                    s.push('\n');
                }
                Block::Table(t) => {
                    for row in &t.rows {
                        s.push_str(&row.join("\t"));
                        s.push('\n');
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}
