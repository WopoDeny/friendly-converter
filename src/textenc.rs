//! Reading text files in whatever encoding they come in.
//!
//! Russian text files are often saved as Windows-1251 by older software,
//! so anything that is not valid UTF-8 / UTF-16 is decoded as Windows-1251.

use anyhow::{Context, Result};
use std::path::Path;

pub fn read_text_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(decode_bytes(&bytes))
}

pub fn decode_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        return text.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (text, _, _) = encoding_rs::WINDOWS_1251.decode(bytes);
            text.into_owned()
        }
    }
}

/// Normalizes Windows / old Mac line endings to `\n`.
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_cp1251() {
        // "Privet" in Cyrillic, encoded as Windows-1251.
        let bytes = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        assert_eq!(decode_bytes(&bytes), "\u{041F}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}");
    }

    #[test]
    fn keeps_utf8() {
        let s = "\u{041F}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}";
        assert_eq!(decode_bytes(s.as_bytes()), s);
    }
}
