use crate::utils::castling::Castling;
use crate::utils::color::Color;
use crate::utils::coord::{Coord, FromIndex};
use crate::utils::draw::Draw;
use crate::utils::figure::Figure;
use crate::utils::piece::Piece;
use std::collections::HashSet;
use std::ops::Range;

// Define types for improved readability.
type FEN = String;
type CoordIdx = Vec<i8>;
type Coords = Vec<Coord>;
type Figures = Vec<Figure>;
type OptFigures = Vec<Option<Figure>>;
type FigSet = HashSet<Figure>;

/// Use a constant to prepare all strings that describe the 32 starting position figures.
const FIGURE_STR_VEC: [&str; 32] = [
    "ra8", "nb8", "bc8", "qd8", "ke8", "bf8", "ng8", "rh8", "pa7", "pb7", "pc7", "pd7", "pe7",
    "pf7", "pg7", "ph7", "Pa2", "Pb2", "Pc2", "Pd2", "Pe2", "Pf2", "Pg2", "Ph2", "Ra1", "Nb1",
    "Bc1", "Qd1", "Ke1", "Bf1", "Ng1", "Rh1",
];

/// Core API for derivation from Forsyth-Edwards-Notation (FEN) or to FEN. Thus, the fields are
/// one-to-one derivations of the parts of the FEN.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Game {
    /// A static vector of references to coordinates, to allow for lookups of coordinates based on
    /// indexes instead of building new coordinates from their index.
    pub board: Coords,

    /// A position reflects figures on the board.
    pub position: OptFigures,

    /// Set of Figures that are on the board.
    pub figures: FigSet,

    /// Currently active color (w/b).
    pub color: Color,

    /// Castling rights (KQkq).
    pub castling: Castling,

    /// Option of a coordinate, if En-Passant is available.
    pub en_passant: Option<Coord>,

    /// Current state of the half-move clock.
    pub half_move_clock: u16,

    /// Current state of the full-move clock.
    pub full_move_clock: u16,
}

impl Game {
    /// Constructs a new game that reflects the game state at the beginning of a standard match.
    pub fn new() -> Self {
        let mut position: OptFigures = vec![None; 64];
        for fstr in FIGURE_STR_VEC {
            let fig = Figure::from(fstr);
            position[fig.coord.idx as usize] = Some(fig);
        }

        return Game {
            board: get_board(),
            position: position.clone(),
            figures: position
                .into_iter()
                .filter(|fig| !fig.is_none())
                .map(|fig| fig.unwrap())
                .collect(),
            color: Color::W,
            castling: Castling::new(),
            en_passant: None,
            half_move_clock: 0,
            full_move_clock: 1,
        };
    }

    pub fn to_fen(self) -> String {
        return [
            position_to_fen(self.position),
            self.color.to_string(),
            self.castling.to_string(),
            match self.en_passant {
                None => "-".to_string(),
                Some(c) => c.to_string(),
            },
            self.half_move_clock.to_string(),
            self.full_move_clock.to_string(),
        ]
        .join(" ");
    }

    pub fn play_move(self, mv: String) -> Self {
        // Separate between castling and a "normal draw" where only one piece is moved.
        if mv.contains("O-O") {
            return self.castle(mv);
        } else {
            // prepare position and figures to be changed.
            let (mut position, mut figures) = (self.position.clone(), self.figures.clone());

            // derive the draw from SAN and identify the moving figure.
            let draw = Draw::from(mv);
            let moving_figure = filter_mover(&draw, &self);

            // update figures & position
            position[moving_figure.coord.idx as usize] = None;
            figures.remove(&moving_figure);
            if draw.is_hit {
                if !self.en_passant.is_none()
                    && (moving_figure.piece == Piece::P)
                    && (draw.target == self.en_passant.unwrap())
                {
                    figures.remove(
                        &figures
                            .clone()
                            .into_iter()
                            .find(|f| f.coord.idx == (draw.target.idx - f.color.factor() * 8))
                            .unwrap(),
                    );
                } else {
                    figures.remove(
                        &figures
                            .clone()
                            .into_iter()
                            .find(|f| f.coord == draw.target)
                            .unwrap(),
                    );
                }
            }
            if draw.is_promo {
                let promoted_figure = Figure {
                    color: self.color,
                    coord: draw.target,
                    piece: draw.promoted_piece.unwrap(),
                };
                position[promoted_figure.coord.idx as usize] = Some(promoted_figure);
                figures.insert(promoted_figure);
            } else {
                let moved_figure = moving_figure.move_to(&draw.target);
                position[moved_figure.coord.idx as usize] = Some(moved_figure);
                figures.insert(moved_figure);
            }

            // Account for En-Passant
            let mut en_passant = None;
            if (moving_figure.piece == Piece::P)
                && ((moving_figure.coord.y - draw.target.y).abs() == 2)
            {
                let ep_idx = (draw.target.idx + self.color.factor() * 8) as usize;
                let ep_coord = self.clone().board[ep_idx];
                let mut ep_candidates = self.figures.clone().into_iter().filter(|f| {
                    f.color == self.color.next()
                        && (f.piece == Piece::P)
                        && (f.coord.y == draw.target.y)
                        && ((f.coord.x - draw.target.x).abs() == 1)
                });

                if !ep_candidates.next().is_none() {
                    en_passant = Some(ep_coord);
                }
            }

            Game {
                board: self.board,
                position,
                figures,
                color: self.color.next(),
                castling: self.castling.update(moving_figure),
                en_passant,
                half_move_clock: if draw.is_hit | (draw.piece == Piece::P) {
                    0
                } else {
                    self.half_move_clock + 1
                },
                full_move_clock: match self.color {
                    Color::B => self.full_move_clock + 1,
                    Color::W => self.full_move_clock,
                },
            }
        }
    }

