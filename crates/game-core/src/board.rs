//! The board and the rules of Reversi.

use std::cmp::Ordering;

use crate::{Cell, Player, Square, BOARD_SIZE, NUM_SQUARES};

/// The eight directions a line of discs can run, as `(d_row, d_col)`.
const DIRECTIONS: [(i8, i8); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// The result of a finished game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Win(Player),
    Draw,
}

/// An 8x8 Reversi position: what sits on every square, plus whose turn it is.
///
/// A `Board` is immutable in the ways that matter: [`apply`](Board::apply) and
/// [`pass`](Board::pass) return a *new* board rather than mutating in place,
/// which keeps them convenient for search (Stage 3) where positions are
/// explored and discarded freely.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    cells: [Cell; NUM_SQUARES],
    to_move: Player,
}

impl Board {
    /// The standard Reversi opening: four discs in the centre, Black to move.
    pub fn new() -> Board {
        let mut cells = [Cell::Empty; NUM_SQUARES];
        // Centre four squares, arranged on the two diagonals.
        cells[idx(3, 3)] = Cell::Disc(Player::White);
        cells[idx(3, 4)] = Cell::Disc(Player::Black);
        cells[idx(4, 3)] = Cell::Disc(Player::Black);
        cells[idx(4, 4)] = Cell::Disc(Player::White);
        Board {
            cells,
            to_move: Player::Black,
        }
    }

    /// Build an arbitrary position from explicit disc placements.
    ///
    /// Handy for tests and for setting up specific scenarios. If a square is
    /// listed under both colours, white wins (it is written last); no square is
    /// out of range because [`Square`] is always valid.
    pub fn from_discs(black: &[Square], white: &[Square], to_move: Player) -> Board {
        let mut cells = [Cell::Empty; NUM_SQUARES];
        for &sq in black {
            cells[sq.index()] = Cell::Disc(Player::Black);
        }
        for &sq in white {
            cells[sq.index()] = Cell::Disc(Player::White);
        }
        Board { cells, to_move }
    }

    /// Whose turn it is.
    pub fn to_move(&self) -> Player {
        self.to_move
    }

    /// The contents of a square.
    pub fn cell(&self, sq: Square) -> Cell {
        self.cells[sq.index()]
    }

    /// Number of discs the given player has on the board.
    pub fn count(&self, player: Player) -> u32 {
        self.cells
            .iter()
            .filter(|&&c| c == Cell::Disc(player))
            .count() as u32
    }

    /// Number of empty squares remaining.
    pub fn empty_count(&self) -> u32 {
        self.cells.iter().filter(|&&c| c == Cell::Empty).count() as u32
    }

    /// Is the given square a legal move for the side to move?
    pub fn is_legal(&self, sq: Square) -> bool {
        !self
            .flips_at(sq.row() as i8, sq.col() as i8, self.to_move)
            .is_empty()
    }

    /// All legal moves for the side to move, in flat-index order.
    pub fn legal_moves(&self) -> Vec<Square> {
        self.moves_for(self.to_move)
    }

    /// Play `sq` for the side to move, returning the resulting position.
    ///
    /// Returns `None` if `sq` is not a legal move (occupied, or flips nothing).
    /// The turn passes to the opponent; callers handle forced passes explicitly
    /// via [`pass`](Board::pass).
    pub fn apply(&self, sq: Square) -> Option<Board> {
        let flips = self.flips_at(sq.row() as i8, sq.col() as i8, self.to_move);
        if flips.is_empty() {
            return None;
        }
        let mut next = self.clone();
        next.cells[sq.index()] = Cell::Disc(self.to_move);
        for i in flips {
            next.cells[i] = Cell::Disc(self.to_move);
        }
        next.to_move = self.to_move.opponent();
        Some(next)
    }

    /// Hand the turn to the opponent without placing a disc.
    ///
    /// Only meaningful when the side to move has no legal move but the game is
    /// not over — see [`must_pass`](Board::must_pass).
    pub fn pass(&self) -> Board {
        let mut next = self.clone();
        next.to_move = self.to_move.opponent();
        next
    }

    /// True when the side to move has no legal move but the opponent does, so a
    /// pass is required to continue.
    pub fn must_pass(&self) -> bool {
        self.moves_for(self.to_move).is_empty()
            && !self.moves_for(self.to_move.opponent()).is_empty()
    }

    /// True when neither player can move: the game is over.
    pub fn is_terminal(&self) -> bool {
        self.moves_for(self.to_move).is_empty()
            && self.moves_for(self.to_move.opponent()).is_empty()
    }

    /// The game result, or `None` if the game is not over yet.
    pub fn outcome(&self) -> Option<Outcome> {
        if !self.is_terminal() {
            return None;
        }
        let black = self.count(Player::Black);
        let white = self.count(Player::White);
        Some(match black.cmp(&white) {
            Ordering::Greater => Outcome::Win(Player::Black),
            Ordering::Less => Outcome::Win(Player::White),
            Ordering::Equal => Outcome::Draw,
        })
    }

    // --- internals -------------------------------------------------------

    /// Contents of `(row, col)`, both assumed in range.
    fn cell_at(&self, row: usize, col: usize) -> Cell {
        self.cells[row * BOARD_SIZE + col]
    }

    /// Every legal move for `player` (independent of whose turn it nominally is,
    /// so we can also ask "does the opponent have a reply?").
    fn moves_for(&self, player: Player) -> Vec<Square> {
        Square::all()
            .filter(|sq| {
                !self
                    .flips_at(sq.row() as i8, sq.col() as i8, player)
                    .is_empty()
            })
            .collect()
    }

