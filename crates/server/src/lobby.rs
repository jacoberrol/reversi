//! The lobby: a single task that owns all matchmaking state.
//!
//! Modeled as an actor — every connection sends [`LobbyCmd`]s over a channel and
//! the lobby processes them one at a time, so there are no locks and no shared
//! mutable state. It holds at most one waiting player; the next arrival is paired
//! with them.

use std::collections::HashMap;

use protocol::{Color, GameMsg, ServerMsg};
use tokio::sync::{mpsc, oneshot};

/// Identifies a connected player for the lifetime of its connection.
pub type PlayerId = u64;

/// Commands sent to the lobby by connection tasks.
pub enum LobbyCmd {
    /// A client finished its handshake and wants a match. `reply` returns its id.
    Join {
        name: String,
        outbox: mpsc::Sender<ServerMsg>,
        reply: oneshot::Sender<PlayerId>,
    },
    /// Forward an in-game message from `from` to its opponent.
    Relay { from: PlayerId, msg: GameMsg },
    /// A connection ended; notify any opponent and clean up.
    Leave { id: PlayerId },
}

struct Waiting {
    id: PlayerId,
    name: String,
    outbox: mpsc::Sender<ServerMsg>,
}

#[derive(Default)]
struct Lobby {
    next_id: PlayerId,
    waiting: Option<Waiting>,
    outboxes: HashMap<PlayerId, mpsc::Sender<ServerMsg>>,
    partners: HashMap<PlayerId, PlayerId>,
}

/// Run the lobby until every connection has dropped its command sender.
pub async fn run(mut rx: mpsc::Receiver<LobbyCmd>) {
    let mut lobby = Lobby::default();
    while let Some(cmd) = rx.recv().await {
        lobby.handle(cmd).await;
    }
}

impl Lobby {
    async fn handle(&mut self, cmd: LobbyCmd) {
        match cmd {
            LobbyCmd::Join {
                name,
                outbox,
                reply,
            } => {
                let id = self.next_id;
                self.next_id += 1;
                self.outboxes.insert(id, outbox.clone());
                let _ = reply.send(id);

                match self.waiting.take() {
                    // Pair with the waiting player: they are Black (joined first),
                    // the newcomer is White.
                    Some(waiting) => {
                        self.partners.insert(waiting.id, id);
                        self.partners.insert(id, waiting.id);
                        let _ = waiting
                            .outbox
                            .send(ServerMsg::Matched {
                                your_color: Color::Black,
                                opponent: name,
                            })
                            .await;
                        let _ = outbox
                            .send(ServerMsg::Matched {
                                your_color: Color::White,
                                opponent: waiting.name,
                            })
                            .await;
                        println!("matched {} (black) vs {} (white)", waiting.id, id);
                    }
                    None => {
                        self.waiting = Some(Waiting { id, name, outbox });
                        println!("player {id} waiting for an opponent");
                    }
                }
            }

            LobbyCmd::Relay { from, msg } => {
                if let Some(&partner) = self.partners.get(&from) {
                    if let Some(outbox) = self.outboxes.get(&partner) {
                        let _ = outbox.send(ServerMsg::Game(msg)).await;
                    }
                }
            }

            LobbyCmd::Leave { id } => {
                self.outboxes.remove(&id);
                if self.waiting.as_ref().map(|w| w.id) == Some(id) {
                    self.waiting = None;
                }
                if let Some(partner) = self.partners.remove(&id) {
                    self.partners.remove(&partner);
                    if let Some(outbox) = self.outboxes.get(&partner) {
                        let _ = outbox.send(ServerMsg::OpponentLeft).await;
                    }
                    println!("player {id} left; notified {partner}");
                }
            }
        }
    }
}
