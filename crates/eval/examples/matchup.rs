//! `just matchup [DEEP] [SHALLOW] [GAMES]` — play a depth-vs-depth match and
//! print the results, so you can watch strength grow with search depth.

use eval::matchup::play_match;

const MATCH_SEED: u64 = 0xA1B2_C3D4_E5F6_0718;

fn main() {
    let mut args = std::env::args().skip(1);
    let deep: u32 = parse_or(args.next(), 3);
    let shallow: u32 = parse_or(args.next(), 1);
    let games: usize = parse_or(args.next(), 50);

    let result = play_match(deep, shallow, games, MATCH_SEED);
    let decisive = result.deep_wins + result.shallow_wins;

    println!("depth {deep} vs depth {shallow} over {games} games:");
    println!("  deeper wins:    {}", result.deep_wins);
    println!("  shallower wins: {}", result.shallow_wins);
    println!("  draws:          {}", result.draws);
    if decisive > 0 {
        let rate = 100.0 * f64::from(result.deep_wins) / f64::from(decisive);
        println!("  deeper win rate (decisive games): {rate:.0}%");
    }
}

fn parse_or<T: std::str::FromStr>(arg: Option<String>, default: T) -> T {
    arg.and_then(|s| s.parse().ok()).unwrap_or(default)
}
