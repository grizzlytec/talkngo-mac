//! Optional sync of `voice_guide.md` from a remote endpoint.
//!
//! Configured by `voice_sync.json` (`{"url": "...", "token": "..."}`) in the
//! app data dir, next to `voice_guide.md`. The file is never part of the repo.
//! Absent or invalid config disables the sync silently; failures keep the
//! cached local guide. The token and the config contents are never logged.

use serde::Deserialize;
use std::path::Path;
use std::time::Duration;
use tauri::AppHandle;

/// Config file names, in order of preference.
const CONFIG_FILES: [&str; 1] = ["voice_sync.json"];
const VERSION_FILE: &str = "voice_guide.version";
const TOKEN_HEADER: &str = "x-voice-guide-token";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SYNC_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Deserialize)]
struct SyncConfig {
    url: String,
    token: String,
}

#[derive(Deserialize, Debug, PartialEq)]
struct GuideResponse {
    version: i64,
    #[allow(dead_code)]
    updated_at: Option<String>,
    body: String,
}

/// Parses the config file text. Errors never include the file contents.
fn parse_config(text: &str) -> Result<SyncConfig, String> {
    let config: SyncConfig =
        serde_json::from_str(text).map_err(|_| "invalid sync config".to_string())?;
    if config.url.trim().is_empty() || config.token.trim().is_empty() {
        return Err("sync config has an empty url or token".to_string());
    }
    Ok(config)
}

fn parse_response(text: &str) -> Result<GuideResponse, String> {
    let response: GuideResponse =
        serde_json::from_str(text).map_err(|_| "invalid voice guide response".to_string())?;
    if response.body.trim().is_empty() {
        return Err("voice guide response had an empty body".to_string());
    }
    Ok(response)
}

/// Writes `contents` to `path` via a temp file and rename.
fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename failed: {e}"))
}

/// Reads the first existing config file in `dir`. `None` = no config file.
fn read_config(dir: &Path) -> Option<Result<SyncConfig, String>> {
    CONFIG_FILES
        .iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok())
        .map(|text| parse_config(&text))
}

/// True when a valid sync config exists (cheap; used to show the sync button).
#[tauri::command]
#[specta::specta]
pub fn voice_sync_configured(app: AppHandle) -> bool {
    let Ok(path) = crate::transform::voice_guide_path(&app) else {
        return false;
    };
    path.parent()
        .and_then(read_config)
        .is_some_and(|config| config.is_ok())
}

/// Downloads the guide and caches it. `Ok(None)` means sync is not configured.
async fn sync_once(app: &AppHandle) -> Result<Option<i64>, String> {
    let guide_path = crate::transform::voice_guide_path(app)?;
    let dir = guide_path.parent().ok_or("no app data dir")?;
    let Some(config) = read_config(dir) else {
        return Ok(None);
    };
    let config = config?;

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "failed to build HTTP client".to_string())?;
    // Transport errors can embed the URL: report only their kind.
    let response = client
        .get(config.url.trim())
        .header(TOKEN_HEADER, config.token.trim())
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "voice guide sync timed out".to_string()
            } else if e.is_connect() {
                "voice guide sync could not connect".to_string()
            } else {
                "voice guide sync request failed".to_string()
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "voice guide sync returned HTTP {}",
            status.as_u16()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|_| "failed to read voice guide response".to_string())?;
    let guide = parse_response(&body)?;
    write_atomic(&guide_path, &guide.body)?;
    let _ = std::fs::write(dir.join(VERSION_FILE), guide.version.to_string());
    log::info!("voice guide synced v{}", guide.version);
    Ok(Some(guide.version))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_voice_guide(app: AppHandle) -> Result<i64, String> {
    sync_once(&app)
        .await?
        .ok_or_else(|| "Sync is not configured".to_string())
}

/// Syncs at startup, then hourly, on a background task.
pub fn start_background_sync(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match sync_once(&app).await {
                Ok(Some(_)) => {}
                Ok(None) => log::info!("voice guide sync disabled (no voice_sync.json)"),
                Err(e) => log::warn!("voice guide sync failed: {e}"),
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_config() {
        let c = parse_config(r#"{"url":"https://x.test/g","token":"abc"}"#).unwrap();
        assert_eq!(c.url, "https://x.test/g");
        assert_eq!(c.token, "abc");
    }

    #[test]
    fn rejects_missing_or_empty_fields() {
        assert!(parse_config(r#"{"url":"https://x.test"}"#).is_err());
        assert!(parse_config(r#"{"token":"abc"}"#).is_err());
        assert!(parse_config(r#"{"url":"https://x.test","token":"  "}"#).is_err());
        assert!(parse_config(r#"{"url":"","token":"abc"}"#).is_err());
        assert!(parse_config("not json").is_err());
    }

    #[test]
    fn config_errors_never_contain_the_token() {
        let secret = "SUPERSECRETTOKEN";
        for text in [
            format!(r#"{{"token":"{secret}"}}"#),
            format!(r#"{{"url":"","token":"{secret}"}}"#),
            format!(r#"{{"url":5,"token":"{secret}"}}"#),
            format!("{secret} garbage"),
        ] {
            let err = parse_config(&text).err().expect("must fail");
            assert!(!err.contains(secret), "{err}");
        }
    }

    #[test]
    fn read_config_uses_first_existing_file_and_reports_invalid_ones() {
        let dir = std::env::temp_dir().join(format!("handy_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_config(&dir).is_none());
        std::fs::write(
            dir.join("voice_sync.json"),
            r#"{"url":"https://new.test","token":"t"}"#,
        )
        .unwrap();
        assert_eq!(read_config(&dir).unwrap().unwrap().url, "https://new.test");
        std::fs::write(dir.join("voice_sync.json"), "nope").unwrap();
        assert!(read_config(&dir).unwrap().is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_response() {
        let r = parse_response(r#"{"version":4,"updated_at":"2026-10-14","body":"Hi"}"#).unwrap();
        assert_eq!(r.version, 4);
        assert_eq!(r.body, "Hi");
        assert!(parse_response(r#"{"version":4,"body":"  "}"#).is_err());
        assert!(parse_response(r#"{"body":"x"}"#).is_err());
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = std::env::temp_dir().join(format!("handy_vs_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("voice_guide.md");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert!(!dir.join("voice_guide.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