    fn castle(self, mv: String) -> Game {
        let mut figures: FigSet = self.figures.clone();
        let mut position: OptFigures = self.position.clone();

        // prepare indexes with
        let king_src: usize;
        let king_tgt: usize;
        let rook_src: usize;
        let rook_tgt: usize;

        // Get the coordinates of the involved king and rook.
        if self.color == Color::B {
            king_src = 4;
            if mv.contains("O-O-O") {
                rook_src = 0;
                king_tgt = 2;
                rook_tgt = 3;
            } else {
                rook_tgt = 5;
                king_tgt = 6;
                rook_src = 7;
            }
        } else {
            king_src = 60;
            if mv.contains("O-O-O") {
                rook_src = 56;
                king_tgt = 58;
                rook_tgt = 59;
            } else {
                rook_tgt = 61;
                king_tgt = 62;
                rook_src = 63;
            }
        }

        // get the according figures that will be involved.
        let king = position[king_src].unwrap().clone();
        let rook = position[rook_src].unwrap().clone();
        let new_king = king.move_to(&self.board[king_tgt]);
        let new_rook = rook.move_to(&self.board[rook_tgt]);

        // update figures by removing king and rook and putting them into their new positions.
        figures.remove(&king);
        figures.remove(&rook);
        figures.insert(new_king);
        figures.insert(new_rook);

        // update position by setting appropriate Figure Options.
        position[king_src] = None;
        position[rook_src] = None;
        position[king_tgt] = Some(new_king);
        position[rook_tgt] = Some(new_rook);

        Game {
            board: self.board,
            position,
            figures,
            color: self.color.next(),
            castling: self.castling.castle(self.color),
            en_passant: None,
            half_move_clock: self.half_move_clock + 1,
            full_move_clock: match self.color {
                Color::W => self.full_move_clock,
                Color::B => self.full_move_clock + 1,
            },
        }
    }

    fn find_king(self, color: Color) -> Figure {
        self.figures
            .into_iter()
            .find(|f| (f.piece == Piece::K) & (f.color == color))
            .unwrap()
            .clone()
    }

    fn remove_figure(self, figure: &Figure) -> Self {
        // clone objects that need to be modified
        let mut new_figures = self.figures.clone();
        let mut new_position = self.position.clone();

        // remove the figure
        new_figures.remove(figure);
        new_position[figure.coord.idx as usize] = None;

        Game {
            board: self.board,
            position: new_position,
            figures: new_figures,
            color: self.color,
            castling: self.castling,
            en_passant: self.en_passant,
            half_move_clock: self.half_move_clock,
            full_move_clock: self.full_move_clock,
        }
    }

    fn move_figure(self, figure: &Figure, target: &Coord) -> Self {
        // clone objects that need to be modified
        let mut new_figures = self.figures.clone();
        let mut new_position = self.position.clone();

        // remove the figure
        let moved_figure = figure.move_to(target);
        new_figures.insert(moved_figure.clone());
        new_figures.remove(figure);
        new_position[target.idx as usize] = Some(moved_figure);
        new_position[figure.coord.idx as usize] = None;

        Game {
            board: self.board,
            position: new_position,
            figures: new_figures,
            color: self.color,
            castling: self.castling,
            en_passant: self.en_passant,
            half_move_clock: self.half_move_clock,
            full_move_clock: self.full_move_clock,
        }
    }
}

impl From<String> for Game {
    fn from(fen: String) -> Self {
        let board = get_board();

        // Split FEN and assign according variables.
        let fen_parts: Vec<&str> = fen.split(" ").collect();

        // Sort string information into the according variables.
        let position_str: FEN = fen_parts[0].to_owned();
        let color_str = fen_parts[1];
        let castling_str = fen_parts[2];
        let ep_str = fen_parts[3];
        let hmc_str = fen_parts[4];
        let fmc_str = fen_parts[5];

        // Derive fields from Strings.
        let position: OptFigures = fen_to_position(position_str, &board);
        let figures: FigSet = position
            .clone()
            .into_iter()
            .filter(|f| !f.is_none())
            .map(|f| f.unwrap())
            .collect();
        let color = Color::from(color_str.chars().next().unwrap());
        let castling = Castling::from(&castling_str[..]);
        let en_passant: Option<Coord> = if ep_str == "-" {
            None
        } else {
            Some(Coord::from(&ep_str[..]))
        };
        let half_move_clock = hmc_str.parse::<u16>().unwrap();
        let full_move_clock = fmc_str.parse::<u16>().unwrap();

        Game {
            board,
            position,
            figures,
            color,
            castling,
            en_passant,
            half_move_clock,
            full_move_clock,
        }
    }
}

//- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
fn get_board() -> Coords {
    let irange = Range { start: 0, end: 64 };
    return Vec::from_iter(irange.map(|i| Coord::from_idx(i)));
}

fn valid_idx(idx: i8) -> bool {
    (idx >= 0) & (idx < 64)
}

