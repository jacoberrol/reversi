//! `just selfplay N` — play N random games and print a summary.
//!
//! Headless and dependency-free: exercises the rules end to end and is a quick
//! smoke test that nothing panics over many full games.

use game_core::selfplay::random_playout;
use game_core::{Outcome, Player};

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let mut black_wins = 0u64;
    let mut white_wins = 0u64;
    let mut draws = 0u64;
    let mut total_plies = 0u64;

    for i in 0..n {
        // Spread seeds apart so consecutive games differ, yet stay reproducible.
        let seed = 0x2545_F491_4F6C_DD1D ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let result = random_playout(seed);
        match result.outcome {
            Outcome::Win(Player::Black) => black_wins += 1,
            Outcome::Win(Player::White) => white_wins += 1,
            Outcome::Draw => draws += 1,
        }
        total_plies += u64::from(result.plies);
    }

    println!("games:      {n}");
    println!("black wins: {black_wins}");
    println!("white wins: {white_wins}");
    println!("draws:      {draws}");
    if n > 0 {
        println!("avg plies:  {:.1}", total_plies as f64 / n as f64);
    }
}
