//! Minimal but valid DOCX (Office Open XML) writer built directly on zip.

use super::{Block, Document, Run};
use anyhow::Result;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Usable text width of an A4 page with 2 cm margins, in EMU.
const CONTENT_WIDTH_EMU: u64 = 6_120_000;
const CONTENT_HEIGHT_EMU: u64 = 8_500_000;
/// Same width in twentieths of a point (for table grid).
const CONTENT_WIDTH_TWIPS: u32 = 9638;

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            _ => out.push(c),
        }
    }
    out
}

struct Ctx {
    body: String,
    images: Vec<(String, Vec<u8>, &'static str)>, // (rel id, bytes, extension)
    /// One w:num per separate ordered list, so numbering restarts.
    ordered_lists: usize,
    current_ordered: Option<usize>,
    drawing_id: usize,
}

pub fn write(doc: &Document, out_path: &Path) -> Result<()> {
    let mut ctx = Ctx { body: String::new(), images: Vec::new(), ordered_lists: 0, current_ordered: None, drawing_id: 0 };

    for block in &doc.blocks {
        if !matches!(block, Block::ListItem { .. }) {
            ctx.current_ordered = None;
        }
        match block {
            Block::Heading { level, runs } => {
                let style = format!("Heading{}", (*level).clamp(1, 6));
                paragraph(&mut ctx.body, Some(&style), None, runs);
            }
            Block::Paragraph(runs) => paragraph(&mut ctx.body, None, None, runs),
            Block::ListItem { ordered, level, runs, .. } => {
                let num_id = if *ordered {
                    let n = match ctx.current_ordered {
                        Some(n) => n,
                        None => {
                            ctx.ordered_lists += 1;
                            ctx.current_ordered = Some(ctx.ordered_lists);
                            ctx.ordered_lists
                        }
                    };
                    // num ids: 1 = bullets, 2.. = ordered list instances
                    n + 1
                } else {
                    1
                };
                paragraph(&mut ctx.body, Some("ListParagraph"), Some((num_id, *level)), runs);
            }
            Block::Code(code) => {
                for line in code.split('\n') {
                    let run = Run { text: line.to_string(), code: true, ..Default::default() };
                    paragraph(&mut ctx.body, Some("Code"), None, &[run]);
                }
            }
            Block::Table(table) => write_table(&mut ctx.body, &table.rows, table.header),
            Block::Image(img) => {
                ctx.drawing_id += 1;
                let id = ctx.drawing_id;
                let rel = format!("rIdImg{id}");
                let (cx, cy) = fit_emu(img.width, img.height);
                let _ = write!(
                    ctx.body,
                    concat!(
                        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:drawing>",
                        "<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
                        "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/>",
                        "<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>",
                        "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
                        "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
                        "<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
                        "<pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"image{id}.{ext}\"/><pic:cNvPicPr/></pic:nvPicPr>",
                        "<pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
                        "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
                        "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>",
                        "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
                    ),
                    cx = cx,
                    cy = cy,
                    id = id,
                    rel = rel,
                    ext = img.extension()
                );
                ctx.images.push((rel, img.bytes.clone(), img.extension()));
            }
            Block::Rule => ctx.body.push_str(
                "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"999999\"/></w:pBdr></w:pPr></w:p>",
            ),
            Block::PageBreak => ctx.body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>"),
        }
    }

    let file = File::create(out_path)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));

    zip.start_file("[Content_Types].xml", deflated())?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;
    zip.start_file("_rels/.rels", deflated())?;
    zip.write_all(ROOT_RELS.as_bytes())?;
    zip.start_file("docProps/core.xml", deflated())?;
    zip.write_all(core_xml(doc.title.as_deref().unwrap_or("")).as_bytes())?;
    zip.start_file("docProps/app.xml", deflated())?;
    zip.write_all(APP_XML.as_bytes())?;
    zip.start_file("word/styles.xml", deflated())?;
    zip.write_all(STYLES_XML.as_bytes())?;
    zip.start_file("word/numbering.xml", deflated())?;
    zip.write_all(numbering_xml(ctx.ordered_lists).as_bytes())?;
    zip.start_file("word/settings.xml", deflated())?;
    zip.write_all(SETTINGS_XML.as_bytes())?;

    let mut rels = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        "<Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>",
        "<Relationship Id=\"rIdSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>"
    ));
    for (i, (rel, bytes, ext)) in ctx.images.iter().enumerate() {
        let name = format!("media/image{}.{ext}", i + 1);
        let _ = write!(
            rels,
            "<Relationship Id=\"{rel}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{name}\"/>"
        );
        zip.start_file(format!("word/{name}"), SimpleFileOptions::default().compression_method(CompressionMethod::Stored))?;
        zip.write_all(bytes)?;
    }
    rels.push_str("</Relationships>");
    zip.start_file("word/_rels/document.xml.rels", deflated())?;
    zip.write_all(rels.as_bytes())?;

    let document = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
            "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
            "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">",
            "<w:body>{body}<w:p/>",
            "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>",
            "<w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/>",
            "</w:sectPr></w:body></w:document>"
        ),
        body = ctx.body
    );
    zip.start_file("word/document.xml", deflated())?;
    zip.write_all(document.as_bytes())?;

    let mut inner = zip.finish()?;
    inner.flush()?;
    Ok(())
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn fit_emu(width_px: u32, height_px: u32) -> (u64, u64) {
    // 96 dpi: 1 px = 9525 EMU
    let mut cx = width_px as u64 * 9525;
    let mut cy = height_px as u64 * 9525;
    if cx > CONTENT_WIDTH_EMU {
        cy = cy * CONTENT_WIDTH_EMU / cx.max(1);
        cx = CONTENT_WIDTH_EMU;
    }
    if cy > CONTENT_HEIGHT_EMU {
        cx = cx * CONTENT_HEIGHT_EMU / cy.max(1);
        cy = CONTENT_HEIGHT_EMU;
    }
    (cx.max(1), cy.max(1))
}

