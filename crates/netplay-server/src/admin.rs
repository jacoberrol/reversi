//! The admin REST control plane, served on the admin host (`NETPLAY_ADMIN_HOST`).
//!
//! `POST /admin/login` exchanges an admin `{name, password}` for a short-lived
//! bearer token (persisted as a session); `POST /admin/tokens` trades a valid
//! bearer for a longer-lived one (so a tool authenticates once and then holds a
//! durable token). The read endpoints require `Authorization: Bearer <token>`.
//! The data comes from the same lobby actor the game relay uses. `GET
//! /admin/openapi.json` (unauthenticated) describes the whole surface.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{header, Method, Request, Response, StatusCode};
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};

use crate::lobby::LobbyCmd;
use crate::store::{self, Role};
use crate::{json_ok, query, text};

/// How long a session from `POST /admin/login` stays valid (one work session).
const SESSION_TTL_HOURS: i64 = 24;
/// Default lifetime of a durable token from `POST /admin/tokens` (weeks, so a
/// tool holds it across restarts without re-sending the password).
const DURABLE_TTL_DAYS: i64 = 30;
/// Upper bound a caller may request for a durable token.
const MAX_DURABLE_TTL_DAYS: i64 = 90;
/// Cap request bodies so a hostile request can't allocate unbounded memory.
const MAX_LOGIN_BODY: usize = 4096;

/// Route an admin-host request. Never a WebSocket upgrade — pure REST.
pub async fn route(
    req: Request<Incoming>,
    pool: SqlitePool,
    lobby_tx: mpsc::Sender<LobbyCmd>,
) -> Response<Full<Bytes>> {
    match (req.method(), req.uri().path()) {
        // Unauthenticated on purpose: a client discovers how to authenticate here.
        (&Method::GET, "/admin/openapi.json") => json_ok(crate::openapi::document().to_string()),
        (&Method::POST, "/admin/login") => login(req, &pool).await,
        (&Method::POST, "/admin/tokens") => issue_token(req, &pool).await,
        (&Method::GET, "/admin/players") => {
            guarded_query(&req, &pool, &lobby_tx, |reply| LobbyCmd::ListPlayers {
                reply,
            })
            .await
        }
        (&Method::GET, "/admin/matches") => {
            guarded_query(&req, &pool, &lobby_tx, |reply| LobbyCmd::ListMatches {
                reply,
            })
            .await
        }
        (&Method::GET, "/admin/stats") => {
            guarded_query(&req, &pool, &lobby_tx, |reply| LobbyCmd::Stats { reply }).await
        }
        _ => text(StatusCode::NOT_FOUND, "not found"),
    }
}

#[derive(Deserialize)]
struct LoginBody {
    name: String,
    password: String,
}

/// Verify an admin login and mint a session bearer token.
async fn login(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let Ok(collected) = Limited::new(req.into_body(), MAX_LOGIN_BODY)
        .collect()
        .await
    else {
        return text(StatusCode::BAD_REQUEST, "bad request body");
    };
    let Ok(body) = serde_json::from_slice::<LoginBody>(&collected.to_bytes()) else {
        return text(StatusCode::BAD_REQUEST, "expected {name, password}");
    };
    match store::verify_account(pool, &body.name, &body.password).await {
        Ok(Some((id, Role::Admin))) => {
            match store::create_session(pool, id, Role::Admin, SESSION_TTL_HOURS).await {
                Ok(token) => json_ok(
                    serde_json::json!({ "token": token, "expires_in_hours": SESSION_TTL_HOURS })
                        .to_string(),
                ),
                Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "session error"),
            }
        }
        Ok(Some((_, Role::Player))) => text(StatusCode::FORBIDDEN, "not an admin"),
        Ok(None) => text(StatusCode::UNAUTHORIZED, "wrong name or password"),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "auth error"),
    }
}

#[derive(Deserialize, Default)]
struct TokenBody {
    /// Requested lifetime in days; clamped to `[1, MAX_DURABLE_TTL_DAYS]`.
    /// Defaults to [`DURABLE_TTL_DAYS`] when absent.
    days: Option<i64>,
}

/// Trade a valid admin bearer for a longer-lived one. The new token is a fresh
/// session for the same account, so it carries the caller's identity and role —
/// a tool logs in once, then holds a durable token instead of the password.
async fn issue_token(req: Request<Incoming>, pool: &SqlitePool) -> Response<Full<Bytes>> {
    let Some((user_id, role)) = admin_identity(&req, pool).await else {
        return text(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    };
    // The body is optional; an absent or empty one takes the default lifetime.
    let Ok(collected) = Limited::new(req.into_body(), MAX_LOGIN_BODY)
        .collect()
        .await
    else {
        return text(StatusCode::BAD_REQUEST, "bad request body");
    };
    let bytes = collected.to_bytes();
    let body = if bytes.is_empty() {
        TokenBody::default()
    } else {
        match serde_json::from_slice::<TokenBody>(&bytes) {
            Ok(body) => body,
            Err(_) => return text(StatusCode::BAD_REQUEST, "expected {days}"),
        }
    };
    let days = body
        .days
        .unwrap_or(DURABLE_TTL_DAYS)
        .clamp(1, MAX_DURABLE_TTL_DAYS);
    let ttl_hours = days * 24;
    match store::create_session(pool, user_id, role, ttl_hours).await {
        Ok(token) => json_ok(
            serde_json::json!({ "token": token, "expires_in_hours": ttl_hours }).to_string(),
        ),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "session error"),
    }
}

/// Require an admin bearer, then answer a lobby query as JSON.
async fn guarded_query<T: serde::Serialize>(
    req: &Request<Incoming>,
    pool: &SqlitePool,
    lobby_tx: &mpsc::Sender<LobbyCmd>,
    make: impl FnOnce(oneshot::Sender<T>) -> LobbyCmd,
) -> Response<Full<Bytes>> {
    if admin_identity(req, pool).await.is_none() {
        return text(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    }
    match query(lobby_tx, make).await {
        Some(value) => json_ok(serde_json::to_string(&value).expect("serializes")),
        None => text(StatusCode::INTERNAL_SERVER_ERROR, "lobby unavailable"),
    }
}

/// The `(user_id, role)` of a valid **admin** bearer token, or `None` if the
/// token is missing, invalid, expired, or belongs to a non-admin.
async fn admin_identity(req: &Request<Incoming>, pool: &SqlitePool) -> Option<(i64, Role)> {
    let token = bearer_token(req)?;
    match store::session_identity(pool, &token).await {
        Ok(Some((id, Role::Admin))) => Some((id, Role::Admin)),
        _ => None,
    }
}

fn bearer_token(req: &Request<Incoming>) -> Option<String> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}
