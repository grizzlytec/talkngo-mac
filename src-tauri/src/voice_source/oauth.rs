//! OAuth 2.1 sign-in for MCP servers: discovery (RFC 9728 / 8414), dynamic
//! client registration, PKCE (S256) and a loopback redirect listener.
//!
//! Tokens, codes and verifiers are never logged and never put in error text.

use super::secrets::{save_tokens, StoredTokens};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{distributions::Alphanumeric, Rng};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const CONN_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_LINE: usize = 8192;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthServer {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("Could not start the network client: {e}"))
}

/// Trim whitespace and a trailing slash; the key under which tokens are stored.
pub fn normalize_server_url(server_url: &str) -> String {
    server_url.trim().trim_end_matches('/').to_string()
}

fn origin_of(server_url: &str) -> Result<String, String> {
    let u = Url::parse(&normalize_server_url(server_url))
        .map_err(|_| "That doesn't look like a valid server address".to_string())?;
    if u.scheme() != "https" && u.scheme() != "http" {
        return Err("The server address must start with https://".to_string());
    }
    Ok(u.origin().ascii_serialization())
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<Value> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

fn parse_auth_server(v: &Value) -> Option<AuthServer> {
    Some(AuthServer {
        authorization_endpoint: v.get("authorization_endpoint")?.as_str()?.to_string(),
        token_endpoint: v.get("token_endpoint")?.as_str()?.to_string(),
        registration_endpoint: v
            .get("registration_endpoint")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        scopes_supported: v
            .get("scopes_supported")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub async fn discover(server_url: &str) -> Result<AuthServer, String> {
    let origin = origin_of(server_url)?;
    let client = http_client()?;

    // RFC 9728: protected resource metadata names the authorization server.
    if let Some(pr) = get_json(
        &client,
        &format!("{origin}/.well-known/oauth-protected-resource"),
    )
    .await
    {
        if let Some(issuer) = pr
            .get("authorization_servers")
            .and_then(|a| a.get(0))
            .and_then(|s| s.as_str())
        {
            let issuer = issuer.trim_end_matches('/');
            let mut candidates = Vec::new();
            if let Ok(iu) = Url::parse(issuer) {
                let path = iu.path().trim_end_matches('/');
                let io = iu.origin().ascii_serialization();
                // RFC 8414 inserts the well-known segment before the issuer path.
                candidates.push(format!("{io}/.well-known/oauth-authorization-server{path}"));
            }
            candidates.push(format!("{issuer}/.well-known/oauth-authorization-server"));
            for c in candidates {
                if let Some(a) = get_json(&client, &c)
                    .await
                    .as_ref()
                    .and_then(parse_auth_server)
                {
                    return Ok(a);
                }
            }
        }
    }

    // Fallback: the server is its own authorization server.
    get_json(
        &client,
        &format!("{origin}/.well-known/oauth-authorization-server"),
    )
    .await
    .as_ref()
    .and_then(parse_auth_server)
    .ok_or_else(|| "Couldn't find the sign-in details for this server".to_string())
}

/// Returns (verifier, S256 challenge as base64url without padding).
pub fn pkce_pair() -> (String, String) {
    let verifier: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    let challenge = challenge_for(&verifier);
    (verifier, challenge)
}

fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(
    as_: &AuthServer,
    client_id: &str,
    redirect: &str,
    challenge: &str,
    state: &str,
    scope: Option<&str>,
) -> String {
    let mut url = match Url::parse(&as_.authorization_endpoint) {
        Ok(u) => u,
        Err(_) => return as_.authorization_endpoint.clone(),
    };
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("client_name", "TalknGo");
        if let Some(s) = scope {
            if as_.scopes_supported.iter().any(|x| x == s) {
                q.append_pair("scope", s);
            }
        }
    }
    url.to_string()
}

/// Reads the code out of `GET /callback?code=..&state=.. HTTP/1.1`.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Result<String, String> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "Unexpected response from the browser".to_string())?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| "Unexpected response from the browser".to_string())?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Err("Sign-in response didn't match this request; please try again".to_string());
    }
    if let Some(e) = error {
        return Err(format!("Sign-in was not completed ({e})"));
    }
    code.filter(|c| !c.is_empty())
        .ok_or_else(|| "Sign-in response had no code".to_string())
}

const OK_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>TalknGo</title></head><body style=\"font-family:-apple-system,sans-serif;text-align:center;margin-top:20vh\"><h2>You're connected.</h2><p>You can close this tab and go back to TalknGo.</p></body></html>";
const ERR_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>TalknGo</title></head><body style=\"font-family:-apple-system,sans-serif;text-align:center;margin-top:20vh\"><h2>Sign-in didn't finish.</h2><p>You can close this tab and go back to TalknGo to try again.</p></body></html>";

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(msg.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let read = async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.contains(&b'\n') || buf.len() >= MAX_REQUEST_LINE {
                break;
            }
        }
        Some(())
    };
    tokio::time::timeout(CONN_READ_TIMEOUT, read).await.ok()??;
    let text = String::from_utf8_lossy(&buf);
    text.lines().next().map(str::to_string)
}

