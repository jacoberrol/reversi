//! Game-agnostic wire protocol for the netplay layer.
//!
//! The lobby/match envelope ([`ClientMsg`]/[`ServerMsg`]) is generic; the actual
//! in-game action rides as an **opaque payload** (`Game(Vec<u8>)`) that the
//! server never decodes. The game defines and codes its own message type and
//! puts the bytes in that payload. Players are identified by an abstract
//! [`Seat`] (seat 0 moves first); the game maps seat to its own player type.
//!
//! Each message is one JSON document; the transport (WebSocket) delimits them,
//! so there's no explicit framing here — just [`to_bytes`]/[`decode`]. Swappable
//! for a binary codec later without touching call sites.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to the message types. The server rejects a
/// [`ClientMsg::Hello`] whose `protocol` doesn't match.
pub const PROTOCOL_VERSION: u16 = 1;

/// Reject WebSocket messages larger than this (messages are tiny; this guards
/// against a hostile client sending a huge frame).
pub const MAX_MESSAGE: usize = 1 << 16;

/// A connected player, for the lifetime of its connection.
pub type PlayerId = u64;

/// A player as advertised in the lobby presence list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
}

/// An abstract seat in a match; seat 0 moves first. The game maps this to its
/// own player type (e.g. Reversi: seat 0 = Black).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat(pub u8);

/// The reference authorization scheme's credential: a versioned shared token.
/// `key_id` selects which key the server checks against, so keys can rotate
/// (old + new coexist during rollout). This is the credential *format* both the
/// client provider and the server's authenticator agree on; the relay itself
/// treats the credential as opaque bytes (see [`ClientMsg::Hello`]).
///
/// Threat model: a client cannot keep a secret — this deters anonymous clients,
/// it is not tamper-proofing. Real closure comes from attestation later, behind
/// the same seam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedTokenCredential {
    pub key_id: u16,
    pub token: String,
}

impl SharedTokenCredential {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("credential always serializes")
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Development key id / token. A convenience default so `just serve`/`just play`
/// work out of the box; **override in production** (the server injects real keys,
/// the app ships its own token).
pub const DEV_KEY_ID: u16 = 1;
pub const DEV_TOKEN: &str = "reversi-dev-token";

/// A message from a client to the server.
///
/// Internally tagged: each message serializes as a flat JSON object with a
/// `"type"` discriminator (e.g. `{"type":"Invite","to":3}`), so non-Rust
/// clients can model it as a conventional tagged union.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// First message on connect: the player's display name, protocol version,
    /// and an **opaque authorization credential** the server's authenticator
    /// interprets (the relay never decodes it — same discipline as `Game`).
    Hello {
        name: String,
        protocol: u16,
        credential: Vec<u8>,
    },
    /// Invite another player (by id) to a game.
    Invite { to: PlayerId },
    /// Accept an invite from `inviter`.
    Accept { inviter: PlayerId },
    /// Decline an invite from `inviter`.
    Decline { inviter: PlayerId },
    /// An opaque in-game payload, to be relayed to the opponent verbatim.
    Game { payload: Vec<u8> },
}

/// A message from the server to a client.
///
/// Internally tagged with a `"type"` discriminator, like [`ClientMsg`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// The other players currently available in the lobby.
    Presence { players: Vec<PlayerInfo> },
    /// You received an invite from player `from` (named `name`).
    Invited { from: PlayerId, name: String },
    /// An invite you sent was declined by player `by`.
    InviteDeclined { by: PlayerId },
    /// Paired with an opponent; you take `seat` (seat 0 moves first).
    Matched { seat: Seat, opponent: String },
    /// An opaque in-game payload from the opponent.
    Game { payload: Vec<u8> },
    /// The opponent disconnected or resigned.
    OpponentLeft,
    /// A protocol-level error (e.g. version mismatch); the connection closes.
    Error { message: String },
}

/// Serialize a message to bytes (the body of one WebSocket message).
pub fn to_bytes<T: Serialize>(msg: &T) -> Vec<u8> {
    serde_json::to_vec(msg).expect("protocol messages always serialize")
}

/// Parse a message from a WebSocket message body.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_client(msg: ClientMsg) {
        let decoded: ClientMsg = decode(&to_bytes(&msg)).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn client_messages_round_trip() {
        round_trip_client(ClientMsg::Hello {
            name: "Jake".into(),
            protocol: PROTOCOL_VERSION,
            credential: SharedTokenCredential {
                key_id: DEV_KEY_ID,
                token: DEV_TOKEN.into(),
            }
            .to_bytes(),
        });
        round_trip_client(ClientMsg::Invite { to: 7 });
        round_trip_client(ClientMsg::Accept { inviter: 7 });
        round_trip_client(ClientMsg::Decline { inviter: 7 });
        round_trip_client(ClientMsg::Game {
            payload: vec![1, 2, 3],
        });
    }

    #[test]
    fn messages_are_flat_type_tagged_json() {
        // The published wire shape: a `"type"` discriminator alongside the
        // fields. Non-Rust clients depend on this, so pin it.
        let json = String::from_utf8(to_bytes(&ClientMsg::Invite { to: 7 })).unwrap();
        assert_eq!(json, r#"{"type":"Invite","to":7}"#);

        let json = String::from_utf8(to_bytes(&ServerMsg::Error {
            message: "nope".into(),
        }))
        .unwrap();
        assert_eq!(json, r#"{"type":"Error","message":"nope"}"#);

        let json = String::from_utf8(to_bytes(&ServerMsg::OpponentLeft)).unwrap();
        assert_eq!(json, r#"{"type":"OpponentLeft"}"#);
    }

    #[test]
    fn server_messages_round_trip() {
        for msg in [
            ServerMsg::Presence {
                players: vec![
                    PlayerInfo {
                        id: 1,
                        name: "Bob".into(),
                    },
                    PlayerInfo {
                        id: 2,
                        name: "Carol".into(),
                    },
                ],
            },
            ServerMsg::Invited {
                from: 1,
                name: "Bob".into(),
            },
            ServerMsg::InviteDeclined { by: 1 },
            ServerMsg::Matched {
                seat: Seat(0),
                opponent: "Bob".into(),
            },
            ServerMsg::Game {
                payload: vec![9, 8, 7],
            },
            ServerMsg::OpponentLeft,
            ServerMsg::Error {
                message: "bad version".into(),
            },
        ] {
            let decoded: ServerMsg = decode(&to_bytes(&msg)).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn shared_token_credential_round_trips() {
        let cred = SharedTokenCredential {
            key_id: 3,
            token: "abc".into(),
        };
        assert_eq!(
            SharedTokenCredential::from_bytes(&cred.to_bytes()),
            Some(cred)
        );
        assert_eq!(SharedTokenCredential::from_bytes(b"not json"), None);
    }
}
