//! Where the "My voice" guide and the dictation words come from: an MCP
//! server, a file, or the built-in guide. Dictation and rewrites only ever
//! read local files; the network is touched by sync alone.

pub mod cache;
pub mod mcp;
pub mod oauth;
pub mod secrets;

use crate::settings::{self, AppSettings};
use crate::transform::{effective_voice_source, VoiceSource};
use cache::SourceStatus;
use mcp::{McpError, McpSession, McpTool};
use secrets::StoredTokens;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::AppHandle;

const SYNC_INTERVAL: Duration = Duration::from_secs(3600);
const CACHE_DIR: &str = "voice_source";
const DEFAULT_GUIDE_FILE: &str = "voice_guide.md";

static SYNCING: AtomicBool = AtomicBool::new(false);

fn app_data(app: &AppHandle) -> Result<PathBuf, String> {
    crate::portable::app_data_dir(app)
        .map_err(|e| format!("Could not resolve the app data dir: {e}"))
}

fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data(app)?.join(CACHE_DIR))
}

fn custom_path(p: &Option<String>) -> Option<PathBuf> {
    p.as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// The guide file for the chosen source (pure; `app_data` is the app data dir).
pub fn guide_path_for(settings: &AppSettings, app_data: &Path) -> PathBuf {
    let default = app_data.join(DEFAULT_GUIDE_FILE);
    match settings.voice_source {
        Some(VoiceSource::BuiltIn) => default,
        Some(VoiceSource::File) | None => {
            custom_path(&settings.voice_guide_path).unwrap_or(default)
        }
        Some(VoiceSource::Mcp) => cache::guide_file(&app_data.join(CACHE_DIR)),
    }
}

pub fn guide_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(guide_path_for(
        &settings::get_settings(app),
        &app_data(app)?,
    ))
}

/// Dictation words from the source (pure; reads local files only).
pub fn source_words_for(settings: &AppSettings, app_data: &Path) -> Vec<String> {
    match effective_voice_source(settings) {
        VoiceSource::Mcp => cache::read_words(&app_data.join(CACHE_DIR)),
        VoiceSource::File => custom_path(&settings.voice_words_path)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| mcp::parse_words(&t))
            .unwrap_or_default(),
        VoiceSource::BuiltIn => Vec::new(),
    }
}

pub fn source_words(app: &AppHandle) -> Vec<String> {
    let settings = settings::get_settings(app);
    match app_data(app) {
        Ok(dir) => source_words_for(&settings, &dir),
        Err(_) => Vec::new(),
    }
}

/// App words first, then source words not already present (case-insensitive).
pub fn merge_words(app_words: &[String], source: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> =
        app_words.iter().map(|w| w.trim().to_lowercase()).collect();
    let mut out = app_words.to_vec();
    for w in source {
        let w = w.trim();
        if !w.is_empty() && seen.insert(w.to_lowercase()) {
            out.push(w.to_string());
        }
    }
    out
}

/// Settings whose `custom_words` include the source's words.
pub fn with_source_words(app: &AppHandle, mut settings: AppSettings) -> AppSettings {
    let merged = merge_words(&settings.custom_words, &source_words(app));
    settings.custom_words = merged;
    settings
}

