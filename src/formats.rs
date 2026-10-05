//! File format detection and the registry of available conversion targets.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    // Documents
    Pdf,
    Docx,
    Txt,
    Md,
    Html,
    // Images
    Png,
    Jpg,
    Webp,
    Bmp,
    Gif,
    Tiff,
    Ico,
    Tga,
    // Structured data
    Json,
    Yaml,
    Toml,
    Xml,
    // Tables
    Csv,
    Xlsx,
    Xls,
    Ods,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Document,
    Image,
    Data,
    Table,
}

/// What a single file can be turned into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    To(Format),
    /// Pull every embedded picture out of a PDF.
    PdfImages,
}

pub const DOC_OUT: [Format; 5] = [Format::Docx, Format::Pdf, Format::Txt, Format::Md, Format::Html];
pub const IMAGE_OUT: [Format; 8] = [
    Format::Png,
    Format::Jpg,
    Format::Webp,
    Format::Bmp,
    Format::Gif,
    Format::Tiff,
    Format::Ico,
    Format::Tga,
];
pub const DATA_OUT: [Format; 4] = [Format::Json, Format::Yaml, Format::Toml, Format::Xml];

impl Format {
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    pub fn from_extension(ext: &str) -> Option<Format> {
        Some(match ext {
            "pdf" => Format::Pdf,
            "docx" | "docm" => Format::Docx,
            "txt" | "text" | "log" => Format::Txt,
            "md" | "markdown" => Format::Md,
            "html" | "htm" | "xhtml" => Format::Html,
            "png" => Format::Png,
            "jpg" | "jpeg" | "jfif" | "jpe" => Format::Jpg,
            "webp" => Format::Webp,
            "bmp" | "dib" => Format::Bmp,
            "gif" => Format::Gif,
            "tif" | "tiff" => Format::Tiff,
            "ico" => Format::Ico,
            "tga" => Format::Tga,
            "json" => Format::Json,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            "xml" => Format::Xml,
            "csv" | "tsv" => Format::Csv,
            "xlsx" | "xlsm" => Format::Xlsx,
            "xls" => Format::Xls,
            "ods" => Format::Ods,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Txt => "txt",
            Format::Md => "md",
            Format::Html => "html",
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Bmp => "bmp",
            Format::Gif => "gif",
            Format::Tiff => "tiff",
            Format::Ico => "ico",
            Format::Tga => "tga",
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Xml => "xml",
            Format::Csv => "csv",
            Format::Xlsx => "xlsx",
            Format::Xls => "xls",
            Format::Ods => "ods",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Docx => "DOCX",
            Format::Txt => "TXT",
            Format::Md => "MD",
            Format::Html => "HTML",
            Format::Png => "PNG",
            Format::Jpg => "JPG",
            Format::Webp => "WEBP",
            Format::Bmp => "BMP",
            Format::Gif => "GIF",
            Format::Tiff => "TIFF",
            Format::Ico => "ICO",
            Format::Tga => "TGA",
            Format::Json => "JSON",
            Format::Yaml => "YAML",
            Format::Toml => "TOML",
            Format::Xml => "XML",
            Format::Csv => "CSV",
            Format::Xlsx => "XLSX",
            Format::Xls => "XLS",
            Format::Ods => "ODS",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Format::Pdf | Format::Docx | Format::Txt | Format::Md | Format::Html => Category::Document,
            Format::Png
            | Format::Jpg
            | Format::Webp
            | Format::Bmp
            | Format::Gif
            | Format::Tiff
            | Format::Ico
            | Format::Tga => Category::Image,
            Format::Json | Format::Yaml | Format::Toml | Format::Xml => Category::Data,
            Format::Csv | Format::Xlsx | Format::Xls | Format::Ods => Category::Table,
        }
    }

    /// Every conversion offered for this input format, best guess first.
    pub fn targets(self) -> Vec<Target> {
        let mut out: Vec<Target> = Vec::new();
        let mut push = |f: Format| {
            if f != self && !out.contains(&Target::To(f)) {
                out.push(Target::To(f));
            }
        };
        match self.category() {
            Category::Document => {
                push(self.default_target_format());
                for f in DOC_OUT {
                    push(f);
                }
            }
            Category::Image => {
                push(self.default_target_format());
                push(Format::Pdf);
                for f in IMAGE_OUT {
                    push(f);
                }
            }
            Category::Data => {
                push(self.default_target_format());
                for f in DATA_OUT {
                    push(f);
                }
                push(Format::Csv);
                push(Format::Xlsx);
            }
            Category::Table => {
                push(self.default_target_format());
                for f in [Format::Xlsx, Format::Csv, Format::Json, Format::Yaml, Format::Xml] {
                    push(f);
                }
                for f in [Format::Pdf, Format::Docx, Format::Html, Format::Md, Format::Txt] {
                    push(f);
                }
            }
        }
        if self == Format::Pdf {
            out.push(Target::PdfImages);
        }
        out
    }

    pub fn default_target(self) -> Target {
        Target::To(self.default_target_format())
    }

    fn default_target_format(self) -> Format {
        match self {
            Format::Pdf => Format::Docx,
            Format::Docx => Format::Pdf,
            Format::Txt => Format::Docx,
            Format::Md => Format::Docx,
            Format::Html => Format::Docx,
            Format::Png => Format::Jpg,
            Format::Jpg | Format::Webp | Format::Bmp | Format::Gif | Format::Tiff | Format::Ico | Format::Tga => {
                Format::Png
            }
            Format::Json => Format::Yaml,
            Format::Yaml | Format::Toml | Format::Xml => Format::Json,
            Format::Csv => Format::Xlsx,
            Format::Xlsx => Format::Csv,
            Format::Xls | Format::Ods => Format::Xlsx,
        }
    }
}

impl Target {
    pub fn label(self) -> String {
        match self {
            Target::To(f) => f.label().to_string(),
            Target::PdfImages => "Images".to_string(),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Target::PdfImages => "extract embedded pictures as PNG/JPG",
            Target::To(Format::Ico) => "max 256x256",
            Target::To(Format::Pdf) => "A4 pages",
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(Format::from_path(Path::new("a/B.PDF")), Some(Format::Pdf));
        assert_eq!(Format::from_path(Path::new("x.jpeg")), Some(Format::Jpg));
        assert_eq!(Format::from_path(Path::new("x.unknown")), None);
    }

    #[test]
    fn targets_never_contain_self() {
        for f in [Format::Pdf, Format::Png, Format::Json, Format::Csv, Format::Xlsx] {
            assert!(!f.targets().contains(&Target::To(f)));
            assert_eq!(f.targets()[0], f.default_target());
        }
    }
}
