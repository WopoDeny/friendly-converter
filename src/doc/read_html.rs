//! Small tolerant HTML reader. It is not a browser: it keeps text structure
//! (headings, paragraphs, lists, tables, code, bold/italic) and ignores styling.

use super::{clean_text, normalize_runs, Block, Document, ImageData, Run, Table};
use crate::textenc::read_text_file;
use anyhow::Result;
use std::path::Path;

pub fn read(path: &Path) -> Result<Document> {
    let text = read_text_file(path)?;
    let mut doc = from_html(&text);
    if doc.title.is_none() {
        doc.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    }
    Ok(doc)
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let end = after.find(';').filter(|&e| e <= 12);
        if let Some(end) = end {
            let entity = &after[1..end];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "laquo" => Some('\u{00AB}'),
                "raquo" => Some('\u{00BB}'),
                "mdash" => Some('\u{2014}'),
                "ndash" => Some('\u{2013}'),
                "hellip" => Some('\u{2026}'),
                "copy" => Some('\u{00A9}'),
                "reg" => Some('\u{00AE}'),
                "trade" => Some('\u{2122}'),
                "bull" => Some('\u{2022}'),
                "middot" => Some('\u{00B7}'),
                "euro" => Some('\u{20AC}'),
                "deg" => Some('\u{00B0}'),
                _ => {
                    if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else if let Some(dec) = entity.strip_prefix('#') {
                        dec.parse::<u32>().ok().and_then(char::from_u32)
                    } else {
                        None
                    }
                }
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

struct Tag<'a> {
    name: String,
    closing: bool,
    raw: &'a str,
}

fn parse_tag(raw: &str) -> Tag<'_> {
    let inner = raw.trim_start_matches('<').trim_end_matches('>').trim();
    let closing = inner.starts_with('/');
    let inner = inner.trim_start_matches('/');
    let name: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    Tag { name, closing, raw }
}

fn tag_attr(raw: &str, name: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let key = format!("{name}=");
    let pos = lower.find(&key)?;
    let rest = &raw[pos + key.len()..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let rest = &rest[1..];
        let end = rest.find(quote)?;
        Some(decode_entities(&rest[..end]))
    } else {
        let end = rest.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(rest.len());
        Some(decode_entities(&rest[..end]))
    }
}

#[derive(Default)]
struct Builder {
    doc: Document,
    runs: Vec<Run>,
    bold: usize,
    italic: usize,
    code: usize,
    heading: Option<u8>,
    lists: Vec<(bool, usize)>,
    in_item: bool,
    pre: usize,
    pre_text: String,
    table: Option<Table>,
    row: Vec<String>,
    cell: Option<String>,
    title: Option<String>,
    in_title: bool,
}

impl Builder {
    fn text(&mut self, raw: &str) {
        let decoded = decode_entities(raw);
        if self.in_title {
            self.title.get_or_insert_with(String::new).push_str(decoded.trim());
            return;
        }
        if self.pre > 0 {
            self.pre_text.push_str(&decoded);
            return;
        }
        let mut collapsed = String::with_capacity(decoded.len());
        let mut prev_space = false;
        for c in decoded.chars() {
            if c.is_whitespace() {
                if !prev_space {
                    collapsed.push(' ');
                }
                prev_space = true;
            } else {
                collapsed.push(c);
                prev_space = false;
            }
        }
        if let Some(cell) = self.cell.as_mut() {
            cell.push_str(&collapsed);
            return;
        }
        if collapsed.trim().is_empty() && self.runs.is_empty() {
            return;
        }
        self.runs.push(Run {
            text: clean_text(&collapsed),
            bold: self.bold > 0,
            italic: self.italic > 0,
            code: self.code > 0,
        });
    }

    fn flush(&mut self) {
        let mut runs = normalize_runs(std::mem::take(&mut self.runs));
        if let Some(first) = runs.first_mut() {
            first.text = first.text.trim_start().to_string();
        }
        if let Some(last) = runs.last_mut() {
            last.text = last.text.trim_end().to_string();
        }
        runs.retain(|r| !r.text.is_empty());
        if runs.is_empty() {
            return;
        }
        if let Some(level) = self.heading {
            self.doc.blocks.push(Block::Heading { level, runs });
        } else if self.in_item {
            let (ordered, number) = self.lists.last().copied().unwrap_or((false, 1));
            let level = self.lists.len().saturating_sub(1).min(8) as u8;
            self.doc.blocks.push(Block::ListItem { ordered, number, level, runs });
        } else {
            self.doc.blocks.push(Block::Paragraph(runs));
        }
    }
}