    /// Indices of the discs that `player` would flip by playing at `(row, col)`.
    ///
    /// Returns an empty vec if the square is occupied or the move flips nothing
    /// (i.e. the move is illegal). A move is legal iff at least one direction
    /// runs over one-or-more opponent discs and is then capped by `player`'s
    /// own disc.
    fn flips_at(&self, row: i8, col: i8, player: Player) -> Vec<usize> {
        let mut flips = Vec::new();
        if self.cell_at(row as usize, col as usize) != Cell::Empty {
            return flips;
        }
        for (d_row, d_col) in DIRECTIONS {
            let mut line = Vec::new();
            let mut r = row + d_row;
            let mut c = col + d_col;
            while (0..BOARD_SIZE as i8).contains(&r) && (0..BOARD_SIZE as i8).contains(&c) {
                match self.cell_at(r as usize, c as usize) {
                    Cell::Empty => {
                        // Ran into a gap before finding our own disc: no capture.
                        break;
                    }
                    Cell::Disc(p) => {
                        if p == player {
                            // Capped by our own disc: everything between flips.
                            flips.extend_from_slice(&line);
                            break;
                        }
                        // An opponent disc: a candidate for flipping.
                        line.push(r as usize * BOARD_SIZE + c as usize);
                    }
                }
                r += d_row;
                c += d_col;
            }
            // Running off the board without hitting our own disc captures
            // nothing; `line` is dropped with the loop.
        }
        flips
    }
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

/// Flat index for a `(row, col)` known to be in range (internal setup helper).
fn idx(row: usize, col: usize) -> usize {
    row * BOARD_SIZE + col
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(row: u8, col: u8) -> Square {
        Square::new(row, col).expect("in-range square")
    }

    #[test]
    fn opening_has_exactly_four_moves_for_black() {
        let board = Board::new();
        assert_eq!(board.to_move(), Player::Black);

        let mut moves = board.legal_moves();
        moves.sort_by_key(|s| s.index());

        let mut expected = vec![sq(2, 3), sq(3, 2), sq(4, 5), sq(5, 4)];
        expected.sort_by_key(|s| s.index());

        assert_eq!(moves, expected);
    }

    #[test]
    fn known_flip_scenario() {
        // From the opening, Black plays (row 2, col 3). Going downward it runs
        // over the white disc at (3, 3) and is capped by Black's disc at (4, 3),
        // so (3, 3) flips to Black.
        let board = Board::new();
        let after = board.apply(sq(2, 3)).expect("legal opening move");

        // The played square and the captured square are now Black.
        assert_eq!(after.cell(sq(2, 3)), Cell::Disc(Player::Black));
        assert_eq!(after.cell(sq(3, 3)), Cell::Disc(Player::Black));

        // One disc placed + one flipped: Black 2 -> 4, White 2 -> 1.
        assert_eq!(after.count(Player::Black), 4);
        assert_eq!(after.count(Player::White), 1);
        assert_eq!(after.to_move(), Player::White);
    }

    #[test]
    fn illegal_moves_are_rejected() {
        let board = Board::new();
        // Centre is occupied.
        assert!(board.apply(sq(3, 3)).is_none());
        // A far corner flips nothing.
        assert!(board.apply(sq(0, 0)).is_none());
        assert!(!board.is_legal(sq(0, 0)));
    }

    #[test]
    fn forced_pass() {
        // White on a1=(0,0), Black on b1=(0,1), Black to move.
        // Black's only white neighbour (0,0) sits on the edge, so Black has no
        // capping square and cannot move; White can play (0,2), capturing (0,1).
        let board = Board::from_discs(&[sq(0, 1)], &[sq(0, 0)], Player::Black);

        assert!(board.legal_moves().is_empty());
        assert!(board.must_pass());
        assert!(!board.is_terminal());

        let passed = board.pass();
        assert_eq!(passed.to_move(), Player::White);
        assert!(passed.legal_moves().contains(&sq(0, 2)));
    }

    #[test]
    fn no_moves_for_either_side_ends_game() {
        // A board with only Black discs: White can't cap anything and Black has
        // no opponent discs to flip, so neither side can move despite empties.
        let board = Board::from_discs(&[sq(3, 3), sq(4, 4)], &[], Player::Black);

        assert!(board.legal_moves().is_empty());
        assert!(board.is_terminal());
        assert!(!board.must_pass());
        assert_eq!(board.empty_count(), (NUM_SQUARES - 2) as u32);
        assert_eq!(board.outcome(), Some(Outcome::Win(Player::Black)));
    }

    #[test]
    fn full_board_is_terminal_and_scored() {
        // Fill every square: 33 Black then 31 White -> Black wins.
        let black: Vec<Square> = (0..33).map(|i| Square::from_index(i).unwrap()).collect();
        let white: Vec<Square> = (33..NUM_SQUARES)
            .map(|i| Square::from_index(i).unwrap())
            .collect();
        let board = Board::from_discs(&black, &white, Player::Black);

        assert_eq!(board.empty_count(), 0);
        assert!(board.is_terminal());
        assert!(board.outcome().is_some());
        assert_eq!(board.outcome(), Some(Outcome::Win(Player::Black)));
    }

    #[test]
    fn full_board_can_draw() {
        let black: Vec<Square> = (0..32).map(|i| Square::from_index(i).unwrap()).collect();
        let white: Vec<Square> = (32..NUM_SQUARES)
            .map(|i| Square::from_index(i).unwrap())
            .collect();
        let board = Board::from_discs(&black, &white, Player::Black);
        assert_eq!(board.outcome(), Some(Outcome::Draw));
    }
}
