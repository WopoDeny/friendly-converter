//! DOCX reader: paragraphs, headings, bold/italic runs, lists, tables,
//! page breaks and embedded pictures.

use super::{clean_text, normalize_runs, runs_text, Block, Document, ImageData, Run, Table};
use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

pub fn read(path: &Path) -> Result<Document> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).context("not a valid DOCX (zip) file")?;

    let document_xml = read_string(&mut archive, "word/document.xml")
        .context("word/document.xml is missing: this is not a Word document")?;
    let styles = read_string(&mut archive, "word/styles.xml").map(|x| parse_styles(&x)).unwrap_or_default();
    let numbering = read_string(&mut archive, "word/numbering.xml").map(|x| parse_numbering(&x)).unwrap_or_default();
    let rels = read_string(&mut archive, "word/_rels/document.xml.rels").map(|x| parse_rels(&x)).unwrap_or_default();

    let mut parser = BodyParser {
        styles,
        numbering,
        rels,
        archive: &mut archive,
        st: State::default(),
    };
    parser.parse(&document_xml)?;

    let mut doc = Document { title: None, blocks: parser.st.blocks };
    doc.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    Ok(doc)
}

fn read_bytes<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn read_string<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Option<String> {
    read_bytes(archive, name).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.local_name().as_ref() == name {
            return a.unescape_value().ok().map(|v| v.into_owned());
        }
    }
    None
}

fn on_off(e: &BytesStart) -> bool {
    !matches!(attr(e, b"val").as_deref(), Some("0") | Some("false") | Some("off") | Some("none"))
}

