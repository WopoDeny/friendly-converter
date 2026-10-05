//! Lays out a [`Document`] onto A4 pages: word wrapping, headings, lists,
//! code blocks, tables with repeated header rows, images and page numbers.

use super::{Block, Document, ImageData, Run, Table};
use crate::pdf::canvas::Rgb;
use crate::pdf::fonts::PdfFont;
use crate::pdf::images::add_image;
use crate::pdf::Canvas;
use anyhow::Result;
use std::path::Path;

const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN_X: f32 = 56.0;
const MARGIN_TOP: f32 = 56.0;
const MARGIN_BOTTOM: f32 = 60.0;
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN_X;
const CONTENT_H: f32 = PAGE_H - MARGIN_TOP - MARGIN_BOTTOM;

const REGULAR: usize = 0;
const BOLD: usize = 1;
const ITALIC: usize = 2;
const BOLD_ITALIC: usize = 3;
const MONO: usize = 4;

const TEXT: Rgb = [0.10, 0.11, 0.10];
const HEADING: Rgb = [0.02, 0.03, 0.02];
const MUTED: Rgb = [0.45, 0.48, 0.46];
const BORDER: Rgb = [0.63, 0.67, 0.64];
const HEADER_FILL: Rgb = [0.91, 0.93, 0.915];
const CODE_FILL: Rgb = [0.95, 0.96, 0.95];

const BODY_SIZE: f32 = 10.5;

#[derive(Default)]
struct Seg {
    text: String,
    font: usize,
}

#[derive(Default)]
struct Line {
    segs: Vec<Seg>,
    width: f32,
}

impl Line {
    fn push(&mut self, text: &str, font: usize, width: f32) {
        if let Some(last) = self.segs.last_mut() {
            if last.font == font {
                last.text.push_str(text);
                self.width += width;
                return;
            }
        }
        self.segs.push(Seg { text: text.to_string(), font });
        self.width += width;
    }
}