pub fn from_html(html: &str) -> Document {
    let mut b = Builder::default();
    let mut i = 0usize;
    let bytes = html.as_bytes();
    let mut skip_until: Option<&'static str> = None;

    while i < html.len() {
        if bytes[i] == b'<' {
            // Comments and doctype.
            if html[i..].starts_with("<!--") {
                i = html[i..].find("-->").map(|e| i + e + 3).unwrap_or(html.len());
                continue;
            }
            let end = match html[i..].find('>') {
                Some(e) => i + e + 1,
                None => break,
            };
            let raw = &html[i..end];
            i = end;
            if raw.starts_with("<!") || raw.starts_with("<?") {
                continue;
            }
            let tag = parse_tag(raw);
            if let Some(stop) = skip_until {
                if tag.closing && tag.name == stop {
                    skip_until = None;
                }
                continue;
            }
            handle_tag(&mut b, &tag, &mut skip_until);
        } else {
            let next = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
            if skip_until.is_none() {
                b.text(&html[i..next]);
            }
            i = next;
        }
    }
    b.flush();
    if let Some(t) = b.table.take() {
        b.doc.blocks.push(Block::Table(t));
    }
    b.doc.title = b.title.filter(|t| !t.is_empty());
    b.doc
}

fn handle_tag(b: &mut Builder, tag: &Tag, skip_until: &mut Option<&'static str>) {
    let name = tag.name.as_str();
    match (name, tag.closing) {
        ("script", false) => *skip_until = Some("script"),
        ("style", false) => *skip_until = Some("style"),
        ("head", false) => {}
        ("title", false) => b.in_title = true,
        ("title", true) => b.in_title = false,
        ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
            b.flush();
            b.heading = name[1..].parse().ok();
        }
        ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
            b.flush();
            b.heading = None;
        }
        ("p" | "div" | "section" | "article" | "header" | "footer" | "blockquote" | "dd" | "dt", _) => b.flush(),
        ("br", _) => {
            if let Some(cell) = b.cell.as_mut() {
                cell.push(' ');
            } else if b.pre > 0 {
                b.pre_text.push('\n');
            } else {
                b.runs.push(Run::plain("\n"));
            }
        }
        ("hr", _) => {
            b.flush();
            b.doc.blocks.push(Block::Rule);
        }
        ("b" | "strong", false) => b.bold += 1,
        ("b" | "strong", true) => b.bold = b.bold.saturating_sub(1),
        ("i" | "em", false) => b.italic += 1,
        ("i" | "em", true) => b.italic = b.italic.saturating_sub(1),
        ("code" | "kbd" | "tt", false) => b.code += 1,
        ("code" | "kbd" | "tt", true) => b.code = b.code.saturating_sub(1),
        ("pre", false) => {
            b.flush();
            b.pre += 1;
        }
        ("pre", true) => {
            b.pre = b.pre.saturating_sub(1);
            if b.pre == 0 {
                let text = decode_entities(&std::mem::take(&mut b.pre_text));
                let text = text.trim_matches('\n').to_string();
                if !text.is_empty() {
                    b.doc.blocks.push(Block::Code(clean_text(&text)));
                }
            }
        }
        ("ul", false) => {
            b.flush();
            b.lists.push((false, 0));
        }
        ("ol", false) => {
            b.flush();
            let start = tag_attr(tag.raw, "start").and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
            b.lists.push((true, start.saturating_sub(1)));
        }
        ("ul" | "ol", true) => {
            b.flush();
            b.lists.pop();
            b.in_item = !b.lists.is_empty();
        }
        ("li", false) => {
            b.flush();
            if let Some(last) = b.lists.last_mut() {
                last.1 += 1;
            }
            b.in_item = true;
        }
        ("li", true) => b.flush(),
        ("table", false) => {
            b.flush();
            b.table = Some(Table { rows: Vec::new(), header: false });
        }
        ("table", true) => {
            if let Some(t) = b.table.take() {
                if !t.rows.is_empty() {
                    b.doc.blocks.push(Block::Table(t));
                }
            }
        }
        ("tr", false) => b.row.clear(),
        ("tr", true) => {
            if let Some(t) = b.table.as_mut() {
                let row = std::mem::take(&mut b.row);
                if !row.is_empty() {
                    t.rows.push(row);
                }
            }
        }
        ("td" | "th", false) => {
            if name == "th" {
                if let Some(t) = b.table.as_mut() {
                    if t.rows.is_empty() {
                        t.header = true;
                    }
                }
            }
            b.cell = Some(String::new());
        }
        ("td" | "th", true) => {
            if let Some(cell) = b.cell.take() {
                b.row.push(cell.trim().to_string());
            }
        }
        ("img", _) => {
            if let Some(src) = tag_attr(tag.raw, "src") {
                if let Some(data) = src.strip_prefix("data:") {
                    use base64::Engine;
                    if let Some((_, b64)) = data.split_once(";base64,") {
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) {
                            if let Some(img) = ImageData::from_bytes(bytes) {
                                b.flush();
                                b.doc.blocks.push(Block::Image(img));
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_html() {
        let doc = from_html(
            "<html><head><title>T</title><style>p{}</style></head><body><h1>Head</h1><p>Hello <b>bold</b> &amp; more</p><ul><li>One</li><li>Two</li></ul><table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table></body></html>",
        );
        assert_eq!(doc.title.as_deref(), Some("T"));
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
        assert!(doc.plain_text().contains("Hello bold & more"));
        assert!(doc.blocks.iter().any(|b| matches!(b, Block::Table(t) if t.rows.len() == 2 && t.header)));
        assert_eq!(doc.blocks.iter().filter(|b| matches!(b, Block::ListItem { .. })).count(), 2);
    }
}
