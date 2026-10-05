use std::path::{Path, PathBuf};

/// `dir/stem.ext`, or `dir/stem_1.ext`, `dir/stem_2.ext`... if taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for i in 1..10_000 {
        let p = dir.join(format!("{stem}_{i}.{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}_{}.{ext}", std::process::id()))
}

pub fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    for i in 1..10_000 {
        let p = parent.join(format!("{name}_{i}"));
        if !p.exists() {
            return p;
        }
    }
    parent.join(format!("{name}_{}", std::process::id()))
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".to_string())
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// Opens a folder in the system file manager.
pub fn open_in_file_manager(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Opens a file in Explorer with the file selected.
pub fn reveal_file(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(parent) = path.parent() {
            open_in_file_manager(parent);
        }
    }
}
