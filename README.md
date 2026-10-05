# Friendly Converter

Offline desktop file converter written in Rust. One `FriendlyConverter.exe`,
no installer, no runtime, no internet. Black board, neon green signal, gold contacts.

## Build

Requirements on Windows:

1. Rust from https://rustup.rs (accept the Visual Studio Build Tools / "Desktop development with C++" prompt).
2. That's it.

```powershell
cargo build --release
```

The executable is `target\release\FriendlyConverter.exe` (single file, C runtime linked statically,
DejaVu fonts embedded). Copy it anywhere.

Run the tests:

```powershell
cargo test
```

The first build downloads and compiles dependencies and takes a few minutes. Later builds are fast.

### Building on Linux for Windows

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64
cargo build --release --target x86_64-pc-windows-gnu
```

(The exe icon is only embedded when building on Windows.)

## Using it

* Drag files or whole folders onto the window, or press **Ctrl+O**.
* Pick the target format per file, or use **set every file to** for the whole list.
* **Convert all** (Ctrl+Enter). Results go next to the source files or into a chosen folder.
  Existing files are never overwritten: `report.pdf` becomes `report_1.pdf`.
* **Merge PDFs** joins all PDFs in the list (in list order) into `merged.pdf`.
* **Images to one PDF** puts every image of the list on its own A4 page.

## Supported conversions

| From | To |
|------|----|
| PDF | DOCX, TXT, MD, HTML, extract embedded images |
| DOCX | PDF, TXT, MD, HTML |
| MD, HTML, TXT | DOCX, PDF, TXT, MD, HTML |
| PNG, JPG, WEBP, BMP, GIF, TIFF, ICO, TGA | any of these, PDF |
| JSON, YAML, TOML, XML | each other, CSV, XLSX |
| CSV | XLSX, JSON, YAML, XML, PDF, DOCX, HTML, MD, TXT |
| XLSX, XLS, ODS | CSV, XLSX, JSON, YAML, XML, PDF, DOCX, HTML, MD, TXT |

Russian (and any other Unicode) text is preserved everywhere: PDFs embed DejaVu fonts with a
ToUnicode map (text stays selectable and searchable), CSV is written as UTF-8 with BOM so Excel opens
it correctly, and old Windows-1251 text files are detected automatically.

## Known limitations

* **PDF to DOCX** transfers text, paragraphs, simple headings and bullet lists. It does not rebuild
  the original layout (columns, exact positions, tables drawn with lines).
* **Scanned PDFs** have no text layer; OCR is not included. Use *extract images* to get the scans.
* **DOCX to PDF** uses its own layout engine: headings, bold/italic, lists, tables and pictures are
  kept, but the result will not match Word page for page (headers/footers, text boxes and complex
  styles are simplified).
* Password protected PDFs are not supported.
* WEBP output is lossless.

## Project layout

```
src/
  main.rs            window setup
  ui/                egui interface: theme, circuit background, widgets, worker thread
  formats.rs         format detection and available targets
  convert/           dispatcher, images, tables (csv/xlsx), data (json/yaml/toml/xml)
  doc/               document model + readers (pdf, docx, md, html, txt) and writers
  pdf/               PDF canvas with Unicode fonts, merge, image extraction
  textenc.rs         UTF-8 / UTF-16 / Windows-1251 detection
tests/               end-to-end conversion tests and the no-Cyrillic-in-code check
assets/              icon and DejaVu fonts (see assets/fonts/LICENSE-DejaVu.txt)
```

Code rule: no Cyrillic characters in source files. `cargo test` enforces it.
