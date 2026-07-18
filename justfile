# Task runner for the reversi workspace. `just` with no argument lists recipes.
# Commands mirror CLAUDE.md; if you need a new one, add it here rather than
# documenting a manual sequence.

# Show the available recipes when `just` is run with no target.
default:
    @just --list

# Run every test in the workspace.
test:
    cargo test --workspace

# Launch the game (debug build).
run:
    cargo run -p app

# Pre-commit gate: formatting must be clean and clippy must be warning-free.
# `-D warnings` promotes every clippy lint to an error. Must pass before commit.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Headless: play N random self-play games in game-core and print results.
# N defaults to 100 when omitted: `just selfplay` or `just selfplay 1000`.
selfplay N="100":
    cargo run -q -p game-core --example selfplay -- {{N}}

# Render one frame offscreen to target/frame.png, for inspecting visual output.
# Wired to a real offscreen renderer in Stage 4.
frame:
    @echo "frame: not implemented yet (arrives in Stage 4)."

# Rebuild the texture atlas from assets/src/ via the Aseprite CLI.
# Wired to the real Aseprite pipeline in a later stage.
atlas:
    @echo "atlas: not implemented yet (arrives in a later asset-pipeline stage)."
