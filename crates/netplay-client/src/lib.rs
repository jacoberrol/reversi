//! Reusable client transport for the netplay layer (WebSocket).
//!
//! The winit event loop stays synchronous. Networking runs on a **dedicated
//! background thread** with its own single-threaded tokio runtime — the runtime
//! never touches the main loop. That thread owns the WebSocket, forwards decoded
//! [`NetEvent`]s to the event loop via an [`EventLoopProxy`], and drains an
//! outgoing channel written by [`NetHandle`] on the main thread.
//!
//! The in-game action is opaque here: [`NetHandle::game`] sends a `Vec<u8>` and
//! [`NetEvent::Game`] delivers one. The game defines and codes its own payload.

use futures_util::{SinkExt, StreamExt};
use netplay_protocol::{
    ClientMsg, PlayerId, PlayerInfo, Seat, ServerMsg, SharedTokenCredential, DEV_KEY_ID, DEV_TOKEN,
    PROTOCOL_VERSION,
};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use winit::event_loop::EventLoopProxy;

/// Supplies the opaque authorization credential sent in the handshake (arbitrary
/// JSON the server's authenticator interprets). A baked token lives behind this
/// now; a platform-attestation provider can replace it later without touching
/// [`connect`].
pub trait AuthProvider {
    fn credential(&self) -> serde_json::Value;
}

/// Reference provider: a versioned shared token.
pub struct SharedToken {
    pub key_id: u16,
    pub token: String,
}

impl SharedToken {
    /// The development default (matches the server's `SharedTokenAuth::dev`).
    pub fn dev() -> Self {
        Self {
            key_id: DEV_KEY_ID,
            token: DEV_TOKEN.to_string(),
        }
    }

    /// Parse an `"id:token"` spec (the same shape as one server entry). The
    /// token may itself contain `:`; only the first separates the key id.
    pub fn from_spec(spec: &str) -> Option<Self> {
        let (id, token) = spec.split_once(':')?;
        Some(Self {
            key_id: id.trim().parse().ok()?,
            token: token.to_string(),
        })
    }

    /// The token from the `NETPLAY_TOKEN` env var (`"id:token"`), or the dev
    /// default when it's unset or malformed. Keeps the real shared secret out
    /// of the binary — it's supplied at runtime, not baked in.
    pub fn from_env_or_dev() -> Self {
        std::env::var("NETPLAY_TOKEN")
            .ok()
            .and_then(|spec| Self::from_spec(&spec))
            .unwrap_or_else(Self::dev)
    }
}

impl AuthProvider for SharedToken {
    fn credential(&self) -> serde_json::Value {
        SharedTokenCredential {
            key_id: self.key_id,
            token: self.token.clone(),
        }
        .to_value()
    }
}

/// A message from the network, injected into the winit event loop as a user
/// event.
#[derive(Debug)]
pub enum NetEvent {
    /// The other players currently available in the lobby.
    Presence(Vec<PlayerInfo>),
    /// You received an invite.
    Invited { from: PlayerId, name: String },
    /// An invite you sent was declined.
    InviteDeclined { by: PlayerId },
    /// Paired with an opponent; you take `seat` (seat 0 moves first).
    Matched { seat: Seat, opponent: String },
    /// An opaque in-game payload from the opponent.
    Game(Vec<u8>),
    /// The opponent disconnected or resigned (server-side).
    OpponentLeft,
    /// The connection closed (or never opened).
    Disconnected,
    /// A protocol/connection error.
    Error(String),
}

/// Sends outgoing messages to the network thread. Held by the main thread.
pub struct NetHandle {
    tx: UnboundedSender<ClientMsg>,
}

impl NetHandle {
    /// Send an opaque in-game payload to the opponent (best effort; a broken
    /// connection surfaces as [`NetEvent::Disconnected`]).
    pub fn game(&mut self, payload: Vec<u8>) {
        let _ = self.tx.send(ClientMsg::Game { payload });
    }

    pub fn invite(&mut self, to: PlayerId) {
        let _ = self.tx.send(ClientMsg::Invite { to });
    }

    pub fn accept(&mut self, inviter: PlayerId) {
        let _ = self.tx.send(ClientMsg::Accept { inviter });
    }

    pub fn decline(&mut self, inviter: PlayerId) {
        let _ = self.tx.send(ClientMsg::Decline { inviter });
    }
}

/// Connect to a WebSocket `url` (`ws://…` or `wss://…`) and spawn the network
/// thread. `name` is the display name; `credential` is the opaque auth payload
/// the server interprets (for the account scheme, `{name, password, register?}`).
/// Returns the send handle immediately; connection results and incoming messages
/// arrive as [`NetEvent`]s on `proxy`.
pub fn connect(
    url: &str,
    name: &str,
    credential: serde_json::Value,
    proxy: EventLoopProxy<NetEvent>,
) -> NetHandle {
    let (tx, rx) = mpsc::unbounded_channel::<ClientMsg>();
    let hello = ClientMsg::Hello {
        name: name.to_string(),
        protocol: PROTOCOL_VERSION,
        credential,
    };
    let url = url.to_string();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                let _ = proxy.send_event(NetEvent::Error(format!("runtime error: {e}")));
                return;
            }
        };
        runtime.block_on(io_loop(url, hello, proxy, rx));
    });

    NetHandle { tx }
}

