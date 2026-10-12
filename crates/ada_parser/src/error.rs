use ada_ast::Span;
use ada_lexer::LexError;
use thiserror::Error;

/// 把片段词法错误的位置映射回源文件（T3.4），规则与 `Parser::from_fragment` 一致。
pub(crate) fn shift_lex_error(err: LexError, base: Span) -> LexError {
    let shift = |line: usize, col: usize| -> (usize, usize) {
        if line == 1 {
            (line + base.line - 1, col + base.col - 1)
        } else {
            (line + base.line - 1, col)
        }
    };
    match err {
        LexError::UnexpectedChar { ch, line, col } => {
            let (line, col) = shift(line, col);
            LexError::UnexpectedChar { ch, line, col }
        }
        LexError::UnterminatedString { line, col } => {
            let (line, col) = shift(line, col);
            LexError::UnterminatedString { line, col }
        }
        LexError::UnterminatedBlockComment { line, col } => {
            let (line, col) = shift(line, col);
            LexError::UnterminatedBlockComment { line, col }
        }
        LexError::InvalidNumber { raw, line, col } => {
            let (line, col) = shift(line, col);
            LexError::InvalidNumber { raw, line, col }
        }
        LexError::UnterminatedInterpExpr { line, col } => {
            let (line, col) = shift(line, col);
            LexError::UnterminatedInterpExpr { line, col }
        }
    }
}

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
    InvalidAssignmentTarget { line: usize, col: usize },

    #[error("Lexer error: {0}")]
    Lex(#[from] LexError),
}

impl ParseError {
    /// 出错位置 `(line, col)`；词法错误沿用其自身位置（T0.6/T7.2）。
    pub fn position(&self) -> (usize, usize) {
        match self {
            ParseError::UnexpectedToken { line, col, .. }
            | ParseError::UnexpectedEof { line, col, .. }
            | ParseError::InvalidAssignmentTarget { line, col } => (*line, *col),
            ParseError::Lex(err) => err.position(),
        }
    }
}
