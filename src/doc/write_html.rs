use super::{Block, Document, Run};
use base64::Engine;
use std::fmt::Write as _;

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn inline(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        let mut t = escape(&run.text).replace('\n', "<br>\n");
        if run.code {
            t = format!("<code>{t}</code>");
        }
        if run.italic {
            t = format!("<em>{t}</em>");
        }
        if run.bold {
            t = format!("<strong>{t}</strong>");
        }
        out.push_str(&t);
    }
    out
}

const STYLE: &str = r#"
  :root { color-scheme: light; }
  body { font-family: "Segoe UI", "DejaVu Sans", Arial, sans-serif; line-height: 1.55; color: #1b1f1c;
         max-width: 820px; margin: 40px auto; padding: 0 24px; background: #fff; }
  h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.4em 0 0.5em; }
  h1 { font-size: 1.9em; border-bottom: 2px solid #1b1f1c; padding-bottom: 0.2em; }
  p { margin: 0 0 0.9em; }
  code, pre { font-family: Consolas, "DejaVu Sans Mono", monospace; font-size: 0.92em; }
  code { background: #f1f3f1; padding: 0.1em 0.3em; }
  pre { background: #f1f3f1; padding: 12px 14px; overflow-x: auto; white-space: pre; }
  table { border-collapse: collapse; margin: 0 0 1.2em; width: 100%; }
  th, td { border: 1px solid #b9c0ba; padding: 6px 9px; text-align: left; vertical-align: top; }
  th { background: #e8ece9; }
  img { max-width: 100%; height: auto; display: block; margin: 0.6em 0 1.2em; }
  hr { border: 0; border-top: 1px solid #b9c0ba; margin: 1.6em 0; }
  .page-break { page-break-after: always; break-after: page; }
  @media print { body { margin: 0; max-width: none; } }
"#;

pub fn render(doc: &Document) -> String {
    let title = doc.title.clone().unwrap_or_else(|| "Document".to_string());
    let mut out = String::new();
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"Friendly Converter\">\n<title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n",
        escape(&title)
    );

    // Open list stack: (ordered, level)
    let mut lists: Vec<bool> = Vec::new();
    let close_lists = |out: &mut String, lists: &mut Vec<bool>, keep: usize| {
        while lists.len() > keep {
            let ordered = lists.pop().unwrap_or(false);
            out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
        }
    };

    for block in &doc.blocks {
        if !matches!(block, Block::ListItem { .. }) {
            close_lists(&mut out, &mut lists, 0);
        }
        match block {
            Block::Heading { level, runs } => {
                let l = (*level).clamp(1, 6);
                let _ = writeln!(out, "<h{l}>{}</h{l}>", inline(runs));
            }
            Block::Paragraph(runs) => {
                let _ = writeln!(out, "<p>{}</p>", inline(runs));
            }
            Block::ListItem { ordered, number, level, runs } => {
                let depth = *level as usize + 1;
                close_lists(&mut out, &mut lists, depth);
                if lists.len() == depth && lists.last() != Some(ordered) {
                    close_lists(&mut out, &mut lists, depth - 1);
                }
                while lists.len() < depth {
                    if *ordered {
                        let _ = writeln!(out, "<ol start=\"{number}\">");
                    } else {
                        out.push_str("<ul>\n");
                    }
                    lists.push(*ordered);
                }
                let _ = writeln!(out, "<li>{}</li>", inline(runs));
            }
            Block::Code(code) => {
                let _ = writeln!(out, "<pre><code>{}</code></pre>", escape(code));
            }
            Block::Table(table) => {
                out.push_str("<table>\n");
                let cols = table.columns();
                for (i, row) in table.rows.iter().enumerate() {
                    let tag = if i == 0 && table.header { "th" } else { "td" };
                    out.push_str("<tr>");
                    for c in 0..cols {
                        let cell = row.get(c).map(|s| s.as_str()).unwrap_or("");
                        let _ = write!(out, "<{tag}>{}</{tag}>", escape(cell).replace('\n', "<br>"));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
            Block::Image(img) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&img.bytes);
                let _ = writeln!(
                    out,
                    "<img src=\"data:{};base64,{b64}\" width=\"{}\" alt=\"\">",
                    img.mime(),
                    img.width
                );
            }
            Block::Rule => out.push_str("<hr>\n"),
            Block::PageBreak => out.push_str("<div class=\"page-break\"></div>\n"),
        }
    }
    close_lists(&mut out, &mut lists, 0);
    out.push_str("</body>\n</html>\n");
    out
}
