# Reversi — Design & Architecture Decisions

> Living document. Started from initial planning session, July 2026.
> Purpose: capture the decisions made so far, the reasoning behind them, and the open questions.

## 1. Project intent

- **The game is Reversi (Othello).** A 2D, two-player abstract-strategy board game on an 8×8 grid: players place discs to flank lines of the opponent's discs and flip them; when neither side has a legal move the game ends and the player with more discs wins. The initial target is single-player vs. an AI opponent, where search depth is the difficulty setting. (This was previously open; it is now a commitment — see §8.)
- **This is primarily a learning vehicle.** The goal is to understand the full stack: modern GPU rendering, the Apple Silicon architecture (unified memory, GPU, Neural Engine), on-device ML, and an AI-assisted asset pipeline. Decisions below are biased toward "learn how it works" over "ship fastest."
- Developed in VS Code, with Claude Code as the agentic assistant.

## 2. Platform targets

| Target | Priority | Notes |
|---|---|---|
| macOS (Apple Silicon, 36GB) | Primary / dev machine | Desktop-first development: fastest iteration, working debuggers |
| iOS | Later port | wgpu runs on the same Metal backend as macOS; low-friction port |
| Android | Later port | wgpu via Vulkan (GLES fallback); expect winit lifecycle/surface-loss work |

Mobile porting notes for later: `aarch64-apple-ios` target + Xcode signing for iOS; NDK + `cargo-ndk` / `cargo-mobile2` / `xbuild` for Android. Design for touch input from day one (turn-based board games suit this). Escape hatch if raw plumbing stops being fun: port to Bevy (same wgpu underneath, mobile problems pre-solved).

## 3. Language & graphics stack

**Decision: Rust + wgpu + winit.**

Reasoning and alternatives considered:

- **OpenGL — rejected.** Deprecated on macOS, frozen at 4.1 (no compute shaders). Fine for 2D but a museum piece on this platform.
- **Metal directly — considered, not chosen.** Best for learning Apple-native machinery, but Mac/iOS-only. wgpu is ~90% conceptually identical (command encoders, pipeline states, bind groups) and its macOS/iOS backend *is* Metal, so most of the learning transfers anyway.
- **wgpu — chosen.** Modern explicit API (WebGPU model), mature Metal backend, portable to Windows/Linux/web/Android. Best-in-class learning resources ("Learn WGPU"). Bonus symmetry: the `burn` ML framework can use wgpu as a compute backend — one API for rendering *and* ML.
- **Python + wgpu-py + MLX — runner-up.** Fastest ML iteration loop; kept in mind for side experiments, not the main game.

Tradeoff accepted: wgpu abstracts away some Apple-specific unified-memory tricks, and the Neural Engine is impractical to reach from Rust (see §5).

Core crates (initial): `wgpu`, `winit`, `image` (PNG decode), `asefile` (load Aseprite files directly), `serde_json` (atlas metadata). ML later: `burn` (wgpu backend).

## 4. Hardware notes (Apple Silicon)

- **Unified memory:** CPU, GPU, and ANE share one RAM pool — no PCIe copies. With 36GB there is no fixed VRAM limit; the GPU claims memory dynamically. "Needs a 24GB card" guides generally translate to "fine."
- **CPU:** game logic, main loop. **GPU:** rendering + ML training/inference (MLX, burn). **Neural Engine (ANE):** power-efficient inference only, reachable *only* via Core ML — cannot be programmed directly.
- ANE reality check: best for small/medium models running continuously; not for training, not for LLM-scale models. Any puzzle-sized model runs fine on GPU or CPU.

## 5. ML plans

