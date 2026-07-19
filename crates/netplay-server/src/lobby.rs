//! The lobby: a single task that owns all matchmaking state.
//!
//! Modeled as an actor — every connection sends [`LobbyCmd`]s over a channel and
//! the lobby processes them one at a time, so there are no locks. It tracks all
//! connected players, broadcasts presence to those not in a game, forwards
//! invites, and pairs players who accept.

use std::collections::HashMap;
use std::time::Instant;

use netplay_protocol::{MatchInfo, PlayerInfo, Seat, ServerMsg, ServerStats};
use tokio::sync::{mpsc, oneshot};

use crate::limits::MAX_LOBBY_PLAYERS;

pub use netplay_protocol::PlayerId;

/// Commands sent to the lobby by connection tasks.
pub enum LobbyCmd {
    Join {
        name: String,
        outbox: mpsc::Sender<ServerMsg>,
        reply: oneshot::Sender<Option<PlayerId>>,
    },
    Invite {
        from: PlayerId,
        to: PlayerId,
    },
    Accept {
        accepter: PlayerId,
        inviter: PlayerId,
    },
    Decline {
        decliner: PlayerId,
        inviter: PlayerId,
    },
    Relay {
        from: PlayerId,
        payload: Vec<u8>,
    },
    Leave {
        id: PlayerId,
    },

    // Read queries for the admin REST API, answered over a oneshot.
    ListPlayers {
        reply: oneshot::Sender<Vec<PlayerInfo>>,
    },
    ListMatches {
        reply: oneshot::Sender<Vec<MatchInfo>>,
    },
    Stats {
        reply: oneshot::Sender<ServerStats>,
    },
}

struct Player {
    name: String,
    outbox: mpsc::Sender<ServerMsg>,
    /// The opponent's id while in a game, else `None` (available in the lobby).
    partner: Option<PlayerId>,
    /// This player's seat while in a game (seat 0 = inviter), else `None`.
    seat: Option<Seat>,
}

struct Lobby {
    next_id: PlayerId,
    players: HashMap<PlayerId, Player>,
    started: Instant,
}

