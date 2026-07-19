//! Player REST auth, served on the game host (everything that isn't the admin
//! host). `POST /login` and `POST /register` exchange `{name, password}` for a
//! bearer token; the client then presents that token in the WebSocket `Hello`
//! (see [`crate::auth`]). This moves the expensive argon2 check off the socket
//! path — it happens once, here, instead of on every connect.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::auth::MIN_PASSWORD_LEN;
use crate::store::{self, CreateError, Role};
use crate::{json_ok, text};

/// How long a player session token stays valid.
const SESSION_TTL_HOURS: i64 = 24;
/// Cap the request body so a hostile request can't allocate unbounded memory.
const MAX_BODY: usize = 4096;

/// Route a game-host auth request. Returns `Ok(response)` when it handled one, or
/// `Err(req)` to hand the request back so the caller can try the WebSocket upgrade
/// (the body is untouched unless a route matched).
pub async fn route(
    req: Request<Incoming>,
    pool: &SqlitePool,
) -> Result<Response<Full<Bytes>>, Request<Incoming>> {
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/login") => Ok(login(req, pool).await),
        (&Method::POST, "/register") => Ok(register(req, pool).await),
        _ => Err(req),
    }
}

#[derive(Deserialize)]
struct Credentials {
    name: String,
    password: String,
}

/// Read and parse a `{name, password}` body, or an error response to return.
async fn credentials(req: Request<Incoming>) -> Result<Credentials, Response<Full<Bytes>>> {
    let Ok(collected) = Limited::new(req.into_body(), MAX_BODY).collect().await else {
        return Err(text(StatusCode::BAD_REQUEST, "bad request body"));
    };
    serde_json::from_slice::<Credentials>(&collected.to_bytes())
        .map_err(|_| text(StatusCode::BAD_REQUEST, "expected {name, password}"))
}

/// A `200` response carrying a freshly minted session token.
async fn issue(pool: &SqlitePool, id: i64, role: Role) -> Response<Full<Bytes>> {
    match store::create_session(pool, id, role, SESSION_TTL_HOURS).await {
        Ok(token) => json_ok(
            serde_json::json!({ "token": token, "expires_in_hours": SESSION_TTL_HOURS })
                .to_string(),
        ),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "session error"),
    }
}

/// Verify an account and mint a token.
async fn login(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let creds = match credentials(req).await {
        Ok(creds) => creds,
        Err(response) => return response,
    };
    match store::verify_account(pool, &creds.name, &creds.password).await {
        Ok(Some((id, role))) => issue(pool, id, role).await,
        Ok(None) => text(StatusCode::UNAUTHORIZED, "wrong name or password"),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "auth error"),
    }
}

/// Register a new account (open registration) and mint a token for it.
async fn register(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let creds = match credentials(req).await {
        Ok(creds) => creds,
        Err(response) => return response,
    };
    if creds.name.trim().is_empty() {
        return text(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    if creds.password.len() < MIN_PASSWORD_LEN {
        return text(
            StatusCode::BAD_REQUEST,
            "password too short (min 8 characters)",
        );
    }
    match store::create_account(pool, &creds.name, &creds.password).await {
        Ok((id, role)) => issue(pool, id, role).await,
        Err(CreateError::NameTaken) => text(StatusCode::CONFLICT, "that name is taken"),
        Err(CreateError::Db(_)) => text(StatusCode::INTERNAL_SERVER_ERROR, "registration error"),
    }
}