/// Splits into "word + trailing spaces" tokens; leading spaces form their own token.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_space = false;
    for (i, c) in s.char_indices() {
        let space = c == ' ';
        if !space && in_space && i > start {
            out.push(&s[start..i]);
            start = i;
        }
        in_space = space;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

struct Layout {
    c: Canvas,
    y: f32,
    empty_page: bool,
}

impl Layout {
    fn new() -> Result<Layout> {
        let c = Canvas::new(PdfFont::standard_set()?);
        let mut l = Layout { c, y: 0.0, empty_page: true };
        l.new_page();
        Ok(l)
    }

    fn new_page(&mut self) {
        self.c.add_page(PAGE_W, PAGE_H);
        self.y = PAGE_H - MARGIN_TOP;
        self.empty_page = true;
    }

    fn space_left(&self) -> f32 {
        self.y - MARGIN_BOTTOM
    }

    fn ensure(&mut self, h: f32) {
        if h > self.space_left() && !self.empty_page {
            self.new_page();
        }
    }

    fn gap(&mut self, h: f32) {
        if !self.empty_page {
            self.y -= h;
        }
    }

    fn font_for(run: &Run, force_bold: bool) -> usize {
        if run.code {
            return MONO;
        }
        match (run.bold || force_bold, run.italic) {
            (false, false) => REGULAR,
            (true, false) => BOLD,
            (false, true) => ITALIC,
            (true, true) => BOLD_ITALIC,
        }
    }

    fn wrap(&mut self, runs: &[Run], size: f32, width: f32, force_bold: bool) -> Vec<Line> {
        let width = width.max(10.0);
        let mut lines = Vec::new();
        let mut cur = Line::default();
        let mut any_text = false;
        for run in runs {
            let font = Self::font_for(run, force_bold);
            let text = run.text.replace('\t', "    ");
            for (pi, piece) in text.split('\n').enumerate() {
                if pi > 0 {
                    lines.push(std::mem::take(&mut cur));
                }
                for token in tokens(piece) {
                    any_text = true;
                    let w_full = self.c.measure(font, size, token);
                    let w_trim = self.c.measure(font, size, token.trim_end());
                    if cur.width + w_trim > width && !cur.segs.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        if token.trim().is_empty() {
                            continue;
                        }
                    }
                    if w_trim > width {
                        let mut buf = [0u8; 4];
                        for ch in token.chars() {
                            let s: &str = ch.encode_utf8(&mut buf);
                            let cw = self.c.measure(font, size, s);
                            if cur.width + cw > width && !cur.segs.is_empty() {
                                lines.push(std::mem::take(&mut cur));
                            }
                            cur.push(s, font, cw);
                        }
                        continue;
                    }
                    cur.push(token, font, w_full);
                }
            }
        }
        if !cur.segs.is_empty() || (any_text && lines.is_empty()) {
            lines.push(cur);
        }
        lines
    }

    fn draw_line_at(&mut self, line: &Line, x: f32, baseline: f32, size: f32, color: Rgb) {
        let mut cx = x;
        for seg in &line.segs {
            let trimmed = seg.text.trim_end_matches(' ');
            self.c.text(seg.font, size, cx, baseline, trimmed, color);
            cx += self.c.measure(seg.font, size, &seg.text);
        }
    }

    fn draw_lines(&mut self, lines: &[Line], x: f32, size: f32, lh: f32, color: Rgb) {
        for line in lines {
            self.ensure(lh);
            let baseline = self.y - size * 1.02;
            self.draw_line_at(line, x, baseline, size, color);
            self.y -= lh;
            self.empty_page = false;
        }
    }

    fn heading(&mut self, level: u8, runs: &[Run]) {
        let size = match level {
            1 => 20.0,
            2 => 16.0,
            3 => 13.5,
            4 => 12.0,
            5 => 11.0,
            _ => 10.5,
        };
        let lh = size * 1.3;
        self.gap(match level {
            1 => 16.0,
            2 => 13.0,
            _ => 10.0,
        });
        // Keep the heading together with at least two lines of the next block.
        self.ensure(lh + BODY_SIZE * 3.2);
        let lines = self.wrap(runs, size, CONTENT_W, true);
        self.draw_lines(&lines, MARGIN_X, size, lh, HEADING);
        if level == 1 {
            let y = self.y - 2.0;
            self.c.line(MARGIN_X, y, MARGIN_X + CONTENT_W, y, 1.2, HEADING);
            self.y -= 6.0;
        }
        self.y -= 4.0;
    }

    fn paragraph(&mut self, runs: &[Run]) {
        let lh = BODY_SIZE * 1.45;
        let lines = self.wrap(runs, BODY_SIZE, CONTENT_W, false);
        self.draw_lines(&lines, MARGIN_X, BODY_SIZE, lh, TEXT);
        self.y -= 6.0;
    }

    fn list_item(&mut self, ordered: bool, number: usize, level: u8, runs: &[Run]) {
        let lh = BODY_SIZE * 1.45;
        let indent = 20.0 + level as f32 * 18.0;
        let marker = if ordered {
            format!("{number}.")
        } else {
            ["\u{2022}", "\u{25E6}", "\u{25AA}"][(level % 3) as usize].to_string()
        };
        let lines = self.wrap(runs, BODY_SIZE, CONTENT_W - indent, false);
        if lines.is_empty() {
            return;
        }
        self.ensure(lh);
        let marker_w = self.c.measure(REGULAR, BODY_SIZE, &marker);
        let baseline = self.y - BODY_SIZE * 1.02;
        self.c.text(REGULAR, BODY_SIZE, MARGIN_X + indent - 6.0 - marker_w, baseline, &marker, TEXT);
        self.draw_lines(&lines, MARGIN_X + indent, BODY_SIZE, lh, TEXT);
        self.y -= 2.0;
    }

    fn code(&mut self, code: &str) {
        let size = 8.8;
        let lh = size * 1.38;
        let run = Run { text: code.to_string(), code: true, ..Default::default() };
        let lines = self.wrap(&[run], size, CONTENT_W - 12.0, false);
        self.gap(2.0);
        for line in &lines {
            self.ensure(lh);
            self.c.fill_rect(MARGIN_X, self.y - lh, CONTENT_W, lh, CODE_FILL);
            let baseline = self.y - size * 1.05;
            self.draw_line_at(line, MARGIN_X + 6.0, baseline, size, TEXT);
            self.y -= lh;
            self.empty_page = false;
        }
        self.y -= 8.0;
    }

    fn layout_row(&mut self, row: &[String], widths: &[f32], size: f32, pad: f32, header: bool) -> Vec<Vec<Line>> {
        let mut cells = Vec::with_capacity(widths.len());
        for (ci, w) in widths.iter().enumerate() {
            let text = row.get(ci).cloned().unwrap_or_default();
            let run = Run { text, bold: header, ..Default::default() };
            cells.push(self.wrap(&[run], size, w - 2.0 * pad, header));
        }
        cells
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(&mut self, cells: &[Vec<Line>], widths: &[f32], row_h: f32, size: f32, lh: f32, pad: f32, header: bool) {
        let top = self.y;
        let total_w: f32 = widths.iter().sum();
        if header {
            self.c.fill_rect(MARGIN_X, top - row_h, total_w, row_h, HEADER_FILL);
        }
        let mut x = MARGIN_X;
        let bottom_limit = (top - row_h).max(MARGIN_BOTTOM - 30.0);
        for (ci, lines) in cells.iter().enumerate() {
            let w = widths[ci];
            let mut ty = top - pad;
            for line in lines {
                let baseline = ty - size * 1.0;
                if baseline < bottom_limit {
                    break;
                }
                self.draw_line_at(line, x + pad, baseline, size, if header { HEADING } else { TEXT });
                ty -= lh;
            }
            self.c.stroke_rect(x, top - row_h, w, row_h, 0.5, BORDER);
            x += w;
        }
        self.y = top - row_h;
        self.empty_page = false;
    }

    fn row_height(cells: &[Vec<Line>], lh: f32, pad: f32) -> f32 {
        let max_lines = cells.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
        max_lines as f32 * lh + 2.0 * pad
    }

    fn table(&mut self, t: &Table) {
        let cols = t.columns();
        if cols == 0 {
            return;
        }
        let size: f32 = if cols > 8 {
            6.8
        } else if cols > 5 {
            8.0
        } else {
            9.0
        };
        let lh = size * 1.3;
        let pad = 4.0;

        let mut natural = vec![18.0f32; cols];
        for (ri, row) in t.rows.iter().enumerate() {
            let font = if t.header && ri == 0 { BOLD } else { REGULAR };
            for (ci, cell) in row.iter().enumerate() {
                let w = cell.split('\n').map(|l| self.c.measure(font, size, l)).fold(0.0f32, f32::max) + 2.0 * pad;
                natural[ci] = natural[ci].max(w.min(CONTENT_W * 0.55));
            }
        }
        let total: f32 = natural.iter().sum();
        let widths: Vec<f32> = natural.iter().map(|w| w / total * CONTENT_W).collect();

        self.gap(4.0);
        let header_row = t.header && t.rows.len() > 1;
        for (ri, row) in t.rows.iter().enumerate() {
            let is_header = header_row && ri == 0;
            let cells = self.layout_row(row, &widths, size, pad, is_header);
            let row_h = Self::row_height(&cells, lh, pad).min(CONTENT_H);
            if row_h > self.space_left() && !self.empty_page {
                self.new_page();
                if header_row && ri > 0 {
                    let head = self.layout_row(&t.rows[0], &widths, size, pad, true);
                    let head_h = Self::row_height(&head, lh, pad).min(CONTENT_H / 3.0);
                    self.draw_row(&head, &widths, head_h, size, lh, pad, true);
                }
            }
            self.draw_row(&cells, &widths, row_h, size, lh, pad, is_header);
        }
        self.y -= 10.0;
    }

    fn image(&mut self, img: &ImageData) -> Result<()> {
        let decoded = match image::load_from_memory(&img.bytes) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        let original = if img.jpeg { Some(img.bytes.as_slice()) } else { None };
        let name = add_image(&mut self.c, &decoded, original)?;
        let mut w = img.width as f32 * 0.75;
        let mut h = img.height as f32 * 0.75;
        if w > CONTENT_W {
            h *= CONTENT_W / w;
            w = CONTENT_W;
        }
        if h > CONTENT_H {
            w *= CONTENT_H / h;
            h = CONTENT_H;
        }
        self.gap(4.0);
        self.ensure(h);
        let x = MARGIN_X + (CONTENT_W - w) / 2.0;
        self.c.draw_image(&name, x, self.y - h, w, h);
        self.y -= h + 10.0;
        self.empty_page = false;
        Ok(())
    }

    fn rule(&mut self) {
        self.gap(6.0);
        self.ensure(10.0);
        self.c.line(MARGIN_X, self.y, MARGIN_X + CONTENT_W, self.y, 0.6, BORDER);
        self.y -= 10.0;
        self.empty_page = false;
    }

    fn page_numbers(&mut self) {
        let n = self.c.page_count();
        if n < 2 {
            return;
        }
        for i in 0..n {
            self.c.select_page(i);
            let label = format!("{} / {}", i + 1, n);
            let w = self.c.measure(REGULAR, 8.0, &label);
            self.c.text(REGULAR, 8.0, (PAGE_W - w) / 2.0, 30.0, &label, MUTED);
        }
    }
}

pub fn render(doc: &Document) -> Result<Vec<u8>> {
    let mut l = Layout::new()?;
    for block in &doc.blocks {
        match block {
            Block::Heading { level, runs } => l.heading(*level, runs),
            Block::Paragraph(runs) => l.paragraph(runs),
            Block::ListItem { ordered, number, level, runs } => l.list_item(*ordered, *number, *level, runs),
            Block::Code(code) => l.code(code),
            Block::Table(t) => l.table(t),
            Block::Image(img) => l.image(img)?,
            Block::Rule => l.rule(),
            Block::PageBreak => {
                if !l.empty_page {
                    l.new_page();
                }
            }
        }
    }
    l.page_numbers();
    l.c.into_bytes(doc.title.as_deref())
}

pub fn write(doc: &Document, out: &Path) -> Result<()> {
    let bytes = render(doc)?;
    std::fs::write(out, bytes)?;
    Ok(())
}