/// styleId -> heading level
fn parse_styles(xml: &str) -> HashMap<String, u8> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"style" => current = attr(&e, b"styleId"),
                b"name" => {
                    if let (Some(id), Some(name)) = (&current, attr(&e, b"val")) {
                        let name = name.to_ascii_lowercase();
                        let level = if let Some(n) = name.strip_prefix("heading ") {
                            n.trim().parse::<u8>().ok()
                        } else if name == "title" {
                            Some(1)
                        } else if name == "subtitle" {
                            Some(2)
                        } else {
                            None
                        };
                        if let Some(l) = level {
                            map.insert(id.clone(), l.clamp(1, 6));
                        }
                    }
                }
                b"outlineLvl" => {
                    if let (Some(id), Some(v)) = (&current, attr(&e, b"val")) {
                        if let Ok(v) = v.parse::<u8>() {
                            if v < 9 {
                                map.entry(id.clone()).or_insert((v + 1).min(6));
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == b"style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// numId -> per-level "is ordered" flags
fn parse_numbering(xml: &str) -> HashMap<String, Vec<bool>> {
    let mut abstracts: HashMap<String, Vec<bool>> = HashMap::new();
    let mut nums: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut cur_abstract: Option<String> = None;
    let mut cur_level: usize = 0;
    let mut cur_num: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"abstractNum" => cur_abstract = attr(&e, b"abstractNumId"),
                b"lvl" => cur_level = attr(&e, b"ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                b"numFmt" => {
                    if let Some(a) = &cur_abstract {
                        let fmt = attr(&e, b"val").unwrap_or_default();
                        let ordered = fmt != "bullet" && fmt != "none";
                        let levels = abstracts.entry(a.clone()).or_default();
                        if levels.len() <= cur_level {
                            levels.resize(cur_level + 1, false);
                        }
                        levels[cur_level] = ordered;
                    }
                }
                b"num" => cur_num = attr(&e, b"numId"),
                b"abstractNumId" => {
                    if let (Some(n), Some(v)) = (&cur_num, attr(&e, b"val")) {
                        nums.insert(n.clone(), v);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"abstractNum" => cur_abstract = None,
                b"num" => cur_num = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    nums.into_iter()
        .filter_map(|(num, abs)| abstracts.get(&abs).map(|l| (num, l.clone())))
        .collect()
}

/// relationship id -> zip path
fn parse_rels(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"TargetMode").as_deref() == Some("External") {
                    continue;
                }
                if let (Some(id), Some(target)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                    map.insert(id, resolve_target(&target));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

fn resolve_target(target: &str) -> String {
    let full = if let Some(abs) = target.strip_prefix('/') {
        abs.to_string()
    } else {
        format!("word/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in full.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[derive(Default)]
struct TableBuild {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: Vec<String>,
    explicit_header: bool,
}

#[derive(Default)]
struct State {
    blocks: Vec<Block>,
    skip: usize,

    in_p: bool,
    p_depth: usize,
    in_ppr: bool,
    p_style: Option<String>,
    outline: Option<u8>,
    num_id: Option<String>,
    ilvl: u8,
    runs: Vec<Run>,
    break_after: bool,
    images: Vec<String>,

    in_r: bool,
    in_rpr: bool,
    in_t: bool,
    bold: bool,
    italic: bool,
    run_text: String,

    tables: Vec<TableBuild>,
    pending_images: Vec<Block>,
    counters: HashMap<(String, u8), usize>,
}

struct BodyParser<'a, R: Read + Seek> {
    styles: HashMap<String, u8>,
    numbering: HashMap<String, Vec<bool>>,
    rels: HashMap<String, String>,
    archive: &'a mut ZipArchive<R>,
    st: State,
}

impl<'a, R: Read + Seek> BodyParser<'a, R> {
    fn parse(&mut self, xml: &str) -> Result<()> {
        let mut reader = Reader::from_str(xml);
        loop {
            let event = reader.read_event().context("broken document.xml")?;
            match event {
                Event::Start(e) => {
                    if self.st.skip > 0 {
                        self.st.skip += 1;
                        continue;
                    }
                    self.open(&e, false);
                }
                Event::Empty(e) => {
                    if self.st.skip > 0 {
                        continue;
                    }
                    self.open(&e, true);
                }
                Event::End(e) => {
                    if self.st.skip > 0 {
                        self.st.skip -= 1;
                        continue;
                    }
                    let name = e.local_name();
                    self.close(name.as_ref());
                }
                Event::Text(t) => {
                    if self.st.skip == 0 && self.st.in_t {
                        let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                        self.st.run_text.push_str(&text);
                    }
                }
                Event::CData(t) => {
                    if self.st.skip == 0 && self.st.in_t {
                        self.st.run_text.push_str(&String::from_utf8_lossy(&t));
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        // Unclosed tables in a broken file: emit what we have.
        while let Some(t) = self.st.tables.pop() {
            if !t.rows.is_empty() {
                self.st.blocks.push(Block::Table(Table { rows: t.rows, header: false }));
            }
        }
        Ok(())
    }

    fn open(&mut self, e: &BytesStart, empty: bool) {
        let st = &mut self.st;
        match e.local_name().as_ref() {
            // Alternate (VML) copies of drawings would duplicate text boxes.
            b"Fallback" if !empty => st.skip = 1,
            b"p" if !empty => {
                if st.in_p {
                    st.p_depth += 1;
                    Self::flush_run(st);
                    st.runs.push(Run::plain("\n"));
                } else {
                    st.in_p = true;
                    st.p_style = None;
                    st.outline = None;
                    st.num_id = None;
                    st.ilvl = 0;
                    st.runs.clear();
                    st.images.clear();
                    st.break_after = false;
                }
            }
            b"pPr" if !empty && st.in_p && !st.in_r => st.in_ppr = true,
            b"pStyle" if st.in_ppr => st.p_style = attr(e, b"val"),
            b"outlineLvl" if st.in_ppr => {
                st.outline = attr(e, b"val").and_then(|v| v.parse::<u8>().ok()).filter(|v| *v < 9).map(|v| (v + 1).min(6))
            }
            b"numId" if st.in_ppr => st.num_id = attr(e, b"val"),
            b"ilvl" if st.in_ppr => st.ilvl = attr(e, b"val").and_then(|v| v.parse().ok()).unwrap_or(0),
            b"r" if !empty => {
                Self::flush_run(st);
                st.in_r = true;
                st.bold = false;
                st.italic = false;
            }
            b"rPr" if !empty && st.in_r => st.in_rpr = true,
            b"b" if st.in_rpr => st.bold = on_off(e),
            b"i" if st.in_rpr => st.italic = on_off(e),
            b"t" if !empty && st.in_r => st.in_t = true,
            b"tab" if st.in_r && !st.in_ppr => st.run_text.push('\t'),
            b"cr" if st.in_r => st.run_text.push('\n'),
            b"br" if st.in_r => {
                if attr(e, b"type").as_deref() == Some("page") {
                    let nothing_yet = st.runs.iter().all(|r| r.text.trim().is_empty()) && st.run_text.trim().is_empty();
                    if nothing_yet && st.tables.is_empty() {
                        st.blocks.push(Block::PageBreak);
                    } else {
                        st.break_after = true;
                    }
                } else {
                    st.run_text.push('\n');
                }
            }
            b"blip" => {
                if let Some(id) = attr(e, b"embed") {
                    st.images.push(id);
                }
            }
            b"imagedata" => {
                if let Some(id) = attr(e, b"id") {
                    st.images.push(id);
                }
            }
            b"tbl" if !empty => st.tables.push(TableBuild::default()),
            b"tr" if !empty => {
                if let Some(t) = st.tables.last_mut() {
                    t.row = Vec::new();
                }
            }
            b"tblHeader" => {
                if let Some(t) = st.tables.last_mut() {
                    if t.rows.is_empty() {
                        t.explicit_header = true;
                    }
                }
            }
            b"tc" if !empty => {
                if let Some(t) = st.tables.last_mut() {
                    t.cell = Vec::new();
                }
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &[u8]) {
        match name {
            b"t" => self.st.in_t = false,
            b"rPr" => self.st.in_rpr = false,
            b"pPr" => self.st.in_ppr = false,
            b"r" => {
                Self::flush_run(&mut self.st);
                self.st.in_r = false;
                self.st.in_rpr = false;
                self.st.in_t = false;
            }
            b"p" => {
                if self.st.p_depth > 0 {
                    self.st.p_depth -= 1;
                    Self::flush_run(&mut self.st);
                    self.st.runs.push(Run::plain("\n"));
                } else {
                    self.finish_paragraph();
                }
            }
            b"tc" => {
                if let Some(t) = self.st.tables.last_mut() {
                    let text = t.cell.join("\n");
                    t.row.push(text);
                    t.cell.clear();
                }
            }
            b"tr" => {
                if let Some(t) = self.st.tables.last_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            b"tbl" => {
                if let Some(t) = self.st.tables.pop() {
                    if let Some(parent) = self.st.tables.last_mut() {
                        // Nested table: flatten into the parent cell.
                        for row in t.rows {
                            parent.cell.push(row.join(" | "));
                        }
                    } else if !t.rows.is_empty() {
                        let header = t.explicit_header || t.rows.len() > 1;
                        self.st.blocks.push(Block::Table(Table { rows: t.rows, header }));
                        let pending = std::mem::take(&mut self.st.pending_images);
                        self.st.blocks.extend(pending);
                    }
                }
            }
            _ => {}
        }
    }

    fn flush_run(st: &mut State) {
        if st.run_text.is_empty() {
            return;
        }
        let text = clean_text(&std::mem::take(&mut st.run_text));
        st.runs.push(Run { text, bold: st.bold, italic: st.italic, code: false });
    }

    fn load_image(&mut self, rel_id: &str) -> Option<ImageData> {
        let path = self.rels.get(rel_id)?.clone();
        let bytes = read_bytes(&mut *self.archive, &path)?;
        ImageData::from_bytes(bytes)
    }

    fn finish_paragraph(&mut self) {
        Self::flush_run(&mut self.st);
        self.st.in_p = false;
        self.st.in_ppr = false;
        let runs = normalize_runs(std::mem::take(&mut self.st.runs));
        let image_ids = std::mem::take(&mut self.st.images);
        let images: Vec<Block> = image_ids.iter().filter_map(|id| self.load_image(id)).map(Block::Image).collect();
        let st = &mut self.st;

        if let Some(table) = st.tables.last_mut() {
            table.cell.push(runs_text(&runs).trim_end().to_string());
            st.pending_images.extend(images);
            return;
        }

        let has_text = runs.iter().any(|r| !r.text.trim().is_empty());
        if has_text {
            let heading = st.outline.or_else(|| st.p_style.as_ref().and_then(|s| self.styles.get(s).copied()));
            let numbered = st.num_id.clone().filter(|id| id != "0");
            if let Some(level) = heading {
                st.blocks.push(Block::Heading { level, runs: trim_runs(runs) });
            } else if let Some(num_id) = numbered {
                let level = st.ilvl.min(8);
                let ordered = self
                    .numbering
                    .get(&num_id)
                    .and_then(|levels| levels.get(level as usize).copied())
                    .unwrap_or(false);
                st.counters.retain(|(id, l), _| !(id == &num_id && *l > level));
                let counter = st.counters.entry((num_id, level)).or_insert(0);
                *counter += 1;
                st.blocks.push(Block::ListItem { ordered, number: *counter, level, runs: trim_runs(runs) });
            } else {
                st.blocks.push(Block::Paragraph(trim_runs(runs)));
            }
        }
        st.blocks.extend(images);
        if st.break_after {
            st.blocks.push(Block::PageBreak);
            st.break_after = false;
        }
    }
}

fn trim_runs(mut runs: Vec<Run>) -> Vec<Run> {
    while runs.first().map(|r| r.text.trim().is_empty()).unwrap_or(false) {
        runs.remove(0);
    }
    while runs.last().map(|r| r.text.trim().is_empty()).unwrap_or(false) {
        runs.pop();
    }
    if let Some(first) = runs.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = runs.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    runs
}
