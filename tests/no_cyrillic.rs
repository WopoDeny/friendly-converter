//! Project rule: no Cyrillic characters in source code, comments or UI strings.
//! Russian text in tests is written with \u{...} escapes.

use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, hits);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "rs" | "toml" | "md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for (n, line) in text.lines().enumerate() {
            if line.chars().any(|c| ('\u{0400}'..='\u{052F}').contains(&c)) {
                hits.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
}

#[test]
fn no_cyrillic_in_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    for dir in ["src", "tests"] {
        scan(&root.join(dir), &mut hits);
    }
    for file in ["Cargo.toml", "README.md", "build.rs"] {
        let p = root.join(file);
        if let Ok(text) = std::fs::read_to_string(&p) {
            for (n, line) in text.lines().enumerate() {
                if line.chars().any(|c| ('\u{0400}'..='\u{052F}').contains(&c)) {
                    hits.push(format!("{}:{}", p.display(), n + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "Cyrillic found in: {hits:#?}");
}
