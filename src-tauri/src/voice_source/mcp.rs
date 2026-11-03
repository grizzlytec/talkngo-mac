//! Minimal MCP (Model Context Protocol) client over streamable HTTP.
//!
//! Just enough to open a session, list tools and call one tool without
//! arguments. Errors never contain the access token.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const PROTOCOL_VERSION: &str = "2025-06-18";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// Responses larger than this are rejected before parsing.
const MAX_BODY_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    /// Number of required arguments (tools we can call need 0).
    pub required_args: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpError {
    Unauthorized,
    Other(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Unauthorized => write!(f, "The server asked to sign in again"),
            McpError::Other(m) => write!(f, "{m}"),
        }
    }
}

fn other<T: Into<String>>(m: T) -> McpError {
    McpError::Other(m.into())
}

pub struct McpSession {
    pub server_name: String,
    url: String,
    token: String,
    session_id: Option<String>,
    client: reqwest::Client,
    next_id: AtomicU64,
}

/// Picks the JSON-RPC message with `id` from a response body, which is plain
/// JSON (`application/json`) or a server-sent event stream.
pub fn parse_rpc_body(content_type: &str, body: &str, id: u64) -> Result<Value, String> {
    let matches = |v: &Value| v.get("id").and_then(Value::as_u64) == Some(id);
    if content_type.contains("text/event-stream") {
        let mut data = String::new();
        // A trailing blank line flushes the last event.
        for line in body.lines().chain(std::iter::once("")) {
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            } else if line.is_empty() && !data.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if matches(&v) {
                        return Ok(v);
                    }
                }
                data.clear();
            }
        }
        return Err("The server's reply had no answer to the request".to_string());
    }
    let v: Value = serde_json::from_str(body)
        .map_err(|_| "The server sent an unreadable reply".to_string())?;
    match v {
        Value::Array(items) => items
            .into_iter()
            .find(matches)
            .ok_or_else(|| "The server's reply had no answer to the request".to_string()),
        v if matches(&v) => Ok(v),
        _ => Err("The server's reply had no answer to the request".to_string()),
    }
}

/// `result` of a JSON-RPC message, or its error text.
fn rpc_result(msg: &Value) -> Result<&Value, McpError> {
    if let Some(err) = msg.get("error") {
        let m = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(other(format!("The server returned an error: {m}")));
    }
    msg.get("result")
        .ok_or_else(|| other("The server's reply had no result"))
}

/// Joined text content of a `tools/call` result; `isError` is an error.
fn text_from_call_result(result: &Value) -> Result<String, String> {
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let detail: String = text.chars().take(200).collect();
        return Err(format!("The tool reported an error: {detail}"));
    }
    Ok(text)
}