fn fen_to_position(fen: FEN, board: &Coords) -> OptFigures {
    // Use intermediate structure to parse the FEN
    let mut figures: OptFigures = vec![None; 64];

    // count through the board/fen using i.
    let mut i: usize = 0;
    for l in fen.chars() {
        if l.is_digit(10) {
            i += l.to_digit(10).unwrap() as usize;
        } else if l == '/' {
            continue;
        } else {
            figures[i] = Some(Figure {
                color: if l.is_lowercase() { Color::B } else { Color::W },
                piece: Piece::from(l),
                coord: board[i],
            });
            i += 1 as usize;
        }
    }

    return figures;
}

fn position_to_fen(position: OptFigures) -> FEN {
    // At several positions numbers have to be added. Thus, use a separate function.
    fn unload_space(mut spacer: u8, fen: &mut FEN) -> u8 {
        if spacer > 0 {
            fen.push(char::from_digit(spacer as u32, 10).unwrap());
            spacer = 0
        }
        spacer
    }

    // Basically, this function wanders through the position and derives letters.
    let mut fen = String::new();
    let mut spacer: u8 = 0;
    for (f, figure) in position.into_iter().enumerate() {
        // Set row separators.
        if (f > 0) & (f % 8 == 0) {
            spacer = unload_space(spacer, &mut fen);
            fen.push('/')
        }

        // Either increase empty space counter (spacer) or set figure.
        if figure.is_none() {
            spacer += 1
        } else {
            spacer = unload_space(spacer, &mut fen);
            fen.push(figure.unwrap().to_char());
        }
    }

    // Repeat writing the empty spaces if there are some:
    unload_space(spacer, &mut fen);

    fen
}

fn filter_mover(draw: &Draw, game: &Game) -> Figure {
    let figs: FigSet = game
        .figures
        .clone()
        .into_iter()
        .filter(|f| (f.color == game.color) & (f.piece == draw.piece))
        .collect();
    return if figs.clone().into_iter().count() == 1 {
        figs.clone().into_iter().next().unwrap()
    } else {
        filter_on_remainder(figs, draw, game)
    };
}

fn filter_on_remainder(figures: FigSet, draw: &Draw, game: &Game) -> Figure {
    let figs: FigSet = if draw.remainder_file.is_none() & draw.remainder_rank.is_none() {
        figures
    } else if !draw.remainder_file.is_none() & !draw.remainder_rank.is_none() {
        figures
            .into_iter()
            .filter(|f| {
                (f.coord.file == draw.remainder_file.unwrap())
                    & (f.coord.rank == draw.remainder_rank.unwrap())
            })
            .collect()
    } else if !draw.remainder_file.is_none() {
        figures
            .into_iter()
            .filter(|f| f.coord.file == draw.remainder_file.unwrap())
            .collect()
    } else if !draw.remainder_rank.is_none() {
        figures
            .into_iter()
            .filter(|f| f.coord.rank == draw.remainder_rank.unwrap())
            .collect()
    } else {
        figures
    };

    return if figs.clone().into_iter().count() == 1 {
        figs.clone().into_iter().next().unwrap()
    } else {
        filter_on_moves(figs, draw, game)
    };
}

fn filter_on_moves(figures: FigSet, draw: &Draw, game: &Game) -> Figure {
    let figs: FigSet = if draw.is_hit {
        figures
            .into_iter()
            .filter(|f| get_hits(f, game).contains(&draw.target))
            .collect()
    } else {
        figures
            .into_iter()
            .filter(|f| get_moves(f, game).contains(&draw.target))
            .collect()
    };
    return if figs.clone().into_iter().count() == 1 {
        figs.clone().into_iter().next().unwrap()
    } else {
        filter_on_pins(figs, draw, game)
    };
}

fn filter_on_pins(figures: FigSet, draw: &Draw, game: &Game) -> Figure {
    // store the kings coordinate of the current moving party.
    let king_coord = game.clone().find_king(game.color).coord;

    // prepare the game to analyze accordingly if the move is a hit.
    let base_game: Game = if draw.is_hit {
        game.clone()
            .remove_figure(&game.position[draw.target.idx as usize].unwrap())
    } else {
        game.clone()
    };

    let mut figs: Figures = Vec::new();
    for fig in figures {
        let alt_game = base_game.clone().move_figure(&fig, &draw.target);

        let n_checkers = alt_game
            .clone()
            .figures
            .into_iter()
            .filter(|f| {
                (f.color != game.color)
                    && ([Piece::R, Piece::B, Piece::Q].contains(&f.piece))
                    && (get_moves(f, &alt_game).contains(&king_coord))
            })
            .count();

        if n_checkers == 0 {
            figs.push(fig);
        }
    }

    return figs.into_iter().next().unwrap();
}

fn get_moves(fig: &Figure, game: &Game) -> Coords {
    let coordis: CoordIdx = match fig.piece {
        Piece::P => get_pawn_moves(fig, game),
        Piece::R => get_rook_moves(fig, game),
        Piece::N => get_knight_moves(fig, game),
        Piece::B => get_bishop_moves(fig, game),
        Piece::Q => get_queen_moves(fig, game),
        Piece::K => get_king_moves(fig, game),
    };

    return coordis
        .into_iter()
        .map(|ci| game.board[ci as usize])
        .collect::<Coords>();
}

fn get_hits(fig: &Figure, game: &Game) -> Coords {
    match fig.piece {
        Piece::P => get_pawn_hits(fig, game)
            .into_iter()
            .map(|ci| game.board[ci as usize])
            .collect::<Coords>(),
        _ => get_moves(fig, game),
    }
}