fn run_xml(out: &mut String, run: &Run) {
    if run.text.is_empty() {
        return;
    }
    out.push_str("<w:r>");
    if run.bold || run.italic || run.code {
        out.push_str("<w:rPr>");
        if run.code {
            out.push_str("<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>");
        }
        if run.bold {
            out.push_str("<w:b/><w:bCs/>");
        }
        if run.italic {
            out.push_str("<w:i/><w:iCs/>");
        }
        out.push_str("</w:rPr>");
    }
    let mut first_line = true;
    for line in run.text.split('\n') {
        if !first_line {
            out.push_str("<w:br/>");
        }
        first_line = false;
        let mut first_tab = true;
        for part in line.split('\t') {
            if !first_tab {
                out.push_str("<w:tab/>");
            }
            first_tab = false;
            if !part.is_empty() {
                let _ = write!(out, "<w:t xml:space=\"preserve\">{}</w:t>", xml_escape(part));
            }
        }
    }
    out.push_str("</w:r>");
}

fn paragraph(out: &mut String, style: Option<&str>, numbering: Option<(usize, u8)>, runs: &[Run]) {
    out.push_str("<w:p>");
    if style.is_some() || numbering.is_some() {
        out.push_str("<w:pPr>");
        if let Some(s) = style {
            let _ = write!(out, "<w:pStyle w:val=\"{s}\"/>");
        }
        if let Some((num, level)) = numbering {
            let _ = write!(out, "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr>", level.min(8));
        }
        out.push_str("</w:pPr>");
    }
    for run in runs {
        run_xml(out, run);
    }
    out.push_str("</w:p>");
}