fn tools_from_result(result: &Value) -> Vec<McpTool> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?.to_string();
                    let description = t
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let required_args = t
                        .get("inputSchema")
                        .and_then(|s| s.get("required"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Some(McpTool {
                        name,
                        description,
                        required_args,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The conventional tool names: (guide, words).
pub fn pick_tools(tools: &[McpTool]) -> (Option<String>, Option<String>) {
    let find = |name: &str| {
        tools
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.name.clone())
    };
    (find("get_voice_guide"), find("get_vocabulary"))
}

/// Words from `{"words":[...]}`, a JSON array, or one word per line. Entries
/// like `Loxone = locks one, lox one` keep only the part before the first `=`.
pub fn parse_words(text: &str) -> Vec<String> {
    let entries: Vec<String> = match serde_json::from_str::<Value>(text.trim()) {
        Ok(Value::Object(map)) => match map.get("words") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        },
        Ok(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => text.lines().map(str::to_string).collect(),
    };
    entries
        .iter()
        .map(|e| e.split('=').next().unwrap_or("").trim().to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Reads the body (capped). For an event stream, stops as soon as the message
/// with `id` has arrived instead of waiting for the stream to close.
async fn read_capped(
    mut response: reqwest::Response,
    content_type: &str,
    id: u64,
) -> Result<String, McpError> {
    let is_sse = content_type.contains("text/event-stream");
    if response
        .content_length()
        .is_some_and(|n| n as usize > MAX_BODY_BYTES)
    {
        return Err(other("The server's reply was too large"));
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| other("Could not read the server's reply"))?
    {
        if buf.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(other("The server's reply was too large"));
        }
        buf.extend_from_slice(&chunk);
        if is_sse && parse_rpc_body(content_type, &String::from_utf8_lossy(&buf), id).is_ok() {
            break;
        }
    }
    String::from_utf8(buf).map_err(|_| other("The server sent an unreadable reply"))
}

fn transport_error(e: &reqwest::Error) -> McpError {
    // Transport errors can embed the URL: report only their kind.
    if e.is_timeout() {
        other("The server took too long to answer")
    } else if e.is_connect() {
        other("Could not connect to the server")
    } else {
        other("The request to the server failed")
    }
}

impl McpSession {
    async fn post(
        &self,
        body: &Value,
        expect_id: Option<u64>,
    ) -> Result<(Option<Value>, Option<String>), McpError> {
        let mut req = self
            .client
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.token))
            .json(body);
        if self.session_id.is_some() || expect_id.is_some() {
            req = req.header("MCP-Protocol-Version", PROTOCOL_VERSION);
        }
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        let response = req.send().await.map_err(|e| transport_error(&e))?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err(McpError::Unauthorized);
        }
        if !status.is_success() {
            return Err(other(format!(
                "The server returned HTTP {}",
                status.as_u16()
            )));
        }
        let sid = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let Some(id) = expect_id else {
            return Ok((None, sid));
        };
        let content_type = response
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = read_capped(response, &content_type, id).await?;
        let msg = parse_rpc_body(&content_type, &text, id).map_err(other)?;
        Ok((Some(msg), sid))
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let (msg, _) = self.post(&body, Some(id)).await?;
        let msg = msg.ok_or_else(|| other("The server sent no reply"))?;
        Ok(rpc_result(&msg)?.clone())
    }

    /// Initializes a session (`initialize` + `notifications/initialized`).
    pub async fn open(url: &str, token: &str) -> Result<McpSession, McpError> {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|_| other("Could not start the network client"))?;
        let mut session = McpSession {
            server_name: String::new(),
            url: url.to_string(),
            token: token.to_string(),
            session_id: None,
            client,
            next_id: AtomicU64::new(1),
        };
        let id = session.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({
            "jsonrpc": "2.0", "id": id, "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "TalknGo", "version": env!("CARGO_PKG_VERSION")},
            }
        });
        let (msg, sid) = session.post(&body, Some(id)).await?;
        let msg = msg.ok_or_else(|| other("The server sent no reply"))?;
        let result = rpc_result(&msg)?;
        session.server_name = result
            .get("serverInfo")
            .and_then(|s| s.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("MCP server")
            .to_string();
        session.session_id = sid;
        session
            .post(
                &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                None,
            )
            .await?;
        Ok(session)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let result = self.request("tools/list", json!({})).await?;
        Ok(tools_from_result(&result))
    }

    /// Calls a tool with no arguments and returns its joined text content.
    pub async fn call_text(&self, tool: &str) -> Result<String, McpError> {
        let result = self
            .request("tools/call", json!({"name": tool, "arguments": {}}))
            .await?;
        text_from_call_result(&result).map_err(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: String::new(),
            required_args: 0,
        }
    }

    #[test]
    fn parses_sse_response_by_id() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{\"x\":1}}\n\n\
                    event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\n\n";
        let v = parse_rpc_body("text/event-stream", body, 3).unwrap();
        assert_eq!(v["result"]["ok"], true);
        assert!(parse_rpc_body("text/event-stream", body, 4).is_err());
    }

    #[test]
    fn parses_json_response() {
        let body = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#;
        let v = parse_rpc_body("application/json", body, 2).unwrap();
        assert!(v["result"]["tools"].is_array());
        assert!(parse_rpc_body("application/json", body, 5).is_err());
        assert!(parse_rpc_body("application/json", "nope", 2).is_err());
    }

    #[test]
    fn pick_tools_finds_convention_names() {
        let tools = [
            tool("other"),
            tool("get_vocabulary"),
            tool("get_voice_guide"),
        ];
        assert_eq!(
            pick_tools(&tools),
            (
                Some("get_voice_guide".to_string()),
                Some("get_vocabulary".to_string())
            )
        );
    }

    #[test]
    fn pick_tools_none_when_missing() {
        assert_eq!(pick_tools(&[tool("other")]), (None, None));
        assert_eq!(pick_tools(&[]), (None, None));
    }

    #[test]
    fn parse_words_accepts_object_array_and_lines() {
        assert_eq!(
            parse_words(r#"{"words":["Loxone","Zephyr"]}"#),
            ["Loxone", "Zephyr"]
        );
        assert_eq!(parse_words(r#"["a","b"]"#), ["a", "b"]);
        assert_eq!(parse_words("one\n  two \n\nthree"), ["one", "two", "three"]);
        assert_eq!(
            parse_words(r#"{"words":["Loxone = locks one, lox one, loxon"," = x","Touch Pure"]}"#),
            ["Loxone", "Touch Pure"]
        );
        assert!(parse_words("").is_empty());
    }

    #[test]
    fn call_text_error_result_is_err() {
        let ok = json!({"content":[{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}]});
        assert_eq!(text_from_call_result(&ok).unwrap(), "a\nb");
        let bad = json!({"isError":true,"content":[{"type":"text","text":"boom"}]});
        assert!(text_from_call_result(&bad).is_err());
    }

    #[test]
    fn tools_parse_required_args() {
        let r = json!({"tools":[{"name":"a","description":"d","inputSchema":{"required":["x"]}},{"name":"b"}]});
        let t = tools_from_result(&r);
        assert_eq!(t[0].required_args, 1);
        assert_eq!(t[1].required_args, 0);
    }
}
