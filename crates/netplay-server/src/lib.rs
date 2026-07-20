//! Reusable relay/matchmaking server library.
//!
//! [`serve`] runs a minimal HTTP/1 front per connection. Requests to the admin
//! host (`NETPLAY_ADMIN_HOST`) go to the REST control plane ([`admin`]);
//! everything else is the game — a WebSocket upgrade hands the socket to the
//! relay, which authorizes, rate-limits, runs the lobby (presence + invites),
//! and forwards paired players' opaque game payloads. TLS is a front-proxy
//! concern; this server speaks plain `ws://`/`http://`. Game-agnostic.

pub mod admin;
pub mod auth;
pub mod limits;
pub mod lobby;
pub mod openapi;
pub mod player;
mod rest;
pub mod store;

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use auth::DbAuth;
use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use limits::{IpGuard, IpLimiter};
use lobby::LobbyCmd;
use netplay_protocol::{ClientMsg, ServerMsg, MAX_MESSAGE, PROTOCOL_VERSION};
use sqlx::SqlitePool;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The WebSocket stream after hyper upgrades the connection.
type WsStream = WebSocketStream<TokioIo<Upgraded>>;
type WsSink = SplitSink<WsStream, Message>;
type WsSource = SplitStream<WsStream>;
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Accept connections on `listener`. Requests to `admin_host` are the REST admin
/// API; everything else is the game (WebSocket). The `pool` backs the accounts
/// and admin sessions. Runs the lobby task internally; never returns normally.
pub async fn serve(listener: TcpListener, pool: SqlitePool, admin_host: String) {
    let admin_host: Arc<str> = Arc::from(admin_host.to_ascii_lowercase());
    let auth = Arc::new(DbAuth::new(pool.clone()));
    let (lobby_tx, lobby_rx) = mpsc::channel(64);
    tokio::spawn(lobby::run(lobby_rx));
    let limiter = Arc::new(Mutex::new(IpLimiter::new()));

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let ip = peer.ip();
                // Per-IP concurrency + new-connection rate, checked at accept.
                if !limiter.lock().expect("limiter lock").admit(ip) {
                    eprintln!("rate-limit: rejected connection from {ip}");
                    continue;
                }
                println!("connection from {peer}");
                let guard = IpGuard::new(limiter.clone(), ip);
                let conn = Conn {
                    pool: pool.clone(),
                    lobby_tx: lobby_tx.clone(),
                    auth: auth.clone(),
                    admin_host: admin_host.clone(),
                };
                tokio::spawn(async move {
                    let _guard = guard; // releases the IP slot when the task ends
                    serve_connection(stream, conn).await;
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

/// Shared per-connection context (cheap to clone).
#[derive(Clone)]
struct Conn {
    pool: SqlitePool,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<DbAuth>,
    admin_host: Arc<str>,
}

/// Run the HTTP/1 front over one accepted TCP connection.
async fn serve_connection(stream: TcpStream, conn: Conn) {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req| {
        let conn = conn.clone();
        async move { route(req, conn).await }
    });

    let mut builder = http1::Builder::new();
    // Bound how long a client may dawdle sending request headers (slow-loris).
    // header_read_timeout requires a registered timer.
    builder.timer(TokioTimer::new());
    builder.header_read_timeout(limits::HANDSHAKE_TIMEOUT);
    if let Err(e) = builder.serve_connection(io, service).with_upgrades().await {
        // A malformed request (scanner / bad probe) is expected on a public port.
        eprintln!("http connection ended: {e}");
    }
}

/// Route one request: admin REST (admin host), a WebSocket upgrade, or 404.
async fn route(req: Request<Incoming>, conn: Conn) -> Result<Response<Full<Bytes>>, Infallible> {
    // The admin host serves the REST control plane.
    if request_host(&req).as_deref() == Some(conn.admin_host.as_ref()) {
        return Ok(admin::route(req, conn.pool, conn.lobby_tx).await);
    }

    // The game host serves player auth over REST (login/register → token)...
    let mut req = match player::route(req, &conn.pool).await {
        Ok(response) => return Ok(response),
        Err(req) => req,
    };

    // ...and gameplay over WebSocket: an upgrade → hand the socket to the relay.
    if hyper_tungstenite::is_upgrade_request(&req) {
        match hyper_tungstenite::upgrade(&mut req, None) {
            Ok((response, websocket)) => {
                let lobby_tx = conn.lobby_tx;
                let auth = conn.auth;
                tokio::spawn(async move {
                    match websocket.await {
                        Ok(ws) => {
                            if let Err(e) = relay(ws, lobby_tx, auth).await {
                                eprintln!("connection error: {e}");
                            }
                        }
                        Err(e) => eprintln!("websocket upgrade failed: {e}"),
                    }
                });
                return Ok(response);
            }
            Err(e) => {
                eprintln!("bad websocket upgrade: {e}");
                return Ok(text(StatusCode::BAD_REQUEST, "bad upgrade"));
            }
        }
    }

    // Not the admin host, not a WebSocket — a health check, scanner, or probe.
    Ok(text(StatusCode::NOT_FOUND, "not found"))
}

/// The requested hostname (lowercased, no port), preferring the proxy's
/// `X-Forwarded-Host` over `Host`.
fn request_host(req: &Request<Incoming>) -> Option<String> {
    let raw = req
        .headers()
        .get("x-forwarded-host")
        .or_else(|| req.headers().get(hyper::header::HOST))?
        .to_str()
        .ok()?;
    let host = raw.split(':').next().unwrap_or(raw);
    Some(host.to_ascii_lowercase())
}

/// Relay one authorized client: read Hello, authorize, join the lobby, forward.
async fn relay(
    ws: WsStream,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<DbAuth>,
) -> Result<(), BoxError> {
    let (sink, mut source) = ws.split();
    let (outbox, out_rx) = mpsc::channel::<ServerMsg>(32);
    tokio::spawn(writer(sink, out_rx));

    // The first message must be a version-matching Hello, within the window.
    let first =
        match tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, next_client(&mut source)).await {
            Ok(msg) => msg,
            Err(_) => {
                eprintln!("rate-limit: hello timed out");
                return Ok(());
            }
        };
    let token = match first {
        Some(ClientMsg::Hello { protocol, token }) if protocol == PROTOCOL_VERSION => token,
        Some(ClientMsg::Hello { .. }) => {
            let _ = outbox
                .send(ServerMsg::Error {
                    message: "protocol version mismatch".to_string(),
                })
                .await;
            return Ok(());
        }
        _ => return Ok(()),
    };

    // The token (from REST login/register) must name a live session; the account's
    // display name comes from it, never from the client.
    let name = match auth.verify(&token).await {
        Some(identity) => {
            println!("authorized (account {})", identity.name);
            identity.name
        }
        None => {
            let _ = outbox
                .send(ServerMsg::Error {
                    message: auth::UNAUTHORIZED_MESSAGE.to_string(),
                })
                .await;
            return Ok(());
        }
    };

    let (reply_tx, reply_rx) = oneshot::channel();
    if lobby_tx
        .send(LobbyCmd::Join {
            name,
            outbox: outbox.clone(),
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return Ok(());
    }
    let id = match reply_rx.await {
        Ok(Some(id)) => id,
        Ok(None) | Err(_) => return Ok(()),
    };

    // Relay the client's messages until it disconnects, metering the inbound rate.
    let mut inbound = limits::message_bucket();
    while let Some(msg) = next_client(&mut source).await {
        if !inbound.try_take() {
            let _ = outbox
                .send(ServerMsg::Error {
                    message: "rate exceeded".to_string(),
                })
                .await;
            eprintln!("rate-limit: message rate exceeded (player {id})");
            break;
        }
        // Gameplay commands are fire-and-forget; a closed channel ends the loop.
        let cmd = match msg {
            ClientMsg::Invite { to } => LobbyCmd::Invite { from: id, to },
            ClientMsg::Accept { inviter } => LobbyCmd::Accept {
                accepter: id,
                inviter,
            },
            ClientMsg::Decline { inviter } => LobbyCmd::Decline {
                decliner: id,
                inviter,
            },
            ClientMsg::Game { payload } => LobbyCmd::Relay { from: id, payload },
            ClientMsg::Hello { .. } => continue, // ignore a stray second Hello
        };
        if lobby_tx.send(cmd).await.is_err() {
            break;
        }
    }

    let _ = lobby_tx.send(LobbyCmd::Leave { id }).await;
    Ok(())
}

/// Send the lobby a query built with `make` and await its oneshot reply.
/// `None` if the lobby channel closed or the reply was dropped.
pub(crate) async fn query<T>(
    lobby_tx: &mpsc::Sender<LobbyCmd>,
    make: impl FnOnce(oneshot::Sender<T>) -> LobbyCmd,
) -> Option<T> {
    let (reply_tx, reply_rx) = oneshot::channel();
    lobby_tx.send(make(reply_tx)).await.ok()?;
    reply_rx.await.ok()
}

/// Drain outgoing messages to the socket as WebSocket binary messages.
async fn writer(mut sink: WsSink, mut rx: mpsc::Receiver<ServerMsg>) {
    while let Some(msg) = rx.recv().await {
        if sink
            .send(Message::binary(netplay_protocol::to_bytes(&msg)))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = sink.close().await;
}

/// Read the next [`ClientMsg`], or `None` on close / error / malformed input.
async fn next_client(source: &mut WsSource) -> Option<ClientMsg> {
    loop {
        match source.next().await? {
            Ok(Message::Binary(bytes)) => {
                if bytes.len() > MAX_MESSAGE {
                    return None;
                }
                return netplay_protocol::decode::<ClientMsg>(&bytes).ok();
            }
            Ok(Message::Text(text)) => {
                if text.len() > MAX_MESSAGE {
                    return None;
                }
                return netplay_protocol::decode::<ClientMsg>(text.as_bytes()).ok();
            }
            Ok(Message::Close(_)) => return None,
            // Ping/Pong/Frame — tungstenite handles keepalive itself.
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// A `200 OK` JSON response.
pub(crate) fn json_ok(body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .expect("response builds")
}

/// A plain-text response with `status`.
pub(crate) fn text(status: StatusCode, msg: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from_static(msg.as_bytes())))
        .expect("response builds")
}