- **In-game ML (portable):** small models — a learned position evaluator and/or an opponent policy net — implemented via `burn` on the wgpu backend, slotting in behind the same evaluator trait as the handcrafted heuristic. Ships to all platforms with the game. (Difficulty is search depth, so a "difficulty model" isn't needed; Reversi has no procedural levels, so the earlier level-generator idea is dropped.)
- **ANE / Core ML:** treated as an **optional side experiment**, not a load-bearing dependency. If pursued: a small self-contained Swift or Python sidecar. (Android has no ANE; its equivalent is NNAPI/LiteRT — different stack.)
- **MLX:** for local training experiments on the Mac, including possible LoRA training (see §6). Python-first; lives outside the game binary.

## 6. Asset pipeline (2D sprites)

> **Deferred past the first iteration.** v1 ships with **procedural graphics only**
> (solid quads + shader-drawn discs — see §7 and §8). This whole sprite-generation
> pipeline is a later-iteration learning goal, not v1 work; it stays documented here
> so the batcher/atlas abstraction is designed to absorb it, but nothing below is built
> until the game is fun with procedural art.

Five stages, all local:

1. **Generate** — Draw Things (free, runs Core ML/Metal on-device — the asset pipeline itself exercises the hardware).
   - Base model: **SDXL Base v1.0 (full precision — the listing without an "(8-bit)" suffix)** + a pixel-art LoRA from Civitai.
   - Sampler DPM++ SDE Karras, ~20–25 steps, generate at 1024×1024 (downscaling hides noise).
   - **Flux.1 Dev** (Q6+) for prompts needing better adherence — ⚠ non-commercial license; use **Flux.1 Schnell** (Apache 2.0) for anything that ships. **Z Image Turbo** for fast drafts. **Fooocus Inpaint SDXL** later, for surgical fixes to near-miss sprites.
   - Skip SDXL Refiner (pushes toward photoreal smoothness — wrong for pixel art).
   - Longer term: train a LoRA on our own curated sprites (Draw Things on-device training; 768×768, network dim 32 is comfortable at 36GB) to lock style consistency.
2. **Clean up / edit** — **Aseprite** (or Pixelorama/LibreSprite free alternatives). Mandatory post-pass on AI output: nearest-neighbor downscale (e.g. 8x) + quantize to an indexed palette to get truly on-grid pixel art.
3. **Animate** — Aseprite timeline/tags/onion-skinning. Puzzle scope keeps this light (tile glow, merge, settle effects).
4. **Pack** — Aseprite CLI export to texture atlas: `aseprite -b sprites.ase --sheet atlas.png --data atlas.json` (scriptable in the build). Dedicated packers (`crunch`, TexturePacker) only if asset count balloons.
5. **Load** — `asefile` for native `.ase` loading during development; atlas PNG + JSON for packaged builds. Renderer maps atlas UV rects onto instanced quads via the sprite batcher.

### Prompting lessons learned (diffusion)
- Describe only what should be **in the frame** — never the asset's intended use ("...to be placed on a game board" summons a board).
- Diffusion cannot output transparency. Generate on a solid contrasting background (e.g. white piece on dark green); knock out in post (`rembg`, scriptable; or Preview's Remove Background).
- Caption-style comma phrasing beats instructions ("a single white reversi stone, one round glossy disc, centered, isolated object, ..." not "create a...").
- Use the negative prompt (e.g. "game board, grid, multiple pieces, pattern, text, blurry").
- For matched variants (white/black piece), use image-to-image at ~50–60% strength, not fresh prompts — consistency beats individual perfection.

### Procedural assets
Simple geometric pieces (e.g. a glossy game disc) are ~15 lines of fragment shader (SDF circle + radial highlight + rim shadow) and scale/recolor/animate for free. **Decision: game pieces will be procedural**; the diffusion pipeline is reserved for tiles, backgrounds, and decorative art where it earns its keep.

### Automation endgame
Draw Things exposes JavaScript batch automation and an MCP server usable from Claude Code → asset generation (generate → knockout → downscale/quantize → atlas) can become a scripted build step.

## 7. Development principles

- **Desktop-first**, port to mobile after the game exists.
- **Programmer art first.** Solid-color quads until the game is fun; the atlas/batcher abstraction absorbs real art later without code changes.
- Build the raw winit/wgpu plumbing to learn it; Bevy remains the acknowledged escape hatch.
- Keep the ANE/Core ML work quarantined from the core game.

## 8. Decisions & open questions

### Decided
- **Game mechanic: Reversi (Othello), 8×8.** Previously the biggest open question; now committed (see §1).
- **Game state: plain structs, no ECS.** Reversi's fixed 8×8 board carries too little entity variety to justify an ECS; plain structs keep `game-core` dependency-free and trivially unit-testable. (No Bevy either — see §7.)
- **First iteration: procedural graphics only.** Solid-color quads and shader-drawn discs; the diffusion/Aseprite sprite pipeline (§6) is deferred until the game is fun. The atlas/batcher abstraction is still built so real art drops in later without code changes.
- **Input handled at one seam in `app`, for portability.** All platform pointer events enter through `WindowState::{set_cursor, mouse_button}`; from there in-game clicks hit-test to a `Square` via board geometry exposed by `render` (the inverse of the draw layout), and lobby clicks feed egui. `game-core` only ever receives a `Square` and stays input-agnostic. Net effect: the macOS→iOS port is confined to that thin seam (winit `MouseInput`/`CursorMoved` today, `Touch` later) — rules, eval, and rendering are untouched. (Honors the "touch from day one" intent in §2. Earlier a dedicated `PointerInput` type held this seam; the egui integration folded it into `WindowState`.)

### Still open
- [ ] Art direction: pixel art vs. HD/vector-ish procedural look (affects LoRA choice and atlas resolution)
- [ ] Audio stack (e.g. `kira`, `rodio`) — undecided
- [ ] First ML experiment: a learned evaluator to augment/replace the handcrafted heuristic, vs. an opponent policy net
- [ ] Distribution/licensing check before shipping any Flux.1 Dev-derived asset

## 9. Networking & multiplayer

North star: **named users discover each other and play over the internet** via a small
Rust server on a cloud VM. We build the real topology now and stage toward that.

- **Relay topology, not direct peer-to-peer.** Both clients dial *out* to a server, which
  pairs them and forwards their game messages. This sidesteps NAT (no hole-punching) and
  means the game-session protocol is identical on LAN and over the internet — direct P2P
  pairing would have been thrown away. (Increment 1 runs the server on localhost.)
- **Deterministic sync via move exchange.** Each client applies moves to its own pure,
  deterministic `game-core`; the server relays messages opaquely. Ordered TCP + determinism
  ⇒ boards can't drift. Forced passes are derived locally on both sides (never sent).
- **A shared `protocol` crate (serde).** One source of truth for the wire format, reused by
  the client and the server. Primitive fields only (a move is a `u8`), so `game-core` never
  gains a serialization dependency and the server never depends on game logic.
- **Internally-tagged JSON (`#[serde(tag = "type")]`).** Every message serializes as a flat
  object with a `"type"` discriminator (`{"type":"Invite","to":3}`) — the conventional
  tagged-union shape a non-Rust client expects, chosen so an admin tool (a Go TUI) can consume
  the protocol without matching serde's default externally-tagged encoding. This forced the
  two odd variants (`Game`, `Error`) from newtype into struct variants (`Game { payload }`,
  `Error { message }`), since internal tagging can't wrap a bare array/string. We kept JSON
  (not protobuf): it's human-readable in logs and we own both ends.
- **Control plane (REST) vs data plane (WebSocket), split by host (Stage 12).** The server runs a
  minimal `hyper` HTTP/1 front per connection and routes on the requested hostname (the exe.dev proxy
  forwards it as `X-Forwarded-Host`, falling back to `Host`): the admin host
  (`admin.netplay.oliverj.network`, configurable via `NETPLAY_ADMIN_HOST`) serves the **REST admin
  API**; every other host is the **game** — a WebSocket upgrade (`hyper-tungstenite`) handed to the
  relay. Both hostnames resolve to the same IP:port and the proxy forwards both, so the split needs
  no extra proxy listener. The WebSocket now carries *only* gameplay (`Hello`/`Invite`/`Accept`/
  `Decline`/`Game` in, presence/match/game out) — no admin messages ride it.
  - **Auth: bearer sessions.** `POST /admin/login` takes `{name, password}`, verifies the account
    (must be an admin), and returns an opaque bearer token; later admin requests carry
    `Authorization: Bearer <token>`. Tokens are 256-bit random, stored **sha256-hashed** in a
    `sessions` table (never raw), with a TTL. (argon2 is for the low-entropy passwords; a
    high-entropy token needs only a fast hash.) **Validation is a pure read** — the lookup filters
    on `expires_at`, so an expired token is never honored, but the dead row is left in place.
    Reclaiming expired rows is an **operator action** (`netplay-server prune-tokens` / `just
    prune-tokens`), deliberately not run on a timer or on every request: an earlier version pruned
    inside the lookup, which turned every admin read into a table-scanning write — the wrong shape,
    even if negligible at this scale.
  - **Two token lifetimes.** A login token is short (one work session, 24 h). `POST /admin/tokens`
    (bearer-guarded, optional `{days}`, default 30, capped at 90) mints a **durable** token — a
    fresh session for the same account — so a tool (the Go TUI) authenticates once with the password
    and then holds a weeks-long token across restarts, never caching the password. Both are ordinary
    session rows; a durable token is just one with a longer TTL, so the same lazy-prune expiry applies.
  - **Endpoints:** `GET /admin/players`, `/admin/matches`, `/admin/stats` (all bearer-guarded), and
    `GET /admin/openapi.json` (**unauthenticated** — a client can't be asked for a token to learn how
    to get one, and the doc holds no secrets) for discovery. We dropped the earlier WS-side `/schema`
    + `/asyncapi.json` docs: with the control surface now genuinely REST, OpenAPI is the fitting
    standard. The OpenAPI document is **hand-written** (`openapi.rs`), not derived — five endpoints
    don't justify re-adding the `schemars`/derive machinery we removed, and a literal sits next to the
    routes it describes. Kept in sync with the routes and `netplay-protocol` types by hand (a test
    asserts every route appears).
- **The winit loop stays synchronous; networking uses a runtime off to the side.** The relay
  (`netplay-server`) uses tokio (per-connection tasks + an in-memory lobby actor). The client
  runs its WebSocket on a **single-thread tokio runtime confined to a dedicated network thread**,
  feeding the winit loop via `EventLoopProxy` and reading an outgoing channel — the runtime never
  touches the main loop. (Originally the client was fully runtime-free over blocking TCP; the
  WebSocket-over-TLS transport made a background runtime the clean choice, so we revised that
  decision — the *intent*, "don't entangle the winit loop with async," still holds.)
- **Transport is swappable behind the connection seam; now WebSocket (Stage 8D1).** Messages are
  serialized JSON, delimited by WebSocket. `ws://` on localhost; **`wss://` in production, with TLS
  terminated by the VPS proxy** which forwards to a plain-`ws://` port on the VM. `--server` is a
  URL. WebSocket, not gRPC: it traverses firewalls over 443 and doesn't force a heavier stack for
  two Rust peers swapping tiny messages.

### Reusable netplay layer (extracted, Stage 8A)
The relay/lobby/transport is a **game-agnostic layer** any 2-player turn-based game in the workspace
can use, split into `netplay-protocol` / `netplay-server` / `netplay-client`. The boundary:
- **Opaque game payload.** The envelope (`Hello`/`Invite`/`Accept`/`Decline`/`Presence`/`Matched`/…)
  is generic; the in-game action rides as `Game(Vec<u8>)` the server never decodes. Reversi defines
  `GameMsg` in `app` and (de)serializes it into that payload. (Rejected: a `ClientMsg<P>` generic —
  leaks generics through a server that never inspects `P`.)
- **`Seat`, not `Color`.** Matches carry `Seat(u8)` (seat 0 moves first); the game maps seat → its
  player type (Reversi: seat 0 = Black). Keeps the relay game-agnostic.
- Stays a workspace-internal crate (no separate repo / published crate until a second consumer
  justifies the versioning overhead).
- **Auth is a seam (Stage 8B).** `Hello` carries an opaque credential — **arbitrary JSON** the
  authenticator interprets (`{name, password[, register]}` for accounts), not a byte blob, so a Go
  client sends a normal object. The server's `Authenticator::verify` runs before the client joins the
  lobby; the relay never inspects the credential, so the shape can change (attestation) without
  touching the envelope. The implementation is `DbAuth` — accounts-only (Stage 10/11 below); an
  attestation authenticator could swap in behind the unchanged trait later.
- **Rate limiting (Stage 8C, done).** Server-side, before the lobby, drop-and-log: a handshake
  timeout, per-IP concurrency + new-connection rate, a per-connection inbound message bucket, and a
  lobby player cap. Tunable consts in `netplay-server::limits`; auth and rate-limit are separate
  seams applied in sequence.
- **Deploy (Stage 8D2, done).** The relay runs on an exe.dev VM under `systemd`, bound to
  `127.0.0.1:8000`; the provider's proxy terminates TLS at `relay.netplay.oliverj.network` and
  forwards to it. The client bakes that `wss://` URL in as `DEFAULT_RELAY_URL` (`--online`);
  `--server URL` overrides for local dev. Clients log in / register on the title screen — accounts-only,
  no shared secret in the client (Stage 11). Deploys are
  **Ansible driven by a manual GitHub Actions
  workflow** (`deploy/`): CI builds a static `x86_64-unknown-linux-musl` binary (no toolchain on the
  VM, no glibc coupling) and the playbook installs it under a locked-down system user with a hardened
  unit; the `NETPLAY_ADMIN` secret and a dedicated deploy SSH key live in GitHub Secrets. Chosen
  over building on the VM (keeps the box minimal) and over a push-triggered deploy (production is
  hand-triggered). See `deploy/README.md`.

### Persistence: SQLite for accounts (Stage 10 — the stateless-relay inflection)
The relay was in-memory only; **durable identity is where it grows a database** (flagged as the
biggest inflection above). We add **SQLite via `sqlx`** (async, tokio-native), holding a `users`
table (name, `password_hash`, `role`). Account secrets are **argon2id** PHC strings (per-hash salt
embedded — the root admin uses a human password, so a slow salted hash is required, not sha256).
This backs **accounts + RBAC**: `Identity` gains a `role`, and
the admin surface is gated on `role == admin` (closing the deferred RBAC item). **Stage 11 goes
accounts-only** — the shared-token/anonymous path is removed and every client logs in or
self-registers (open registration; min 8-char password; argon2id). This reverses the earlier
shared-token deterrence gate and anonymous-play decisions deliberately. Decisions:
- **`sqlx` + bundled SQLite**, not `rusqlite`/`diesel`: async fits the tokio server without
  `spawn_blocking`, and bundling the SQLite C source keeps the static-musl deploy build free of a
  system `libsqlite3` (musl-tools already present handles the C compile).
- **Migrations embedded + auto-run on startup** (`sqlx::migrate!` over `migrations/`), tracked in
  `_sqlx_migrations`, so a deploy stays one step (the server migrates itself). Runtime query API (no
  compile-time DB), so CI needs no database.
- **State lives in systemd's `StateDirectory=netplay`** (`/var/lib/netplay`, writable under
  `ProtectSystem=strict`); the DB path is `NETPLAY_DB`. Since accounts are server infrastructure the
  store is a `netplay-server` module — this does make the "reusable relay" stateful, recorded here
  rather than left to drift.
- Because the `Hello` credential is **opaque JSON**, moving auth from `{key_id, token}` to account
  credentials needs no protocol/wire change — only the authenticator's interpretation changes.

### UI: egui for menus/lobby (decided)
On-screen text and the lobby use **egui** (`egui` + `egui-wgpu`, on our wgpu 0.20). We evaluated
hand-rolling a bitmap-font + custom widgets (fits the "build the plumbing" ethos, unstubs textures)
vs. egui, rendered a themed mockup of each, and chose egui: richer, faster, and — themed (custom
`Visuals`, rounded `Frame`s, no `Window` chrome, no default gray) — it reads as a game menu, not a
debug panel. It draws on its own `egui-wgpu` pass over the surface; the board stays on our custom
renderer. We deliberately skip `egui-winit` (it pins winit 0.29, conflicting with our 0.30) and
hand-feed pointer input to egui instead. If the look ever grates, the lobby sits behind our own
screen state so a custom UI could replace it.

### Staged
- ✅ Increment 2: named presence + invite lobby (egui). Auto-match replaced by presence + invites.
- ✅ Deploy to a cloud VM (Stage 8D): WebSocket transport + TLS-fronted relay on exe.dev.
- ✅ Accounts + RBAC on SQLite (Stage 10): argon2id accounts; admin surface gated by role.
- ✅ Accounts-only login/register on the title screen (Stage 11): shared token removed.
- Admin REST control plane on its own host, bearer sessions + OpenAPI discovery (Stage 12): SSE events to follow.
- Out of scope for now: reconnect, spectating, NAT traversal.