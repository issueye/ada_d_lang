use ada_lexer::LexError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("Unexpected token '{found}' at line {line}, col {col}, expected {expected}")]
    UnexpectedToken {
        found: String,
        expected: String,
        line: usize,
        col: usize,
    },

    #[error("Unexpected end of input at line {line}, col {col}, expected {expected}")]
    UnexpectedEof {
        expected: String,
        line: usize,
        col: usize,
    },

    #[error("Invalid assignment target at line {line}, col {col}")]
    InvalidAssignmentTarget {
        line: usize,
        col: usize,
    },

    #[error("Lexer error: {0}")]
    Lex(#[from] LexError),
}
