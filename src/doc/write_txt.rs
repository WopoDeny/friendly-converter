use super::{runs_text, Block, Document};
use std::fmt::Write as _;

pub fn render(doc: &Document) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        match block {
            Block::Heading { level, runs } => {
                let text = runs_text(runs);
                ensure_blank_line(&mut out);
                out.push_str(&text);
                out.push('\n');
                if *level <= 2 {
                    let ch = if *level == 1 { '=' } else { '-' };
                    let width = text.chars().count().min(80);
                    out.extend(std::iter::repeat(ch).take(width));
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::Paragraph(runs) => {
                out.push_str(&runs_text(runs));
                out.push_str("\n\n");
            }
            Block::ListItem { ordered, number, level, runs } => {
                let indent = "  ".repeat(*level as usize);
                if *ordered {
                    let _ = writeln!(out, "{indent}{number}. {}", runs_text(runs));
                } else {
                    let _ = writeln!(out, "{indent}\u{2022} {}", runs_text(runs));
                }
            }
            Block::Code(code) => {
                ensure_blank_line(&mut out);
                for line in code.lines() {
                    let _ = writeln!(out, "    {line}");
                }
                out.push('\n');
            }
            Block::Table(table) => {
                ensure_blank_line(&mut out);
                out.push_str(&render_table(&table.rows, table.header));
                out.push('\n');
            }
            Block::Image(img) => {
                let _ = writeln!(out, "[image {}x{}]\n", img.width, img.height);
            }
            Block::Rule => out.push_str("----------------------------------------\n\n"),
            Block::PageBreak => out.push_str("\n\u{000C}\n"),
        }
    }
    let trimmed = out.trim_end().to_string();
    trimmed + "\n"
}

fn ensure_blank_line(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while !out.ends_with("\n\n") {
        out.push('\n');
    }
}

/// Fixed-width table using box-free ASCII so it survives any viewer.
pub fn render_table(rows: &[Vec<String>], header: bool) -> String {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let mut widths = vec![1usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let w = cell.replace('\n', " ").chars().count().min(40);
            widths[i] = widths[i].max(w);
        }
    }
    let mut out = String::new();
    for (ri, row) in rows.iter().enumerate() {
        let mut line = String::new();
        for (ci, width) in widths.iter().enumerate() {
            let cell = row.get(ci).map(|c| c.replace('\n', " ")).unwrap_or_default();
            let cell: String = cell.chars().take(*width).collect();
            let pad = width.saturating_sub(cell.chars().count());
            if ci > 0 {
                line.push_str(" | ");
            }
            line.push_str(&cell);
            line.extend(std::iter::repeat(' ').take(pad));
        }
        out.push_str(line.trim_end());
        out.push('\n');
        if ri == 0 && header && rows.len() > 1 {
            let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            out.push_str(&sep.join("-+-"));
            out.push('\n');
        }
    }
    out
}