/// Waits for one `/callback` request (ignoring strays like /favicon.ico).
/// Takes ownership of the listener so it is always closed on return.
async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    limit: Duration,
) -> Result<String, String> {
    let work = async {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("Could not listen for the browser: {e}"))?;
            let Some(line) = read_request_line(&mut stream).await else {
                continue;
            };
            let is_callback = line
                .split_whitespace()
                .nth(1)
                .map(|t| t == "/callback" || t.starts_with("/callback?"))
                .unwrap_or(false);
            if !is_callback {
                respond(&mut stream, "404 Not Found", "Not found").await;
                continue;
            }
            let result = parse_callback(&line, expected_state);
            match &result {
                Ok(_) => respond(&mut stream, "200 OK", OK_PAGE).await,
                Err(_) => respond(&mut stream, "400 Bad Request", ERR_PAGE).await,
            }
            return result;
        }
    };
    let out = tokio::time::timeout(limit, work).await;
    drop(listener);
    out.unwrap_or_else(|_| {
        Err("Timed out waiting for you to sign in in the browser. Please try again.".to_string())
    })
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn tokens_from_response(
    v: &Value,
    client_id: &str,
    token_endpoint: &str,
    old_refresh: Option<String>,
) -> Result<StoredTokens, String> {
    let access_token = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "The server's sign-in response had no access token".to_string())?
        .to_string();
    let refresh_token = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or(old_refresh);
    let expires_at = v
        .get("expires_in")
        .and_then(|x| x.as_i64())
        .map(|s| now_secs() + s);
    Ok(StoredTokens {
        access_token,
        refresh_token,
        expires_at,
        client_id: client_id.to_string(),
        token_endpoint: token_endpoint.to_string(),
    })
}

/// Error text from a failed token call: status plus the OAuth `error` code only.
async fn token_error(what: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    let code = resp
        .json::<Value>()
        .await
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    match code {
        Some(c) => format!("{what} failed ({status}: {c})"),
        None => format!("{what} failed ({status})"),
    }
}

async fn post_token_form(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    what: &str,
) -> Result<Value, String> {
    let resp = client.post(endpoint).form(form).send().await.map_err(|e| {
        format!(
            "{what} failed: couldn't reach the server ({})",
            e.without_url()
        )
    })?;
    if !resp.status().is_success() {
        return Err(token_error(what, resp).await);
    }
    resp.json::<Value>()
        .await
        .map_err(|_| format!("{what} failed: unreadable response"))
}

fn random_state() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

pub async fn connect(app: &AppHandle, server_url: &str) -> Result<StoredTokens, String> {
    let server_url = normalize_server_url(server_url);
    let as_ = discover(&server_url).await?;
    let registration = as_
        .registration_endpoint
        .clone()
        .ok_or_else(|| "This server doesn't support app sign-in".to_string())?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Could not open a local port for sign-in: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Could not open a local port for sign-in: {e}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let client = http_client()?;
    let wants_voice = as_.scopes_supported.iter().any(|s| s == "voice");
    let mut reg_body = json!({
        "client_name": "TalknGo",
        "redirect_uris": [redirect],
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    });
    if wants_voice {
        reg_body["scope"] = json!("voice");
    }
    let resp = client
        .post(&registration)
        .json(&reg_body)
        .send()
        .await
        .map_err(|e| format!("Registering with the server failed ({})", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(token_error("Registering with the server", resp).await);
    }
    let reg: Value = resp
        .json()
        .await
        .map_err(|_| "Registering with the server failed: unreadable response".to_string())?;
    let client_id = reg
        .get("client_id")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "The server didn't return a client id".to_string())?
        .to_string();

    let (verifier, challenge) = pkce_pair();
    let state = random_state();
    let url = authorize_url(
        &as_,
        &client_id,
        &redirect,
        &challenge,
        &state,
        wants_voice.then_some("voice"),
    );
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Could not open the browser: {e}"))?;

    let code = wait_for_callback(listener, &state, CALLBACK_TIMEOUT).await?;

    let v = post_token_form(
        &client,
        &as_.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("client_id", &client_id),
            ("code_verifier", &verifier),
        ],
        "Sign-in",
    )
    .await?;
    let tokens = tokens_from_response(&v, &client_id, &as_.token_endpoint, None)?;
    save_tokens(&server_url, &tokens)?;
    Ok(tokens)
}

