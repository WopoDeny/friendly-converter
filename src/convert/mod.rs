//! Conversion dispatcher: picks the right reader/writer pair for a job.

pub mod data;
pub mod image;
pub mod table;

use crate::doc::{self, Block, Document, Run, Table};
use crate::formats::{Category, Format, Target};
use crate::pdf;
use crate::util::{file_stem, unique_path};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use table::Sheet;

#[derive(Debug, Clone)]
pub struct Options {
    /// None = next to the source file.
    pub out_dir: Option<PathBuf>,
    pub csv_delimiter: u8,
    pub jpeg_quality: u8,
}

impl Default for Options {
    fn default() -> Self {
        Options { out_dir: None, csv_delimiter: b';', jpeg_quality: 92 }
    }
}

impl Options {
    pub fn output_dir_for(&self, input: &Path) -> PathBuf {
        match &self.out_dir {
            Some(d) => d.clone(),
            None => input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
        }
    }
}

pub fn read_document(path: &Path, format: Format) -> Result<Document> {
    match format {
        Format::Pdf => doc::read_pdf::read(path),
        Format::Docx => doc::read_docx::read(path),
        Format::Txt => doc::read_txt::read(path),
        Format::Md => doc::read_md::read(path),
        Format::Html => doc::read_html::read(path),
        other => bail!("{} is not a document format", other.label()),
    }
}

pub fn write_document(document: &Document, format: Format, out: &Path) -> Result<()> {
    match format {
        Format::Docx => doc::write_docx::write(document, out),
        Format::Pdf => doc::write_pdf::write(document, out),
        Format::Txt => Ok(std::fs::write(out, doc::write_txt::render(document))?),
        Format::Md => doc::write_md::write(document, out),
        Format::Html => Ok(std::fs::write(out, doc::write_html::render(document))?),
        other => bail!("{} is not a document format", other.label()),
    }
}

pub fn sheets_to_document(sheets: &[Sheet], title: &str) -> Document {
    let mut d = Document { title: Some(title.to_string()), blocks: Vec::new() };
    for sheet in sheets {
        if sheets.len() > 1 {
            d.blocks.push(Block::Heading { level: 2, runs: vec![Run::plain(sheet.name.clone())] });
        }
        if !sheet.rows.is_empty() {
            d.blocks.push(Block::Table(Table { rows: sheet.rows.clone(), header: sheet.rows.len() > 1 }));
        }
    }
    d
}

fn write_sheets(sheets: &[Sheet], to: Format, dir: &Path, stem: &str, opts: &Options) -> Result<Vec<PathBuf>> {
    match to {
        Format::Xlsx => {
            let out = unique_path(dir, stem, "xlsx");
            guarded(&out, || table::write_xlsx(sheets, &out))?;
            Ok(vec![out])
        }
        Format::Csv => {
            let mut outputs = Vec::new();
            for sheet in sheets {
                let name = if sheets.len() > 1 {
                    let safe: String = sheet
                        .name
                        .chars()
                        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                        .collect();
                    format!("{stem}_{safe}")
                } else {
                    stem.to_string()
                };
                let out = unique_path(dir, &name, "csv");
                guarded(&out, || table::write_csv(sheet, &out, opts.csv_delimiter))?;
                outputs.push(out);
            }
            Ok(outputs)
        }
        other => bail!("{} is not a table format", other.label()),
    }
}

/// Runs a writer and removes the half-written file if it fails.
fn guarded(out: &Path, f: impl FnOnce() -> Result<()>) -> Result<()> {
    match f() {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(out);
            Err(e)
        }
    }
}

/// Converts one file. Returns every file that was written.
pub fn convert_file(input: &Path, from: Format, target: Target, opts: &Options) -> Result<Vec<PathBuf>> {
    if !input.is_file() {
        bail!("file not found: {}", input.display());
    }
    let dir = opts.output_dir_for(input);
    std::fs::create_dir_all(&dir)?;
    let stem = file_stem(input);

    let to = match target {
        Target::PdfImages => {
            if from != Format::Pdf {
                bail!("image extraction works only for PDF files");
            }
            let report = pdf::extract::extract_images(input, &dir, &stem)?;
            return Ok(report.saved);
        }
        Target::To(f) => f,
    };
    if to == from {
        bail!("the file is already {}", to.label());
    }

    match (from.category(), to.category()) {
        (Category::Table, Category::Table) => {
            let sheets = table::read_tables(input, from)?;
            write_sheets(&sheets, to, &dir, &stem, opts)
        }
        (Category::Data, Category::Table) => {
            let value = data::read_value(input, from)?;
            let sheets = data::value_to_sheets(&value, &stem)?;
            write_sheets(&sheets, to, &dir, &stem, opts)
        }
        (from_cat, to_cat) => {
            let out = unique_path(&dir, &stem, to.extension());
            guarded(&out, || match (from_cat, to_cat) {
                (Category::Image, Category::Image) => image::convert(input, to, &out, opts.jpeg_quality),
                (Category::Image, Category::Document) if to == Format::Pdf => {
                    pdf::images::images_to_pdf(&[input.to_path_buf()], &out)
                }
                (Category::Document, Category::Document) => {
                    let document = read_document(input, from)?;
                    if document.is_empty() {
                        bail!("the document has no readable content");
                    }
                    write_document(&document, to, &out)
                }
                (Category::Data, Category::Data) => {
                    let value = data::read_value(input, from)?;
                    std::fs::write(&out, data::render_value(&value, to)?)?;
                    Ok(())
                }
                (Category::Table, Category::Data) => {
                    let sheets = table::read_tables(input, from)?;
                    let value = data::sheets_to_value(&sheets);
                    std::fs::write(&out, data::render_value(&value, to)?)?;
                    Ok(())
                }
                (Category::Table, Category::Document) => {
                    let sheets = table::read_tables(input, from)?;
                    write_document(&sheets_to_document(&sheets, &stem), to, &out)
                }
                _ => bail!("{} to {} is not supported", from.label(), to.label()),
            })?;
            Ok(vec![out])
        }
    }
}

/// Merges several PDFs into `merged.pdf` in the output folder of the first one.
pub fn merge_pdfs(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    let first = inputs.first().ok_or_else(|| anyhow::anyhow!("no files"))?;
    let dir = opts.output_dir_for(first);
    std::fs::create_dir_all(&dir)?;
    let out = unique_path(&dir, "merged", "pdf");
    guarded(&out, || pdf::merge::merge_pdfs(inputs, &out))?;
    Ok(out)
}

/// Puts several images into one PDF, one image per page.
pub fn images_to_one_pdf(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    let first = inputs.first().ok_or_else(|| anyhow::anyhow!("no files"))?;
    let dir = opts.output_dir_for(first);
    std::fs::create_dir_all(&dir)?;
    let out = unique_path(&dir, "images", "pdf");
    guarded(&out, || pdf::images::images_to_pdf(inputs, &out))?;
    Ok(out)
}
