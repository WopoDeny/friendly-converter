//! Markdown reader (CommonMark + tables + strikethrough).

use super::{clean_text, normalize_runs, Block, Document, ImageData, Run, Table};
use crate::textenc::read_text_file;
use anyhow::Result;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::Path;

pub fn read(path: &Path) -> Result<Document> {
    let text = read_text_file(path)?;
    let mut doc = from_markdown(&text, path.parent());
    doc.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    Ok(doc)
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

struct ListState {
    ordered: bool,
    next: usize,
}

pub fn from_markdown(text: &str, base_dir: Option<&Path>) -> Document {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(text, options);

    let mut doc = Document::default();
    let mut runs: Vec<Run> = Vec::new();
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut heading: Option<u8> = None;
    let mut lists: Vec<ListState> = Vec::new();
    let mut item_number: Option<usize> = None;
    let mut in_code_block = false;
    let mut code = String::new();
    let mut table: Option<Table> = None;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_cell = false;
    let mut image_depth = 0usize;
    let mut pending_images: Vec<Block> = Vec::new();

    let push_text = |runs: &mut Vec<Run>, s: &str, bold: usize, italic: usize, code: bool| {
        runs.push(Run { text: clean_text(s), bold: bold > 0, italic: italic > 0, code });
    };

    fn flush_item(doc: &mut Document, runs: &mut Vec<Run>, lists: &[ListState], item_number: &mut Option<usize>) {
        let collected = normalize_runs(std::mem::take(runs));
        if collected.iter().all(|r| r.text.trim().is_empty()) {
            return;
        }
        let level = lists.len().saturating_sub(1).min(8) as u8;
        let ordered = lists.last().map(|l| l.ordered).unwrap_or(false);
        let number = item_number.take().unwrap_or(1);
        doc.blocks.push(Block::ListItem { ordered, number, level, runs: collected });
    }

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => heading = Some(heading_level(level)),
                Tag::Strong => bold += 1,
                Tag::Emphasis => italic += 1,
                Tag::CodeBlock(_) => {
                    in_code_block = true;
                    code.clear();
                }
                Tag::List(start) => {
                    if !runs.is_empty() && !lists.is_empty() {
                        flush_item(&mut doc, &mut runs, &lists, &mut item_number);
                    }
                    lists.push(ListState { ordered: start.is_some(), next: start.unwrap_or(1) as usize });
                }
                Tag::Item => {
                    if let Some(list) = lists.last_mut() {
                        item_number = Some(list.next);
                        list.next += 1;
                    }
                }
                Tag::Table(_) => table = Some(Table { rows: Vec::new(), header: true }),
                Tag::TableHead | Tag::TableRow => row.clear(),
                Tag::TableCell => {
                    cell.clear();
                    in_cell = true;
                }
                Tag::Image { dest_url, .. } => {
                    image_depth += 1;
                    let url = dest_url.to_string();
                    if let Some(img) = load_local_image(&url, base_dir) {
                        pending_images.push(Block::Image(img));
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    let level = heading.take().unwrap_or(1);
                    let collected = normalize_runs(std::mem::take(&mut runs));
                    doc.blocks.push(Block::Heading { level, runs: collected });
                }
                TagEnd::Paragraph => {
                    if lists.is_empty() {
                        let collected = normalize_runs(std::mem::take(&mut runs));
                        if !collected.is_empty() {
                            doc.blocks.push(Block::Paragraph(collected));
                        }
                        doc.blocks.append(&mut pending_images);
                    } else {
                        runs.push(Run::plain(" "));
                    }
                }
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    let c = code.trim_end_matches('\n').to_string();
                    doc.blocks.push(Block::Code(c));
                }
                TagEnd::Item => {
                    flush_item(&mut doc, &mut runs, &lists, &mut item_number);
                    doc.blocks.append(&mut pending_images);
                }
                TagEnd::List(_) => {
                    lists.pop();
                }
                TagEnd::TableCell => {
                    in_cell = false;
                    row.push(cell.trim().to_string());
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    if let Some(t) = table.as_mut() {
                        t.rows.push(std::mem::take(&mut row));
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = table.take() {
                        doc.blocks.push(Block::Table(t));
                    }
                }
                TagEnd::Image => image_depth = image_depth.saturating_sub(1),
                _ => {}
            },
            Event::Text(t) => {
                if image_depth > 0 {
                    continue;
                }
                if in_code_block {
                    code.push_str(&t);
                } else if in_cell {
                    cell.push_str(&t);
                } else {
                    push_text(&mut runs, &t, bold, italic, false);
                }
            }
            Event::Code(t) => {
                if in_cell {
                    cell.push_str(&t);
                } else {
                    push_text(&mut runs, &t, bold, italic, true);
                }
            }
            Event::SoftBreak => {
                if in_cell {
                    cell.push(' ');
                } else {
                    push_text(&mut runs, " ", bold, italic, false);
                }
            }
            Event::HardBreak => {
                if in_cell {
                    cell.push(' ');
                } else {
                    push_text(&mut runs, "\n", bold, italic, false);
                }
            }
            Event::Rule => doc.blocks.push(Block::Rule),
            Event::TaskListMarker(done) => {
                push_text(&mut runs, if done { "[x] " } else { "[ ] " }, 0, 0, false);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                let lower = h.to_ascii_lowercase();
                if lower.contains("page-break") || lower.contains("pagebreak") {
                    doc.blocks.push(Block::PageBreak);
                }
            }
            _ => {}
        }
    }
    let rest = normalize_runs(runs);
    if !rest.is_empty() {
        doc.blocks.push(Block::Paragraph(rest));
    }
    doc.blocks.append(&mut pending_images);
    doc
}

fn load_local_image(url: &str, base_dir: Option<&Path>) -> Option<ImageData> {
    if url.starts_with("http://") || url.starts_with("https://") {
        return None;
    }
    if let Some(data) = url.strip_prefix("data:") {
        use base64::Engine;
        let (_, b64) = data.split_once(";base64,")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
        return ImageData::from_bytes(bytes);
    }
    let path = Path::new(url);
    let full = if path.is_absolute() { path.to_path_buf() } else { base_dir?.join(path) };
    let bytes = std::fs::read(full).ok()?;
    ImageData::from_bytes(bytes)
}
