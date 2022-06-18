use crate::numeric_enum;

numeric_enum! {
    pub enum PieceKind: u8 {
        King   = 0,
        Queen  = 1,
        Rook   = 2,
        Pawn   = 3,
        Bishop = 4,
        Knight = 5,
    }

    pub enum Color: u8 {
        Black = 0,
        White = 1,
    }
}

impl Color {
    pub fn new_random() -> Self {
        if rand::random() {
            Color::Black
        } else {
            Color::White
        }
    }
}

pub struct Piece {
    // would love to use type: PieceType, but no way I am using r#type every time I want to use it.
    pub kind: PieceKind,
    pub color: Color,
}

pub struct Coordinate {}

pub struct GameState {
    state: [[Option<Piece>; 8]; 8],
}

// macro fun (could've just used constants)
macro_rules! game_state {
    (@single WK) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::King,   color: $crate::chess::game::Color::White }) };
    (@single WQ) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Queen,  color: $crate::chess::game::Color::White }) };
    (@single WR) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Rook,   color: $crate::chess::game::Color::White }) };
    (@single WB) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Bishop, color: $crate::chess::game::Color::White }) };
    (@single WN) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Knight, color: $crate::chess::game::Color::White }) };
    (@single WP) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Pawn,   color: $crate::chess::game::Color::White }) };
    (@single BK) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::King,   color: $crate::chess::game::Color::Black }) };
    (@single BQ) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Queen,  color: $crate::chess::game::Color::Black }) };
    (@single BR) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Rook,   color: $crate::chess::game::Color::Black }) };
    (@single BB) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Bishop, color: $crate::chess::game::Color::Black }) };
    (@single BN) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Knight, color: $crate::chess::game::Color::Black }) };
    (@single BP) => { Some($crate::chess::game::Piece { kind: $crate::chess::game::PieceKind::Pawn,   color: $crate::chess::game::Color::Black }) };
    (@single __) => { None };
    ($([$($t:tt)*])*) => {
        $crate::chess::game::GameState {
            state: [
                $([
                    $(game_state!(@single $t)),*
                ]),*
            ]
        }
    };
}

impl GameState {
    pub fn new() -> Self {
        game_state![
            [BR BB BB BQ BK BB BN BR]
            [BP BP BP BP BP BP BP BP]
            [__ __ __ __ __ __ __ __]
            [__ __ __ __ __ __ __ __]
            [__ __ __ __ __ __ __ __]
            [__ __ __ __ __ __ __ __]
            [WP WP WP WP WP WP WP WP]
            [WR WB WB WQ WK WB WN WR]
        ]
    }
}
