//! On-disk cache for the MCP voice source: `guide.md`, `words.json` and
//! `status.json` in `<app data>/voice_source/`. Every write is atomic; a bad
//! download never replaces the previous good copy.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const GUIDE_FILE: &str = "guide.md";
pub const WORDS_FILE: &str = "words.json";
pub const STATUS_FILE: &str = "status.json";
/// Guides larger than this are rejected.
pub const MAX_GUIDE_BYTES: usize = 200 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SourceStatus {
    pub server_name: Option<String>,
    pub guide_version: Option<i64>,
    pub synced_at: Option<i64>,
    pub error: Option<String>,
    pub needs_sign_in: bool,
}

pub fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename failed: {e}"))
}

pub fn guide_file(dir: &Path) -> PathBuf {
    dir.join(GUIDE_FILE)
}

pub fn read_status(dir: &Path) -> SourceStatus {
    std::fs::read_to_string(dir.join(STATUS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn write_status(dir: &Path, status: &SourceStatus) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cache dir failed: {e}"))?;
    let text = serde_json::to_string(status).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(STATUS_FILE), &text)
}

/// Splits a tool result into guide text and version: `{"version","guide"}`
/// JSON gives both, anything else is the plain guide text.
pub fn parse_guide(text: &str) -> (String, Option<i64>) {
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(text.trim()) {
        if let Some(guide) = map.get("guide").and_then(|g| g.as_str()) {
            return (
                guide.to_string(),
                map.get("version").and_then(|v| v.as_i64()),
            );
        }
    }
    (text.to_string(), None)
}

/// Validates and stores a fetched guide in `dir/guide.md`. Empty or oversized
/// guides are rejected and the previous file is kept. Returns the version.
pub fn store_guide(dir: &Path, raw: &str) -> Result<Option<i64>, String> {
    let (guide, version) = parse_guide(raw);
    if guide.trim().is_empty() {
        return Err("The server sent an empty voice guide; kept the previous one".to_string());
    }
    if guide.len() > MAX_GUIDE_BYTES {
        return Err("The server's voice guide is too large; kept the previous one".to_string());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cache dir failed: {e}"))?;
    write_atomic(&guide_file(dir), &guide)?;
    Ok(version)
}

/// Stores the parsed words in `dir/words.json`. An empty list keeps the
/// previous cache (a server hiccup must not wipe the vocabulary).
pub fn store_words(dir: &Path, words: &[String]) -> Result<(), String> {
    if words.is_empty() {
        return Err("The server sent no words; kept the previous list".to_string());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cache dir failed: {e}"))?;
    let text = serde_json::to_string(words).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(WORDS_FILE), &text)
}

pub fn read_words(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(WORDS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Removes the whole cache directory (disconnect).
pub fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}
