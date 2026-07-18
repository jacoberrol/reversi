//! Reversi relay server library.
//!
//! [`serve`] accepts client connections, auto-pairs the first two waiting
//! players, and relays their in-game messages. Each connection is a tokio task;
//! a per-player writer task drains an outbox channel to the socket, and the
//! [`lobby`] task owns all matchmaking state. Splitting this from the binary
//! lets integration tests drive a real server on an ephemeral port.

pub mod lobby;

use std::io;

use lobby::LobbyCmd;
use protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

/// Reject frames larger than this (messages are tiny).
const MAX_FRAME: usize = 1 << 16;

/// Accept and serve connections on `listener` until it errors fatally. Runs the
/// lobby task internally; never returns under normal operation.
pub async fn serve(listener: TcpListener) {
    let (lobby_tx, lobby_rx) = mpsc::channel(64);
    tokio::spawn(lobby::run(lobby_rx));

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                println!("connection from {peer}");
                let lobby_tx = lobby_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, lobby_tx).await {
                        eprintln!("connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

/// Handle one client: handshake, join the lobby, then relay its game messages.
async fn handle(stream: TcpStream, lobby_tx: mpsc::Sender<LobbyCmd>) -> io::Result<()> {
    let (mut read_half, write_half) = stream.into_split();
    let (outbox, out_rx) = mpsc::channel::<ServerMsg>(32);
    tokio::spawn(writer(write_half, out_rx));

    // The first frame must be a version-matching Hello.
    let name = match read_client(&mut read_half).await? {
        Some(ClientMsg::Hello { name, protocol }) if protocol == PROTOCOL_VERSION => name,
        Some(ClientMsg::Hello { .. }) => {
            let _ = outbox
                .send(ServerMsg::Error("protocol version mismatch".to_string()))
                .await;
            return Ok(());
        }
        _ => return Ok(()), // no Hello, or the peer closed
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
        Ok(id) => id,
        Err(_) => return Ok(()),
    };

    // Forward the client's lobby/game messages until it disconnects.
    while let Some(msg) = read_client(&mut read_half).await? {
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
            ClientMsg::Game(game) => LobbyCmd::Relay {
                from: id,
                msg: game,
            },
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
        if write_half.write_all(&protocol::encode(&msg)).await.is_err() {
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
    protocol::decode::<ClientMsg>(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
