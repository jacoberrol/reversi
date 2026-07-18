# Puzzle Game — Design & Architecture Decisions

> Living document. Started from initial planning session, July 2026.
> Purpose: capture the decisions made so far, the reasoning behind them, and the open questions.

## 1. Project intent

- A 2D, puzzle-forward game. Gameplay concept TBD (Othello/Reversi-like board mechanics are one candidate direction).
- **This is primarily a learning vehicle.** The goal is to understand the full stack: modern GPU rendering, the Apple Silicon architecture (unified memory, GPU, Neural Engine), on-device ML, and an AI-assisted asset pipeline. Decisions below are biased toward "learn how it works" over "ship fastest."
- Developed in VS Code, with Claude Code as the agentic assistant.

## 2. Platform targets

| Target | Priority | Notes |
|---|---|---|
| macOS (Apple Silicon, 36GB) | Primary / dev machine | Desktop-first development: fastest iteration, working debuggers |
| iOS | Later port | wgpu runs on the same Metal backend as macOS; low-friction port |
| Android | Later port | wgpu via Vulkan (GLES fallback); expect winit lifecycle/surface-loss work |

Mobile porting notes for later: `aarch64-apple-ios` target + Xcode signing for iOS; NDK + `cargo-ndk` / `cargo-mobile2` / `xbuild` for Android. Design for touch input from day one (puzzle games suit this). Escape hatch if raw plumbing stops being fun: port to Bevy (same wgpu underneath, mobile problems pre-solved).

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

- **In-game ML (portable):** small models — puzzle difficulty evaluator, level generator, policy net for an opponent — implemented via `burn` on the wgpu backend. Ships to all platforms with the game.
- **ANE / Core ML:** treated as an **optional side experiment**, not a load-bearing dependency. If pursued: a small self-contained Swift or Python sidecar. (Android has no ANE; its equivalent is NNAPI/LiteRT — different stack.)
- **MLX:** for local training experiments on the Mac, including possible LoRA training (see §6). Python-first; lives outside the game binary.

## 6. Asset pipeline (2D sprites)

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
- **Programmer art first.** Solid-color quads until the puzzle is fun; the atlas/batcher abstraction absorbs real art later without code changes.
- Build the raw winit/wgpu plumbing to learn it; Bevy remains the acknowledged escape hatch.
- Keep the ANE/Core ML work quarantined from the core game.

## 8. Open questions

- [ ] The actual puzzle mechanic (Othello-like is a candidate, not a commitment)
- [ ] Art direction: pixel art vs. HD/vector-ish procedural look (affects LoRA choice and atlas resolution)
- [ ] ECS or plain structs for game state? (puzzle scope may not need an ECS)
- [ ] Audio stack (e.g. `kira`, `rodio`) — undecided
- [ ] What the first ML experiment should be (difficulty evaluator vs. level generator)
- [ ] Distribution/licensing check before shipping any Flux.1 Dev-derived asset