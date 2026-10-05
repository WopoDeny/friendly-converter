use super::{Block, Document, Run};
use anyhow::Result;
use std::fmt::Write as _;
use std::path::Path;

/// Renders Markdown. Images are written into `<stem>_media/` next to the output.
pub fn write(doc: &Document, out_path: &Path) -> Result<()> {
    let stem = out_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let media_dir_name = format!("{stem}_media");
    let media_dir = out_path.with_file_name(&media_dir_name);
    let mut image_index = 0usize;
    let mut out = String::new();

    for block in &doc.blocks {
        match block {
            Block::Heading { level, runs } => {
                let _ = writeln!(out, "{} {}\n", "#".repeat((*level).clamp(1, 6) as usize), inline(runs, false));
            }
            Block::Paragraph(runs) => {
                let _ = writeln!(out, "{}\n", inline(runs, true));
            }
            Block::ListItem { ordered, number, level, runs } => {
                let indent = "    ".repeat(*level as usize);
                if *ordered {
                    let _ = writeln!(out, "{indent}{number}. {}", inline(runs, false));
                } else {
                    let _ = writeln!(out, "{indent}- {}", inline(runs, false));
                }
            }
            Block::Code(code) => {
                ensure_blank_line(&mut out);
                let fence = if code.contains("```") { "~~~~" } else { "```" };
                let _ = writeln!(out, "{fence}\n{code}\n{fence}\n");
            }
            Block::Table(table) => {
                ensure_blank_line(&mut out);
                let cols = table.columns();
                if cols == 0 {
                    continue;
                }
                let (head, body): (Vec<String>, &[Vec<String>]) = if table.header {
                    (table.rows[0].clone(), &table.rows[1..])
                } else {
                    ((1..=cols).map(|i| format!("Column {i}")).collect(), &table.rows[..])
                };
                let fmt_row = |row: &[String]| -> String {
                    let cells: Vec<String> = (0..cols).map(|i| escape_cell(row.get(i).map(|s| s.as_str()).unwrap_or(""))).collect();
                    format!("| {} |", cells.join(" | "))
                };
                let _ = writeln!(out, "{}", fmt_row(&head));
                let _ = writeln!(out, "|{}", " --- |".repeat(cols));
                for row in body {
                    let _ = writeln!(out, "{}", fmt_row(row));
                }
                out.push('\n');
            }
            Block::Image(img) => {
                image_index += 1;
                std::fs::create_dir_all(&media_dir)?;
                let name = format!("image{image_index}.{}", img.extension());
                std::fs::write(media_dir.join(&name), &img.bytes)?;
                let _ = writeln!(out, "![image {image_index}]({media_dir_name}/{name})\n");
            }
            Block::Rule => out.push_str("---\n\n"),
            Block::PageBreak => out.push_str("<div style=\"page-break-after: always\"></div>\n\n"),
        }
    }
    let text = out.trim_end().to_string() + "\n";
    std::fs::write(out_path, text)?;
    Ok(())
}

fn ensure_blank_line(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn escape_cell(text: &str) -> String {
    escape(text).replace('|', "\\|").replace('\n', "<br>")
}

fn inline(runs: &[Run], allow_breaks: bool) -> String {
    let mut out = String::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        let body = if run.code {
            format!("`{}`", run.text.replace('`', "'"))
        } else {
            escape(&run.text)
        };
        // Markdown markers must hug the text, so move outer spaces outside.
        let lead = body.len() - body.trim_start().len();
        let trail = body.len() - body.trim_end().len();
        let core = body.trim();
        if core.is_empty() {
            out.push_str(&body);
            continue;
        }
        let marker = match (run.bold, run.italic) {
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            _ => "",
        };
        out.push_str(&body[..lead]);
        out.push_str(marker);
        out.push_str(core);
        out.push_str(marker);
        out.push_str(&body[body.len() - trail..]);
    }
    if allow_breaks {
        out.replace('\n', "  \n")
    } else {
        out.replace('\n', " ")
    }
}
