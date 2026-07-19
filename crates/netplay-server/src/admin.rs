//! The admin REST control plane, served on the admin host (`NETPLAY_ADMIN_HOST`).
//!
//! `POST /admin/login` exchanges an admin `{name, password}` for a bearer token
//! (persisted as a session); the read endpoints require `Authorization: Bearer
//! <token>`. The data comes from the same lobby actor the game relay uses.

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

/// How long an admin session stays valid.
const SESSION_TTL_HOURS: i64 = 24;
/// Cap the login body so a hostile request can't allocate unbounded memory.
const MAX_LOGIN_BODY: usize = 4096;

/// Route an admin-host request. Never a WebSocket upgrade — pure REST.
pub async fn route(
    req: Request<Incoming>,
    pool: SqlitePool,
    lobby_tx: mpsc::Sender<LobbyCmd>,
) -> Response<Full<Bytes>> {
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/admin/login") => login(req, &pool).await,
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

/// Require an admin bearer, then answer a lobby query as JSON.
async fn guarded_query<T: serde::Serialize>(
    req: &Request<Incoming>,
    pool: &SqlitePool,
    lobby_tx: &mpsc::Sender<LobbyCmd>,
    make: impl FnOnce(oneshot::Sender<T>) -> LobbyCmd,
) -> Response<Full<Bytes>> {
    if !is_admin(req, pool).await {
        return text(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    }
    match query(lobby_tx, make).await {
        Some(value) => json_ok(serde_json::to_string(&value).expect("serializes")),
        None => text(StatusCode::INTERNAL_SERVER_ERROR, "lobby unavailable"),
    }
}

/// Whether the request carries a valid admin bearer token.
async fn is_admin(req: &Request<Incoming>, pool: &SqlitePool) -> bool {
    let Some(token) = bearer_token(req) else {
        return false;
    };
    matches!(
        store::session_role(pool, &token).await,
        Ok(Some(Role::Admin))
    )
}

fn bearer_token(req: &Request<Incoming>) -> Option<String> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}
