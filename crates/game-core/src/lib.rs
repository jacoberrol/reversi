//! Pure game core: board, rules, move generation, and (later) search.
//!
//! This crate depends on nothing but `std`. It knows nothing about rendering,
//! windowing, or I/O — that keeps the rules fully testable with `cargo test`.

// Stage 2 fills this in with `Board`, `Cell`, `Player`, `Square`, and rules.
