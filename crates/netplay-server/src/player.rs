//! Player REST auth, served on the game host (everything that isn't the admin
//! host). `POST /login` and `POST /register` exchange `{name, password}` for a
//! bearer token; the client then presents that token in the WebSocket `Hello`
//! (see [`crate::auth`]). This keeps the expensive argon2 check off the socket
//! path — it happens once, here, instead of on every connect.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use sqlx::SqlitePool;

use crate::rest::{read_json_body, token_response, Credentials, SESSION_TTL_HOURS};
use crate::store::{self, CreateError};
use crate::{json_ok, text};

/// Minimum length for a self-registered password.
pub const MIN_PASSWORD_LEN: usize = 8;
/// Maximum length for an account name — it's broadcast in lobby presence to
/// every client, so an unbounded name is a cheap abuse vector.
pub const MAX_NAME_LEN: usize = 32;

/// Route a game-host auth request. Returns `Ok(response)` when it handled one, or
/// `Err(req)` to hand the request back so the caller can try the WebSocket upgrade
/// (the body is untouched unless a route matched).
pub async fn route(
    req: Request<Incoming>,
    pool: &SqlitePool,
) -> Result<Response<Full<Bytes>>, Request<Incoming>> {
    match (req.method(), req.uri().path()) {
        // Unauthenticated on purpose, like the admin doc: it's how a client
        // discovers the auth endpoints in the first place.
        (&Method::GET, "/openapi.json") => {
            Ok(json_ok(crate::openapi::player_document().to_string()))
        }
        (&Method::POST, "/login") => Ok(login(req, pool).await),
        (&Method::POST, "/register") => Ok(register(req, pool).await),
        _ => Err(req),
    }
}

/// Verify an account and mint a token.
async fn login(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let creds: Credentials = match read_json_body(req, "expected {name, password}").await {
        Ok(creds) => creds,
        Err(response) => return response,
    };
    match store::verify_account(pool, &creds.name, &creds.password).await {
        Ok(Some((id, role))) => token_response(pool, id, role, SESSION_TTL_HOURS).await,
        Ok(None) => text(StatusCode::UNAUTHORIZED, "wrong name or password"),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "auth error"),
    }
}

/// Register a new account (open registration) and mint a token for it.
async fn register(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let creds: Credentials = match read_json_body(req, "expected {name, password}").await {
        Ok(creds) => creds,
        Err(response) => return response,
    };
    if creds.name.trim().is_empty() {
        return text(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    if creds.name.len() > MAX_NAME_LEN {
        return text(StatusCode::BAD_REQUEST, "name too long (max 32 characters)");
    }
    if creds.password.len() < MIN_PASSWORD_LEN {
        return text(
            StatusCode::BAD_REQUEST,
            "password too short (min 8 characters)",
        );
    }
    match store::create_account(pool, &creds.name, &creds.password).await {
        Ok((id, role)) => token_response(pool, id, role, SESSION_TTL_HOURS).await,
        Err(CreateError::NameTaken) => text(StatusCode::CONFLICT, "that name is taken"),
        Err(CreateError::Db(_)) => text(StatusCode::INTERNAL_SERVER_ERROR, "registration error"),
    }
}