fn write_table(out: &mut String, rows: &[Vec<String>], header: bool) {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    let col_w = CONTENT_WIDTH_TWIPS / cols as u32;
    out.push_str("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/><w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>");
    for _ in 0..cols {
        let _ = write!(out, "<w:gridCol w:w=\"{col_w}\"/>");
    }
    out.push_str("</w:tblGrid>");
    for (ri, row) in rows.iter().enumerate() {
        let is_header = header && ri == 0;
        out.push_str("<w:tr>");
        if is_header {
            out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        }
        for ci in 0..cols {
            let cell = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            let _ = write!(out, "<w:tc><w:tcPr><w:tcW w:w=\"{col_w}\" w:type=\"dxa\"/>");
            if is_header {
                out.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"E8ECE9\"/>");
            }
            out.push_str("</w:tcPr>");
            let mut wrote = false;
            for line in cell.split('\n') {
                let run = Run { text: line.to_string(), bold: is_header, ..Default::default() };
                paragraph(out, Some("TableText"), None, &[run]);
                wrote = true;
            }
            if !wrote {
                out.push_str("<w:p/>");
            }
            out.push_str("</w:tc>");
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl><w:p/>");
}

fn core_xml(title: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" ",
            "xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
            "<dc:title>{}</dc:title><dc:creator>Friendly Converter</dc:creator>",
            "</cp:coreProperties>"
        ),
        xml_escape(title)
    )
}

fn numbering_xml(ordered_lists: usize) -> String {
    let mut s = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
        "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">"
    ));
    // abstract 0: bullets
    s.push_str("<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    for lvl in 0..9u32 {
        let ind = 720 + lvl * 360;
        let _ = write!(
            s,
            "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{ind}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            bullets[(lvl % 3) as usize]
        );
    }
    s.push_str("</w:abstractNum>");
    // abstract 1: decimal
    s.push_str("<w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    let fmts = ["decimal", "lowerLetter", "lowerRoman"];
    for lvl in 0..9u32 {
        let ind = 720 + lvl * 360;
        let _ = write!(
            s,
            "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{}\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{ind}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            fmts[(lvl % 3) as usize],
            lvl + 1
        );
    }
    s.push_str("</w:abstractNum>");
    s.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for i in 0..ordered_lists {
        let _ = write!(
            s,
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"1\"/></w:lvlOverride></w:num>",
            i + 2
        );
    }
    s.push_str("</w:numbering>");
    s
}

const CONTENT_TYPES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
    "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
    "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
    "<Default Extension=\"png\" ContentType=\"image/png\"/>",
    "<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>",
    "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>",
    "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
    "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>",
    "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>",
    "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>",
    "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
    "<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
    "</Types>"
);

const ROOT_RELS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
    "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>",
    "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>",
    "</Relationships>"
);

const APP_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">",
    "<Application>Friendly Converter</Application></Properties>"
);

const SETTINGS_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:defaultTabStop w:val=\"708\"/><w:characterSpacingControl w:val=\"doNotCompress\"/>",
    "<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat>",
    "</w:settings>"
);

const STYLES_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:docDefaults><w:rPrDefault><w:rPr>",
    "<w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/>",
    "<w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"ru-RU\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/>",
    "</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"264\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"36\"/><w:szCs w:val=\"36\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"280\" w:after=\"100\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"30\"/><w:szCs w:val=\"30\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"26\"/><w:szCs w:val=\"26\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"200\" w:after=\"60\"/><w:outlineLvl w:val=\"3\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"200\" w:after=\"60\"/><w:outlineLvl w:val=\"4\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:i/><w:sz w:val=\"22\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"200\" w:after=\"60\"/><w:outlineLvl w:val=\"5\"/></w:pPr><w:rPr><w:i/><w:sz w:val=\"22\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:after=\"40\"/><w:ind w:left=\"720\"/><w:contextualSpacing/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F1F3F1\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>",
    "<w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"19\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"TableText\"><w:name w:val=\"Table Text\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/>",
    "<w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
    "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/>",
    "<w:tblPr><w:tblBorders>",
    "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/>",
    "<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/>",
    "<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"A0A8A2\"/>",
    "</w:tblBorders><w:tblCellMar><w:top w:w=\"40\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"40\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
    "</w:styles>"
);
