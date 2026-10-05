//! Tabular data: CSV and spreadsheets (XLSX / XLS / ODS).

use crate::formats::Format;
use crate::textenc::read_text_file;
use anyhow::{bail, Context, Result};
use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::{Format as XlsxFormat, Workbook};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

impl Sheet {
    /// Drops trailing empty rows and trailing empty cells.
    pub fn trim(&mut self) {
        for row in &mut self.rows {
            while row.last().map(|c| c.trim().is_empty()).unwrap_or(false) {
                row.pop();
            }
        }
        while self.rows.last().map(|r| r.is_empty()).unwrap_or(false) {
            self.rows.pop();
        }
    }
}

pub fn detect_delimiter(text: &str) -> u8 {
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(10).collect();
    let mut best = (b',', 0usize);
    for d in [b';', b',', b'\t', b'|'] {
        let counts: Vec<usize> = sample
            .iter()
            .map(|line| {
                let mut in_quotes = false;
                let mut n = 0;
                for c in line.bytes() {
                    if c == b'"' {
                        in_quotes = !in_quotes;
                    } else if c == d && !in_quotes {
                        n += 1;
                    }
                }
                n
            })
            .collect();
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        // Prefer delimiters that appear on every line.
        let score = if min > 0 { total * 10 } else { total };
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

pub fn read_csv(path: &Path) -> Result<Vec<Sheet>> {
    let text = read_text_file(path)?;
    let delimiter = if path.extension().map(|e| e.eq_ignore_ascii_case("tsv")).unwrap_or(false) {
        b'\t'
    } else {
        detect_delimiter(&text)
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.context("broken CSV row")?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    let mut sheet = Sheet { name: crate::util::file_stem(path), rows };
    sheet.trim();
    Ok(vec![sheet])
}

/// Excel serial date (1900 system) to "YYYY-MM-DD[ HH:MM[:SS]]".
fn excel_serial_to_string(serial: f64) -> String {
    if !(0.0..2_958_466.0).contains(&serial) {
        return serial.to_string();
    }
    let days = serial.floor() as i64;
    let secs = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // 1899-12-30 is serial 0 (accounts for the 1900 leap year bug).
    let z = days - 25_569 + 719_468; // days since 0000-03-01 for Unix epoch shift
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (hh, mm, ss) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if days == 0 && secs > 0 {
        format!("{hh:02}:{mm:02}:{ss:02}")
    } else if secs == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else if ss == 0 {
        format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
    }
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(),
        Data::DateTime(dt) => {
            let text = dt.to_string();
            match text.parse::<f64>() {
                Ok(serial) => excel_serial_to_string(serial),
                Err(_) => text,
            }
        }
        other => other.to_string(),
    }
}

pub fn read_spreadsheet(path: &Path) -> Result<Vec<Sheet>> {
    let mut workbook = open_workbook_auto(path).with_context(|| format!("cannot open spreadsheet {}", path.display()))?;
    let mut sheets = Vec::new();
    for name in workbook.sheet_names().to_owned() {
        let range = match workbook.worksheet_range(&name) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rows: Vec<Vec<String>> = range.rows().map(|r| r.iter().map(cell_to_string).collect()).collect();
        let mut sheet = Sheet { name, rows };
        sheet.trim();
        if !sheet.rows.is_empty() {
            sheets.push(sheet);
        }
    }
    if sheets.is_empty() {
        bail!("the spreadsheet has no data");
    }
    Ok(sheets)
}

pub fn read_tables(path: &Path, format: Format) -> Result<Vec<Sheet>> {
    match format {
        Format::Csv => read_csv(path),
        Format::Xlsx | Format::Xls | Format::Ods => read_spreadsheet(path),
        _ => bail!("{} is not a table format", format.label()),
    }
}

pub fn write_csv(sheet: &Sheet, out: &Path, delimiter: u8) -> Result<()> {
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    // BOM so Excel detects UTF-8 (Cyrillic stays readable).
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    {
        let mut writer = csv::WriterBuilder::new().delimiter(delimiter).flexible(true).from_writer(&mut file);
        for row in &sheet.rows {
            writer.write_record(row)?;
        }
        writer.flush()?;
    }
    file.flush()?;
    Ok(())
}

/// A string that should be stored as a number in a spreadsheet / JSON.
/// Leading zeros ("007"), very long digit strings and "+7..." phone numbers stay text.
pub fn as_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() || t != s || t.len() > 15 {
        return None;
    }
    let digits = t.trim_start_matches('-');
    if digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.") {
        return None;
    }
    if !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E') {
        return None;
    }
    if !t.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn sheet_name(name: &str, used: &mut Vec<String>) -> String {
    let mut clean: String = name
        .chars()
        .map(|c| if matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') { '_' } else { c })
        .take(31)
        .collect();
    clean = clean.trim_matches('\'').to_string();
    if clean.is_empty() {
        clean = format!("Sheet{}", used.len() + 1);
    }
    let mut candidate = clean.clone();
    let mut i = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
        let suffix = format!(" ({i})");
        let base: String = clean.chars().take(31 - suffix.chars().count()).collect();
        candidate = format!("{base}{suffix}");
        i += 1;
    }
    used.push(candidate.clone());
    candidate
}

pub fn write_xlsx(sheets: &[Sheet], out: &Path) -> Result<()> {
    let mut workbook = Workbook::new();
    let header = XlsxFormat::new().set_bold();
    let mut used = Vec::new();
    for sheet in sheets {
        let name = sheet_name(&sheet.name, &mut used);
        let ws = workbook.add_worksheet();
        ws.set_name(&name)?;
        for (r, row) in sheet.rows.iter().enumerate() {
            if r >= 1_048_576 {
                break;
            }
            for (c, value) in row.iter().enumerate() {
                if c >= 16_384 || value.is_empty() {
                    continue;
                }
                let (r, c) = (r as u32, c as u16);
                if r == 0 {
                    ws.write_string_with_format(r, c, value, &header)?;
                } else if let Some(n) = as_number(value) {
                    ws.write_number(r, c, n)?;
                } else {
                    ws.write_string(r, c, value)?;
                }
            }
        }
        if !sheet.rows.is_empty() {
            ws.set_freeze_panes(1, 0)?;
        }
        if sheet.rows.len() <= 5_000 {
            ws.autofit();
        }
    }
    if sheets.is_empty() {
        workbook.add_worksheet();
    }
    workbook.save(out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_dates() {
        assert_eq!(excel_serial_to_string(45_000.0), "2023-03-15");
        assert_eq!(excel_serial_to_string(45_000.5), "2023-03-15 12:00");
    }

    #[test]
    fn numbers() {
        assert_eq!(as_number("42"), Some(42.0));
        assert_eq!(as_number("-3.5"), Some(-3.5));
        assert_eq!(as_number("007"), None);
        assert_eq!(as_number("+79001234567"), None);
        assert_eq!(as_number("12 345"), None);
    }

    #[test]
    fn delimiter() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3\n"), b';');
        assert_eq!(detect_delimiter("a,b,c\n1,2,3\n"), b',');
    }
}
