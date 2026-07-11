//! cloud.rs — Droptime Cloud auth plumbing (build plan §9).
//!
//! Two Rust surfaces the webview auth module needs, both deliberately kept out
//! of the webview so they sidestep browser constraints:
//!
//! 1. `oauth_start` — a one-shot loopback listener. The webview opens the system
//!    browser to `app.trydroptime.com/api/logger/auth?redirect_uri=http://127.0.0.1:<port>/callback`;
//!    after Clerk sign-in that route 302s back to the loopback with
//!    `?token=<sign_in_token>&state=<nonce>`. We accept ONE request, parse it,
//!    and emit `oauth-callback` to the webview. Loopback is the cross-platform
//!    path — no custom-scheme registration and no signed `/Applications` build
//!    (which macOS deep links would require).
//!
//! 2. `cloud_fetch` — a fetch proxy over the already-vendored `ureq`. Clerk's
//!    FAPI rejects any request carrying BOTH `Origin` and `Authorization`
//!    (clerk/javascript#4725), and a Tauri webview always injects `Origin`. The
//!    webview patches `fetch` for `clerk.trydroptime.com` to route through this
//!    command, where Rust omits `Origin` and forwards the `Authorization`/
//!    `__client` token. Reusing `ureq` (see update.rs) means no `tauri-plugin-http`,
//!    no http capability, and no CSP change. (Convex's own WebSocket is direct
//!    from the webview and needs only a CSP `connect-src`.)

use std::io::{Read, Write};
use std::net::TcpListener;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::LoggerError;

// ---------------------------------------------------------------------------
// Loopback OAuth listener
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OAuthCallback {
    token: String,
    state: String,
}

const RESPONSE_OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><meta charset=utf-8><title>Droptime Logger</title><body style=\"font-family:system-ui,sans-serif;background:#0d2418;color:#fdf7ee;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"text-align:center\"><h1 style=\"color:#daf698;margin:0 0 8px\">Signed in</h1><p style=\"opacity:.8\">You can close this tab and return to the Droptime Logger.</p></div>";

/// Start the one-shot loopback listener; returns the bound ephemeral port.
/// Spawns a detached thread that accepts a single connection, emits
/// `oauth-callback`, then exits (or lives until app exit if abandoned).
pub fn start(app: AppHandle) -> Result<u16, LoggerError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new()
        .name("oauth-loopback".into())
        .spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let (token, state) = parse_callback(&req);
                let _ = stream.write_all(RESPONSE_OK.as_bytes());
                let _ = stream.flush();
                if let (Some(token), Some(state)) = (token, state) {
                    let _ = app.emit("oauth-callback", OAuthCallback { token, state });
                }
            }
        })
        .map_err(|e| LoggerError::io(format!("oauth listener thread: {e}")))?;
    Ok(port)
}

/// Parse `token` + `state` out of the GET request line's query string.
fn parse_callback(req: &str) -> (Option<String>, Option<String>) {
    let Some(line) = req.lines().next() else {
        return (None, None);
    };
    let Some(path) = line.split_whitespace().nth(1) else {
        return (None, None);
    };
    let Some(query) = path.split_once('?').map(|(_, q)| q) else {
        return (None, None);
    };
    let mut token = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "token" => token = Some(urldecode(v)),
                "state" => state = Some(urldecode(v)),
                _ => {}
            }
        }
    }
    (token, state)
}

/// Minimal percent-decoding. `state` is restricted to URL-unreserved chars
/// server-side (verbatim round-trip); the token decodes defensively.
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// cloud_fetch proxy (Clerk FAPI, via ureq)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFetchReq {
    pub method: String,
    pub url: String,
    /// Header name/value pairs to forward (Origin is deliberately NOT set).
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFetchResp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Allowlist: only Clerk's FAPI host may be proxied. A bug that let arbitrary
/// URLs through would be an SSRF-shaped hole, so this is a hard gate.
fn allowed(url: &str) -> bool {
    url.starts_with("https://clerk.trydroptime.com/")
}

/// Perform the request via ureq and collect the response verbatim. A 4xx/5xx
/// (which Clerk FAPI uses for auth state, not just errors) is returned as a
/// normal response, not an error.
pub fn fetch(req: CloudFetchReq) -> Result<CloudFetchResp, LoggerError> {
    if !allowed(&req.url) {
        return Err(LoggerError::invalid_args(format!(
            "cloud_fetch refused non-allowlisted url: {}",
            req.url
        )));
    }
    let mut request = ureq::request(&req.method, &req.url);
    for (k, v) in &req.headers {
        // Never forward Origin — the whole point of the proxy.
        if k.eq_ignore_ascii_case("origin") || k.eq_ignore_ascii_case("host") {
            continue;
        }
        request = request.set(k, v);
    }
    let result = match req.body {
        Some(body) => request.send_string(&body),
        None => request.call(),
    };
    let resp = match result {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_code, resp)) => resp, // auth-state responses
        Err(ureq::Error::Transport(t)) => {
            return Err(LoggerError::io(format!("cloud_fetch transport: {t}")))
        }
    };
    let status = resp.status();
    let headers = resp
        .headers_names()
        .into_iter()
        .filter_map(|name| resp.header(&name).map(|v| (name.clone(), v.to_string())))
        .collect();
    let body = resp
        .into_string()
        .map_err(|e| LoggerError::io(format!("cloud_fetch read: {e}")))?;
    Ok(CloudFetchResp {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_and_state_from_get_line() {
        let req =
            "GET /callback?token=abc.def-ghi&state=nonce123 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let (token, state) = parse_callback(req);
        assert_eq!(token.as_deref(), Some("abc.def-ghi"));
        assert_eq!(state.as_deref(), Some("nonce123"));
    }

    #[test]
    fn tolerates_missing_query_and_empty() {
        assert_eq!(parse_callback("GET /callback HTTP/1.1\r\n"), (None, None));
        assert_eq!(parse_callback(""), (None, None));
    }

    #[test]
    fn percent_and_plus_decode() {
        let (token, _) = parse_callback("GET /callback?token=a%2Bb%2Fc&state=x HTTP/1.1");
        assert_eq!(token.as_deref(), Some("a+b/c"));
    }

    #[test]
    fn cloud_fetch_allowlist_blocks_other_hosts() {
        assert!(allowed("https://clerk.trydroptime.com/v1/client"));
        assert!(!allowed("https://evil.example.com/"));
        assert!(!allowed("http://clerk.trydroptime.com/")); // https only
        let err = fetch(CloudFetchReq {
            method: "GET".into(),
            url: "https://evil.example.com/".into(),
            headers: vec![],
            body: None,
        })
        .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgs);
    }
}
