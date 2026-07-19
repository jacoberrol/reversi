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

### Stage 7 — Increment 2 (named presence + invite lobby, egui) ✅
- ✅ Adopted **egui** for on-screen UI (evaluated custom-vs-egui via themed mockups; chose egui,
  themed to a non-"windowy" game look). `egui` + `egui-wgpu` on wgpu 0.20; no `egui-winit` (winit
  version clash) — pointer input hand-fed. See DESIGN §9.
- ✅ Protocol: player identity + presence + invites (`PlayerInfo`, `Invite`/`Accept`/`Decline`,
  `Presence`/`Invited`/`InviteDeclined`). Server lobby rewritten: tracks all players, broadcasts
  presence, forwards invites, pairs on accept. Auto-match retired.
- ✅ Client: `app` refactored to lib+bin; new `egui_layer` (live egui) + `lobby` (themed UI, state,
  actions); `session` gained a Lobby/InGame screen state machine; `gpu` routes rendering + input by
  screen. Lobby → invite/accept → in-game (reusing the same board render + animator).
- ✅ Verify: protocol round-trips; **relay integration test** rewritten for the invite flow (connect →
  presence → invite → accept → relay → disconnect); `just lobby-frame` renders the real lobby UI
  offscreen (reviewed). `just demo` (two windows) is the live invite-and-play test.

### Stage 7 — later increments 🔮
- 🔮 The cloud deploy (TLS + TCP→WebSocket) is now **Stage 8, Stage D** below — it comes *after*
  extracting and hardening the netplay layer, so those land in the reusable home first.
- 🔮 In-app name entry + a graphical main menu (name is a CLI arg for now); in-game egui HUD.

### Stage 8 — Netplay: extraction & hardening 🔮
Turn the Reversi-specific relay/lobby into a **reusable, authorized, rate-limited netplay layer**
any 2-player turn-based game in the workspace can use, and add the safety controls it needs before
facing the open internet. Extends DESIGN §9 (does not contradict it — reconcile §9 first if it ever
seems to). Honest non-goal: this deters and provides clean seams; it does **not** make the client
tamper-proof.

**Design decisions (self-contained; the scratch `netplay-plan.md` will be deleted):**
- **Reuse boundary via a workspace-internal crate split** (no new repo yet). The server already
  relays game messages opaquely, so the seam largely exists:
  - `netplay-protocol` — framing (`encode`/`decode`/`read_frame`, `MAX_FRAME`, version) + the generic
    envelope (`Hello`/`Invite`/`Accept`/`Decline`/`Presence`/`Matched`/`OpponentLeft`/`Error`) + an
    **opaque `Game` payload the server never decodes** + auth handshake types.
  - `netplay-server` — today's relay/lobby actor almost verbatim; `Color` → **`Seat`** (`Seat(u8)`,
    seat 0 = first to move); add the auth gate + rate limiting.
  - `netplay-client` — today's `net.rs` transport (blocking TCP, `try_clone` split, read thread →
    `EventLoopProxy`); the game owns its payload type.
  - Reversi keeps `GameMsg`, seat↔player mapping (seat 0 = Black), `session.rs`, and all of
    `game-core`/`eval`/`render`. (Rejected: generic `ClientMsg<P>` — leaks generics through the server
    for no gain since it never inspects the payload.)
- **Auth is a seam, not a token.** Server `Authenticator::verify(credential) -> Result<Identity, _>`
  (called after the version check, **before** `Join`); client `AuthProvider::credential()`. `Hello`
  gains a **versioned credential** (`{ key_id, token }`); `SharedTokenAuth` holds a small *set* of
  valid keys so `N`/`N+1` coexist during rotation (rotation ships via app update). `Identity` stays
  thin ("is this my app?", not "who is the user?"). Threat model: a client can't keep a secret
  (extractable via `strings`/proxy) — so this is deterrence + a swap-in point for attestation, not
  security to bet on. Plain token over TLS ≈ HMAC for less complexity (HMAC defends the wrong flank).
- **Rate limiting**, server-side at the connection boundary, before the lobby; drop **and log** on
  breach (silent throttling reads as "server broken"). Layers, all tunable `const`s in one place:
  handshake timeout (~5s), per-IP concurrent cap (~8) + new-connection bucket (~10/10s), per-connection
  inbound message bucket (~20/s, burst 40), existing `MAX_FRAME` (64 KiB), lobby caps (max players, max
  pending invites/player). Auth and rate-limit are two separate seams applied in sequence.

