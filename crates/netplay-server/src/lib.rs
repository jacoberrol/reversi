//! Reusable relay/matchmaking server library.
//!
//! [`serve`] accepts client connections, runs a lobby (presence + invites), and
//! relays paired players' opaque game payloads. Each connection is a tokio task;
//! a per-player writer task drains an outbox channel to the socket, and the
//! [`lobby`] task owns all matchmaking state. Game-agnostic — it never decodes
//! the payload. Splitting this from the binary lets integration tests drive a
//! real server on an ephemeral port.

pub mod auth;
pub mod limits;
pub mod lobby;

use std::io;
use std::sync::{Arc, Mutex};

use auth::Authenticator;
use limits::{IpGuard, IpLimiter};
use lobby::LobbyCmd;
use netplay_protocol::{ClientMsg, ServerMsg, MAX_FRAME, PROTOCOL_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

/// Accept and serve connections on `listener`, authorizing each with `auth`,
/// until it errors fatally. Runs the lobby task internally; never returns under
/// normal operation.
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

/// Handle one client: handshake, authorize, join the lobby, then relay.
async fn handle(
    stream: TcpStream,
    lobby_tx: mpsc::Sender<LobbyCmd>,
    auth: Arc<dyn Authenticator>,
) -> io::Result<()> {
    let (mut read_half, write_half) = stream.into_split();
    let (outbox, out_rx) = mpsc::channel::<ServerMsg>(32);
    tokio::spawn(writer(write_half, out_rx));

    // The first frame must be a version-matching Hello, within the handshake
    // window (guards against idle/slowloris sockets).
    let first =
        match tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, read_client(&mut read_half)).await {
            Ok(result) => result?,
            Err(_) => {
                eprintln!("rate-limit: handshake timed out");
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
        _ => return Ok(()), // no Hello, or the peer closed
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
        // Rejected (lobby full) or the lobby is gone.
        Ok(None) | Err(_) => return Ok(()),
    };

    // Forward the client's lobby/game messages until it disconnects, metering
    // the inbound rate.
    let mut inbound = limits::message_bucket();
    while let Some(msg) = read_client(&mut read_half).await? {
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

/// Drain outgoing messages to the socket as length-delimited frames.
async fn writer(mut write_half: OwnedWriteHalf, mut rx: mpsc::Receiver<ServerMsg>) {
    while let Some(msg) = rx.recv().await {
        if write_half
            .write_all(&netplay_protocol::encode(&msg))
            .await
            .is_err()
        {
            break;
        }
    }
}

/// Read one framed [`ClientMsg`], or `None` on a clean disconnect.
async fn read_client(read_half: &mut OwnedReadHalf) -> io::Result<Option<ClientMsg>> {
    let mut len_buf = [0u8; 4];
    match read_half.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    read_half.read_exact(&mut body).await?;
    netplay_protocol::decode::<ClientMsg>(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
