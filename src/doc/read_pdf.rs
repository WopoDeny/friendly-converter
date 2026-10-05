//! PDF text extraction.
//!
//! Only the text layer is read: layout (columns, exact positions) is not
//! reconstructed. Paragraphs, simple headings and bullet lists are guessed
//! from line structure.

use super::{clean_text, Block, Document, Run};
use crate::textenc::normalize_newlines;
use anyhow::{anyhow, bail, Context, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

pub fn read(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let text = extract_text(&bytes)?;
    if text.chars().filter(|c| c.is_alphanumeric()).count() < 3 {
        bail!("no text layer found (scanned PDF?). OCR is not supported");
    }
    let mut doc = Document { title: None, blocks: structure_text(&text) };
    doc.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    Ok(doc)
}

/// Tries pdf-extract first (best Unicode support), then lopdf as a fallback.
pub fn extract_text(bytes: &[u8]) -> Result<String> {
    let primary = catch_unwind(AssertUnwindSafe(|| pdf_extract::extract_text_from_mem(bytes)));
    let primary_err = match primary {
        Ok(Ok(text)) if !text.trim().is_empty() => return Ok(text),
        Ok(Ok(_)) => "empty text".to_string(),
        Ok(Err(e)) => format!("{e:?}"),
        Err(_) => "parser crashed".to_string(),
    };

    let fallback = catch_unwind(AssertUnwindSafe(|| -> Result<String> {
        let doc = lopdf::Document::load_mem(bytes)?;
        if doc.trailer.get(b"Encrypt").is_ok() {
            bail!("the PDF is encrypted (password protected)");
        }
        let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
        Ok(doc.extract_text(&pages)?)
    }));
    match fallback {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => {
            if primary_err == "empty text" {
                Ok(String::new())
            } else {
                Err(anyhow!("cannot read PDF text: {e}"))
            }
        }
        Err(_) => Err(anyhow!("cannot read PDF text: {primary_err}")),
    }
}

const BULLETS: [&str; 7] = ["\u{2022} ", "\u{25CF} ", "\u{25AA} ", "\u{00B7} ", "- ", "* ", "\u{2013} "];

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        let space = c == ' ' || c == '\t';
        if space {
            if !prev_space {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
        prev_space = space;
    }
    out.trim().to_string()
}

fn is_heading(line: &str) -> Option<u8> {
    let len = line.chars().count();
    if len < 2 || len > 90 {
        return None;
    }
    let last = line.chars().last()?;
    if matches!(last, '.' | ',' | ';' | ':' | '-') {
        return None;
    }
    let letters: Vec<char> = line.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 3 && letters.iter().all(|c| c.is_uppercase()) {
        return Some(2);
    }
    // "1.2 Section title" style numbering.
    let mut chars = line.chars().peekable();
    let mut saw_digit = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            saw_digit = true;
            chars.next();
        } else if c == '.' && saw_digit {
            chars.next();
        } else {
            break;
        }
    }
    if saw_digit && chars.next() == Some(' ') {
        let rest: String = chars.collect();
        let first = rest.chars().next()?;
        if first.is_uppercase() && rest.chars().count() <= 70 {
            return Some(3);
        }
    }
    None
}

fn bullet_text(line: &str) -> Option<&str> {
    BULLETS.iter().find_map(|b| line.strip_prefix(b)).map(|s| s.trim_start())
}

fn join_line(buf: &mut String, line: &str) {
    if buf.is_empty() {
        buf.push_str(line);
        return;
    }
    let ends_with_hyphen = buf.ends_with('-')
        && buf.chars().rev().nth(1).map(|c| c.is_alphabetic()).unwrap_or(false);
    let next_lower = line.chars().next().map(|c| c.is_lowercase()).unwrap_or(false);
    if ends_with_hyphen && next_lower {
        buf.pop();
        buf.push_str(line);
    } else {
        buf.push(' ');
        buf.push_str(line);
    }
}

pub fn structure_text(text: &str) -> Vec<Block> {
    let text = normalize_newlines(text);
    let mut blocks = Vec::new();
    let mut para = String::new();
    let mut para_lines = 0usize;
    let mut list_counter = 0usize;

    fn flush(blocks: &mut Vec<Block>, para: &mut String, lines: &mut usize) {
        if para.is_empty() {
            return;
        }
        let text = std::mem::take(para);
        if *lines == 1 {
            if let Some(level) = is_heading(&text) {
                blocks.push(Block::Heading { level, runs: vec![Run::plain(text)] });
                *lines = 0;
                return;
            }
        }
        blocks.push(Block::Paragraph(vec![Run::plain(text)]));
        *lines = 0;
    }

    let pages: Vec<&str> = text.split('\u{000C}').collect();
    let page_count = pages.len();
    for (page_index, page) in pages.into_iter().enumerate() {
        for raw in page.split('\n') {
            let line = collapse_spaces(&clean_text(raw));
            if line.is_empty() {
                flush(&mut blocks, &mut para, &mut para_lines);
                continue;
            }
            if let Some(item) = bullet_text(&line) {
                flush(&mut blocks, &mut para, &mut para_lines);
                list_counter += 1;
                blocks.push(Block::ListItem {
                    ordered: false,
                    number: list_counter,
                    level: 0,
                    runs: vec![Run::plain(item.to_string())],
                });
                continue;
            }
            list_counter = 0;
            join_line(&mut para, &line);
            para_lines += 1;
        }
        flush(&mut blocks, &mut para, &mut para_lines);
        if page_index + 1 < page_count && !page.trim().is_empty() {
            blocks.push(Block::PageBreak);
        }
    }
    flush(&mut blocks, &mut para, &mut para_lines);
    while matches!(blocks.last(), Some(Block::PageBreak)) {
        blocks.pop();
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_lines_and_hyphens() {
        let blocks = structure_text("INTRODUCTION\n\nThis is a long sen-\ntence that\ncontinues.\n\n- item one\n- item two\n");
        assert!(matches!(&blocks[0], Block::Heading { level: 2, .. }));
        match &blocks[1] {
            Block::Paragraph(runs) => assert_eq!(runs[0].text, "This is a long sentence that continues."),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(&blocks[2], Block::ListItem { .. }));
        assert!(matches!(&blocks[3], Block::ListItem { .. }));
    }
}