**Roadmap (ordering matters — extract first):**
- ✅ **Stage A — Extract `netplay-{protocol,server,client}`.** `Color`→`Seat`, opaque `Game(Vec<u8>)`
  payload; Reversi keeps `GameMsg` (in `app::game_msg`) + seat↔player mapping. Behavior-preserving;
  relay + protocol tests pass adapted; offscreen renders unchanged.
- ✅ **Stage B — Auth seam.** Server `Authenticator::verify` (before Join) + client `AuthProvider`;
  `Hello` carries an opaque credential; `SharedTokenAuth`/`SharedToken` (key-id'd token, `NETPLAY_TOKENS`
  env or dev default) behind the seam. Thin `Identity`. Rejection tested end-to-end.
- ✅ **Stage C — Rate limiting.** Handshake timeout (~5s), per-IP concurrency + new-connection rate
  (`IpLimiter`), per-connection inbound message bucket, lobby player cap. All tunable `const`s in
  `netplay-server::limits`; drop + log. (Invite spam is covered by the message bucket.)
- ✅ **Stage D1 — WebSocket transport swap.** Server (`tokio-tungstenite`, plain `ws://`) and client
  (WebSocket on a tokio runtime confined to the network thread; winit loop stays sync) speak WebSocket;
  `--server` is now a URL (`ws://…` local, `wss://…` deployed). Protocol messages unchanged; length
  framing replaced by WS message delimiting. Relay test rewritten over WS. Testable on localhost.
- ✅ **Stage D2 — Deploy (relay.netplay.oliverj.network).** Ansible playbook (`deploy/`) — locked-down
  `netplay` system user, hardened `systemd` unit bound to `127.0.0.1:8000`, `NETPLAY_TOKENS` env file —
  driven by a manual-dispatch GitHub Actions workflow that builds a static `x86_64-musl` binary and runs
  the playbook over a dedicated CI SSH key (GH Secrets). TLS terminated by the exe.dev proxy → `ws://` on
  the VM. Client bakes in `DEFAULT_RELAY_URL` (`--online`). I prepared; owner adds secrets and triggers.
- 🔮 **Stage E (later) — Attestation.** Swap `AuthProvider` to App Attest (iOS) / Play Integrity
  (Android) behind the unchanged seam. Web-distributed macOS stays at token+TLS deterrence.

### Stage 9 — Self-describing protocol + admin console 🔮
Motivated by an out-of-repo Go admin TUI: give the relay a rigorous, published, cross-language
contract while keeping serde/JSON (readable; we own both ends).
- ✅ **Increment 1 — Normalize the wire shape.** Internally-tagged JSON (`#[serde(tag = "type")]`)
  across `ClientMsg`/`ServerMsg`/`GameMsg`; `Game`/`Error` became struct variants. Flag-day break
  (redeploy server + rebuild clients together). Shape pinned by a test.
- ✅ **Increment 2 — `/schema` endpoint.** `schemars`-generated JSON Schema (behind a `schema`
  feature) served as a service descriptor over a minimal `hyper` HTTP/1 front (`GET /schema`; `/`
  upgrades to WS via `hyper-tungstenite`). Self-describing service; tested end-to-end.
- ✅ **Increment 3a — Admin queries (dev, no RBAC).** `ListPlayers`/`ListMatches`/`GetStats`
  request-reply messages, answered from the lobby actor (oneshot round-trip). In the published
  `/schema`. RBAC stays on the backlog.
- ✅ **Increment 3b — Admin event stream.** `SubscribeEvents` → live push of `PlayerJoined`/
  `PlayerLeft`/`MatchStarted`, so the TUI updates without polling. The lobby marks subscribed
  connections and broadcasts events to them. In the published `/schema`.

### Stage 10 — Accounts + RBAC on SQLite ✅
Durable identity: the relay gains a database. Named accounts have a role; the admin surface is gated
on it (closing the admin-RBAC backlog item). Anonymous play (shared token → `player`) stays for now
(removed in Stage 11).
- ✅ **Increment 1 — DB infrastructure.** `sqlx` + bundled SQLite; `store` module opens the DB
  (`NETPLAY_DB`, default `./netplay.db`) and runs embedded migrations on startup; `users` table
  (`0001_create_users.sql`). Ansible: `StateDirectory=netplay` + `NETPLAY_DB`. `just migrate-add`.
  No behavior change yet.
