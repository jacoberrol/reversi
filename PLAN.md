# PLAN.md — Reversi build plan & progress tracker

> Living document. This is the **execution tracker**: what's done, what's next, and
> how we know a step is finished. Architecture *rationale* lives in [DESIGN.md](DESIGN.md);
> standing rules and commands live in [CLAUDE.md](CLAUDE.md). Update this file as work
> lands and as plans change in flight.
>
> Last updated: 2026-07-18.

## How we work

- macOS is the dev target. Rust + wgpu + winit, plain structs (no ECS/Bevy).
- `main` is protected: every change lands via a **PR** that passes CI, **squash-merged**.
- Each stage ends with a green `just check && just test` (+ any stage-specific verify)
  and a commit/PR. Never merge with failing checks.

Status legend: ✅ done · 🚧 in progress · ⬜ not started · 🔮 future / not yet scheduled

## Milestones

### Infra — repo, CI, protection ✅
- ✅ Cargo workspace: `game-core`, `eval`, `render`, `app` (deps: `app → {render, eval} → game-core`)
- ✅ `justfile` with `check`, `test`, `run`, `selfplay`, `frame`, `atlas` (`selfplay`/`frame`/`atlas` stubbed)
- ✅ Pushed to GitHub; repo public
- ✅ GitHub Actions CI: `just check` + `just test` on PRs and `main`
- ✅ Branch protection ruleset on `main`: require PR, require `check-and-test`, squash-only, no force-push/delete

### Stage 1 — Workspace skeleton ✅
- ✅ `git init`, Rust `.gitignore`, workspace, justfile, `assets/`+`scripts/` with `.gitkeep`
- ✅ Verify: `just check` and `just test` green on empty workspace
- Commit: `Scaffold Cargo workspace skeleton`

### Stage 2 — game-core: board & rules 🚧 (next)
Pure Rust, std only, no panics in the public API (invalid squares / illegal moves → `Result`/`Option`).
- ⬜ `Board`, `Cell`, `Player`, `Square` newtype
- ⬜ Move generation, disc flipping, pass handling, terminal detection
- ⬜ Tests: opening has exactly 4 legal moves for Black; a known flip scenario; forced pass;
  full-board and no-moves-for-both game end
- ⬜ Perft-style test: 1,000 random games to completion, no panics, disc counts always sum correctly
- ⬜ Wire `just selfplay N` to a game-core example/binary
- ⬜ Verify: `just check && just test && just selfplay 1000`

### Stage 3 — eval + search ⬜
- ⬜ `eval`: handcrafted evaluation (corner control, mobility, disc parity) behind an `Evaluator` trait
- ⬜ Minimax + alpha-beta with a depth parameter (depth = difficulty); location justified in the PR
- ⬜ Tests: depth-1 takes an available corner; deeper beats shallower over a 50-game match (fixed seeds, statistical)
- ⬜ Verify: checks + tests, and report depth-vs-depth match results

### Stage 4 — window & first pixels ⬜
- ⬜ `app`: winit window (main thread on macOS), wgpu setup, clear-color loop, resize/surface-loss handling
- ⬜ `render`: instanced colored-quad batcher (texture support stubbed); draw the 8×8 board + procedural flat discs
- ⬜ Implement `just frame` (offscreen render → `target/frame.png`); self-check the PNG before claiming it works
- ⬜ Mouse click → board square mapping; play vs. the depth-3 AI
- ⬜ Verify: `just check && just test && just frame`, review PNG, then `just run`

## Backlog / future (post-Stage 4) 🔮
- 🔮 Shader polish for procedural discs (SDF circle + highlight + rim), flip/settle animation
- 🔮 Real `just atlas`: Aseprite CLI pipeline for tiles/backgrounds (requires `aseprite` on PATH)
- 🔮 Texture-backed sprites through the batcher (unstub texture support)
- 🔮 ML evaluator via `burn` (wgpu backend) behind the `Evaluator` trait — first ML experiment (see DESIGN §8)
- 🔮 Audio stack decision (`kira`/`rodio`)
- 🔮 Art-direction decision (pixel vs. procedural-HD)
- 🔮 Mobile port (iOS first, then Android) — no scaffolding until the game exists

## In-flight change log
Record notable plan/scope changes here so the "why" survives.
- 2026-07-18 — Committed to Reversi as the mechanic; dropped the ML level-generator idea
  (Reversi has no levels). Confirmed plain structs over ECS. See DESIGN §1, §5, §8.
- 2026-07-18 — Repo made public to enable free branch protection; PR-only + squash-only flow on `main`.
