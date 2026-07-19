//! Reusable relay/matchmaking server library (WebSocket transport).
//!
//! [`serve`] accepts WebSocket connections, authorizes and rate-limits them,
//! runs a lobby (presence + invites), and relays paired players' opaque game
//! payloads. TLS is handled by a front proxy at deploy time; this server speaks
//! plain `ws://`. Game-agnostic — it never decodes the payload.

pub mod auth;
pub mod limits;
pub mod lobby;

use std::sync::{Arc, Mutex};

use auth::Authenticator;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use limits::{IpGuard, IpLimiter};
use lobby::LobbyCmd;
use netplay_protocol::{ClientMsg, ServerMsg, MAX_MESSAGE, PROTOCOL_VERSION};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

type WsSink = SplitSink<WebSocketStream<TcpStream>, Message>;
type WsSource = SplitStream<WebSocketStream<TcpStream>>;

/// Accept and serve WebSocket connections on `listener`, authorizing each with
/// `auth`. Runs the lobby task internally; never returns under normal operation.
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
                    if let Err(e) = handle(stream, lobby_tx, auth).await {
                        eprintln!("connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Handle one client: WebSocket handshake, authorize, join the lobby, then relay.
async fn handle(
    stream: TcpStream,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<dyn Authenticator>,
) -> Result<(), BoxError> {
    // The WebSocket upgrade must complete within the handshake window.
    let ws = match tokio::time::timeout(
        limits::HANDSHAKE_TIMEOUT,
        tokio_tungstenite::accept_async(stream),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            eprintln!("rate-limit: websocket handshake timed out");
            return Ok(());
        }
    };

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
                .send(ServerMsg::Error("protocol version mismatch".to_string()))
                .await;
            return Ok(());
        }
        _ => return Ok(()),
    };

    // Authorize before the client can touch the lobby.
    match auth.verify(&credential) {
        Ok(identity) => println!("authorized (key {})", identity.key_id),
        Err(e) => {
            let _ = outbox.send(ServerMsg::Error(e.message().to_string())).await;
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
                .send(ServerMsg::Error("rate exceeded".to_string()))
                .await;
            eprintln!("rate-limit: message rate exceeded (player {id})");
            break;
        }
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
            ClientMsg::Game(payload) => LobbyCmd::Relay { from: id, payload },
            ClientMsg::Hello { .. } => continue, // ignore a stray second Hello
        };
        if lobby_tx.send(cmd).await.is_err() {
            break;
        }
    }

    let _ = lobby_tx.send(LobbyCmd::Leave { id }).await;
    Ok(())
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