- ✅ **Increment 2 — Accounts + RBAC.** `Identity.role`; async `Authenticator` (async-trait) with a
  DB-backed `DbAuth` — `{name, password}` argon2id-verified against the DB, else shared-token
  anonymous `player`. `NETPLAY_ADMIN="name:password"` seeds/rotates the admin on startup. The relay
  gates the admin surface on `role == admin` (non-admins refused, not disconnected). `NETPLAY_ADMIN`
  added to the deploy workflow. argon2 verify runs on `spawn_blocking`; a dev-profile `opt-level=3`
  for argon2/blake2 keeps debug/CI test runs fast.

### Stage 11 — Accounts-only: in-app login/register ✅
Remove anonymous play and the shared token entirely; every client logs in or self-registers with a
name + password. Open registration (anyone can create an account). Reverses two recorded decisions
(the shared-token deterrence gate; anonymous play).
- ✅ **Increment 1 — Server registration + accounts-only.** `store::create_account`; the
  authenticator handles login (`{name,password}`) and register (`{...,register:true}`, min 8-char
  password, unique name) and **drops the anonymous fallback** — every connection must be an account.
  New `AuthError::{BadLogin, NameTaken, WeakPassword}`. Server tests moved to register/login.
- ✅ **Increment 2 — Client login/register menu (egui).** A `Screen::Login` title screen: name +
  masked password, Log in / Create account, inline errors; connect fires on submit with the account
  credential. Added keyboard→egui text-input plumbing (we skip egui-winit). Remembers the username
  (per-OS config). `--name` removed; `netplay_client::connect` takes the credential directly.
  `just login-frame` renders it offscreen.
- ✅ **Increment 3 — Delete the shared token.** Removed `SharedTokenCredential`/`DEV_*`,
  `SharedTokenAuth`, `AuthProvider`/`SharedToken`, the `NETPLAY_TOKENS`/`NETPLAY_TOKEN` env, and the
  `rotate-token`/`set-token` recipes; dropped the dead `AuthError::{UnknownKey,BadToken}`. `just deploy`
  no longer needs the `NETPLAY_TOKENS` secret (env template/workflow updated); docs updated.