async fn io_loop(
    url: String,
    hello: ClientMsg,
    proxy: EventLoopProxy<NetEvent>,
    mut outgoing: UnboundedReceiver<ClientMsg>,
) {
    // rustls 0.23 needs a process-wide crypto provider chosen explicitly; ours
    // is `ring`. Installing is idempotent (Err if already set — harmless), and
    // must happen before tokio-tungstenite builds the wss:// ClientConfig.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let ws = match tokio_tungstenite::connect_async(url.as_str()).await {
        Ok((ws, _response)) => ws,
        Err(e) => {
            let _ = proxy.send_event(NetEvent::Error(format!("could not connect: {e}")));
            return;
        }
    };
    let (mut sink, mut source) = ws.split();

    if sink
        .send(Message::binary(netplay_protocol::to_bytes(&hello)))
        .await
        .is_err()
    {
        let _ = proxy.send_event(NetEvent::Disconnected);
        return;
    }

    loop {
        tokio::select! {
            incoming = source.next() => match incoming {
                Some(Ok(Message::Binary(bytes))) => {
                    if !forward(&proxy, &bytes) { break; }
                }
                Some(Ok(Message::Text(text))) => {
                    if !forward(&proxy, text.as_bytes()) { break; }
                }
                Some(Ok(Message::Close(_))) | None => {
                    let _ = proxy.send_event(NetEvent::Disconnected);
                    break;
                }
                // Ping/Pong/Frame — tungstenite handles keepalive itself.
                Some(Ok(_)) => {}
                Some(Err(_)) => {
                    let _ = proxy.send_event(NetEvent::Disconnected);
                    break;
                }
            },
            outgoing = outgoing.recv() => match outgoing {
                Some(msg) => {
                    if sink
                        .send(Message::binary(netplay_protocol::to_bytes(&msg)))
                        .await
                        .is_err()
                    {
                        let _ = proxy.send_event(NetEvent::Disconnected);
                        break;
                    }
                }
                // The handle was dropped (the app is closing): stop.
                None => break,
            },
        }
    }
}

/// Decode a server message and forward it. Returns `false` if the event loop has
/// exited (stop the network thread).
fn forward(proxy: &EventLoopProxy<NetEvent>, bytes: &[u8]) -> bool {
    let event = match netplay_protocol::decode::<ServerMsg>(bytes) {
        Ok(msg) => server_msg_to_event(msg),
        Err(_) => Some(NetEvent::Error("malformed message from server".to_string())),
    };
    match event {
        Some(event) => proxy.send_event(event).is_ok(),
        None => true, // nothing to surface; keep the network thread alive
    }
}

fn server_msg_to_event(msg: ServerMsg) -> Option<NetEvent> {
    Some(match msg {
        ServerMsg::Presence { players } => NetEvent::Presence(players),
        ServerMsg::Invited { from, name } => NetEvent::Invited { from, name },
        ServerMsg::InviteDeclined { by } => NetEvent::InviteDeclined { by },
        ServerMsg::Matched { seat, opponent } => NetEvent::Matched { seat, opponent },
        ServerMsg::Game { payload } => NetEvent::Game(payload),
        ServerMsg::OpponentLeft => NetEvent::OpponentLeft,
        ServerMsg::Error { message } => NetEvent::Error(message),
        // Admin/control messages are for admin tools, not the game client.
        ServerMsg::Players { .. }
        | ServerMsg::Matches { .. }
        | ServerMsg::Stats { .. }
        | ServerMsg::PlayerJoined { .. }
        | ServerMsg::PlayerLeft { .. }
        | ServerMsg::MatchStarted { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::SharedToken;

    #[test]
    fn from_spec_parses_id_and_token() {
        let t = SharedToken::from_spec("2:9f3c").expect("valid spec");
        assert_eq!(t.key_id, 2);
        assert_eq!(t.token, "9f3c");
    }

    #[test]
    fn from_spec_keeps_colons_in_the_token() {
        // Only the first colon separates the key id.
        let t = SharedToken::from_spec("7:ab:cd").expect("valid spec");
        assert_eq!(t.key_id, 7);
        assert_eq!(t.token, "ab:cd");
    }

    #[test]
    fn from_spec_rejects_malformed() {
        assert!(SharedToken::from_spec("no-colon").is_none());
        assert!(SharedToken::from_spec("notanumber:tok").is_none());
    }

    #[test]
    fn crypto_provider_lets_a_tls_config_build() {
        // Reproduces the wss:// handshake path that panicked when no crypto
        // provider was installed. Building a ClientConfig resolves the process
        // default provider — this must not panic.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let roots = rustls::RootCertStore::empty();
        let _config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
    }
}
