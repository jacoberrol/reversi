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

# Run the relay/matchmaking server (localhost:5000 by default).
serve ADDR="127.0.0.1:5000":
    cargo run -p netplay-server -- {{ADDR}}

# Launch the game in online mode, connecting to a relay server.
# Two of these (with the same ADDR) auto-match and play each other.
play ADDR="127.0.0.1:5000" NAME="Player":
    cargo run -p app -- --server {{ADDR}} --name {{NAME}}

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
    ./target/debug/app --server "${addr}" --name Alice &
    alice=$!
    ./target/debug/app --server "${addr}" --name Bob &
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