### Stage 12 — Admin REST control plane (own host, bearer sessions) 🔨
Move admin off the gameplay WebSocket. The relay routes by requested hostname: the admin host
(`admin.netplay.oliverj.network`, `NETPLAY_ADMIN_HOST`; both it and `relay.*` resolve to the same
IP:port, split on the proxy's `X-Forwarded-Host`) serves a **REST admin API**; every other host is
the game WebSocket, which now carries *only* gameplay. Supersedes Stage 9's WS admin console +
`/schema`/`/asyncapi.json` docs (removed).
- 🔨 **Increment 1 — Strip WS to gameplay + REST control plane.** Removed the admin `ClientMsg`/
  `ServerMsg` variants, lobby `Subscribe`/broadcast, and the `schemars` schema surface (`/schema`,
  `/asyncapi.json`, the `schema` feature). New `sessions` table (`0002`); `store` gains
  session-token create/verify (256-bit random token, **sha256-hashed** at rest, TTL, lazy prune).
  New `admin` module: `POST /admin/login` (`{name,password}` → bearer token, admin-only) and
  bearer-guarded `GET /admin/{players,matches,stats}`. `serve` takes the admin host and routes on
  it. `just schema`/`asyncapi` recipes removed; `NETPLAY_ADMIN_HOST` added to the systemd unit.
  Follow-up: `POST /admin/tokens` (bearer-guarded, optional `{days}`, default 30, max 90) mints a
  **durable** token so a tool authenticates once and holds a weeks-long token without re-sending the
  password.
- 🔮 **Increment 2 — SSE `/admin/events`.** Re-add lobby event broadcast, streamed to bearer-guarded
  SSE subscribers (replaces the old WS `SubscribeEvents`).
- ✅ **Increment 3 — `GET /admin/openapi.json`.** Hand-written OpenAPI 3.0 document (`openapi.rs`)
  describing the admin REST API, served unauthenticated so a client can discover how to authenticate.
  `just openapi` fetches it.

### Stage 13 — Player auth over REST 🔨
Mirror the admin split for gameplay: authenticate over REST for a bearer token, present the token on
the WebSocket. Moves the argon2 check off the socket path (once, not per connect) and unifies auth on
tokens. Flag-day protocol break (`netplay-protocol` v2).
- ✅ **Increment 1 — server + protocol + client (one PR, since the protocol change is a flag day).**
  `POST /login` + `/register` on the game host (`player` module) → `{token, expires_in_hours}` (24h);
  register creates the account and returns a token in one step. WS `Hello` drops `name`/credential and
  carries the `token`; `Authenticator::verify` becomes `store::session_account` (token → id/role/name),
  so the display name comes from the account, never the client. `netplay-client` gains an `auth` SDK
  (`player_login`/`player_register` + `admin_login`/`admin_durable_token` for a future Rust admin tool)
  over blocking **ureq** (rustls+ring, no aws-lc/OpenSSL) and `login_and_connect` (REST then WS on the
  network thread). App `submit_login` just hands over name/password/register.
- 🔮 **Follow-up — persist a long-lived token** so a returning player skips the login screen (optional).

**Deferred:** separate repo / published crate (until a second consumer exists); N-player /
spectating / reconnect; client async / WASM browser client.

**Open questions:** WASM/web client ever wanted (the only thing that would force client async)?
token format (plain versioned random over TLS is likely enough); where the per-IP limiter lives
(standalone type vs. folded into the lobby actor).

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
- 2026-07-18 — Stage 7 Increment 2: named presence + invite lobby. Adopted egui (themed to a game look)
  for on-screen UI after a custom-vs-egui mockup bake-off. Protocol gains presence/invites; server lobby
  rewritten; `app` refactored to lib+bin with a Lobby/InGame screen state machine. `PointerInput` seam
  folded into `WindowState`. Verified via the invite-flow relay test + offscreen lobby render.
- 2026-07-18 — Added **Stage 8** (netplay extraction + hardening): reusable `netplay-*` crates with a
  `Seat`/opaque-payload boundary, an auth seam (versioned token), rate limiting, then TLS+WebSocket
  (folds in the old deploy increment) and attestation later. Planned only; not started.
- 2026-07-18 — Stage 8A done: extracted `netplay-{protocol,server,client}` from `protocol`/`server`/
  `net.rs`. Game-agnostic (`Seat`, opaque `Game(Vec<u8>)`); Reversi's `GameMsg` moves to `app::game_msg`.
  Behavior-preserving (relay + protocol tests pass). `just serve` now runs `netplay-server`.
- 2026-07-18 — Stage 8B done: client authorization seam. `Authenticator`/`AuthProvider` traits;
  `Hello` carries an opaque credential; `SharedTokenAuth`/`SharedToken` reference impl (versioned token,
  `NETPLAY_TOKENS` env or dev default). Server rejects bad credentials before Join (tested).
- 2026-07-18 — Stage 8C done: server-side rate limiting (`netplay-server::limits`). Handshake timeout,
  per-IP concurrency + connection-rate (`IpLimiter`), per-connection message token bucket, lobby player
  cap. Drop-and-log; tunable consts. Added tokio `time` feature. Unit-tested.
- 2026-07-18 — Stage 8D1 done: WebSocket transport. Server on `tokio-tungstenite` (plain ws); client on
  WebSocket over a tokio runtime confined to the network thread (winit loop stays sync — revised the
  "client fully async-free" note). `--server` is now a ws/wss URL. Protocol unchanged; relay test over WS.
- 2026-07-18 — Stage 8D2 done: deploy tooling. `deploy/` Ansible playbook (locked-down `netplay` user,
  hardened systemd unit on `127.0.0.1:8000`, `NETPLAY_TOKENS` env) + manual-dispatch `Deploy relay`
  workflow that builds a static `x86_64-musl` binary and runs the playbook via a dedicated CI SSH key
  (GH Secrets). Client bakes in `DEFAULT_RELAY_URL` = `wss://relay.netplay.oliverj.network` (`--online`)
  and reads its shared token from `NETPLAY_TOKEN` env (dev default if unset — secret never baked in);
  `just online` / `just deploy` added. Owner supplies secrets and triggers the workflow.
- 2026-07-18 — Stage 9 increment 1: normalized the wire shape to internally-tagged JSON
  (`#[serde(tag = "type")]`) across `ClientMsg`/`ServerMsg`/`GameMsg`; `Game`/`Error` became struct
  variants (`{payload}`/`{message}`). Flat `{"type":…}` shape pinned by a test. Flag-day break — the
  deployed relay needs a redeploy and clients a rebuild together.
- 2026-07-18 — Stage 9 increment 2: self-describing `/schema` endpoint. `schemars` (behind a
  `schema` feature; client stays lean) generates JSON Schema for the wire types; `service_descriptor()`
  wraps it with metadata. The server grew a minimal `hyper` HTTP/1 front — `GET /schema` returns the
  descriptor, `/` upgrades to WebSocket via `hyper-tungstenite` — replacing the raw `accept_async`
  path. `just schema` fetches it. Tested end-to-end (plain GET + the WS relay over the new front).
- 2026-07-18 — Stage 9 increment 3a: admin queries (dev, no RBAC). New `ListPlayers`/`ListMatches`/
  `GetStats` requests with `Players`/`Matches`/`Stats` replies (in the published schema); the lobby
  actor answers via a oneshot round-trip and now tracks each player's seat + a start `Instant` for
  uptime. The game client ignores admin replies. Tested end-to-end (match two players, a third
  connection queries state).
- 2026-07-18 — Stage 9 increment 3b: admin event stream. `SubscribeEvents` marks a connection
  subscribed; the lobby pushes `PlayerJoined`/`PlayerLeft`/`MatchStarted` to subscribers as they
  happen. Tested end-to-end (subscribe, then observe a join, a match, and a leave).
- 2026-07-18 — Stage 9: `Hello.credential` changed from an opaque byte array to an opaque **JSON
  value** (wire: `"credential":{"key_id":2,"token":"…"}` instead of a byte array). `Authenticator`/
  `AuthProvider` now pass `serde_json::Value`; the relay still never inspects it. Pre-deploy ergonomics
  for non-Rust clients.
- 2026-07-18 — Stage 9: added `GET /asyncapi.json` — the message protocol as a standard **AsyncAPI
  3.0** document (reuses the schemars schemas, subschemas inlined). `just asyncapi` fetches it.
  Also marked the slow eval strength test `#[ignore]` (run with `--ignored` / `just matchup`) so the
  default `cargo test`/CI run is faster.
- 2026-07-18 — Stage 9: cleaned up the AsyncAPI doc — split the two `oneOf` blobs into **one named
  message per variant** (`ClientHello`, `ServerMatched`, …; prefixed since `Game` is on both sides)
  so tooling shows a real catalog instead of anonymous "any" unions, and gave the opaque `credential`
  a *described* schema instead of a bare any.
- 2026-07-19 — Stage 10 increment 1: DB infrastructure. Added `sqlx` + bundled SQLite; a `store`
  module opens `NETPLAY_DB` (default `./netplay.db`), creating it and running embedded migrations on
  startup; first migration `0001_create_users.sql` (`users`: name, password_hash, role). Ansible unit
  gained `StateDirectory=netplay` + `NETPLAY_DB=/var/lib/netplay/netplay.db`. `just migrate-add`;
  local DBs gitignored. No behavior change — the store just exists; auth still shared-token.
- 2026-07-19 — Stage 10 increment 2: accounts + RBAC. `Authenticator` is now async (async-trait);
  `DbAuth` verifies `{name, password}` against the argon2id `users` table (else shared-token
  anonymous player). `Identity` carries a `role`; the relay refuses the admin surface to non-admins.
  `NETPLAY_ADMIN="name:password"` seeds/rotates the admin on boot; added to the deploy env. Fixed a
  test race (subscribe-vs-join) and added `[profile.dev.package.argon2/blake2] opt-level = 3` so the
  otherwise ~1–2 s/hash debug argon2 runs fast in tests/CI (~20 ms).
- 2026-07-19 — Stage 11 increment 1: server accounts-only. `DbAuth` dropped the shared-token
  anonymous fallback; every connection is now a login (`{name,password}`) or registration
  (`{...,register:true}` → `store::create_account`, min 8-char password, unique name). Open
  registration. New `AuthError::{BadLogin,NameTaken,WeakPassword}`; server tests moved to
  register/login. Client still ships the shared token until increment 2 — don't deploy this alone.
- 2026-07-19 — Stage 11 increment 2: client login/register screen. New `Screen::Login` (egui): name +
  masked password, Log in / Create account, inline auth errors; connect fires on submit with the
  `{name,password,register?}` credential. Added keyboard→egui text-input plumbing (winit `KeyEvent` →
  `Event::Text`/editing keys) since we skip egui-winit. Remembers the username (`directories` config).
  `--name` gone; `netplay_client::connect` now takes the credential; `just login-frame` +
  `login_frame` example. `just online`/`play`/`demo` no longer pass a name.
- 2026-07-19 — Stage 11 increment 3: removed the shared-token machinery now that auth is
  accounts-only. Deleted `SharedTokenCredential`/`DEV_*` (protocol), `SharedTokenAuth`
  (server), `AuthProvider`/`SharedToken` (client), the `NETPLAY_TOKENS`/`NETPLAY_TOKEN`
  env + `rotate-token`/`set-token` recipes, and the dead `AuthError::{UnknownKey,BadToken}`.
  `just deploy` drops the `NETPLAY_TOKENS` secret (env template + workflow); DESIGN/deploy docs
  updated. Stage 11 complete.
- 2026-07-19 — Stage 12 increment 1: admin moved off the gameplay WebSocket onto a REST control
  plane on its own host. The server now routes on the requested hostname (proxy's
  `X-Forwarded-Host`, else `Host`): the admin host (`NETPLAY_ADMIN_HOST`, default
  `admin.netplay.oliverj.network`) → REST `admin` module; every other host → game WebSocket, now
  gameplay-only. `POST /admin/login` returns a bearer token (admin accounts only), stored
  sha256-hashed in a new `sessions` table (`0002`, TTL + lazy prune); `GET /admin/{players,matches,
  stats}` are bearer-guarded. Removed the admin `ClientMsg`/`ServerMsg` variants, the lobby
  `Subscribe`/event broadcast, and the entire `schemars` schema surface (`/schema`, `/asyncapi.json`,
  the `schema` feature, `just schema`/`asyncapi`). Added `sha2` dep, `NETPLAY_ADMIN_HOST` to the
  systemd unit. Supersedes Stage 9's WS admin console. SSE `/admin/events` + `/admin/openapi.json`
  land in increments 2–3.
- 2026-07-19 — Stage 12: added `POST /admin/tokens` — a bearer-guarded endpoint that trades a valid
  admin session for a longer-lived one (optional `{days}`, default 30, capped at 90), so a tool (the
  Go TUI) authenticates once and holds a durable token instead of re-sending the password. Generalized
  `store::session_role` → `session_identity` (returns `(user_id, role)`) so a fresh session can be
  minted for the caller's account; `is_admin` became `admin_identity`. Tested end-to-end.
- 2026-07-19 — Stage 12: session validation is now a **pure read** — dropped the inline
  `DELETE` that `session_identity` ran on every lookup (it turned each admin read into a
  table-scanning write; correctness already lived in the `expires_at` filter). Reclaiming expired
  rows moved to an operator action: `store::prune_expired_sessions`, exposed as the
  `netplay-server prune-tokens` subcommand and `just prune-tokens` (`deploy/README` documents the
  on-VM invocation). Chose operator-run over a periodic background sweep to avoid the extra moving part.
- 2026-07-19 — Stage 12 increment 3: `GET /admin/openapi.json` — a hand-written OpenAPI 3.0 document
  (`openapi.rs`) for the admin REST API, served **unauthenticated** (a client can't be asked for a
  token to learn how to get one). Hand-written rather than re-adding the `schemars` derive machinery
  removed in increment 1 — five endpoints don't justify it, and a test asserts every route appears.
  `just openapi` fetches it. Stage 12 complete bar the SSE `/admin/events` increment.
- 2026-07-19 — Stage 13 increment 1: player auth moved to REST. `POST /login`/`/register` on the game
  host (`player` module) mint a session token (`{token, expires_in_hours}`, 24h); the WS `Hello` now
  carries that **token** instead of a name + `{name,password}` credential, and `Authenticator::verify`
  is a `store::session_account` lookup (token → id/role/name) — the display name comes from the account.
  `netplay-protocol` bumped to **v2** (flag-day: `Hello { protocol, token }`). `netplay-client` gained
  an `auth` SDK (`player_login`/`player_register` + `admin_login`/`admin_durable_token`) over blocking
  **ureq** (rustls+ring — avoids reqwest's aws-lc C build) and `login_and_connect` (REST then WS on the
  network thread); the app stopped hand-building credentials. `store::create_account` now returns the
  new row id. Because the protocol change breaks the client, server + protocol + client landed in one
  PR to keep `main` green. Tested: player REST status codes, WS token accept/reject, and a
  `netplay-client` integration test hitting a real relay.