fn get_pawn_hits(fig: &Figure, game: &Game) -> CoordIdx {
    // prepare empty vec to be pushed with possible moves.
    let mut coordix: CoordIdx = vec![];
    let (ci, f) = (fig.coord.idx, fig.color.factor());

    // Add hits if appropriate.
    for i in [7, 9] {
        let ti: i8 = ci - f * i;
        if valid_idx(ti) && !game.position[ti as usize].is_none() {
            if game.position[ti as usize].unwrap().color != fig.color {
                coordix.push(ti);
            }

            // If the possible target is the en-passant square, add it.
            if !game.en_passant.is_none() && (game.en_passant.unwrap().idx == ti) {
                coordix.push(ti);
            }
        }
    }

    coordix
}

fn get_pawn_moves(fig: &Figure, game: &Game) -> CoordIdx {
    // prepare empty vec to be pushed with possible moves.
    let mut coordix: CoordIdx = vec![];
    let (ci, f) = (fig.coord.idx, fig.color.factor());

    // add the index of the square in front, if unblocked.
    let ti: i8 = ci - f * 8; // target Index
    if valid_idx(ti) && game.position[ti as usize].is_none() {
        coordix.push(ti);
    }

    // if the pawn hasn't moved yet, add the square two apart, if unblocked.
    //  Note: The square in front must be accessible to make the 2nd valid.
    if (fig.color.is_white() & (fig.coord.y == 1)) | (fig.color.is_black() & (fig.coord.y == 6)) {
        let tii: i8 = &ci - f * 16;
        if valid_idx(tii) & game.position[ti as usize].is_none() {
            if !coordix.is_empty() {
                coordix.push(tii);
            }
        }
    }

    return coordix;
}

fn get_knight_moves(fig: &Figure, game: &Game) -> CoordIdx {
    // prepare basics
    let mut coordix: CoordIdx = vec![];
    let ci = fig.coord.idx;

    // loop over possible jump locations and check if those feasible.
    for i in [-17, -15, -10, -6, 6, 10, 15, 17] {
        let ti: i8 = ci + i;
        if valid_idx(ti) && ((fig.coord.x - game.board[ti as usize].x).abs() < 3) {
            if game.position[ti as usize].is_none() {
                coordix.push(ti);
            } else {
                if game.position[ti as usize].unwrap().color != fig.color {
                    coordix.push(ti);
                }
            }
        }
    }

    return coordix;
}

fn get_bishop_moves(fig: &Figure, game: &Game) -> CoordIdx {
    // prepare basics
    let mut coordix: CoordIdx = vec![];
    let ci = fig.coord.idx;

    for d in [-9, -7, 7, 9] {
        // deltas as in distance to current array position.
        let mut f: i8 = 1; // factor to stretch delta d.
        let mut ti = ci + (f * d);
        let mut unblocked: bool = true;
        while unblocked
            && valid_idx(ti)
            && ((game.board[ti as usize].main_diagonal == fig.coord.main_diagonal)
                | (game.board[ti as usize].anti_diagonal == fig.coord.anti_diagonal))
        {
            if game.position[ti as usize].is_none() {
                coordix.push(ti);
            } else {
                unblocked = false;
                if game.position[ti as usize].unwrap().color != fig.color {
                    coordix.push(ti);
                }
            }

            // update indexes
            f += 1;
            ti = ci + (f * d);
        }
    }

    return coordix;
}

fn get_rook_moves(fig: &Figure, game: &Game) -> CoordIdx {
    // prepare basics
    let mut coordix: CoordIdx = vec![];
    let ci = fig.coord.idx;

    for d in [-8, -1, 1, 8] {
        // deltas as in distance to current array position.
        let mut f: i8 = 1; // factor to stretch delta d.
        let mut ti = ci + (f * d);

        let mut unblocked: bool = true;
        while unblocked
            && valid_idx(ti)
            && ((game.board[ti as usize].x == fig.coord.x)
                | (game.board[ti as usize].y == fig.coord.y))
        {
            if game.position[ti as usize].is_none() {
                coordix.push(ti);
            } else {
                unblocked = false;
                if game.position[ti as usize].unwrap().color != fig.color {
                    coordix.push(ti);
                }
            }

            // update indexes
            f += 1;
            ti = ci + (f * d);
        }
    }

    return coordix;
}

fn get_queen_moves(fig: &Figure, game: &Game) -> CoordIdx {
    let mut coordix: CoordIdx = vec![];

    // As the queen unions the moves from bishop and rook, mirror the union.
    let bishop_coordix = get_bishop_moves(fig, game);
    let rook_coordix = get_rook_moves(fig, game);

    coordix.extend(bishop_coordix);
    coordix.extend(rook_coordix);

    return coordix;
}

fn get_king_moves(fig: &Figure, game: &Game) -> CoordIdx {
    let mut coordix: CoordIdx = vec![];
    let ci = fig.coord.idx;
    for i in [-9, -8, -7, -1, 1, 7, 8, 9] {
        let ti = ci + i;
        if valid_idx(ti)
            && (((fig.coord.x - game.board[ti as usize].x).abs() <= 1)
                | ((fig.coord.y - game.board[ti as usize].x).abs() <= 1))
        {
            if game.position[ti as usize].is_none() {
                coordix.push(ti);
            } else {
                if game.position[ti as usize].unwrap().color != fig.color {
                    coordix.push(ti)
                }
            }
        }
    }

    return coordix;
}

//- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[allow(dead_code)]
fn coords_from_san(coords: Vec<&str>) -> Coords {
    Vec::from(coords)
        .into_iter()
        .map(|c| Coord::from(c))
        .collect::<Coords>()
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_pawn_a2() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Pa2"), &game),
        coords_from_san(Vec::from(["a3", "a4"]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_black_pawn_g7() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("pg7"), &game),
        coords_from_san(Vec::from(["g6", "g5"]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_knight_b1() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Nb1"), &game),
        coords_from_san(Vec::from(["a3", "c3"]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_bishop_c1() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Bc1"), &game),
        coords_from_san(Vec::from([]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_black_rook_h8() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("rh8"), &game),
        coords_from_san(Vec::from([]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_queen_d1() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Qd1"), &game),
        coords_from_san(Vec::from([]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_king_e1() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Ke1"), &game),
        coords_from_san(Vec::from([]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_bishop_a3() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Ba3"), &game),
        coords_from_san(Vec::from(["b4", "c5", "d6", "e7"]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_black_bishop_a3() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("ba3"), &game),
        coords_from_san(Vec::from(["b4", "c5", "d6", "b2"]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_white_rook_e4() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("Re4"), &game),
        coords_from_san(Vec::from([
            "e5", "e6", "e7", "d4", "c4", "b4", "a4", "f4", "g4", "h4", "e3"
        ]))
    );
}

#[test]
fn check_moves_and_blocks_in_new_game_for_black_rook_e4() {
    let game = Game::new();
    assert_eq!(
        get_moves(&Figure::from("re4"), &game),
        coords_from_san(Vec::from([
            "e5", "e6", "d4", "c4", "b4", "a4", "f4", "g4", "h4", "e3", "e2"
        ]))
    );
}

#[test]
fn check_game_from_fen_base() {
    let fen: String = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string();
    let game = Game::from(fen);
    assert_eq!(game, Game::new());
}

#[test]
/// Final position from https://lichess.org/U1N9Qa74/black
fn check_game_from_fen() {
    let fen: String = "5rk1/1b2n1pp/4R3/1p3pN1/2pP4/r5PP/P4P2/2RQ2Kq w - - 1 24".to_string();
    let game = Game::from(fen);

    // Write down individual position by hand
    let figures = [
        "rf8", "kg8", "bb7", "ne7", "pg7", "ph7", "Re6", "pb5", "pf5", "Ng5", "pc4", "Pd4", "ra3",
        "Pg3", "Ph3", "Pa2", "Pf2", "Rc1", "Qd1", "Kg1", "qh1",
    ];
    // Test easy translations first and use different paths to derive the same:
    let mut position: OptFigures = vec![None; 64];
    for fig_str in figures {
        let fig = Figure::from(fig_str);
        position[fig.coord.idx as usize] = Some(fig);
    }

    let empty_castle = Castling {
        white_kingside: false,
        white_queenside: false,
        black_kingside: false,
        black_queenside: false,
    };

    assert_eq!(game.color, Color::W);
    assert_eq!(game.castling, empty_castle);
    assert_eq!(game.en_passant, None);
    assert_eq!(game.half_move_clock, 1);
    assert_eq!(game.full_move_clock, 24);
    assert_eq!(game.position, position);
}

#[test]
/// Final position from https://lichess.org/U1N9Qa74/black
fn check_fen_conversion_pt0() {
    let fen = "5rk1/1b2n1pp/4R3/1p3pN1/2pP4/r5PP/P4P2/2RQ2Kq w - - 1 24".to_string();
    assert_eq!(Game::from(fen.clone()).to_fen(), fen);
}

#[test]
fn check_king_extraction() {
    let game = Game::new();
    assert_eq!(game.clone().find_king(Color::W), Figure::from("Ke1"));
    assert_eq!(game.clone().find_king(Color::B), Figure::from("ke8"));
}

#[test]
fn check_filter_mover_detection_base() {
    let game = Game::new();
    let draw = Draw::from("Nc3".to_string());
    assert_eq!(Figure::from("Nb1"), filter_mover(&draw, &game))
}

#[test]
fn check_filter_mover_detection_pawn_hit() {
    let game = Game::from("k7/8/2q3q1/1PP5/8/8/NR6/KN1N3B w - - 0 1".to_string());
    let draw = Draw::from("bxc6".to_string());
    assert_eq!(Figure::from("Pb5"), filter_mover(&draw, &game))
}

#[test]
fn check_filter_mover_detection_pawn_move() {
    let game = Game::from("k7/8/2q3q1/1PP5/8/8/NR6/KN1N3B w - - 0 1".to_string());
    let draw = Draw::from("b6".to_string());
    assert_eq!(Figure::from("Pb5"), filter_mover(&draw, &game))
}

#[test]
fn check_mover_detection_with_remainder() {
    let game = Game::from("k7/8/q1q3q1/1PP5/8/8/RR6/KN5B b - - 0 1".to_string());
    let draw = Draw::from("Qgg2".to_string());

    assert_eq!(Figure::from("qg6"), filter_mover(&draw, &game));
}

#[test]
fn check_mover_detection_with_pinned_queen() {
    let game = Game::from("k7/8/q1q3q1/1PP5/8/8/RR6/KN5B b - - 0 1".to_string());
    let draw = Draw::from("Qd6".to_string());

    assert_eq!(Figure::from("qg6"), filter_mover(&draw, &game));
}

#[test]
fn check_mover_detection_with_movable_pinned_queen() {
    let game = Game::from("k7/8/q1q3q1/1PP5/8/8/RR6/KN5B b - - 0 1".to_string());
    let draw = Draw::from("Qb7".to_string());

    assert_eq!(Figure::from("qc6"), filter_mover(&draw, &game));
}

#[test]
fn check_mover_detection_with_hit_from_queen() {
    let game = Game::from("k3R3/8/q1q3q1/1PP5/8/8/RR6/KN5B b - - 0 1".to_string());
    let draw = Draw::from("Qxe8".to_string());

    assert_eq!(Figure::from("qg6"), filter_mover(&draw, &game));
}

#[test]
fn check_castling() {
    let game = Game::from("4k2r/8/8/8/8/8/8/R3K3 w Qk - 0 1".to_string());

    let g1 = game.play_move("O-O-O".to_string());
    let g2 = g1.play_move("O-O".to_string());

    assert_eq!(
        g2.figures,
        HashSet::from_iter(["Kc1", "Rd1", "rf8", "kg8"].map(|x| Figure::from(x)))
    );
}

#[test]
/// Somehow, in a previous approach the initial construction of the figures went wrong,
/// thus add a lengthy test...
fn check_board() {
    let game = Game::new();

    assert_eq!(
        game.position,
        Vec::from([
            Some(Figure::from("ra8")),
            Some(Figure::from("nb8")),
            Some(Figure::from("bc8")),
            Some(Figure::from("qd8")),
            Some(Figure::from("ke8")),
            Some(Figure::from("bf8")),
            Some(Figure::from("ng8")),
            Some(Figure::from("rh8")),
            Some(Figure::from("pa7")),
            Some(Figure::from("pb7")),
            Some(Figure::from("pc7")),
            Some(Figure::from("pd7")),
            Some(Figure::from("pe7")),
            Some(Figure::from("pf7")),
            Some(Figure::from("pg7")),
            Some(Figure::from("ph7")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Figure::from("Pa2")),
            Some(Figure::from("Pb2")),
            Some(Figure::from("Pc2")),
            Some(Figure::from("Pd2")),
            Some(Figure::from("Pe2")),
            Some(Figure::from("Pf2")),
            Some(Figure::from("Pg2")),
            Some(Figure::from("Ph2")),
            Some(Figure::from("Ra1")),
            Some(Figure::from("Nb1")),
            Some(Figure::from("Bc1")),
            Some(Figure::from("Qd1")),
            Some(Figure::from("Ke1")),
            Some(Figure::from("Bf1")),
            Some(Figure::from("Ng1")),
            Some(Figure::from("Rh1")),
        ])
    );
}

#[test]
/// https://lichess.org/hWMPaRcI
fn check_playing_games_pt1() {
    let mut game = Game::new();
    let mvs = [
        "c4", "c5", "Nc3", "e5", "e3", "Nf6", "Nf3", "Nc6", "b3", "e4", "Ng1", "d6", "d4", "Bg4",
        "Qd2", "Bd7", "dxc5", "dxc5", "Nd5", "Nxd5", "cxd5", "Nb4", "Qc3", "b6", "Qc4", "Bc8",
        "a3", "Na6", "Qxe4+", "Be7", "Bb2", "Bb7", "Rd1", "O-O", "Bc4", "Nc7", "Bd3", "g6", "Bc4",
        "Bf6", "Bxf6", "Qxf6", "Ne2", "Rae8", "Qg4", "Rd8", "e4", "Bc8", "Qf4", "Qxf4", "Nxf4",
        "b5", "d6", "Na6", "Bxb5", "Nb8", "e5", "a6", "Bc4", "Nc6", "O-O", "Nxe5", "Rfe1", "Nxc4",
        "bxc4", "Bb7", "Re7", "Bc6", "Ra7", "Rfe8", "h3", "Ba4", "Rd2", "Re1+", "Kh2", "Re4",
        "Rxa6", "Rxc4", "g3", "Rc2", "Rxc2", "Bxc2", "a4", "c4", "Rc6", "Bb3", "a5", "Bd1", "a6",
        "g5", "Ne2", "Bxe2", "a7", "Bf3", "Rb6", "Ra8", "Rb8+", "Rxb8", "axb8=Q+", "Kg7", "d7",
        "g4", "d8=Q", "gxh3", "Qd4+", "f6", "Qb7+", "Kg6", "Qxf3", "Kf7", "Qdxf6+", "Ke8", "Qe4+",
        "Kd7", "Qfe6+", "Kc7", "Qd4", "Kb7", "Qed5+", "Kc7", "Q4xc4+", "Kb6",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(
        game.to_fen(),
        "8/7p/1k6/3Q4/2Q5/6Pp/5P1K/8 w - - 1 62".to_string()
    )
}

/// https://lichess.org/BpKMwGdB
#[test]
fn check_playing_games_pt2() {
    let mut game = Game::new();
    let mvs = [
        "c4", "e5", "Nc3", "Bc5", "a3", "Nf6", "e3", "e4", "b4", "Bd6", "d4", "exd3", "Bxd3",
        "Be5", "Bb2", "d6", "Nf3", "h6", "Bc2", "O-O", "Nxe5", "Nbd7", "Nxd7", "Bxd7", "Nd5",
        "Bg4", "f3", "Bh5", "Nxf6+", "gxf6", "O-O", "Qe7", "Re1", "Rae8", "Qd2", "Bg6", "e4",
        "Kh7", "a4", "Rg8", "a5", "Bh5", "Bc1", "Rg6", "a6", "b6", "Rb1", "Bxf3", "e5", "fxe5",
        "Bxg6+", "Kxg6", "Qxh6+", "Kf5", "Rf1", "e4", "gxf3", "Rg8+", "Kh1", "Rg6", "fxe4+", "Ke6",
        "Qh3+", "Ke5", "Qf5+", "Kd4", "Qxf7", "Qxe4+", "Qf3", "Qxb1", "Qe3+", "Kxc4", "Qf4+",
        "Kb5", "Qf5+", "Qxf5", "Rxf5+", "Kxa6", "h4", "Rg4", "h5", "Rxb4", "Rf4", "Rb1", "Rf1",
        "Rb5", "h6", "Rh5+", "Kg2", "Re5", "Rf7", "Re8", "h7", "Rh8", "Bb2", "Rxh7", "Rxh7", "c5",
        "Kf2", "d5", "Ke2", "b5", "Kd2", "c4", "Kc3", "Kb6", "Ba3", "a6", "Rh5", "Kc6", "Rh6+",
        "Kd7", "Kd4", "a5", "Kxd5", "c3", "Kc5", "b4", "Bc1", "b3", "Rh2", "a4", "Kb4", "b2",
        "Bxb2", "cxb2", "Rxb2", "Kc6", "Kxa4", "Kd5", "Rb4", "Kc5", "Ka5", "Kd5", "Kb5", "Ke5",
        "Rc4", "Kd5", "Kb4", "Ke5", "Kc5", "Kf5", "Rd4", "Ke5", "Kc4", "Kf5", "Kd5", "Kf6", "Re4",
        "Kf5", "Kd4", "Kf6", "Re5", "Kg6", "Ke4", "Kf6", "Kf4", "Kg6", "Rf5", "Kg7", "Ke5", "Kg6",
        "Ke4", "Kg7", "Ke5", "Kg6", "Ke6", "Kg7", "Rf6", "Kg8", "Ke7", "Kg7", "Ke6", "Kg8", "Kf5",
        "Kg7", "Kg5", "Kh7", "Rg6", "Kh8", "Kf6", "Kh7", "Kf7", "Kh8", "Kf8", "Kh7", "Kf7", "Kh8",
        "Rh6#",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(game.to_fen(), "7k/5K2/7R/8/8/8/8/8 b - - 60 95".to_string())
}

/// https://lichess.org/K8nhk3Jx
#[test]
fn check_playing_games_pt3() {
    let mut game = Game::new();
    let mvs = [
        "c4", "e5", "Nc3", "Nf6", "e3", "d5", "cxd5", "Nxd5", "Nxd5", "Qxd5", "b3", "Bb4", "Nf3",
        "Bg4", "Bc4", "Qd6", "O-O", "e4", "h3", "exf3", "hxg4", "fxg2", "Qf3", "Qe5", "d4", "Qa5",
        "Rd1", "Bc3", "Qxf7+", "Kd8", "Qd5+", "Qxd5", "Bxd5", "Bxa1", "Ba3", "Bc3", "Kxg2", "Nd7",
        "f4", "c6", "Bc4", "Kc7", "e4", "Rae8", "e5", "Kb8", "Rd3", "Be1", "Bf7", "Ref8", "Bxf8",
        "Rxf8", "Bc4", "Rxf4", "g5", "Rg4+", "Kf1", "Bh4", "e6", "Nb6", "Re3", "Bxg5", "Re5",
        "Nc8", "d5", "cxd5", "Bxd5", "h6", "Kf2", "Ne7", "Bf3", "Rf4", "Kg3", "Rf5", "Rxf5",
        "Nxf5+", "Kg4", "Ne3+", "Kh5", "b5", "Kg6", "Kc7", "Kxg7", "Kd6", "Be2", "a6", "a4", "b4",
        "Bxa6", "Kxe6", "Bc8+", "Ke7", "a5", "Nd5", "Bh3", "Nc7", "Bf1", "Ke6", "a6", "Nxa6",
        "Bxa6", "Kf5", "Bc4", "h5", "Bd3+", "Kg4", "Kg6", "h4", "Be2+", "Kf4", "Kh5", "h3", "Ba6",
        "Bf6", "Bb7", "h2", "Kg6", "Bc3", "Kh5", "Kg3", "Kg5", "Kf2", "Kg4", "Kg1", "Kh3", "h1=Q+",
        "Bxh1", "Kxh1", "Kg3", "Kg1", "Kf3", "Kf1", "Ke3", "Ke1", "Kd3", "Kd1", "Kc4", "Kc2",
        "Kb5", "Kxb3", "Ka5", "Ka3", "Kb5", "b3", "Kc4", "Ba1", "Kd3", "b2", "Kc2", "Ka2", "Kc3",
        "b1=Q+", "Kc4", "Qc1+", "Kb5", "Ka3", "Kb6", "Bd4+", "Kb7", "Ka4", "Ka6", "Qc6#",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(
        game.to_fen(),
        "8/8/K1q5/8/k2b4/8/8/8 w - - 10 82".to_string()
    )
}

/// https://lichess.org/9opx3qh7
#[test]
fn check_playing_games_pt4() {
    let mut game = Game::new();
    let mvs = [
        "d4", "e5", "dxe5", "d6", "exd6", "Bxd6", "Nf3", "Nf6", "Nc3", "O-O", "a3", "Nc6", "e3",
        "a6", "Be2", "h6", "O-O", "Ne5", "Bd2", "Nxf3+", "Bxf3", "Be5", "Rc1", "c6", "Qe2", "Qd6",
        "Rfd1", "Bxh2+", "Kh1", "Be5", "e4", "Bxc3", "Bxc3", "Qe6", "Rd3", "Bd7", "Rcd1", "Rad8",
        "Bxf6", "gxf6", "Rd6", "Qe7", "Rd1d2", "Be6", "Rxd8", "Rxd8", "Rxd8+", "Qxd8", "c4", "Qd4",
        "c5", "Qxc5", "Qd2", "f5", "exf5", "Bxf5", "Qxh6", "Bg6", "Be4", "Bxe4", "Qh4", "Bg6",
        "Qd8+", "Kg7", "Qc7", "b5", "b4", "Qc1+", "Kh2", "Qxa3", "Qe5+", "Kg8", "Qe8+", "Kg7",
        "Qxc6", "Qxb4", "Qxa6", "Qh4+", "Kg1", "b4", "Qa1+", "Qf6", "Qa4", "Qc3", "f3", "b3",
        "Qa3", "Qc2", "Kh2", "b2",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(
        game.to_fen(),
        "8/5pk1/6b1/8/8/Q4P2/1pq3PK/8 w - - 0 46".to_string()
    )
}

/// https://lichess.org/1hi3aveq
#[test]
fn check_playing_games_pt5() {
    let mut game = Game::new();
    let mvs = [
        "e4", "g6", "d4", "d6", "Nf3", "c6", "h3", "Nf6", "Bg5", "Nxe4", "Qe2", "Bf5", "Nbd2",
        "Qa5", "c3", "Nxd2", "Bxd2", "Nd7", "b4", "Qa3", "Ng5", "h5", "Qc4", "d5", "Qe2", "Qb2",
        "Qd1", "Bc2", "Qc1", "Qxc1+", "Rxc1", "Ba4", "Bd3", "Nb6", "O-O", "Nc4", "Bxc4", "dxc4",
        "Bf4", "Bh6", "Rfe1", "O-O", "Rxe7", "Rae8", "Rxb7", "f6", "Ne6", "Rxe6", "Bxh6", "Rf7",
        "Rb8+", "Kh7", "Bf4", "g5", "Bd2", "Re2", "Be1", "Rfe7", "Kf1", "Bc2", "Rc8", "Bd3",
        "Rxc6", "Rc2+", "Kg1", "Rxc1", "Rxf6", "h4", "g4", "Rexe1+", "Kg2", "Be4+", "f3", "Rc2#",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv.clone());
    }

    assert_eq!(
        game.to_fen(),
        "8/p6k/5R2/6p1/1PpPb1Pp/2P2P1P/P1r3K1/4r3 w - - 1 38".to_string()
    )
}

///https://lichess.org/qdwt3dtw
#[test]
fn check_playing_games_pt6() {
    let mut game = Game::new();
    let mvs = [
        "e4", "e5", "Nf3", "Nc6", "Bc4", "Nf6", "Nc3", "d5", "exd5", "Bf5", "dxc6", "Rb8", "Ng5",
        "Qd4", "Bxf7+", "Kd8", "Ne6+", "Bxe6", "Bxe6", "bxc6", "d3", "Qc5", "Bg5", "Qe7", "Bc4",
        "Rb4", "b3", "h6", "Bd2", "Rxc4", "bxc4", "Qe6", "Rb1", "Qc8", "f3", "Bc5", "Na4", "Bd4",
        "Bb4", "c5", "Bxc5", "Kd7", "Bxd4", "Ke8", "Bxe5", "Ng4", "Bxg7", "Kf7", "Bxh8", "Qxh8",
        "fxg4", "Qf6", "Qf3", "Ke7", "Qxf6+", "Kxf6", "O-O+",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(
        game.to_fen(),
        "8/p1p5/5k1p/8/N1P3P1/3P4/P1P3PP/1R3RK1 b - - 1 29".to_string()
    )
}

/// https://lichess.org/ktey4t74
#[test]
fn check_playing_games_pt7() {
    let mut game = Game::new();
    let mvs = [
        "d4", "d5", "c4", "e6", "Nc3", "Bb4", "e3", "dxc4", "Ne2", "Nf6", "a3", "Bxc3+", "Nxc3",
        "O-O", "Bxc4", "a6", "e4", "b5", "Bb3", "e5", "Bg5", "exd4", "Nd5", "Bg4", "f3", "Be6",
        "Bxf6", "gxf6", "Qxd4", "Bxd5", "Bxd5", "c6", "O-O", "cxd5", "exd5", "Nc6", "Qg4+", "Kh8",
        "dxc6", "Qd6", "Rac1", "Rac8", "Qb4", "Qe5", "Rfe1", "Qg5", "c7", "Rg8", "g3", "f5", "Rc6",
        "f4", "Qd4+", "Rg7", "Re8+", "Rxe8", "c8=Q", "Rg8", "Qxg8+", "Kxg8", "Rc8+",
    ]
    .map(|mv| mv.to_string());

    for mv in mvs {
        game = game.play_move(mv);
    }

    assert_eq!(
        game.to_fen(),
        "2R3k1/5prp/p7/1p4q1/3Q1p2/P4PP1/1P5P/6K1 b - - 1 31".to_string()
    )
}
