//! Client-side auth SDK for the netplay REST endpoints.
//!
//! Every Rust consumer of the relay authenticates through here rather than
//! hand-rolling HTTP. Players call [`player_login`] / [`player_register`] and then
//! open the WebSocket with the returned token; a Rust admin tool would call
//! [`admin_login`] / [`admin_durable_token`]. Each function POSTs JSON and returns
//! the bearer token the server minted.
//!
//! These are **blocking** (ureq): the game calls them on its network thread just
//! before the async WebSocket loop, and a synchronous admin tool can call them
//! directly. `base_url` is the HTTP origin of the host that serves the endpoint —
//! the game host for players (e.g. `https://relay.netplay.oliverj.network`), the
//! admin host for admin calls.

use serde::Deserialize;

/// A minted bearer token and how long it lives.
#[derive(Clone, Debug, Deserialize)]
pub struct Token {
    pub token: String,
    pub expires_in_hours: i64,
}

/// Why an auth request failed.
#[derive(Clone, Debug)]
pub enum AuthError {
    /// The server refused the request (bad password, name taken, …). Carries the
    /// server's message, suitable to show the user.
    Rejected(String),
    /// The request never got a well-formed answer (network, TLS, decode).
    Transport(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Rejected(msg) | AuthError::Transport(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for AuthError {}

/// Log in an existing account: `POST {base_url}/login`.
pub fn player_login(base_url: &str, name: &str, password: &str) -> Result<Token, AuthError> {
    post_credentials(base_url, "/login", name, password)
}

/// Register a new account and get a token in one step: `POST {base_url}/register`.
pub fn player_register(base_url: &str, name: &str, password: &str) -> Result<Token, AuthError> {
    post_credentials(base_url, "/register", name, password)
}

/// Admin login: `POST {base_url}/admin/login` (admin host). For a Rust admin tool.
pub fn admin_login(base_url: &str, name: &str, password: &str) -> Result<Token, AuthError> {
    post_credentials(base_url, "/admin/login", name, password)
}

/// Trade an admin bearer for a longer-lived one: `POST {base_url}/admin/tokens`.
/// `days` is the requested lifetime (server clamps it); `None` takes the default.
pub fn admin_durable_token(
    base_url: &str,
    bearer: &str,
    days: Option<i64>,
) -> Result<Token, AuthError> {
    let request = ureq::post(&format!("{base_url}/admin/tokens"))
        .set("Authorization", &format!("Bearer {bearer}"));
    let body = match days {
        Some(days) => serde_json::json!({ "days": days }),
        None => serde_json::json!({}),
    };
    read_token(request.send_json(body))
}

/// POST a `{name, password}` body and read back a [`Token`].
fn post_credentials(
    base_url: &str,
    path: &str,
    name: &str,
    password: &str,
) -> Result<Token, AuthError> {
    let request = ureq::post(&format!("{base_url}{path}"));
    let body = serde_json::json!({ "name": name, "password": password });
    read_token(request.send_json(body))
}

/// Interpret a ureq result: 2xx → the token; a status error → the server's text;
/// anything else → a transport error.
fn read_token(result: Result<ureq::Response, ureq::Error>) -> Result<Token, AuthError> {
    match result {
        Ok(response) => response
            .into_json::<Token>()
            .map_err(|e| AuthError::Transport(e.to_string())),
        Err(ureq::Error::Status(_, response)) => {
            let message = response.into_string().unwrap_or_default();
            Err(AuthError::Rejected(if message.trim().is_empty() {
                "authentication failed".to_string()
            } else {
                message.trim().to_string()
            }))
        }
        Err(e) => Err(AuthError::Transport(e.to_string())),
    }
}
