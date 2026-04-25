use std::io;
use std::path::Path;
use std::process::Command;

// Formats mat2 supports; checked by extension before attempting to strip.
const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "tiff", "tif", "bmp", "webp",
    "pdf",
    "docx", "xlsx", "pptx",
    "odt", "odp", "ods", "odg",
    "mp3", "flac", "ogg", "m4a",
    "mp4", "mkv",
    "epub",
    "zip",
];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Returns the metadata-stripped bytes of `path`, or an error.
/// Falls through to the original file if mat2 exits non-zero.
pub fn strip(path: &Path) -> io::Result<Vec<u8>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("tmp");

    // Copy to a temp file with the same extension so mat2 recognises the format.
    let tmp = tempfile::Builder::new()
        .suffix(&format!(".{}", ext))
        .tempfile()?;
    let tmp_path = tmp.path().to_owned();

    std::fs::copy(path, &tmp_path)?;

    let status = Command::new("mat2")
        .arg("--inplace")
        .arg(&tmp_path)
        .status();

    match status {
        Ok(s) if s.success() => std::fs::read(&tmp_path),
        // mat2 not installed or unsupported format: return original unchanged.
        _ => std::fs::read(path),
    }
}