pub async fn refresh(t: &StoredTokens) -> Result<StoredTokens, String> {
    let rt = t
        .refresh_token
        .clone()
        .ok_or_else(|| "Sign-in expired; please connect again".to_string())?;
    let client = http_client()?;
    let v = post_token_form(
        &client,
        &t.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &rt),
            ("client_id", &t.client_id),
        ],
        "Refreshing sign-in",
    )
    .await?;
    tokens_from_response(&v, &t.client_id, &t.token_endpoint, Some(rt))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal HTTP server: `routes` maps path -> (status, json body).
    async fn serve(routes: Vec<(&'static str, u16, String)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, st, b)| (*st, b.clone()))
                        .unwrap_or((404, "{}".to_string()));
                    let msg = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(msg.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn sample_as(scopes: &[&str]) -> AuthServer {
        AuthServer {
            authorization_endpoint: "https://x.test/authorize".into(),
            token_endpoint: "https://x.test/token".into(),
            registration_endpoint: Some("https://x.test/register".into()),
            scopes_supported: scopes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (v, c) = pkce_pair();
        assert_eq!(v.len(), 64);
        assert_eq!(c, challenge_for(&v));
    }

    #[test]
    fn authorize_url_requests_voice_only_when_supported() {
        let with = authorize_url(
            &sample_as(&["mcp", "voice"]),
            "cid",
            "http://127.0.0.1:1/callback",
            "chal",
            "st",
            Some("voice"),
        );
        assert!(with.contains("scope=voice"));
        assert!(with.contains("code_challenge_method=S256"));
        assert!(with.contains("client_name=TalknGo"));
        let without = authorize_url(
            &sample_as(&["mcp"]),
            "cid",
            "http://127.0.0.1:1/callback",
            "chal",
            "st",
            Some("voice"),
        );
        assert!(!without.contains("scope="));
    }

    #[test]
    fn parse_callback_rejects_wrong_state() {
        let e = parse_callback("GET /callback?code=abc&state=bad HTTP/1.1", "good").unwrap_err();
        assert!(!e.contains("abc"));
        assert!(parse_callback(
            "GET /callback?error=access_denied&state=good HTTP/1.1",
            "good"
        )
        .is_err());
    }

    #[test]
    fn parse_callback_returns_code() {
        assert_eq!(
            parse_callback("GET /callback?code=abc%20d&state=good HTTP/1.1", "good").unwrap(),
            "abc d"
        );
    }

    #[tokio::test]
    async fn discover_falls_back_to_origin_metadata() {
        let meta = json!({
            "authorization_endpoint": "http://h/authorize",
            "token_endpoint": "http://h/token",
            "registration_endpoint": "http://h/register",
            "scopes_supported": ["mcp", "voice"],
        })
        .to_string();
        let base = serve(vec![("/.well-known/oauth-authorization-server", 200, meta)]).await;
        let a = discover(&format!("  {base}/mcp/ ")).await.unwrap();
        assert_eq!(a.token_endpoint, "http://h/token");
        assert_eq!(a.scopes_supported, vec!["mcp", "voice"]);
    }

    #[tokio::test]
    async fn discover_follows_protected_resource_metadata() {
        let meta = json!({
            "authorization_endpoint": "http://h/authorize",
            "token_endpoint": "http://h/token",
        })
        .to_string();
        let auth = serve(vec![("/.well-known/oauth-authorization-server", 200, meta)]).await;
        let pr = json!({ "authorization_servers": [auth] }).to_string();
        let rs = serve(vec![("/.well-known/oauth-protected-resource", 200, pr)]).await;
        let a = discover(&format!("{rs}/mcp")).await.unwrap();
        assert_eq!(a.token_endpoint, "http://h/token");
    }

    #[tokio::test]
    async fn callback_ignores_strays_and_returns_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let h =
            tokio::spawn(
                async move { wait_for_callback(listener, "st", Duration::from_secs(5)).await },
            );
        for path in ["/favicon.ico", "/callback?code=zzz&state=st"] {
            let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            c.write_all(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).await.unwrap();
            if path.starts_with("/callback") {
                assert!(out.contains("You can close this tab"));
            }
        }
        assert_eq!(h.await.unwrap().unwrap(), "zzz");
    }

    #[tokio::test]
    async fn callback_times_out_and_frees_the_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = wait_for_callback(listener, "st", Duration::from_millis(150))
            .await
            .unwrap_err();
        assert!(err.contains("Timed out"));
        TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn refresh_keeps_old_refresh_token_when_omitted() {
        let body = json!({"access_token": "new", "expires_in": 3600}).to_string();
        let base = serve(vec![("/token", 200, body)]).await;
        let old = StoredTokens {
            access_token: "old".into(),
            refresh_token: Some("keepme".into()),
            expires_at: Some(1),
            client_id: "cid".into(),
            token_endpoint: format!("{base}/token"),
        };
        let t = refresh(&old).await.unwrap();
        assert_eq!(t.access_token, "new");
        assert_eq!(t.refresh_token.as_deref(), Some("keepme"));
        assert!(t.expires_at.unwrap() > now_secs() + 3000);
    }
}
