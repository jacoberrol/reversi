# reversi

A from-scratch **Reversi (Othello)** game in Rust, built with `wgpu` + `winit` — no
game engine. It's a learning project: the point is to understand the full stack (modern
GPU rendering on Apple Silicon, on-device ML later, an AI-assisted asset pipeline later),
so the code favors the simple explicit version over clever abstraction.

- **Game:** 8×8 Reversi, single-player vs. an AI opponent (search depth = difficulty).
- **Primary target:** macOS (Apple Silicon). iOS/Android are later ports.
- **v1 graphics:** procedural only (solid quads + shader-drawn discs). The sprite/atlas
  pipeline is deferred — see [DESIGN.md](DESIGN.md) §6.

## Documentation map

| File | Role |
|---|---|
| `README.md` | You are here — what it is, how to build and run. |
| [PLAN.md](PLAN.md) | Living progress tracker: stages, checklists, change log. |
| [DESIGN.md](DESIGN.md) | Architecture decisions and their rationale. |
| [CLAUDE.md](CLAUDE.md) | Operating rules for the AI assistant (and a crisp house style). |

## Prerequisites

- **Rust** (stable, edition 2021) — <https://rustup.rs>
- **[`just`](https://github.com/casey/just)** command runner — `brew install just`
  (or `cargo install just`)
- Aseprite is **not** required for v1 (only the deferred `just atlas` needs it).

## Getting started

```sh
git clone https://github.com/jacoberrol/reversi.git
cd reversi
just            # list available recipes
just check      # cargo fmt --check + clippy -D warnings (the pre-commit gate)
just test       # run the workspace test suite
```

Other recipes (`atlas` is stubbed until the deferred sprite pipeline — see [PLAN.md](PLAN.md)):

| Command | What it does |
|---|---|
| `just run` | Launch the game window and play Black vs. the AI (White). Click a difficulty button (or press `1`–`4`); at game over, click the board or press `R` for a new game. The title bar shows turn, difficulty, and result. |
| `just serve [ADDR]` | Run the multiplayer relay server (default `127.0.0.1:5000`). |
| `just prune-tokens` | Operator action: delete expired admin sessions from the DB (`NETPLAY_DB`). |
| `just online` | Play online against the public relay (baked-in `wss://` URL); log in / register on the title screen. |
| `just play [URL]` | Play against a specific relay (e.g. a local `ws://` server). |
| `just deploy` | Trigger the relay deploy to the exe.dev VM (manual CI workflow; see [deploy/README.md](deploy/README.md)). |
| `just selfplay N` | Headless: play N random self-play games. |
| `just matchup [DEEP] [SHALLOW] [GAMES]` | Play a depth-vs-depth AI match and print the score. |
| `just frame` | Render one board frame to `target/frame.png` for visual inspection. |
| `just atlas` | Rebuild the texture atlas via Aseprite CLI (deferred; see DESIGN §6). |

### Multiplayer (LAN / localhost)

Each client **logs in or creates an account** on a title screen, then sees others in a **lobby**;
one invites another, they accept, and the game begins. The fastest way to try it on one machine:

```sh
just demo   # builds, starts a relay, opens two windows — register two accounts, then invite/accept
```

Or run the pieces yourself (three terminals):

```sh
just serve                     # relay listens on 127.0.0.1:5000 (plain ws)
just play ws://127.0.0.1:5000  # window 1: log in / register
just play ws://127.0.0.1:5000  # window 2: log in / register
```

Create a different account in each window, then click **Invite** next to the other player and
**Accept** in the other. The inviter plays Black (moves first). Across two Macs on the same Wi-Fi,
run `just serve 0.0.0.0:5000` on one and `just play ws://<that-Mac's-IP>:5000` on each.

### Multiplayer (over the internet)

`just online` connects to the public relay at `wss://relay.netplay.oliverj.network` (TLS terminated
by a front proxy that forwards to the server on the VM). Log in or create an account on the title
screen — the relay is **accounts-only** (open registration). Deploying/operating that relay is
documented in [deploy/README.md](deploy/README.md); the architecture is in [DESIGN.md §9](DESIGN.md).

## Project layout

A Cargo workspace with a strict, one-directional dependency graph
(`app → {render, eval, netplay-protocol, netplay-client} → game-core`;
`netplay-{server,client} → netplay-protocol`; never the reverse):

| Crate | Responsibility | Depends on |
|---|---|---|
| `crates/game-core` | Board, rules, move generation, search. **Pure** (std only, no I/O). | — |
| `crates/eval` | Position evaluation (heuristics now, ML later) behind a trait. | game-core |
| `crates/render` | `wgpu` sprite/quad batcher and board geometry. No game logic. | game-core |
| `crates/netplay-protocol` | Game-agnostic wire format (serde): framing, lobby/match envelope, **opaque game payload**. | — |
| `crates/netplay-client` | Reusable client transport (WebSocket on a background runtime → winit events). | netplay-protocol |
| `crates/netplay-server` | Reusable relay/matchmaking server (`tokio`, WebSocket). Relays the payload opaquely. | netplay-protocol |
| `crates/app` | `winit` shell + input + `egui` lobby; defines the Reversi `GameMsg`; the only crate that touches windowing. | render, eval, netplay-*, game-core |

Keeping `game-core` and `eval` pure means the rules and AI are fully testable with
`cargo test` alone; networking (WebSocket) lives only in `netplay-client` (on a background
tokio runtime; the winit loop stays synchronous) and `netplay-server` (tokio). The netplay layer
is game-agnostic (any 2-player turn-based game can use it), and the transport is swappable (DESIGN §9).

## Development workflow

`main` is protected — no direct pushes. Every change goes:

**feature branch → PR → CI passes (`check-and-test`) → squash merge** (branch auto-deletes).

The squash commit uses the PR title + body, so history stays one clean commit per change.
Run `just check && just test` locally before pushing. Progress is tracked in
[PLAN.md](PLAN.md).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
