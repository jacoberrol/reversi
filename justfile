# Task runner for the reversi workspace. `just` with no argument lists recipes.
# Commands mirror CLAUDE.md; if you need a new one, add it here rather than
# documenting a manual sequence.

# Show the available recipes when `just` is run with no target.
default:
    @just --list

# Run every test in the workspace.
test:
    cargo test --workspace

# Launch the game vs the AI (debug build).
run:
    cargo run -p app

# Run the relay/matchmaking server (binds host:port; plain WebSocket).
serve ADDR="127.0.0.1:5000":
    cargo run -p netplay-server -- {{ADDR}}

# Launch the game against a specific relay by WebSocket URL.
# `URL` is ws://host:port locally, or wss://host for a TLS-fronted deploy.
play URL="ws://127.0.0.1:5000" NAME="Player":
    cargo run -p app -- --server {{URL}} --name {{NAME}}

# Prompts for a token (no echo) and stores it in the macOS login Keychain.
# Owners mint tokens with `just rotate-token` instead.
# Store a token to JOIN a relay someone else runs.
set-token:
    security add-generic-password -U -a "$USER" -s netplay-token -w
    @echo "stored 'netplay-token' in your login keychain"

# Stores it in BOTH the macOS Keychain (for `just online`) and the
# NETPLAY_TOKENS GitHub secret (for the server). KEY_ID is any small integer
# (avoid the dev id 1). Run `just deploy` after so the server picks it up.
# Owner: mint a fresh relay token into the Keychain + GitHub secret.
rotate-token KEY_ID="2":
    #!/usr/bin/env bash
    set -euo pipefail
    command -v gh >/dev/null || { echo "gh CLI not found (brew install gh)"; exit 1; }
    token="{{KEY_ID}}:$(openssl rand -hex 32)"
    security add-generic-password -U -a "$USER" -s netplay-token -w "$token"
    printf '%s' "$token" | gh secret set NETPLAY_TOKENS
    echo "rotated: new token in Keychain + GitHub secret NETPLAY_TOKENS"
    echo "next: 'just deploy' to apply it on the relay (the old token works until then)"

# The token comes from an already-set NETPLAY_TOKEN, else the Keychain (see
# `just set-token`), else the dev default (which the deployed relay rejects).
# Play against the public relay (baked-in wss:// URL) — no local server needed.
online NAME="Player":
    NETPLAY_TOKEN="${NETPLAY_TOKEN:-$(security find-generic-password -s netplay-token -w 2>/dev/null || true)}" \
      cargo run -p app -- --online --name {{NAME}}

# Deploy the relay to the exe.dev VM (manual GitHub Actions workflow).
# Requires the DEPLOY_SSH_KEY and NETPLAY_TOKENS repo secrets — see deploy/README.md.
deploy:
    gh workflow run "Deploy relay"

# Fetch the relay's self-describing wire contract (JSON Schema + metadata).
# Pretty-prints with jq if present. URL defaults to the public relay.
schema URL="https://relay.netplay.oliverj.network":
    curl -fsS {{URL}}/schema | { jq . 2>/dev/null || cat; }

# Stops the server automatically when both windows close (or on Ctrl-C). Uses
# port 5099 to avoid clashing with a manual `just serve`. In one window click
# Invite next to the other player; in the other, click Accept.
# One-shot local multiplayer test: a relay plus two lobby windows.
demo:
    #!/usr/bin/env bash
    set -euo pipefail
    addr="127.0.0.1:5099"
    echo "building..."
    cargo build -q -p netplay-server -p app
    ./target/debug/netplay-server "${addr}" &
    server_pid=$!
    trap 'kill "${server_pid}" 2>/dev/null || true' EXIT
    sleep 1
    echo "opening two windows (Alice, Bob) against ${addr}"
    ./target/debug/app --server "ws://${addr}" --name Alice &
    alice=$!
    ./target/debug/app --server "ws://${addr}" --name Bob &
    bob=$!
    wait "${alice}" "${bob}"

# Pre-commit gate: formatting must be clean and clippy must be warning-free.
# `-D warnings` promotes every clippy lint to an error. Must pass before commit.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Headless: play N random self-play games in game-core and print results.
# N defaults to 100 when omitted: `just selfplay` or `just selfplay 1000`.
selfplay N="100":
    cargo run -q -p game-core --example selfplay -- {{N}}

# Play a depth-vs-depth AI match and print results (deeper should win).
# Defaults: depth 3 vs depth 1 over 50 games. `just matchup 4 2 20` to vary.
matchup DEEP="3" SHALLOW="1" GAMES="50":
    cargo run -q -p eval --example matchup --release -- {{DEEP}} {{SHALLOW}} {{GAMES}}

# Render one frame offscreen to target/frame.png, for inspecting visual output.
frame:
    cargo run -q -p render --example frame

# Render the egui lobby mockup offscreen to target/lobby.png (look-and-feel spike).
lobby-frame:
    cargo run -q -p app --example lobby_frame

# Rebuild the texture atlas from assets/src/ via the Aseprite CLI.
# Wired to the real Aseprite pipeline in a later stage.
atlas:
    @echo "atlas: not implemented yet (arrives in a later asset-pipeline stage)."
