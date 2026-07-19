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
//!
//! Auth lives in [`auth`]: log in / register over REST for a bearer token, which
//! the WebSocket [`ClientMsg::Hello`] then presents. [`login_and_connect`] chains
//! the two on the network thread so the caller supplies name + password once.

pub mod auth;

use futures_util::{SinkExt, StreamExt};
use netplay_protocol::{ClientMsg, PlayerId, PlayerInfo, Seat, ServerMsg, PROTOCOL_VERSION};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use winit::event_loop::EventLoopProxy;

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

/// Connect to a WebSocket `url` (`ws://…` or `wss://…`) with an already-obtained
/// session `token` and spawn the network thread. Returns the send handle
/// immediately; connection results and incoming messages arrive as [`NetEvent`]s
/// on `proxy`. Most callers want [`login_and_connect`], which gets the token first.
pub fn connect(url: &str, token: String, proxy: EventLoopProxy<NetEvent>) -> NetHandle {
    let (tx, rx) = mpsc::unbounded_channel::<ClientMsg>();
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        token,
    };
    let url = url.to_string();
    std::thread::spawn(move || {
        if let Some(runtime) = build_runtime(&proxy) {
            runtime.block_on(io_loop(url, hello, proxy, rx));
        }
    });
    NetHandle { tx }
}

/// Log in (or register) over REST for a token, then [`connect`] with it — all on
/// the network thread. On auth failure a [`NetEvent::Error`] carries the server's
/// message and no socket is opened. `url` is the game host's WebSocket URL; the
/// REST origin is derived from it (`ws→http`, `wss→https`).
pub fn login_and_connect(
    url: &str,
    name: &str,
    password: &str,
    register: bool,
    proxy: EventLoopProxy<NetEvent>,
) -> NetHandle {
    let (tx, rx) = mpsc::unbounded_channel::<ClientMsg>();
    let url = url.to_string();
    let (name, password) = (name.to_string(), password.to_string());
    std::thread::spawn(move || {
        let base = http_origin(&url);
        let result = if register {
            auth::player_register(&base, &name, &password)
        } else {
            auth::player_login(&base, &name, &password)
        };
        let token = match result {
            Ok(token) => token.token,
            Err(e) => {
                let _ = proxy.send_event(NetEvent::Error(e.to_string()));
                return;
            }
        };
        let hello = ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            token,
        };
        if let Some(runtime) = build_runtime(&proxy) {
            runtime.block_on(io_loop(url, hello, proxy, rx));
        }
    });
    NetHandle { tx }
}

/// The HTTP origin for the REST auth endpoints, derived from the WebSocket URL:
/// `ws://host:port/…` → `http://host:port`, `wss://…` → `https://…` (path dropped,
/// since the auth endpoints live at the origin root).
fn http_origin(ws_url: &str) -> String {
    let (scheme, rest) = match ws_url.split_once("://") {
        Some(("wss", rest)) => ("https", rest),
        Some(("ws", rest)) => ("http", rest),
        // Already an http(s) URL (or unknown) — keep the scheme as given.
        Some((scheme, rest)) => (scheme, rest),
        None => ("http", ws_url),
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{authority}")
}

/// Build the network thread's single-threaded runtime, reporting failure as a
/// [`NetEvent::Error`] and returning `None`.
fn build_runtime(proxy: &EventLoopProxy<NetEvent>) -> Option<tokio::runtime::Runtime> {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => Some(runtime),
        Err(e) => {
            let _ = proxy.send_event(NetEvent::Error(format!("runtime error: {e}")));
            None
        }
    }
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
        Err(_) => NetEvent::Error("malformed message from server".to_string()),
    };
    proxy.send_event(event).is_ok()
}

fn server_msg_to_event(msg: ServerMsg) -> NetEvent {
    match msg {
        ServerMsg::Presence { players } => NetEvent::Presence(players),
        ServerMsg::Invited { from, name } => NetEvent::Invited { from, name },
        ServerMsg::InviteDeclined { by } => NetEvent::InviteDeclined { by },
        ServerMsg::Matched { seat, opponent } => NetEvent::Matched { seat, opponent },
        ServerMsg::Game { payload } => NetEvent::Game(payload),
        ServerMsg::OpponentLeft => NetEvent::OpponentLeft,
        ServerMsg::Error { message } => NetEvent::Error(message),
    }
}

#[cfg(test)]
mod tests {
    use super::http_origin;

    #[test]
    fn http_origin_maps_ws_schemes_and_drops_the_path() {
        assert_eq!(http_origin("ws://127.0.0.1:5000"), "http://127.0.0.1:5000");
        assert_eq!(
            http_origin("wss://relay.netplay.oliverj.network"),
            "https://relay.netplay.oliverj.network"
        );
        assert_eq!(http_origin("wss://host:8443/play"), "https://host:8443");
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
