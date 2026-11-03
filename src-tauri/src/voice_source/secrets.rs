//! Secure storage for MCP access/refresh tokens.
//!
//! On macOS the tokens live in the login keychain and on Windows in Credential
//! Manager (service "TalknGo MCP", account = server URL) as one JSON item.
//! Elsewhere, and in tests, a
//! process-wide in-memory map stands in so CI on Linux can run.

use serde::{Deserialize, Serialize};

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const SERVICE: &str = "TalknGo MCP";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token expires.
    pub expires_at: Option<i64>,
    pub client_id: String,
    pub token_endpoint: String,
}

// Never print token values, even through `{:?}`.
impl std::fmt::Debug for StoredTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredTokens")
            .field("expires_at", &self.expires_at)
            .field("client_id", &self.client_id)
            .field("token_endpoint", &self.token_endpoint)
            .finish_non_exhaustive()
    }
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
mod backend {
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static STORE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

    pub fn set(account: &str, value: &str) -> Result<(), String> {
        STORE
            .lock()
            .map_err(|_| "token store poisoned".to_string())?
            .insert(account.to_string(), value.to_string());
        Ok(())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        Ok(STORE
            .lock()
            .map_err(|_| "token store poisoned".to_string())?
            .get(account)
            .cloned())
    }

    pub fn remove(account: &str) -> Result<(), String> {
        STORE
            .lock()
            .map_err(|_| "token store poisoned".to_string())?
            .remove(account);
        Ok(())
    }
}

#[cfg(all(any(target_os = "macos", target_os = "windows"), not(test)))]
mod backend {
    use super::SERVICE;

    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| format!("Secure storage error: {e}"))
    }

    pub fn set(account: &str, value: &str) -> Result<(), String> {
        entry(account)?
            .set_password(value)
            .map_err(|e| format!("Could not save to secure storage: {e}"))
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Could not read secure storage: {e}")),
        }
    }

    pub fn remove(account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Could not delete from secure storage: {e}")),
        }
    }
}

pub fn save_tokens(server_url: &str, t: &StoredTokens) -> Result<(), String> {
    let json = serde_json::to_string(t).map_err(|e| format!("Could not encode tokens: {e}"))?;
    backend::set(&super::oauth::normalize_server_url(server_url), &json)
}

pub fn load_tokens(server_url: &str) -> Result<Option<StoredTokens>, String> {
    match backend::get(&super::oauth::normalize_server_url(server_url))? {
        None => Ok(None),
        Some(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|_| "Stored sign-in is unreadable; please connect again".to_string()),
    }
}

pub fn delete_tokens(server_url: &str) -> Result<(), String> {
    backend::remove(&super::oauth::normalize_server_url(server_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_delete_roundtrip() {
        let url = "https://example.test/mcp";
        let t = StoredTokens {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: Some(123),
            client_id: "cid".into(),
            token_endpoint: "https://example.test/token".into(),
        };
        assert!(load_tokens(url).unwrap().is_none());
        save_tokens(url, &t).unwrap();
        assert_eq!(load_tokens(url).unwrap(), Some(t));
        delete_tokens(url).unwrap();
        assert!(load_tokens(url).unwrap().is_none());
        delete_tokens(url).unwrap();
    }

    #[test]
    fn urls_are_normalized_on_every_call() {
        let t = StoredTokens {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: None,
            client_id: "c".into(),
            token_endpoint: "https://n.test/token".into(),
        };
        save_tokens(" https://norm.test/mcp/ ", &t).unwrap();
        assert!(load_tokens("https://norm.test/mcp").unwrap().is_some());
        delete_tokens("https://norm.test/mcp/").unwrap();
        assert!(load_tokens("https://norm.test/mcp/").unwrap().is_none());
    }
}
