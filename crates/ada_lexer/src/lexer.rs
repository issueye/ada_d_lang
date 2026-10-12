use crate::token::{InterpPart, Token, TokenKind};
use ada_ast::Span;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum LexError {
    #[error("Unexpected character '{ch}' at line {line}, col {col}")]
    UnexpectedChar { ch: char, line: usize, col: usize },

    #[error("Unterminated string literal at line {line}, col {col}")]
    UnterminatedString { line: usize, col: usize },

    #[error("Unterminated block comment at line {line}, col {col}")]
    UnterminatedBlockComment { line: usize, col: usize },

    #[error("Invalid number literal '{raw}' at line {line}, col {col}")]
    InvalidNumber { raw: String, line: usize, col: usize },

    #[error("Unterminated expression in string interpolation at line {line}, col {col}")]
    UnterminatedInterpExpr { line: usize, col: usize },
}

pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            source,
            chars,
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|(_, c)| *c)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.cursor + 1).map(|(_, c)| *c)
    }

    fn current_byte_pos(&self) -> usize {
        self.chars
            .get(self.cursor)
            .map(|(p, _)| *p)
            .unwrap_or(self.source.len())
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((_, c)) = self.chars.get(self.cursor) {
            let ch = *c;
            self.cursor += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(ch)
        } else {
            None
        }
    }

    fn matches(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens: Vec<Token> = Vec::new();

        while let Some(ch) = self.peek() {
            let start_byte = self.current_byte_pos();
            let start_line = self.line;
            let start_col = self.col;

            // 1. Whitespace & Newline
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
                continue;
            }

            if ch == '\n' {
                self.advance();
                // Check if last non-newline token can end a statement
                if let Some(last) = tokens.last() {
                    if Self::can_end_statement(&last.kind) {
                        let span = Span::new(start_byte, start_byte + 1, start_line, start_col);
                        // Avoid consecutive newline tokens
                        if last.kind != TokenKind::Newline {
                            tokens.push(Token::new(TokenKind::Newline, span));
                        }
                    }
                }
                continue;
            }

            // 2. Comments
            if ch == '/' && self.peek_next() == Some('/') {
                // Line comment
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }

            if ch == '/' && self.peek_next() == Some('*') {
                // Block comment
                self.advance(); // consume '/'
                self.advance(); // consume '*'
                let mut closed = false;
                while let Some(c) = self.advance() {
                    if c == '*' && self.peek() == Some('/') {
                        self.advance(); // consume '/'
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(LexError::UnterminatedBlockComment {
                        line: start_line,
                        col: start_col,
                    });
                }
                continue;
            }

            // 3. String Interpolation `$"..."`
            if ch == '$' && self.peek_next() == Some('"') {
                self.advance(); // consume '$'
                self.advance(); // consume '"'
                let token = self.lex_interpolated_string(start_byte, start_line, start_col)?;
                tokens.push(token);
                continue;
            }

            // 4. Regular String `"..."`
            if ch == '"' {
                self.advance(); // consume '"'
                let token = self.lex_regular_string(start_byte, start_line, start_col)?;
                tokens.push(token);
                continue;
            }

            // 5. Numbers
            if ch.is_ascii_digit() {
                let token = self.lex_number(start_byte, start_line, start_col)?;
                tokens.push(token);
                continue;
            }

            // 6. Identifiers and Keywords
            if ch.is_alphabetic() || ch == '_' {
                let token = self.lex_identifier(start_byte, start_line, start_col);
                tokens.push(token);
                continue;
            }

            // 7. Operators & Delimiters
            self.advance();
            let kind = match ch {
                '+' => {
                    if self.matches('=') {
                        TokenKind::PlusEq
                    } else {
                        TokenKind::Plus
                    }
                }
                '-' => {
                    if self.matches('>') {
                        TokenKind::ThinArrow
                    } else if self.matches('=') {
                        TokenKind::MinusEq
                    } else {
                        TokenKind::Minus
                    }
                }
                '*' => {
                    if self.matches('=') {
                        TokenKind::StarEq
                    } else {
                        TokenKind::Star
                    }
                }
                '/' => {
                    if self.matches('=') {
                        TokenKind::SlashEq
                    } else {
                        TokenKind::Slash
                    }
                }
                '%' => {
                    if self.matches('=') {
                        TokenKind::PercentEq
                    } else {
                        TokenKind::Percent
                    }
                }
                '=' => {
                    if self.matches('>') {
                        TokenKind::FatArrow
                    } else if self.matches('=') {
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    }
                }
                '!' => {
                    if self.matches('=') {
                        TokenKind::BangEq
                    } else {
                        TokenKind::Bang
                    }
                }
                '<' => {
                    if self.matches('<') {
                        TokenKind::LtLt
                    } else if self.matches('=') {
                        TokenKind::LtEq
                    } else {
                        TokenKind::Lt
                    }
                }
                '>' => {
                    if self.matches('>') {
                        TokenKind::GtGt
                    } else if self.matches('=') {
                        TokenKind::GtEq
                    } else {
                        TokenKind::Gt
                    }
                }
                '&' => {
                    if self.matches('&') {
                        TokenKind::AmpAmp
                    } else {
                        TokenKind::Amp
                    }
                }
                '|' => {
                    if self.matches('>') {
                        TokenKind::PipeGreater
                    } else if self.matches('|') {
                        TokenKind::PipePipe
                    } else {
                        TokenKind::Pipe
                    }
                }
                '?' => {
                    if self.matches('?') {
                        TokenKind::QuestionQuestion
                    } else if self.matches('.') {
                        TokenKind::QuestionDot
                    } else {
                        return Err(LexError::UnexpectedChar {
                            ch: '?',
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
                '^' => TokenKind::Caret,
                '~' => TokenKind::Tilde,
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '[' => TokenKind::LBracket,
                ']' => TokenKind::RBracket,
                '{' => TokenKind::LBrace,
                '}' => TokenKind::RBrace,
                ',' => TokenKind::Comma,
                ':' => TokenKind::Colon,
                '.' => TokenKind::Dot,
                ';' => TokenKind::Semicolon,
                _ => {
                    return Err(LexError::UnexpectedChar {
                        ch,
                        line: start_line,
                        col: start_col,
                    });
                }
            };

            let end_byte = self.current_byte_pos();
            tokens.push(Token::new(
                kind,
                Span::new(start_byte, end_byte, start_line, start_col),
            ));
        }

        // Add trailing newline if last token could end a statement and isn't already newline or semicolon
        if let Some(last) = tokens.last() {
            if Self::can_end_statement(&last.kind) && last.kind != TokenKind::Newline {
                tokens.push(Token::new(
                    TokenKind::Newline,
                    Span::new(self.source.len(), self.source.len(), self.line, self.col),
                ));
            }
        }

        tokens.push(Token::new(
            TokenKind::Eof,
            Span::new(self.source.len(), self.source.len(), self.line, self.col),
        ));

        Ok(tokens)
    }

    fn can_end_statement(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::String(_)
                | TokenKind::InterpString(_)
                | TokenKind::Bool(_)
                | TokenKind::Nil
                | TokenKind::Ident(_)
                | TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::RBrace
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
        )
    }

    fn lex_identifier(&mut self, start_byte: usize, start_line: usize, start_col: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let end_byte = self.current_byte_pos();
        let word = &self.source[start_byte..end_byte];
        let kind = match word {
            "fn" => TokenKind::Fn,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "spawn" => TokenKind::Spawn,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" | "ret" => TokenKind::Return,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "true" => TokenKind::Bool(true),
            "false" => TokenKind::Bool(false),
            "nil" | "null" => TokenKind::Nil,
            _ => TokenKind::Ident(word.to_string()),
        };

        Token::new(kind, Span::new(start_byte, end_byte, start_line, start_col))
    }

    fn lex_number(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, LexError> {
        let mut is_float = false;

        // Check for 0x / 0b
        if self.peek() == Some('0') {
            if self.peek_next() == Some('x') || self.peek_next() == Some('X') {
                self.advance(); // 0
                self.advance(); // x
                while let Some(c) = self.peek() {
                    if c.is_ascii_hexdigit() || c == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let end_byte = self.current_byte_pos();
                let raw = self.source[start_byte + 2..end_byte].replace('_', "");
                let val = i64::from_str_radix(&raw, 16).map_err(|_| LexError::InvalidNumber {
                    raw: self.source[start_byte..end_byte].to_string(),
                    line: start_line,
                    col: start_col,
                })?;
                return Ok(Token::new(
                    TokenKind::Int(val),
                    Span::new(start_byte, end_byte, start_line, start_col),
                ));
            } else if self.peek_next() == Some('b') || self.peek_next() == Some('B') {
                self.advance(); // 0
                self.advance(); // b
                while let Some(c) = self.peek() {
                    if c == '0' || c == '1' || c == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let end_byte = self.current_byte_pos();
                let raw = self.source[start_byte + 2..end_byte].replace('_', "");
                let val = i64::from_str_radix(&raw, 2).map_err(|_| LexError::InvalidNumber {
                    raw: self.source[start_byte..end_byte].to_string(),
                    line: start_line,
                    col: start_col,
                })?;
                return Ok(Token::new(
                    TokenKind::Int(val),
                    Span::new(start_byte, end_byte, start_line, start_col),
                ));
            }
        }

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else if c == '.' && self.peek_next().is_some_and(|next| next.is_ascii_digit()) {
                is_float = true;
                self.advance(); // consume '.'
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() || d == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
            } else if (c == 'e' || c == 'E') && is_float {
                self.advance();
                if self.peek() == Some('+') || self.peek() == Some('-') {
                    self.advance();
                }
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() || d == '_' {
                        self.advance();
                    } else {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        let end_byte = self.current_byte_pos();
        let raw = self.source[start_byte..end_byte].replace('_', "");

        let kind = if is_float {
            let val = raw.parse::<f64>().map_err(|_| LexError::InvalidNumber {
                raw: raw.clone(),
                line: start_line,
                col: start_col,
            })?;
            TokenKind::Float(val)
        } else {
            let val = raw.parse::<i64>().map_err(|_| LexError::InvalidNumber {
                raw: raw.clone(),
                line: start_line,
                col: start_col,
            })?;
            TokenKind::Int(val)
        };

        Ok(Token::new(
            kind,
            Span::new(start_byte, end_byte, start_line, start_col),
        ))
    }

    fn lex_regular_string(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, LexError> {
        let mut buf = String::new();

        while let Some(c) = self.advance() {
            match c {
                '"' => {
                    let end_byte = self.current_byte_pos();
                    return Ok(Token::new(
                        TokenKind::String(buf),
                        Span::new(start_byte, end_byte, start_line, start_col),
                    ));
                }
                '\\' => {
                    let esc = match self.advance() {
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some('r') => '\r',
                        Some('\\') => '\\',
                        Some('"') => '"',
                        Some('0') => '\0',
                        Some(other) => other,
                        None => {
                            return Err(LexError::UnterminatedString {
                                line: start_line,
                                col: start_col,
                            });
                        }
                    };
                    buf.push(esc);
                }
                _ => buf.push(c),
            }
        }

        Err(LexError::UnterminatedString {
            line: start_line,
            col: start_col,
        })
    }

    fn lex_interpolated_string(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, LexError> {
        let mut parts = Vec::new();
        let mut text_buf = String::new();

        while let Some(c) = self.peek() {
            match c {
                '"' => {
                    self.advance(); // consume closing quote
                    if !text_buf.is_empty() {
                        parts.push(InterpPart::Lit(text_buf));
                    }
                    let end_byte = self.current_byte_pos();
                    return Ok(Token::new(
                        TokenKind::InterpString(parts),
                        Span::new(start_byte, end_byte, start_line, start_col),
                    ));
                }
                '{' => {
                    self.advance(); // consume '{'
                    if !text_buf.is_empty() {
                        parts.push(InterpPart::Lit(text_buf.clone()));
                        text_buf.clear();
                    }

                    // Collect code until matching '}'
                    let code_start_byte = self.current_byte_pos();
                    let code_start_line = self.line;
                    let code_start_col = self.col;
                    let mut brace_depth = 1;
                    let mut code_str = String::new();

                    while let Some(inner) = self.advance() {
                        if inner == '{' {
                            brace_depth += 1;
                            code_str.push(inner);
                        } else if inner == '}' {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                            code_str.push(inner);
                        } else {
                            code_str.push(inner);
                        }
                    }

                    if brace_depth != 0 {
                        return Err(LexError::UnterminatedInterpExpr {
                            line: code_start_line,
                            col: code_start_col,
                        });
                    }

                    let code_end_byte = self.current_byte_pos();
                    parts.push(InterpPart::Code(
                        code_str,
                        Span::new(
                            code_start_byte,
                            code_end_byte,
                            code_start_line,
                            code_start_col,
                        ),
                    ));
                }
                '\\' => {
                    self.advance();
                    match self.advance() {
                        Some('n') => text_buf.push('\n'),
                        Some('t') => text_buf.push('\t'),
                        Some('r') => text_buf.push('\r'),
                        Some('\\') => text_buf.push('\\'),
                        Some('"') => text_buf.push('"'),
                        Some('{') => text_buf.push('{'),
                        Some(other) => text_buf.push(other),
                        None => {
                            return Err(LexError::UnterminatedString {
                                line: start_line,
                                col: start_col,
                            });
                        }
                    }
                }
                _ => {
                    self.advance();
                    text_buf.push(c);
                }
            }
        }

        Err(LexError::UnterminatedString {
            line: start_line,
            col: start_col,
        })
    }
}