/// The old `voice_sync.json` path only runs while no source was ever chosen.
pub fn legacy_sync_enabled(settings: &AppSettings) -> bool {
    settings.voice_source.is_none()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

enum Fail {
    NeedsSignIn,
    Other(String),
}

/// Runs `f` with the stored access token. On `Unauthorized`, refreshes once
/// (saving the new tokens) and retries once; any failure there means the
/// person has to sign in again.
async fn with_token<T, F, Fut>(key: &str, f: F) -> Result<T, Fail>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, McpError>>,
{
    let tokens: StoredTokens = secrets::load_tokens(key)
        .map_err(Fail::Other)?
        .ok_or(Fail::NeedsSignIn)?;
    match f(tokens.access_token.clone()).await {
        Ok(v) => return Ok(v),
        Err(McpError::Other(m)) => return Err(Fail::Other(m)),
        Err(McpError::Unauthorized) => {}
    }
    let fresh = oauth::refresh(&tokens)
        .await
        .map_err(|_| Fail::NeedsSignIn)?;
    secrets::save_tokens(key, &fresh).map_err(Fail::Other)?;
    match f(fresh.access_token.clone()).await {
        Ok(v) => Ok(v),
        Err(McpError::Unauthorized) => Err(Fail::NeedsSignIn),
        Err(McpError::Other(m)) => Err(Fail::Other(m)),
    }
}

struct Fetched {
    server_name: String,
    guide: Option<Result<String, String>>,
    words: Option<Result<String, String>>,
}

async fn fetch_all(
    url: String,
    token: String,
    guide_tool: Option<String>,
    words_tool: Option<String>,
) -> Result<Fetched, McpError> {
    let session = McpSession::open(&url, &token).await?;
    let (mut guide_tool, mut words_tool) = (guide_tool, words_tool);
    if guide_tool.is_none() || words_tool.is_none() {
        let (g, w) = mcp::pick_tools(&session.list_tools().await?);
        guide_tool = guide_tool.or(g);
        words_tool = words_tool.or(w);
    }
    let call = |tool: Option<String>| async {
        match tool {
            None => Ok(None),
            Some(t) => match session.call_text(&t).await {
                Ok(text) => Ok(Some(Ok(text))),
                Err(McpError::Unauthorized) => Err(McpError::Unauthorized),
                Err(McpError::Other(m)) => Ok(Some(Err(m))),
            },
        }
    };
    let guide = call(guide_tool).await?;
    let words = call(words_tool).await?;
    Ok(Fetched {
        server_name: session.server_name.clone(),
        guide,
        words,
    })
}

/// Fetches from the MCP server and updates the cache. Without a connected
/// MCP source it only reports the status. Never touches `voice_guide.md`.
pub async fn sync_now(app: &AppHandle) -> Result<SourceStatus, String> {
    let settings = settings::get_settings(app);
    let dir = cache_dir(app)?;
    if effective_voice_source(&settings) != VoiceSource::Mcp {
        return Ok(cache::read_status(&dir));
    }
    if SYNCING.swap(true, Ordering::SeqCst) {
        return Ok(cache::read_status(&dir));
    }
    let result = sync_mcp(&settings, &dir).await;
    SYNCING.store(false, Ordering::SeqCst);
    result
}

async fn sync_mcp(settings: &AppSettings, dir: &Path) -> Result<SourceStatus, String> {
    let mut status = cache::read_status(dir);
    let Some(url) = settings
        .voice_mcp_url
        .as_deref()
        .map(oauth::normalize_server_url)
        .filter(|u| !u.is_empty())
    else {
        status.error = Some("No server address set".to_string());
        cache::write_status(dir, &status)?;
        return Ok(status);
    };
    let fetched = with_token(&url, |token| {
        let (url, guide_tool, words_tool) = (
            url.clone(),
            settings.voice_mcp_guide_tool.clone(),
            settings.voice_mcp_words_tool.clone(),
        );
        fetch_all(url, token, guide_tool, words_tool)
    })
    .await;
    match fetched {
        Err(Fail::NeedsSignIn) => {
            status.needs_sign_in = true;
            status.error = Some("Sign in again to keep syncing".to_string());
        }
        Err(Fail::Other(m)) => {
            status.error = Some(m);
        }
        Ok(f) => {
            status.server_name = Some(f.server_name);
            status.needs_sign_in = false;
            let mut errors: Vec<String> = Vec::new();
            match f.guide {
                Some(Ok(raw)) => match cache::store_guide(dir, &raw) {
                    Ok(version) => {
                        status.guide_version = version;
                        status.synced_at = Some(now_secs());
                    }
                    Err(e) => errors.push(e),
                },
                Some(Err(e)) => errors.push(e),
                None => errors.push("No voice guide tool found on the server".to_string()),
            }
            match f.words {
                Some(Ok(raw)) => {
                    if let Err(e) = cache::store_words(dir, &mcp::parse_words(&raw)) {
                        errors.push(e);
                    }
                }
                Some(Err(e)) => errors.push(e),
                None => {}
            }
            status.error = errors.first().cloned();
        }
    }
    cache::write_status(dir, &status)?;
    Ok(status)
}

/// Syncs at startup, then hourly, on a background task.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(e) = sync_now(&app).await {
                log::warn!("voice source sync failed: {e}");
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

fn mcp_url(app: &AppHandle) -> Result<String, String> {
    settings::get_settings(app)
        .voice_mcp_url
        .as_deref()
        .map(oauth::normalize_server_url)
        .filter(|u| !u.is_empty())
        .ok_or_else(|| "No server address set".to_string())
}

/// Signs in to the server, makes MCP the source and syncs once.
#[tauri::command]
#[specta::specta]
pub async fn voice_source_connect(app: AppHandle, url: String) -> Result<SourceStatus, String> {
    let url = oauth::normalize_server_url(&url);
    if url.is_empty() {
        return Err("Enter the server address".to_string());
    }
    let mut s = settings::get_settings(&app);
    s.voice_mcp_url = Some(url.clone());
    settings::write_settings(&app, s);
    oauth::connect(&app, &url).await?;
    let mut s = settings::get_settings(&app);
    s.voice_source = Some(VoiceSource::Mcp);
    settings::write_settings(&app, s);
    sync_now(&app).await
}

/// Forgets the sign-in and the cache and goes back to the built-in guide.
#[tauri::command]
#[specta::specta]
pub fn voice_source_disconnect(app: AppHandle) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    if let Some(url) = s.voice_mcp_url.as_deref().map(oauth::normalize_server_url) {
        secrets::delete_tokens(&url)?;
    }
    cache::clear(&cache_dir(&app)?);
    s.voice_mcp_url = None;
    s.voice_mcp_guide_tool = None;
    s.voice_mcp_words_tool = None;
    s.voice_source = Some(VoiceSource::BuiltIn);
    settings::write_settings(&app, s);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn voice_source_sync(app: AppHandle) -> Result<SourceStatus, String> {
    sync_now(&app).await
}

#[tauri::command]
#[specta::specta]
pub fn voice_source_status(app: AppHandle) -> SourceStatus {
    cache_dir(&app)
        .map(|d| cache::read_status(&d))
        .unwrap_or_default()
}

/// Tools the connected server offers (for choosing the guide/words tools).
#[tauri::command]
#[specta::specta]
pub async fn voice_source_tools(app: AppHandle) -> Result<Vec<McpTool>, String> {
    let url = mcp_url(&app)?;
    let tools = with_token(&url, |token| {
        let url = url.clone();
        async move { McpSession::open(&url, &token).await?.list_tools().await }
    })
    .await;
    match tools {
        Ok(t) => Ok(t),
        Err(Fail::NeedsSignIn) => Err("Sign in again to see the tools".to_string()),
        Err(Fail::Other(m)) => Err(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn words(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("talkngo_vs_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn merge_words_app_wins_case_insensitive() {
        assert_eq!(
            merge_words(&words(&["Loxone"]), &words(&["loxone", "Touch Pure"])),
            words(&["Loxone", "Touch Pure"])
        );
        assert_eq!(merge_words(&[], &words(&[" a ", "A", ""])), words(&["a"]));
    }

    #[test]
    fn guide_path_by_source() {
        let data = Path::new("/data");
        let mut s = get_default_settings();
        s.voice_source = None;
        s.voice_guide_path = None;
        assert_eq!(guide_path_for(&s, data), data.join("voice_guide.md"));
        s.voice_guide_path = Some(" /drive/g.md ".to_string());
        assert_eq!(guide_path_for(&s, data), PathBuf::from("/drive/g.md"));
        s.voice_source = Some(VoiceSource::BuiltIn);
        assert_eq!(guide_path_for(&s, data), data.join("voice_guide.md"));
        s.voice_source = Some(VoiceSource::File);
        assert_eq!(guide_path_for(&s, data), PathBuf::from("/drive/g.md"));
        s.voice_source = Some(VoiceSource::Mcp);
        assert_eq!(guide_path_for(&s, data), data.join("voice_source/guide.md"));
    }

    #[test]
    fn oversized_guide_keeps_previous_cache() {
        let dir = tmp("big");
        cache::store_guide(&dir, "old guide").unwrap();
        let big = "x".repeat(cache::MAX_GUIDE_BYTES + 1);
        assert!(cache::store_guide(&dir, &big).is_err());
        assert_eq!(
            std::fs::read_to_string(cache::guide_file(&dir)).unwrap(),
            "old guide"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_guide_keeps_previous_cache() {
        let dir = tmp("empty");
        assert_eq!(
            cache::store_guide(&dir, r#"{"version":4,"guide":"v4 text"}"#).unwrap(),
            Some(4)
        );
        assert!(cache::store_guide(&dir, "  \n").is_err());
        assert!(cache::store_guide(&dir, r#"{"version":5,"guide":""}"#).is_err());
        assert_eq!(
            std::fs::read_to_string(cache::guide_file(&dir)).unwrap(),
            "v4 text"
        );
        assert!(cache::store_words(&dir, &[]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mcp_sync_never_touches_builtin_guide() {
        let data = tmp("builtin");
        let builtin = data.join("voice_guide.md");
        std::fs::write(&builtin, "built-in").unwrap();
        let dir = data.join(CACHE_DIR);
        cache::store_guide(&dir, "from server").unwrap();
        cache::store_words(&dir, &words(&["Loxone"])).unwrap();
        cache::write_status(&dir, &SourceStatus::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&builtin).unwrap(), "built-in");
        assert_eq!(
            std::fs::read_to_string(dir.join("guide.md")).unwrap(),
            "from server"
        );
        cache::clear(&dir);
        assert_eq!(std::fs::read_to_string(&builtin).unwrap(), "built-in");
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn legacy_sync_skipped_when_source_set() {
        let mut s = get_default_settings();
        s.voice_source = None;
        assert!(legacy_sync_enabled(&s));
        for src in [VoiceSource::BuiltIn, VoiceSource::File, VoiceSource::Mcp] {
            s.voice_source = Some(src);
            assert!(!legacy_sync_enabled(&s));
        }
    }

    #[test]
    fn source_words_by_source() {
        let data = tmp("words");
        let mut s = get_default_settings();
        s.voice_source = Some(VoiceSource::BuiltIn);
        assert!(source_words_for(&s, &data).is_empty());
        cache::store_words(&data.join(CACHE_DIR), &words(&["Kodiak"])).unwrap();
        s.voice_source = Some(VoiceSource::Mcp);
        assert_eq!(source_words_for(&s, &data), words(&["Kodiak"]));
        let f = data.join("w.txt");
        std::fs::write(&f, "Loxone = locks one\nTouch Pure").unwrap();
        s.voice_source = Some(VoiceSource::File);
        s.voice_words_path = Some(f.to_string_lossy().to_string());
        assert_eq!(
            source_words_for(&s, &data),
            words(&["Loxone", "Touch Pure"])
        );
        std::fs::remove_dir_all(&data).unwrap();
    }
}
