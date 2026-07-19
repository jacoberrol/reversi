//! Reusable relay/matchmaking server library (WebSocket transport).
//!
//! [`serve`] runs a minimal HTTP/1 front on each connection: `GET /schema`
//! returns the self-describing service descriptor, and a WebSocket upgrade on
//! `/` hands the socket to the relay — which authorizes, rate-limits, runs the
//! lobby (presence + invites), and forwards paired players' opaque game
//! payloads. TLS is handled by a front proxy at deploy time; this server speaks
//! plain `ws://`/`http://`. Game-agnostic — it never decodes the payload.

pub mod auth;
pub mod limits;
pub mod lobby;

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use auth::Authenticator;
use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use limits::{IpGuard, IpLimiter};
use lobby::LobbyCmd;
use netplay_protocol::{ClientMsg, ServerMsg, MAX_MESSAGE, PROTOCOL_VERSION};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The WebSocket stream after hyper upgrades the connection.
type WsStream = WebSocketStream<TokioIo<Upgraded>>;
type WsSink = SplitSink<WsStream, Message>;
type WsSource = SplitStream<WsStream>;
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Accept connections on `listener`, serving each with an HTTP/1 front (schema +
/// WebSocket upgrade) authorized by `auth`. Runs the lobby task internally;
/// never returns under normal operation.
pub async fn serve(listener: TcpListener, auth: Arc<dyn Authenticator>) {
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
                let lobby_tx = lobby_tx.clone();
                let auth = auth.clone();
                tokio::spawn(async move {
                    let _guard = guard; // releases the IP slot when the task ends
                    serve_connection(stream, lobby_tx, auth).await;
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

/// Run the HTTP/1 front over one accepted TCP connection.
async fn serve_connection(
    stream: TcpStream,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<dyn Authenticator>,
) {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req| {
        let lobby_tx = lobby_tx.clone();
        let auth = auth.clone();
        async move { route(req, lobby_tx, auth).await }
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

/// Route one HTTP request: the schema document, a WebSocket upgrade, or 404.
async fn route(
    mut req: Request<Incoming>,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<dyn Authenticator>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Self-describing wire contract for non-Rust clients.
    if req.method() == Method::GET && req.uri().path() == "/schema" {
        let body = netplay_protocol::service_descriptor().to_string();
        return Ok(json_ok(body));
    }
    // The same contract as a standard AsyncAPI 3.0 document.
    if req.method() == Method::GET && req.uri().path() == "/asyncapi.json" {
        let body = netplay_protocol::asyncapi_document().to_string();
        return Ok(json_ok(body));
    }

    // A WebSocket upgrade → hand the socket to the relay.
    if hyper_tungstenite::is_upgrade_request(&req) {
        match hyper_tungstenite::upgrade(&mut req, None) {
            Ok((response, websocket)) => {
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

    // Not the schema, not a WebSocket — a health check, scanner, or plain probe.
    Ok(text(StatusCode::NOT_FOUND, "not found"))
}

/// Relay one authorized client: read Hello, authorize, join the lobby, forward.
async fn relay(
    ws: WsStream,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<dyn Authenticator>,
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
    let (name, credential) = match first {
        Some(ClientMsg::Hello {
            name,
            protocol,
            credential,
        }) if protocol == PROTOCOL_VERSION => (name, credential),
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

    // Authorize before the client can touch the lobby.
    match auth.verify(&credential) {
        Ok(identity) => println!("authorized (key {})", identity.key_id),
        Err(e) => {
            let _ = outbox
                .send(ServerMsg::Error {
                    message: e.message().to_string(),
                })
                .await;
            return Ok(());
        }
    }

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
        // Play commands are fire-and-forget; admin queries need a reply routed
        // back to this client. A closed lobby or client channel ends the loop.
        match msg {
            ClientMsg::Invite { to } => {
                if lobby_tx
                    .send(LobbyCmd::Invite { from: id, to })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            ClientMsg::Accept { inviter } => {
                let cmd = LobbyCmd::Accept {
                    accepter: id,
                    inviter,
                };
                if lobby_tx.send(cmd).await.is_err() {
                    break;
                }
            }
            ClientMsg::Decline { inviter } => {
                let cmd = LobbyCmd::Decline {
                    decliner: id,
                    inviter,
                };
                if lobby_tx.send(cmd).await.is_err() {
                    break;
                }
            }
            ClientMsg::Game { payload } => {
                if lobby_tx
                    .send(LobbyCmd::Relay { from: id, payload })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            ClientMsg::ListPlayers => {
                let Some(players) = query(&lobby_tx, |reply| LobbyCmd::ListPlayers { reply }).await
                else {
                    break;
                };
                if outbox.send(ServerMsg::Players { players }).await.is_err() {
                    break;
                }
            }
            ClientMsg::ListMatches => {
                let Some(matches) = query(&lobby_tx, |reply| LobbyCmd::ListMatches { reply }).await
                else {
                    break;
                };
                if outbox.send(ServerMsg::Matches { matches }).await.is_err() {
                    break;
                }
            }
            ClientMsg::GetStats => {
                let Some(stats) = query(&lobby_tx, |reply| LobbyCmd::Stats { reply }).await else {
                    break;
                };
                if outbox.send(ServerMsg::Stats { stats }).await.is_err() {
                    break;
                }
            }
            ClientMsg::SubscribeEvents => {
                if lobby_tx.send(LobbyCmd::Subscribe { id }).await.is_err() {
                    break;
                }
            }
            ClientMsg::Hello { .. } => continue, // ignore a stray second Hello
        }
    }

    let _ = lobby_tx.send(LobbyCmd::Leave { id }).await;
    Ok(())
}

/// Send the lobby a query built with `make` and await its oneshot reply.
/// `None` if the lobby channel closed or the reply was dropped.
async fn query<T>(
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
fn json_ok(body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .expect("response builds")
}

/// A plain-text response with `status`.
fn text(status: StatusCode, msg: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from_static(msg.as_bytes())))
        .expect("response builds")
}
