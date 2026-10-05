//! End-to-end conversions on generated files, including Russian text.

use friendly_converter::convert::{convert_file, merge_pdfs, Options};
use friendly_converter::doc::{self, Block, Document, Run, Table};
use friendly_converter::formats::{Format, Target};
use std::path::PathBuf;

/// "Privet, mir! Proverka konvertera." in Cyrillic.
const RU: &str = "\u{041f}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}, \u{043c}\u{0438}\u{0440}! \u{041f}\u{0440}\u{043e}\u{0432}\u{0435}\u{0440}\u{043a}\u{0430} \u{043a}\u{043e}\u{043d}\u{0432}\u{0435}\u{0440}\u{0442}\u{0435}\u{0440}\u{0430}.";
/// "Zagolovok" (heading) in Cyrillic.
const RU_HEAD: &str = "\u{0417}\u{0430}\u{0433}\u{043e}\u{043b}\u{043e}\u{0432}\u{043e}\u{043a}";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("friendly_converter_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_doc() -> Document {
    Document {
        title: Some("Test".into()),
        blocks: vec![
            Block::Heading { level: 1, runs: vec![Run::plain(RU_HEAD)] },
            Block::Paragraph(vec![Run::plain(RU), Run::bold(" bold"), Run::plain(" text.")]),
            Block::ListItem { ordered: false, number: 1, level: 0, runs: vec![Run::plain("first")] },
            Block::ListItem { ordered: true, number: 1, level: 0, runs: vec![Run::plain("second")] },
            Block::Table(Table {
                rows: vec![vec!["A".into(), "B".into()], vec!["1".into(), RU_HEAD.into()]],
                header: true,
            }),
            Block::Code("let x = 1;".into()),
        ],
    }
}

fn opts() -> Options {
    Options::default()
}

#[test]
fn docx_roundtrip_keeps_russian_text() {
    let dir = temp_dir("docx");
    let path = dir.join("doc.docx");
    doc::write_docx::write(&sample_doc(), &path).unwrap();
    let back = doc::read_docx::read(&path).unwrap();
    let text = back.plain_text();
    assert!(text.contains(RU), "text was: {text}");
    assert!(back.blocks.iter().any(|b| matches!(b, Block::Heading { level: 1, .. })));
    assert!(back.blocks.iter().any(|b| matches!(b, Block::Table(t) if t.rows.len() == 2)));
}

#[test]
fn pdf_roundtrip_keeps_russian_text() {
    let dir = temp_dir("pdf");
    let path = dir.join("doc.pdf");
    doc::write_pdf::write(&sample_doc(), &path).unwrap();
    let back = doc::read_pdf::read(&path).unwrap();
    let text = back.plain_text();
    assert!(text.contains(RU_HEAD), "text was: {text}");
}

#[test]
fn every_document_target_works() {
    let dir = temp_dir("targets");
    let md = dir.join("source.md");
    std::fs::write(&md, format!("# {RU_HEAD}\n\n{RU} **bold**\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n")).unwrap();
    for target in [Format::Docx, Format::Pdf, Format::Txt, Format::Html] {
        let out = convert_file(&md, Format::Md, Target::To(target), &opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert!(std::fs::metadata(&out[0]).unwrap().len() > 0);
    }
    let docx = dir.join("source.docx");
    for target in [Format::Pdf, Format::Txt, Format::Md, Format::Html] {
        convert_file(&docx, Format::Docx, Target::To(target), &opts()).unwrap();
    }
    let txt = std::fs::read_to_string(dir.join("source.txt")).unwrap();
    assert!(txt.contains(RU));
}

#[test]
fn image_conversions() {
    let dir = temp_dir("images");
    let png = dir.join("pic.png");
    let mut img = image::RgbaImage::new(64, 40);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 4) as u8, (y * 6) as u8, 128, if x < 8 { 0 } else { 255 }]);
    }
    img.save(&png).unwrap();
    for target in [Format::Jpg, Format::Webp, Format::Bmp, Format::Gif, Format::Tiff, Format::Ico, Format::Tga, Format::Pdf] {
        let out = convert_file(&png, Format::Png, Target::To(target), &opts()).unwrap();
        assert!(std::fs::metadata(&out[0]).unwrap().len() > 0, "{target:?}");
    }
    let jpg = dir.join("pic.jpg");
    let pdf = convert_file(&jpg, Format::Jpg, Target::To(Format::Pdf), &opts()).unwrap();
    let pdf2 = convert_file(&png, Format::Png, Target::To(Format::Pdf), &opts()).unwrap();
    let merged = merge_pdfs(&[pdf[0].clone(), pdf2[0].clone()], &opts()).unwrap();
    let doc = lopdf::Document::load(&merged).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    let extracted = convert_file(&pdf[0], Format::Pdf, Target::PdfImages, &opts()).unwrap();
    assert_eq!(extracted.len(), 1);
}

#[test]
fn table_and_data_conversions() {
    let dir = temp_dir("tables");
    let csv = dir.join("people.csv");
    std::fs::write(&csv, format!("name;age;code\n{RU_HEAD};30;007\nBob;41;010\n")).unwrap();
    let xlsx = convert_file(&csv, Format::Csv, Target::To(Format::Xlsx), &opts()).unwrap();
    let json = convert_file(&xlsx[0], Format::Xlsx, Target::To(Format::Json), &opts()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json[0]).unwrap()).unwrap();
    assert_eq!(value[0]["name"], RU_HEAD);
    assert_eq!(value[0]["age"], 30);
    assert_eq!(value[1]["code"], "010");
    for target in [Format::Yaml, Format::Toml, Format::Xml, Format::Csv] {
        convert_file(&json[0], Format::Json, Target::To(target), &opts()).unwrap();
    }
    for target in [Format::Pdf, Format::Docx, Format::Html, Format::Md] {
        convert_file(&csv, Format::Csv, Target::To(target), &opts()).unwrap();
    }
    let back_csv = dir.join("people_1.csv");
    let text = std::fs::read_to_string(back_csv).unwrap();
    assert!(text.contains(RU_HEAD));
}
