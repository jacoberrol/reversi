# CLAUDE.md

2D puzzle game in Rust + wgpu. **Learning project**: the human is here to understand
the stack, so explain non-obvious choices as you make them, and prefer the simple
explicit version over the clever abstraction.

Architecture rationale and all standing decisions live in **DESIGN.md**. Read it before
structural work. If a change would contradict a decision recorded there, stop and ask —
do not "improve" the architecture toward generic defaults.

## Commands

- `just test` — run all tests (workspace)
- `just run` — launch the game (debug)
- `just check` — cargo fmt --check + clippy with `-D warnings`; must pass before any commit
- `just selfplay N` — headless: N random self-play games in game-core, prints results
- `just frame` — render one frame to `target/frame.png` (use this to inspect visual output)
- `just atlas` — rebuild texture atlas from `assets/src/` via Aseprite CLI

(If a command is missing, add it to the justfile rather than documenting a manual sequence.)

## Architecture rules

- `crates/game-core` — board, rules, move generation, search. **Pure.** No wgpu, no winit,
  no I/O, no async. Everything here must be testable via `cargo test` alone.
- `crates/eval` — position evaluation (heuristics now, ML later). Depends only on game-core.
- `crates/render` — wgpu sprite batcher, atlas loading. Keep thin; no game logic.
- `crates/app` — winit shell; wires the others together. Only crate that may touch windowing.
  Defines the Reversi `GameMsg` and rides it inside the netplay layer's opaque payload.
- **Reusable netplay layer (game-agnostic — no game logic):**
  - `crates/netplay-protocol` — the wire format (serde): framing + lobby/match envelope + an
    **opaque `Game(Vec<u8>)` payload the server never decodes**. Players are a `Seat` (seat 0 moves
    first), not a game-specific color.
  - `crates/netplay-server` — the relay/matchmaking server binary (tokio). Depends on `netplay-protocol` only.
  - `crates/netplay-client` — the client transport (blocking TCP, read thread → winit `EventLoopProxy`).
    The game supplies/interprets the payload.
- Dependency direction: `app → {render, eval, netplay-protocol, netplay-client} → game-core`;
  `netplay-{server,client} → netplay-protocol`. Never the reverse.
- **I/O and async live outside the pure crates.** `game-core`/`eval` stay pure (no I/O, no async).
  Networking is WebSocket: `netplay-server` (tokio) and `netplay-client` (WebSocket on a tokio
  runtime confined to a background thread — the winit loop stays synchronous). See DESIGN.md §9.
- **No ECS, no Bevy** (recorded decision — plain structs; see DESIGN.md §8 / ECS note).
- Game pieces are procedural (shaders); generated images are for tiles/backgrounds only.

## Conventions

- Rust 2021, `cargo fmt` defaults, clippy warnings are errors.
- Newtypes over bare primitives for domain values (`Square`, `Ply`); exhaustive matches
  on domain enums — no `_` arms on `Cell`/`Player`.
- One concept per file; keep files under ~300 lines, split before they grow past it.
- Tests live next to the code (`#[cfg(test)]`) for units; `tests/` for cross-crate behavior.
- Every rules-level behavior (legal moves, flips, pass, game end) needs a test before
  it's considered done.

## Workflow

- Work in small verified steps: after each meaningful change run `just check && just test`.
- `main` is protected: **no direct pushes.** Every change lands via a feature branch → PR →
  passing CI (`check-and-test`) → **squash merge** (the only allowed merge method; the branch
  auto-deletes). Never merge with failing checks.
- Squash commit = the PR title + body, so keep PR titles imperative and one line.
- Track work in `PLAN.md`: update stage checkboxes and the change log as things land.
- macOS is the dev target; iOS/Android come later — don't add mobile scaffolding yet,
  but don't block it either (no desktop-only crates in game-core/eval).

## Gotchas

- winit event loop must run on the main thread on macOS.
- wgpu surface is lost/reconfigured on resize — handle `SurfaceError::Lost/Outdated`
  by reconfiguring, don't panic.
- You cannot see the screen. To judge rendering, use `just frame` and read the PNG.
- Aseprite CLI (`aseprite -b`) must be on PATH for `just atlas`; if absent, say so
  instead of silently skipping.