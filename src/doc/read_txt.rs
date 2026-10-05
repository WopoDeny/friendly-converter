use super::{clean_text, Block, Document, Run};
use crate::textenc::{normalize_newlines, read_text_file};
use anyhow::Result;
use std::path::Path;

pub fn read(path: &Path) -> Result<Document> {
    let text = read_text_file(path)?;
    Ok(from_text(&text))
}

/// Blank lines separate paragraphs, single line breaks are preserved inside them,
/// form feed characters become page breaks.
pub fn from_text(text: &str) -> Document {
    let text = normalize_newlines(text);
    let mut doc = Document::default();
    let mut current: Vec<String> = Vec::new();

    fn flush(current: &mut Vec<String>, doc: &mut Document) {
        if current.is_empty() {
            return;
        }
        let joined = current.join("\n");
        doc.blocks.push(Block::Paragraph(vec![Run::plain(joined)]));
        current.clear();
    }

    for raw_line in text.split('\n') {
        let mut parts = raw_line.split('\u{000C}').peekable();
        while let Some(part) = parts.next() {
            let line = clean_text(part);
            let line = line.trim_end();
            if line.trim().is_empty() {
                flush(&mut current, &mut doc);
            } else {
                current.push(line.to_string());
            }
            if parts.peek().is_some() {
                flush(&mut current, &mut doc);
                doc.blocks.push(Block::PageBreak);
            }
        }
    }
    flush(&mut current, &mut doc);
    doc
}
