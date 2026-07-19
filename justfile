# Task runner for the reversi workspace. `just` with no argument lists recipes.
# Commands mirror CLAUDE.md; if you need a new one, add it here rather than
# documenting a manual sequence.

# Show the available recipes when `just` is run with no target.
default:
    @just --list

# Run every test in the workspace.
test:
    cargo test --workspace

# Scaffold a new timestamped SQL migration under the server's migrations/ dir.
# Needs sqlx-cli: cargo install sqlx-cli --no-default-features --features sqlite
migrate-add NAME:
    sqlx migrate add --source crates/netplay-server/migrations {{NAME}}

# Remove all build artifacts (the whole target/, including rust-analyzer's dir).
clean:
    cargo clean

# From-scratch build of the whole workspace (all crates, all targets). Also
# clears stale rust-analyzer state — restart the RA server afterward.
rebuild: clean
    cargo build --workspace --all-targets

# Launch the game vs the AI (debug build).
run:
    cargo run -p app

# Run the relay/matchmaking server (binds host:port; plain WebSocket).
serve ADDR="127.0.0.1:5000":
    cargo run -p netplay-server -- {{ADDR}}

# Launch the game against a specific relay by WebSocket URL (opens the login
# screen). `URL` is ws://host:port locally, or wss://host for a TLS-fronted deploy.
play URL="ws://127.0.0.1:5000":
    cargo run -p app -- --server {{URL}}

# Prompts twice for the password (not echoed) and stores NETPLAY_ADMIN=
# "name:password" in GitHub Secrets. The server seeds/rotates the admin on the
# next `just deploy`. NAME can't contain a colon.
# Set the relay's admin account (name + password) in GitHub Secrets.
set-admin NAME:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v gh >/dev/null || { echo "gh CLI not found (brew install gh)"; exit 1; }
    case "{{NAME}}" in *:*) echo "admin name cannot contain ':'"; exit 1;; esac
    read -r -s -p "password for admin '{{NAME}}': " p1; echo
    read -r -s -p "confirm password: " p2; echo
    [ -n "$p1" ] || { echo "empty password; aborting"; exit 1; }
    [ "$p1" = "$p2" ] || { echo "passwords differ; aborting"; exit 1; }
    printf '%s:%s' "{{NAME}}" "$p1" | gh secret set NETPLAY_ADMIN
    echo "set NETPLAY_ADMIN for admin '{{NAME}}'"
    echo "next: 'just deploy' to seed/rotate the admin on the relay"

# Play against the public relay (baked-in wss:// URL). Opens the login screen —
# log in or create an account. No local server needed.
online:
    cargo run -p app -- --online

# Deploy the relay to the exe.dev VM (manual GitHub Actions workflow).
# Requires the DEPLOY_SSH_KEY and NETPLAY_ADMIN repo secrets — see deploy/README.md.
deploy:
    gh workflow run "Deploy relay"

# Fetch the relay's self-describing wire contract (JSON Schema + metadata).
# Pretty-prints with jq if present. URL defaults to the public relay.
schema URL="https://relay.netplay.oliverj.network":
    curl -fsS {{URL}}/schema | { jq . 2>/dev/null || cat; }

# Fetch the relay's AsyncAPI 3.0 document (the standard WebSocket message spec).
asyncapi URL="https://relay.netplay.oliverj.network":
    curl -fsS {{URL}}/asyncapi.json | { jq . 2>/dev/null || cat; }

# Stops the server automatically when both windows close (or on Ctrl-C). Uses
# port 5099 to avoid clashing with a manual `just serve`. Each window shows the
# login screen — create two accounts (e.g. Alice / Bob), then invite + accept.
# One-shot local multiplayer test: a relay plus two game windows.
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
    echo "opening two windows against ${addr} — register two accounts to play"
    ./target/debug/app --server "ws://${addr}" &
    one=$!
    ./target/debug/app --server "ws://${addr}" &
    two=$!
    wait "${one}" "${two}"

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

# Render the login screen offscreen to target/login.png.
login-frame:
    cargo run -q -p app --example login_frame

# Rebuild the texture atlas from assets/src/ via the Aseprite CLI.
# Wired to the real Aseprite pipeline in a later stage.
atlas:
    @echo "atlas: not implemented yet (arrives in a later asset-pipeline stage)."
