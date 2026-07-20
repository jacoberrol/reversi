//! Plumbing shared by the REST auth handlers (`admin` and `player`): request-body
//! reading, the `{name, password}` credential shape, and token-response minting.
//! One home for these so the two surfaces can't drift apart.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::store::{self, Role};
use crate::{json_ok, text};

/// How long a login session token stays valid (player and admin alike; the
/// admin's durable tokens from `POST /admin/tokens` choose their own TTL).
pub(crate) const SESSION_TTL_HOURS: i64 = 24;
/// Cap request bodies so a hostile request can't allocate unbounded memory.
pub(crate) const MAX_BODY: usize = 4096;

/// A login/registration request body.
#[derive(Deserialize)]
pub(crate) struct Credentials {
    pub name: String,
    pub password: String,
}

/// Read a JSON body (capped at [`MAX_BODY`]), or the error response to return.
/// `expected` names the shape in the 400 message (e.g. `"{name, password}"`).
pub(crate) async fn read_json_body<T: DeserializeOwned>(
    req: Request<Incoming>,
    expected: &'static str,
) -> Result<T, Response<Full<Bytes>>> {
    let bytes = read_body(req).await?;
    serde_json::from_slice::<T>(&bytes).map_err(|_| text(StatusCode::BAD_REQUEST, expected))
}

/// Like [`read_json_body`], but an empty body yields `T::default()` — for
/// endpoints whose body is optional (serde treats an empty slice as an error).
pub(crate) async fn read_json_body_or_default<T: DeserializeOwned + Default>(
    req: Request<Incoming>,
    expected: &'static str,
) -> Result<T, Response<Full<Bytes>>> {
    let bytes = read_body(req).await?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice::<T>(&bytes).map_err(|_| text(StatusCode::BAD_REQUEST, expected))
}

/// Collect a request body, capped at [`MAX_BODY`].
async fn read_body(req: Request<Incoming>) -> Result<Bytes, Response<Full<Bytes>>> {
    match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(_) => Err(text(StatusCode::BAD_REQUEST, "bad request body")),
    }
}

/// Mint a session for `user_id` and answer `200 {token, expires_in_hours}`.
pub(crate) async fn token_response(
    pool: &SqlitePool,
    user_id: i64,
    role: Role,
    ttl_hours: i64,
) -> Response<Full<Bytes>> {
    match store::create_session(pool, user_id, role, ttl_hours).await {
        Ok(token) => json_ok(
            serde_json::json!({ "token": token, "expires_in_hours": ttl_hours }).to_string(),
        ),
        Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "session error"),
    }
}
