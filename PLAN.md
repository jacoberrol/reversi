# PLAN.md — Reversi build plan & progress tracker

> Living document. This is the **execution tracker**: what's done, what's next, and
> how we know a step is finished. Architecture *rationale* lives in [DESIGN.md](DESIGN.md);
> standing rules and commands live in [CLAUDE.md](CLAUDE.md). Update this file as work
> lands and as plans change in flight.
>
> Last updated: 2026-07-18.

## How we work

- macOS is the dev target. Rust + wgpu + winit, plain structs (no ECS/Bevy).
- **v1 graphics are procedural only** — solid quads + shader-drawn discs. The diffusion/Aseprite
  sprite pipeline (DESIGN §6) is explicitly out of scope until the game is fun.
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

### Stage 2 — game-core: board & rules ✅
Pure Rust, std only, no panics in the public API (invalid squares / illegal moves → `Result`/`Option`).
- ✅ `Board`, `Cell`, `Player`, `Square` newtype (one concept per file)
- ✅ Move generation, disc flipping, pass handling, terminal detection (`apply`/`pass`/`is_terminal`/`outcome`)
- ✅ Tests: opening has exactly 4 legal moves for Black; a known flip scenario; forced pass;
  full-board and no-moves-for-both game end (7 unit tests)
- ✅ Perft-style test: 1,000 random games to completion, no panics, disc counts always sum to 64 every ply
- ✅ Wire `just selfplay N` → `game-core` `selfplay` example (deterministic, seeded)
- ✅ Verify: `just check && just test && just selfplay 1000` (avg ~60.5 plies/game)

### Stage 3 — eval + search ✅
- ✅ `eval`: handcrafted `Heuristic` (corner control, mobility, disc parity) implementing the `Evaluator` trait
- ✅ Negamax + alpha-beta with a depth parameter (depth = difficulty). **Placement:** search + `Evaluator`
  trait in `game-core` (CLAUDE.md assigns "search" there; trait sits beside search so it stays generic
  without depending on `eval`); concrete `Heuristic` in `eval`. ML evaluators later implement the same trait.
- ✅ Tests: depth-1 takes an available corner; deeper (d3) beats shallower (d1) over a seeded 50-game match
- ✅ Verify: checks + tests + `just matchup` → **depth 3 beat depth 1: 46–3–1 (94% of decisive games)**

### Stage 4 — window & first pixels ✅
- ✅ `app`: winit 0.30 window (event loop on the main thread), wgpu 0.20 setup, `ControlFlow::Wait`
  render loop, resize + surface `Lost/Outdated` reconfigure (no panics)
- ✅ `render`: instanced colored-quad batcher (one pipeline, `MAX_INSTANCES` buffer; texture support
  still stubbed); draws the 8×8 board (backing + cells + grid gaps), procedural flat discs (SDF circle
  with a soft edge in the fragment shader), and translucent legal-move hints
- ✅ `just frame` → offscreen render to `target/frame.png` (headless wgpu, texture readback, `image` PNG
  encode); self-checked the PNG (opening + 1 move shows both colours + hints correctly)
- ✅ **Input abstraction (port-ready).** `PointerInput { x, y, phase }` in `app`:
  - ✅ macOS now: winit `MouseInput` (+ tracked `CursorMoved`) → `PointerInput`
  - ✅ iOS later: winit `Touch` → the same `PointerInput` (no changes below `app`)
  - ✅ `render::board_view` owns the layout; `square_at` is the pixel→`Square` inverse for hit-testing
  - ✅ `game-core` only ever receives a `Square`
- ✅ Wire-up: human `PointerInput` → `game-core` move → `eval` reply (**depth 6**, see note) → redraw
- ✅ Verify: `just check && just test && just frame` (PNG reviewed). `just run` is the interactive play test.

> Depth note: bumped the AI from the originally-planned depth 3 to **depth 6** (`app::game::AI_DEPTH`).
> The Stage-3 benchmark showed depth 6 is ~0.2s worst case on this hardware — instant and much stronger.

## Backlog / future (post-Stage 4) 🔮
- 🔮 Game-over UI (winner/score banner) and a restart control — v1 has no end-of-game screen yet
- 🔮 Difficulty selector mapped to search depth (Easy 2 / Medium 4 / Hard 6 / Expert 7–8)
- 🔮 Iterative deepening with a per-move time budget + endgame solver (bounded latency, perfect endgame)
- 🔮 Shader polish for procedural discs (highlight + rim), flip/settle animation
- 🔮 **Deferred sprite pipeline (not v1):** real `just atlas` via Aseprite CLI for tiles/backgrounds
  (requires `aseprite` on PATH), plus the diffusion generation steps in DESIGN §6
- 🔮 **Deferred sprite pipeline (not v1):** texture-backed sprites through the batcher (unstub texture support)
- 🔮 ML evaluator via `burn` (wgpu backend) behind the `Evaluator` trait — first ML experiment (see DESIGN §8)
- 🔮 Audio stack decision (`kira`/`rodio`)
- 🔮 Art-direction decision (pixel vs. procedural-HD)
- 🔮 Mobile port (iOS first, then Android) — no scaffolding until the game exists

## In-flight change log
Record notable plan/scope changes here so the "why" survives.
- 2026-07-18 — Committed to Reversi as the mechanic; dropped the ML level-generator idea
  (Reversi has no levels). Confirmed plain structs over ECS. See DESIGN §1, §5, §8.
- 2026-07-18 — Scoped v1 to **procedural graphics only**; the diffusion/Aseprite sprite
  pipeline (DESIGN §6) is deferred until the game is fun. See DESIGN §6, §8.
- 2026-07-18 — Adopted a **`PointerInput` abstraction** in `app` (mouse now, touch later) so the
  macOS→iOS port only touches that layer; `game-core` stays `Square`-only. See DESIGN §8, PLAN Stage 4.
- 2026-07-18 — Added `README.md` (human entry point) and made the branch→PR→CI→squash flow explicit
  in CLAUDE.md (it predated branch protection).
- 2026-07-18 — Repo made public to enable free branch protection; PR-only + squash-only flow on `main`.
- 2026-07-18 — Stage 2 complete: `game-core` board + rules (immutable `apply`, exhaustive enum
  matches, `Square`-validated API, no public-API panics). Design choice: `apply`/`pass` return a
  new `Board` rather than mutating, for cheap search in Stage 3.
- 2026-07-18 — Stage 3 complete: negamax + alpha-beta search and the `Evaluator` trait in `game-core`
  (per CLAUDE.md), handcrafted `Heuristic` in `eval`. Depth = difficulty. Depth 3 beats depth 1 46–3–1.
  Added `just matchup` to visualize strength-vs-depth.
- 2026-07-18 — Stage 4 complete: winit/wgpu window + `render` quad batcher (pinned wgpu 0.20 / winit 0.30).
  `just frame` renders headless to a PNG for self-verification. `PointerInput` input abstraction lands.
  AI default set to depth 6 (instant, per benchmark). First external deps enter the tree.
