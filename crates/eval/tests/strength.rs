//! Deeper search should beat shallower search over a fixed-seed match.
//!
//! Search is deterministic, so games are diversified by a few random opening
//! plies (seeded) and by alternating which colour the deeper engine plays. The
//! whole match is reproducible from the fixed seed below.

use eval::matchup::play_match;

const MATCH_SEED: u64 = 0xA1B2_C3D4_E5F6_0718;

#[test]
fn deeper_search_beats_shallower() {
    let result = play_match(3, 1, 50, MATCH_SEED);
    println!(
        "depth 3 vs depth 1 over 50 games: deep {} - {} shallow, {} draws",
        result.deep_wins, result.shallow_wins, result.draws
    );

    assert!(
        result.deep_wins > result.shallow_wins,
        "deeper search should win more games: deep={} shallow={} draws={}",
        result.deep_wins,
        result.shallow_wins,
        result.draws
    );
    // Not just ahead, but clearly dominant — guards against a lucky near-tie.
    assert!(
        result.deep_wins >= 33,
        "deeper search should dominate, got only {} wins",
        result.deep_wins
    );
}
