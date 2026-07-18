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

Other recipes (some are stubbed until their stage lands — see [PLAN.md](PLAN.md)):

| Command | What it does |
|---|---|
| `just run` | Launch the game (window arrives in Stage 4). |
| `just selfplay N` | Headless: play N random self-play games (Stage 2). |
| `just frame` | Render one frame to `target/frame.png` for visual inspection (Stage 4). |
| `just atlas` | Rebuild the texture atlas via Aseprite CLI (deferred; see DESIGN §6). |

## Project layout

A Cargo workspace of four crates with a strict, one-directional dependency graph
(`app → {render, eval} → game-core`, never the reverse):

| Crate | Responsibility | Depends on |
|---|---|---|
| `crates/game-core` | Board, rules, move generation, search. **Pure** (std only, no I/O). | — |
| `crates/eval` | Position evaluation (heuristics now, ML later) behind a trait. | game-core |
| `crates/render` | `wgpu` sprite/quad batcher and board geometry. No game logic. | game-core |
| `crates/app` | `winit` shell + input; the only crate that touches windowing. | render, eval, game-core |

Keeping `game-core` and `eval` pure means the rules and AI are fully testable with
`cargo test` alone, and the eventual iOS port is confined to the input/windowing layer
in `app` (see the `PointerInput` note in DESIGN §8).

## Development workflow

`main` is protected — no direct pushes. Every change goes:

**feature branch → PR → CI passes (`check-and-test`) → squash merge** (branch auto-deletes).

The squash commit uses the PR title + body, so history stays one clean commit per change.
Run `just check && just test` locally before pushing. Progress is tracked in
[PLAN.md](PLAN.md).

## License

MIT OR Apache-2.0.
