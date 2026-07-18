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
| `just play [ADDR] [NAME]` | Launch the game in online mode, connecting to a relay. |
| `just selfplay N` | Headless: play N random self-play games. |
| `just matchup [DEEP] [SHALLOW] [GAMES]` | Play a depth-vs-depth AI match and print the score. |
| `just frame` | Render one board frame to `target/frame.png` for visual inspection. |
| `just atlas` | Rebuild the texture atlas via Aseprite CLI (deferred; see DESIGN §6). |

### Multiplayer (LAN / localhost)

Two players connect to a small relay server, which auto-pairs them. On one machine, use three
terminals:

```sh
just serve                       # start the relay on 127.0.0.1:5000
just play 127.0.0.1:5000 Alice   # window 1 (becomes Black)
just play 127.0.0.1:5000 Bob     # window 2 (becomes White)
```

The first two players to connect are matched automatically. Across two Macs on the same Wi-Fi,
run `just serve 0.0.0.0:5000` on one and `just play <that-Mac's-IP>:5000 <name>` on each. See
[DESIGN.md §9](DESIGN.md) for the architecture and the road to internet play.

## Project layout

A Cargo workspace with a strict, one-directional dependency graph
(`app → {render, eval, protocol} → game-core`; `server → protocol`; never the reverse):

| Crate | Responsibility | Depends on |
|---|---|---|
| `crates/game-core` | Board, rules, move generation, search. **Pure** (std only, no I/O). | — |
| `crates/eval` | Position evaluation (heuristics now, ML later) behind a trait. | game-core |
| `crates/render` | `wgpu` sprite/quad batcher and board geometry. No game logic. | game-core |
| `crates/protocol` | Multiplayer wire format (serde). Primitive types only. | — |
| `crates/app` | `winit` shell + input + client networking; the only crate that touches windowing. | render, eval, protocol, game-core |
| `crates/server` | Relay/matchmaking server (`tokio`). Relays messages opaquely. | protocol |

Keeping `game-core` and `eval` pure means the rules and AI are fully testable with
`cargo test` alone; I/O and async live only in `app` (client, async-free) and `server`
(tokio). The eventual iOS port is confined to the input/windowing layer in `app` (see the
`PointerInput` note in DESIGN §8), and the networking is transport-swappable (DESIGN §9).

## Development workflow

`main` is protected — no direct pushes. Every change goes:

**feature branch → PR → CI passes (`check-and-test`) → squash merge** (branch auto-deletes).

The squash commit uses the PR title + body, so history stays one clean commit per change.
Run `just check && just test` locally before pushing. Progress is tracked in
[PLAN.md](PLAN.md).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
