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

### Stage 5 — UI enhancements ✅
No text renderer yet (textures still stubbed), so text goes in the **window title bar** and interactive
UI is drawn with quads. A real in-scene glyph renderer stays on the backlog.
- ✅ **Game-over UI**: dim overlay over the board with the winner's disc; title shows result + score;
  click the board (or press `R`) to start a new game.
- ✅ **Difficulty selector**: a row of four quad buttons (increasing bars = Easy/Medium/Hard/Expert →
  depth 2/4/6/8), selected one highlighted; click (or press `1`–`4`) to set it; applies immediately.
  Title names the current difficulty. `app::game` gained a `Difficulty` type; depth is no longer a const.
- ✅ `render::board_view` gained a control strip in the layout, `difficulty_button_at` hit-testing, and a
  `scene()` composer (board + controls + overlay) shared by the window and `just frame`.
- ✅ Verify: `just check && just test && just frame` (both PNG scenes reviewed — controls + game-over).
  `just run` is the interactive test.

### Stage 6 — visual polish ✅
- ✅ **Shader polish**: quad shader now handles shapes (plain rect / rounded rect / disc) via `shape` +
  `param`. Discs get a **specular highlight + rim shadow** (glossy); cells/buttons get **rounded corners**.
- ✅ **Board polish**: rounded board frame (tray look), rounded cells, Othello **star points** at the
  2nd/6th grid-line intersections, and soft **drop shadows** under discs.
- ✅ **Disc-flip animation**: `app::anim::Animator` plays a queue of move transitions; each move's flipped
  discs animate edge-on (x-squash + color swap at the midpoint) and the placed disc pops in (ease-out-back).
  Human move then AI move animate in sequence. Drives a per-frame redraw loop while active, back to
  redraw-on-event when idle. Board input is ignored mid-animation.
- ✅ Verify: `just check && just test && just frame` — reviewed static polish, game-over overlay, and a
  mid-flip frame (edge-on squash confirmed). `just run` is the live animation test.

### Stage 7 — networked multiplayer, Increment 1 (relay + auto-match, localhost) ✅
North star: named users discover each other over the internet via a cloud server. This increment
stands up the **real relay topology** on localhost so it isn't throwaway. See DESIGN §9.
- ✅ `crates/protocol`: serde wire format (primitive fields, no `game-core` dep), length-delimited
  JSON framing, `Color`/`GameMsg`/`ClientMsg`/`ServerMsg`, version handshake. Round-trip tests.
- ✅ `crates/server`: tokio relay (lib + thin bin). Auto-pairs the first two waiting clients
  (Black/White), relays game messages via a per-connection writer task + an in-memory lobby actor,
  reports disconnects. `just serve`.
- ✅ `app` network mode: `--server ADDR --name NAME` (`just play`). `EventLoop<NetEvent>` + a
  background TCP read thread → `EventLoopProxy`; client stays async-free (`TcpStream::try_clone`).
  `game.rs` split into `play_local`/`apply_remote_move` (+ local pass resolution); remote moves
  animate through the existing `Animator`. Difficulty UI hidden; status in the title. Logic factored
  into `session.rs`.
- ✅ Verify: protocol round-trip tests; a headless **relay integration test** (real server + two
  loopback clients: auto-match, relay, disconnect); a **sync test** (two networked clients stay
  identical to game end); server binary boots/binds/accepts. `just run` (single-player) + two
  `just play` windows (localhost) is the interactive test.

### Stage 7 — later increments 🔮
- 🔮 Increment 2: named presence + invite (lobby UI; first on-screen text renderer).
- 🔮 Increment 3: deploy the server to a cloud VM — add TLS, swap TCP→WebSocket behind the
  connection seam (reusing `protocol`). Out of scope now: accounts/auth, reconnect, spectating, NAT.

## Backlog / future (post-Stage 7) 🔮
- 🔮 **Search: move ordering** in alpha-beta (try corners / high-mobility / previous-best moves first, or
  order by a shallow pass). Better ordering ⇒ far more pruning ⇒ effectively deeper search at the same cost.
- 🔮 **Search: exact endgame solver** — once ≤ ~14–16 empties remain, search to the end on exact disc
  count (no heuristic). Cheap there (branching collapses) and plays the endgame perfectly.
- 🔮 Search: iterative deepening with a per-move time budget (bounded latency regardless of position)
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
- 2026-07-18 — Stage 5 complete: game-over overlay + difficulty selector. No glyph renderer yet, so text
  lives in the window title; interactive UI is quads. `board_view::scene` now composes board+controls+
  overlay for both the window and `just frame`. `Difficulty` (Easy/Medium/Hard/Expert → depth 2/4/6/8).
- 2026-07-18 — Stage 6 complete: visual polish. Shader generalized to shapes (rounded rects, glossy
  discs with highlight+rim); board gains a tray frame, star points, disc shadows. Disc-flip animation via
  an app-side `Animator` that turns the event-driven UI into a per-frame loop while a move plays.
- 2026-07-18 — Stage 7 Increment 1: networked multiplayer (LAN/localhost). New `protocol` (serde) and
  `server` (tokio relay, auto-match) crates; client gains a network mode over blocking TCP + winit user
  events, staying async-free. Real relay topology (client→server) chosen so internet-later reuses it.
  Session/net logic factored into `session.rs`. Verified headless (relay + sync tests). See DESIGN §9.
