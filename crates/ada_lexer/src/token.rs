use ada_ast::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    Lit(String),
    Code(String, Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    InterpString(Vec<InterpPart>),
    Bool(bool),
    Nil,

    // Identifiers & Keywords
    Ident(String),
    Fn,
    Async,
    Await,
    Spawn,
    Let,
    Mut,
    If,
    Else,
    Match,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    Import,
    From,

    // Operators & Punctuation
    Plus,             // +
    Minus,            // -
    Star,             // *
    Slash,            // /
    Percent,          // %
    Eq,               // =
    EqEq,             // ==
    BangEq,           // !=
    Lt,               // <
    LtEq,             // <=
    Gt,               // >
    GtEq,             // >=
    Bang,             // !
    AmpAmp,           // &&
    PipePipe,         // ||
    Amp,              // &
    Pipe,             // |
    Caret,            // ^
    Tilde,            // ~
    LtLt,             // <<
    GtGt,             // >>
    PlusEq,           // +=
    MinusEq,          // -=
    StarEq,           // *=
    SlashEq,          // /=
    PercentEq,        // %=
    PipeGreater,      // |>
    QuestionQuestion, // ??
    QuestionDot,      // ?.
    FatArrow,         // =>
    ThinArrow,        // ->

    // Delimiters
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    LBrace,    // {
    RBrace,    // }
    Comma,     // ,
    Colon,     // :
    Dot,       // .
    Semicolon, // ;
    Newline,   // \n

    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Int(n) => write!(f, "{n}"),
            TokenKind::Float(n) => write!(f, "{n}"),
            TokenKind::String(s) => write!(f, "\"{s}\""),
            TokenKind::InterpString(_) => write!(f, "<interpolated string>"),
            TokenKind::Bool(b) => write!(f, "{b}"),
            TokenKind::Nil => write!(f, "nil"),
            TokenKind::Ident(id) => write!(f, "{id}"),
            TokenKind::Fn => write!(f, "fn"),
            TokenKind::Async => write!(f, "async"),
            TokenKind::Await => write!(f, "await"),
            TokenKind::Spawn => write!(f, "spawn"),
            TokenKind::Let => write!(f, "let"),
            TokenKind::Mut => write!(f, "mut"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::Match => write!(f, "match"),
            TokenKind::While => write!(f, "while"),
            TokenKind::For => write!(f, "for"),
            TokenKind::In => write!(f, "in"),
            TokenKind::Return => write!(f, "return"),
            TokenKind::Break => write!(f, "break"),
            TokenKind::Continue => write!(f, "continue"),
            TokenKind::Import => write!(f, "import"),
            TokenKind::From => write!(f, "from"),
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Star => write!(f, "*"),
            TokenKind::Slash => write!(f, "/"),
            TokenKind::Percent => write!(f, "%"),
            TokenKind::Eq => write!(f, "="),
            TokenKind::EqEq => write!(f, "=="),
            TokenKind::BangEq => write!(f, "!="),
            TokenKind::Lt => write!(f, "<"),
            TokenKind::LtEq => write!(f, "<="),
            TokenKind::Gt => write!(f, ">"),
            TokenKind::GtEq => write!(f, ">="),
            TokenKind::Bang => write!(f, "!"),
            TokenKind::AmpAmp => write!(f, "&&"),
            TokenKind::PipePipe => write!(f, "||"),
            TokenKind::Amp => write!(f, "&"),
            TokenKind::Pipe => write!(f, "|"),
            TokenKind::Caret => write!(f, "^"),
            TokenKind::Tilde => write!(f, "~"),
            TokenKind::LtLt => write!(f, "<<"),
            TokenKind::GtGt => write!(f, ">>"),
            TokenKind::PlusEq => write!(f, "+="),
            TokenKind::MinusEq => write!(f, "-="),
            TokenKind::StarEq => write!(f, "*="),
            TokenKind::SlashEq => write!(f, "/="),
            TokenKind::PercentEq => write!(f, "%="),
            TokenKind::PipeGreater => write!(f, "|>"),
            TokenKind::QuestionQuestion => write!(f, "??"),
            TokenKind::QuestionDot => write!(f, "?."),
            TokenKind::FatArrow => write!(f, "=>"),
            TokenKind::ThinArrow => write!(f, "->"),
            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::LBracket => write!(f, "["),
            TokenKind::RBracket => write!(f, "]"),
            TokenKind::LBrace => write!(f, "{{"),
            TokenKind::RBrace => write!(f, "}}"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Dot => write!(f, "."),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::Newline => write!(f, "\\n"),
            TokenKind::Eof => write!(f, "<EOF>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}