/// Run the lobby until every connection has dropped its command sender.
pub async fn run(mut rx: mpsc::Receiver<LobbyCmd>) {
    let mut lobby = Lobby {
        next_id: 0,
        players: HashMap::new(),
        started: Instant::now(),
    };
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
                if self.players.len() >= MAX_LOBBY_PLAYERS {
                    let _ = outbox
                        .send(ServerMsg::Error {
                            message: "lobby full".to_string(),
                        })
                        .await;
                    let _ = reply.send(None);
                    eprintln!("rate-limit: lobby full, rejected {name}");
                    return;
                }
                let id = self.next_id;
                self.next_id += 1;
                self.players.insert(
                    id,
                    Player {
                        name,
                        outbox,
                        partner: None,
                        seat: None,
                    },
                );
                let _ = reply.send(Some(id));
                println!("player {id} joined");
                self.broadcast_presence().await;
            }

            LobbyCmd::Invite { from, to } => {
                if self.is_available(from) && self.is_available(to) {
                    let name = self.players[&from].name.clone();
                    self.send(to, ServerMsg::Invited { from, name }).await;
                }
            }

            LobbyCmd::Accept { accepter, inviter } => {
                if self.is_available(accepter) && self.is_available(inviter) {
                    self.start_match(inviter, accepter).await;
                }
            }

            LobbyCmd::Decline { decliner, inviter } => {
                if self.is_available(inviter) {
                    self.send(inviter, ServerMsg::InviteDeclined { by: decliner })
                        .await;
                }
            }

            LobbyCmd::Relay { from, payload } => {
                if let Some(partner) = self.players.get(&from).and_then(|p| p.partner) {
                    self.send(partner, ServerMsg::Game { payload }).await;
                }
            }

            LobbyCmd::Leave { id } => {
                if let Some(player) = self.players.remove(&id) {
                    if let Some(partner) = player.partner {
                        if let Some(p) = self.players.get_mut(&partner) {
                            p.partner = None;
                            p.seat = None;
                        }
                        self.send(partner, ServerMsg::OpponentLeft).await;
                    }
                    println!("player {id} left");
                    self.broadcast_presence().await;
                }
            }

            LobbyCmd::ListPlayers { reply } => {
                let _ = reply.send(self.player_list());
            }
            LobbyCmd::ListMatches { reply } => {
                let _ = reply.send(self.match_list());
            }
            LobbyCmd::Stats { reply } => {
                let _ = reply.send(self.stats());
            }
        }
    }

    /// Every connected player as `PlayerInfo`.
    fn player_list(&self) -> Vec<PlayerInfo> {
        self.players
            .iter()
            .map(|(&id, p)| PlayerInfo {
                id,
                name: p.name.clone(),
            })
            .collect()
    }

    /// Every active match, keyed off the seat-0 player so each pair appears once.
    fn match_list(&self) -> Vec<MatchInfo> {
        self.players
            .iter()
            .filter(|(_, p)| p.seat == Some(Seat(0)))
            .filter_map(|(&id, p)| {
                let partner = p.partner?;
                let other = self.players.get(&partner)?;
                Some(MatchInfo {
                    seat0: PlayerInfo {
                        id,
                        name: p.name.clone(),
                    },
                    seat1: PlayerInfo {
                        id: partner,
                        name: other.name.clone(),
                    },
                })
            })
            .collect()
    }

    /// A snapshot of the relay counters.
    fn stats(&self) -> ServerStats {
        let matches_active = self
            .players
            .values()
            .filter(|p| p.seat == Some(Seat(0)))
            .count() as u32;
        ServerStats {
            players_online: self.players.len() as u32,
            matches_active,
            uptime_seconds: self.started.elapsed().as_secs(),
        }
    }

    fn is_available(&self, id: PlayerId) -> bool {
        self.players.get(&id).is_some_and(|p| p.partner.is_none())
    }

    /// Pair two available players. The inviter plays Black (moves first).
    async fn start_match(&mut self, inviter: PlayerId, accepter: PlayerId) {
        let inviter_name = self.players[&inviter].name.clone();
        let accepter_name = self.players[&accepter].name.clone();
        if let Some(p) = self.players.get_mut(&inviter) {
            p.partner = Some(accepter);
            p.seat = Some(Seat(0));
        }
        if let Some(p) = self.players.get_mut(&accepter) {
            p.partner = Some(inviter);
            p.seat = Some(Seat(1));
        }
        self.send(
            inviter,
            ServerMsg::Matched {
                seat: Seat(0),
                opponent: accepter_name,
            },
        )
        .await;
        self.send(
            accepter,
            ServerMsg::Matched {
                seat: Seat(1),
                opponent: inviter_name,
            },
        )
        .await;
        println!("matched {inviter} (seat 0) vs {accepter} (seat 1)");
        self.broadcast_presence().await;
    }

    async fn send(&self, id: PlayerId, msg: ServerMsg) {
        if let Some(player) = self.players.get(&id) {
            let _ = player.outbox.send(msg).await;
        }
    }

    /// Send every available player the list of the *other* available players.
    async fn broadcast_presence(&self) {
        let available: Vec<(PlayerId, PlayerInfo)> = self
            .players
            .iter()
            .filter(|(_, p)| p.partner.is_none())
            .map(|(&id, p)| {
                (
                    id,
                    PlayerInfo {
                        id,
                        name: p.name.clone(),
                    },
                )
            })
            .collect();

        for (recipient, _) in &available {
            let others = available
                .iter()
                .filter(|(id, _)| id != recipient)
                .map(|(_, info)| info.clone())
                .collect();
            self.send(*recipient, ServerMsg::Presence { players: others })
                .await;
        }
    }
}
