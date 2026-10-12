use crate::error::{shift_lex_error, ParseError};
use ada_ast::*;
use ada_lexer::{InterpPart, Lexer, Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Lowest = 0,
    Assign = 1,       // =, +=, -=, etc.
    NullCoalesce = 2, // ??
    LogicalOr = 3,    // ||
    LogicalAnd = 4,   // &&
    BitOr = 5,        // |
    BitXor = 6,       // ^
    BitAnd = 7,       // &
    Equality = 8,     // ==, !=
    Relational = 9,   // <, <=, >, >=
    Shift = 10,       // <<, >>
    Term = 11,        // +, -
    Factor = 12,      // *, /, %
    Pipe = 13,        // |>
    Unary = 14,       // -, !, ~
    Call = 15,        // (), [], ., ?.
}

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
    }

    pub fn from_source(source: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        Ok(Self::new(tokens))
    }

    /// 解析一段**片段**并把它的位置映射回源文件（T3.4）。
    ///
    /// 用于字符串插值 `$"{...}"`：片段是在别处被切出来的子串，
    /// `base` 是它在源文件中的起始位置。映射规则：
    /// - 字节偏移整体加上 `base.start`；
    /// - 行号加上 `base.line - 1`；
    /// - 仅**片段第一行**的列号加上 `base.col - 1`（后续行本来就从列 1 开始）。
    pub fn from_fragment(fragment: &str, base: Span) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(fragment);
        let tokens = lexer.tokenize().map_err(|err| shift_lex_error(err, base))?;
        let shifted = tokens
            .into_iter()
            .map(|mut token| {
                let on_first_line = token.span.line == 1;
                token.span.start += base.start;
                token.span.end += base.start;
                token.span.line += base.line - 1;
                if on_first_line {
                    token.span.col += base.col - 1;
                }
                token
            })
            .collect();
        Ok(Self::new(shifted))
    }

    fn peek(&self) -> &Token {
        if self.cursor < self.tokens.len() {
            &self.tokens[self.cursor]
        } else {
            self.tokens.last().unwrap()
        }
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn peek_next(&self) -> &Token {
        if self.cursor + 1 < self.tokens.len() {
            &self.tokens[self.cursor + 1]
        } else {
            self.tokens.last().unwrap()
        }
    }

    fn advance(&mut self) -> Token {
        if self.cursor < self.tokens.len() {
            let tok = self.tokens[self.cursor].clone();
            self.cursor += 1;
            tok
        } else {
            self.tokens.last().unwrap().clone()
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn skip_newlines(&mut self) {
        while self.check(&TokenKind::Newline) {
            self.advance();
        }
    }

    fn consume_terminator(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Semicolon) {
            self.advance();
        }
    }

    fn expect(&mut self, expected: TokenKind, desc: &str) -> Result<Token, ParseError> {
        self.skip_newlines();
        let tok = self.peek();
        if std::mem::discriminant(&tok.kind) == std::mem::discriminant(&expected) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                found: tok.kind.to_string(),
                expected: desc.to_string(),
                line: tok.span.line,
                col: tok.span.col,
            })
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut stmts = Vec::new();
        self.consume_terminator();

        while !self.is_eof() {
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
            self.consume_terminator();
        }

        Ok(Program { stmts })
    }

    // ==========================================
    // Statements
    // ==========================================

    pub fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.skip_newlines();
        let tok = self.peek().clone();

        match &tok.kind {
            TokenKind::Let => self.parse_let_stmt(false),
            TokenKind::Mut => self.parse_let_stmt(true),
            TokenKind::Async => {
                self.advance(); // consume `async`
                self.expect(TokenKind::Fn, "fn after async")?;
                self.parse_fn_stmt(true, tok.span)
            }
            TokenKind::Fn => {
                self.advance(); // consume `fn`
                self.parse_fn_stmt(false, tok.span)
            }
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Break => {
                let tok = self.advance();
                self.consume_terminator();
                Ok(Stmt::Break { span: tok.span })
            }
            TokenKind::Continue => {
                let tok = self.advance();
                self.consume_terminator();
                Ok(Stmt::Continue { span: tok.span })
            }
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_let_stmt(&mut self, is_mut_prefix: bool) -> Result<Stmt, ParseError> {
        let start_tok = self.advance(); // consume `let` or `mut`
        let mut is_mut = is_mut_prefix;

        if !is_mut_prefix && self.check(&TokenKind::Mut) {
            self.advance(); // consume `mut` after `let`
            is_mut = true;
        }

        let name_tok = self.expect_ident("variable name")?;
        let name = match name_tok.kind {
            TokenKind::Ident(s) => s,
            _ => unreachable!(),
        };

        let mut type_ann = None;
        if self.check(&TokenKind::Colon) {
            self.advance();
            let type_tok = self.expect_ident("type annotation")?;
            if let TokenKind::Ident(t) = type_tok.kind {
                type_ann = Some(t);
            }
        }

        let mut init = None;
        if self.check(&TokenKind::Eq) {
            self.advance(); // consume '='
            self.skip_newlines();
            init = Some(self.parse_expr()?);
        }

        let end_span = init.as_ref().map(|e| e.span()).unwrap_or(name_tok.span);
        self.consume_terminator();

        Ok(Stmt::Let {
            name,
            is_mut,
            type_ann,
            init,
            span: start_tok.span.merge(end_span),
        })
    }

    fn parse_fn_stmt(&mut self, is_async: bool, start_span: Span) -> Result<Stmt, ParseError> {
        let name_tok = self.expect_ident("function name")?;
        let name = match name_tok.kind {
            TokenKind::Ident(s) => s,
            _ => unreachable!(),
        };

        self.expect(TokenKind::LParen, "(")?;
        let params = self.parse_param_list()?;
        self.expect(TokenKind::RParen, ")")?;

        let mut ret_type = None;
        if self.check(&TokenKind::Colon) || self.check(&TokenKind::ThinArrow) {
            self.advance();
            let type_tok = self.expect_ident("return type")?;
            if let TokenKind::Ident(t) = type_tok.kind {
                ret_type = Some(t);
            }
        }

        let body = if self.check(&TokenKind::Eq) {
            // Single-line expression function: fn add(x, y) = x + y
            self.advance();
            self.skip_newlines();
            let expr = self.parse_expr()?;
            self.consume_terminator();
            expr
        } else if self.check(&TokenKind::LBrace) {
            // Block body function: fn add(x, y) { ... }
            self.parse_block_expr()?
        } else {
            let next = self.peek();
            return Err(ParseError::UnexpectedToken {
                found: next.kind.to_string(),
                expected: "'=' or '{' for function body".to_string(),
                line: next.span.line,
                col: next.span.col,
            });
        };

        let span = start_span.merge(body.span());
        Ok(Stmt::Fn {
            name,
            is_async,
            params,
            ret_type,
            body,
            span,
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::RParen) && !self.is_eof() {
            let name_tok = self.expect_ident("parameter name")?;
            let name = match name_tok.kind {
                TokenKind::Ident(s) => s,
                _ => unreachable!(),
            };

            let mut type_ann = None;
            if self.check(&TokenKind::Colon) {
                self.advance();
                let type_tok = self.expect_ident("parameter type")?;
                if let TokenKind::Ident(t) = type_tok.kind {
                    type_ann = Some(t);
                }
            }

            params.push(Param {
                name,
                type_ann,
                span: name_tok.span,
            });

            if self.check(&TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }

        Ok(params)
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        let while_tok = self.advance(); // consume `while`
        self.skip_newlines();
        let cond = self.parse_expr()?;
        let body = self.parse_block_expr()?;
        let span = while_tok.span.merge(body.span());
        Ok(Stmt::While { cond, body, span })
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let for_tok = self.advance(); // consume `for`
        self.skip_newlines();

        let var = if self.check(&TokenKind::LBracket) {
            self.advance(); // consume '['
            let mut vars = Vec::new();
            while !self.check(&TokenKind::RBracket) && !self.is_eof() {
                let id_tok = self.expect_ident("destructured loop variable")?;
                if let TokenKind::Ident(s) = id_tok.kind {
                    vars.push(s);
                }
                if self.check(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RBracket, "]")?;
            ForVar::Tuple(vars)
        } else {
            let var_tok = self.expect_ident("loop variable")?;
            let name = match var_tok.kind {
                TokenKind::Ident(s) => s,
                _ => unreachable!(),
            };
            ForVar::Single(name)
        };

        self.expect(TokenKind::In, "'in'")?;
        self.skip_newlines();
        let iter = self.parse_expr()?;
        let body = self.parse_block_expr()?;
        let span = for_tok.span.merge(body.span());
        Ok(Stmt::For {
            var,
            iter,
            body,
            span,
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        let ret_tok = self.advance(); // consume `return` / `ret`
        let mut value = None;

        if !matches!(
            self.peek_kind(),
            TokenKind::Newline | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
        ) {
            value = Some(self.parse_expr()?);
        }

        let end_span = value.as_ref().map(|v| v.span()).unwrap_or(ret_tok.span);
        self.consume_terminator();
        Ok(Stmt::Return {
            value,
            span: ret_tok.span.merge(end_span),
        })
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt, ParseError> {
        let expr = self.parse_expr()?;
        let mut has_semicolon = false;
        if self.check(&TokenKind::Semicolon) {
            self.advance();
            has_semicolon = true;
        }
        self.consume_terminator();
        let span = expr.span();
        Ok(Stmt::Expr {
            expr,
            has_semicolon,
            span,
        })
    }

    // ==========================================
    // Expressions (Pratt Parser)
    // ==========================================

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_precedence(Precedence::Lowest)
    }

    fn parse_expr_precedence(&mut self, prec: Precedence) -> Result<Expr, ParseError> {
        self.skip_newlines();
        let mut lhs = self.parse_prefix()?;

        while !self.is_eof() {
            // If next token is Newline, check if the token after it is an infix continuation operator
            if self.check(&TokenKind::Newline) {
                let next_tok = self.peek_next();
                if self.is_continuation_op(&next_tok.kind) {
                    self.advance(); // consume newline to continue expression
                } else {
                    break;
                }
            }

            let next_prec = self.get_infix_precedence(self.peek_kind());
            if prec >= next_prec {
                break;
            }
            lhs = self.parse_infix(lhs)?;
        }

        Ok(lhs)
    }

    fn is_continuation_op(&self, kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::PipeGreater
                | TokenKind::QuestionQuestion
                | TokenKind::Dot
                | TokenKind::QuestionDot
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::AmpAmp
                | TokenKind::PipePipe
        )
    }

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let tok = self.advance();

        match tok.kind {
            TokenKind::Int(n) => Ok(Expr::Literal(Literal::Int(n), tok.span)),
            TokenKind::Float(n) => Ok(Expr::Literal(Literal::Float(n), tok.span)),
            TokenKind::String(s) => Ok(Expr::Literal(Literal::String(s), tok.span)),
            TokenKind::Bool(b) => Ok(Expr::Literal(Literal::Bool(b), tok.span)),
            TokenKind::Nil => Ok(Expr::Literal(Literal::Nil, tok.span)),
            TokenKind::InterpString(parts) => self.parse_interp_string(parts, tok.span),

            TokenKind::Ident(name) => {
                // Check for single parameter arrow function: `x => expr`
                if self.check(&TokenKind::FatArrow) {
                    self.advance(); // consume '=>'
                    self.skip_newlines();
                    let body = self.parse_expr()?;
                    let span = tok.span.merge(body.span());
                    return Ok(Expr::Closure {
                        params: vec![Param {
                            name,
                            type_ann: None,
                            span: tok.span,
                        }],
                        body: Box::new(body),
                        span,
                    });
                }
                Ok(Expr::Ident(name, tok.span))
            }

            TokenKind::Minus => {
                let expr = self.parse_expr_precedence(Precedence::Unary)?;
                let span = tok.span.merge(expr.span());
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Bang => {
                let expr = self.parse_expr_precedence(Precedence::Unary)?;
                let span = tok.span.merge(expr.span());
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Tilde => {
                let expr = self.parse_expr_precedence(Precedence::Unary)?;
                let span = tok.span.merge(expr.span());
                Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                    span,
                })
            }

            TokenKind::Await => {
                let expr = self.parse_expr_precedence(Precedence::Unary)?;
                let span = tok.span.merge(expr.span());
                Ok(Expr::Await {
                    expr: Box::new(expr),
                    span,
                })
            }

            TokenKind::Spawn => {
                let expr = self.parse_expr_precedence(Precedence::Unary)?;
                let span = tok.span.merge(expr.span());
                Ok(Expr::Spawn {
                    expr: Box::new(expr),
                    span,
                })
            }

            TokenKind::LParen => self.parse_paren_or_closure(tok.span),
            TokenKind::LBracket => self.parse_array_literal(tok.span),
            TokenKind::LBrace => self.parse_brace_expr(tok.span),
            TokenKind::If => self.parse_if_expr(tok.span),
            TokenKind::Match => self.parse_match_expr(tok.span),

            _ => Err(ParseError::UnexpectedToken {
                found: tok.kind.to_string(),
                expected: "expression".to_string(),
                line: tok.span.line,
                col: tok.span.col,
            }),
        }
    }

    fn parse_interp_string(&self, parts: Vec<InterpPart>, span: Span) -> Result<Expr, ParseError> {
        let mut ast_parts = Vec::new();

        for part in parts {
            match part {
                InterpPart::Lit(s) => ast_parts.push(StringPart::Lit(s)),
                InterpPart::Code(code, span) => {
                    // T3.4：把片段内解析出的位置映射回源文件，使插值里的错误
                    // 也能指向正确行列
                    let mut sub_parser = Parser::from_fragment(&code, span)?;
                    let expr = sub_parser.parse_expr()?;
                    ast_parts.push(StringPart::Expr(expr));
                }
            }
        }

        Ok(Expr::StringInterp {
            parts: ast_parts,
            span,
        })
    }

    fn parse_paren_or_closure(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        self.skip_newlines();

        // Check for 0-arg lambda: `() => expr`
        if self.check(&TokenKind::RParen) {
            let rparen = self.advance();
            if self.check(&TokenKind::FatArrow) {
                self.advance(); // consume `=>`
                self.skip_newlines();
                let body = self.parse_expr()?;
                let span = start_span.merge(body.span());
                return Ok(Expr::Closure {
                    params: Vec::new(),
                    body: Box::new(body),
                    span,
                });
            } else {
                // Empty unit literal / nil
                return Ok(Expr::Literal(Literal::Nil, start_span.merge(rparen.span)));
            }
        }

        // Could be `(expr)` OR `(param1, param2) => expr`
        let first_expr = self.parse_expr()?;

        // If next is `,` or we can check if it's closure params:
        if self.check(&TokenKind::Comma) {
            // Definitely multiple params for a closure
            let mut params = Vec::new();
            if let Expr::Ident(name, s) = first_expr {
                params.push(Param {
                    name,
                    type_ann: None,
                    span: s,
                });
            } else {
                return Err(ParseError::UnexpectedToken {
                    found: "non-identifier in lambda parameter list".to_string(),
                    expected: "identifier".to_string(),
                    line: first_expr.span().line,
                    col: first_expr.span().col,
                });
            }

            while self.check(&TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
                if self.check(&TokenKind::RParen) {
                    break;
                }
                let param_tok = self.expect_ident("parameter name")?;
                if let TokenKind::Ident(name) = param_tok.kind {
                    params.push(Param {
                        name,
                        type_ann: None,
                        span: param_tok.span,
                    });
                }
            }

            self.expect(TokenKind::RParen, ")")?;
            self.expect(TokenKind::FatArrow, "=>")?;
            self.skip_newlines();
            let body = self.parse_expr()?;
            let span = start_span.merge(body.span());
            return Ok(Expr::Closure {
                params,
                body: Box::new(body),
                span,
            });
        }

        let rparen = self.expect(TokenKind::RParen, ")")?;

        // Check if `(x) => expr`
        if self.check(&TokenKind::FatArrow) {
            if let Expr::Ident(name, s) = first_expr {
                self.advance(); // consume `=>`
                self.skip_newlines();
                let body = self.parse_expr()?;
                let span = start_span.merge(body.span());
                return Ok(Expr::Closure {
                    params: vec![Param {
                        name,
                        type_ann: None,
                        span: s,
                    }],
                    body: Box::new(body),
                    span,
                });
            }
        }

        // Otherwise simple grouped expression
        let _span = start_span.merge(rparen.span);
        Ok(first_expr)
    }

    fn parse_array_literal(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        let mut items = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::RBracket) && !self.is_eof() {
            let item = self.parse_expr()?;
            items.push(item);

            self.skip_newlines();
            if self.check(&TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }

        let rbracket = self.expect(TokenKind::RBracket, "]")?;
        Ok(Expr::Array {
            items,
            span: start_span.merge(rbracket.span),
        })
    }

    /// `{` 之后是 map 字面量还是块？（决策 D4 的唯一判定点）
    ///
    /// **前置条件**：游标必须已经停在 `{` **之后**（`parse_brace_expr` 的调用约定）。
    ///
    /// Map 形状：`{ key: val }`、`{ key, ... }`、`{ key }`、`{}`（空 map）。
    /// 语句（`let`/`for`/…）一定走块。所有"括号体"位置（表达式、`match` 分支体、
    /// 闭包体）最终都经过 `parse_brace_expr`，保证判定一致（T3.3 修订）。
    fn brace_starts_map(&self) -> bool {
        // 空 `{}` 判为 map（D4）
        if self.check(&TokenKind::RBrace) {
            return true;
        }
        matches!(
            (self.peek_kind(), &self.peek_next().kind),
            (TokenKind::Ident(_), TokenKind::Colon)
                | (TokenKind::String(_), TokenKind::Colon)
                | (TokenKind::Ident(_), TokenKind::Comma)
                | (TokenKind::Ident(_), TokenKind::RBrace)
        )
    }

    fn parse_brace_expr(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        self.skip_newlines();

        // 1. Check if empty map `{}` or empty block `{}`
        if self.check(&TokenKind::RBrace) {
            let rbrace = self.advance();
            // Default empty `{}` to empty map
            return Ok(Expr::Map {
                entries: Vec::new(),
                span: start_span.merge(rbrace.span),
            });
        }

        // 2. Disambiguate Map vs Block（与 `match` 分支体共用同一判定）
        if self.brace_starts_map() {
            self.parse_map_literal_tail(start_span)
        } else {
            self.parse_block_expr_tail(start_span)
        }
    }

    fn parse_map_literal_tail(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        let mut entries = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            self.skip_newlines();
            let key_tok = self.advance();
            let key = match key_tok.kind {
                TokenKind::Ident(ref s) => Expr::Literal(Literal::String(s.clone()), key_tok.span),
                TokenKind::String(ref s) => Expr::Literal(Literal::String(s.clone()), key_tok.span),
                other => {
                    return Err(ParseError::UnexpectedToken {
                        found: other.to_string(),
                        expected: "map key (identifier or string)".to_string(),
                        line: key_tok.span.line,
                        col: key_tok.span.col,
                    });
                }
            };

            let value = if self.check(&TokenKind::Colon) {
                self.advance(); // consume ':'
                self.skip_newlines();
                self.parse_expr()?
            } else {
                // Shorthand `{ name }` -> `{ "name": name }`
                if let TokenKind::Ident(name) = key_tok.kind {
                    Expr::Ident(name, key_tok.span)
                } else {
                    return Err(ParseError::UnexpectedToken {
                        found: ":".to_string(),
                        expected: "colon ':' after map key".to_string(),
                        line: key_tok.span.line,
                        col: key_tok.span.col,
                    });
                }
            };

            entries.push((key, value));

            self.skip_newlines();
            if self.check(&TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }

        let rbrace = self.expect(TokenKind::RBrace, "}")?;
        Ok(Expr::Map {
            entries,
            span: start_span.merge(rbrace.span),
        })
    }

    pub fn parse_block_expr(&mut self) -> Result<Expr, ParseError> {
        let lbrace = self.expect(TokenKind::LBrace, "{")?;
        self.parse_block_expr_tail(lbrace.span)
    }

    fn parse_block_expr_tail(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        let mut stmts = Vec::new();
        let mut trailing = None;
        self.consume_terminator();

        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            let stmt = self.parse_stmt()?;

            // Check if this is the last statement without semicolon (trailing expression)
            if self.check(&TokenKind::RBrace) {
                if let Stmt::Expr {
                    expr,
                    has_semicolon: false,
                    ..
                } = stmt
                {
                    trailing = Some(Box::new(expr));
                    break;
                }
            }

            stmts.push(stmt);
            self.consume_terminator();
        }

        let rbrace = self.expect(TokenKind::RBrace, "}")?;
        Ok(Expr::Block {
            stmts,
            trailing,
            span: start_span.merge(rbrace.span),
        })
    }

    fn parse_if_expr(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        self.skip_newlines();
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block_expr()?;
        self.skip_newlines();

        let mut else_branch = None;
        if self.check(&TokenKind::Else) {
            self.advance(); // consume `else`
            self.skip_newlines();
            if self.check(&TokenKind::If) {
                let else_if_tok = self.advance();
                else_branch = Some(Box::new(self.parse_if_expr(else_if_tok.span)?));
            } else {
                else_branch = Some(Box::new(self.parse_block_expr()?));
            }
        }

        let end_span = else_branch
            .as_ref()
            .map(|e| e.span())
            .unwrap_or(then_branch.span());
        Ok(Expr::If {
            cond: Box::new(cond),
            then_branch: Box::new(then_branch),
            else_branch,
            span: start_span.merge(end_span),
        })
    }

    fn parse_match_expr(&mut self, start_span: Span) -> Result<Expr, ParseError> {
        self.skip_newlines();
        let target = self.parse_expr()?;
        self.expect(TokenKind::LBrace, "{")?;
        self.consume_terminator();

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_eof() {
            let arm = self.parse_match_arm()?;
            arms.push(arm);
            self.consume_terminator();
        }

        let rbrace = self.expect(TokenKind::RBrace, "}")?;
        Ok(Expr::Match {
            target: Box::new(target),
            arms,
            span: start_span.merge(rbrace.span),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm, ParseError> {
        self.skip_newlines();
        let pattern_tok = self.peek().clone();
        let pattern = match &pattern_tok.kind {
            TokenKind::Ident(s) if s == "_" => {
                self.advance();
                Pattern::Wildcard
            }
            TokenKind::Ident(s) => {
                self.advance();
                Pattern::Ident(s.clone())
            }
            TokenKind::Int(n) => {
                self.advance();
                Pattern::Literal(Literal::Int(*n))
            }
            // T3.7 修订：负数模式字面量（`-5 => …`）。
            // 此前 `-` 落到"未知模式"分支报语法错，而 `< -5` 之类的比较模式却可以写，
            // 口径不一致。负数与整数共用同一条模式路径，因此大负数同样精确。
            TokenKind::Minus => {
                self.advance();
                let number_tok = self.peek().clone();
                match &number_tok.kind {
                    TokenKind::Int(n) => {
                        self.advance();
                        Pattern::Literal(Literal::Int(-*n))
                    }
                    TokenKind::Float(n) => {
                        self.advance();
                        Pattern::Literal(Literal::Float(-*n))
                    }
                    other => {
                        return Err(ParseError::UnexpectedToken {
                            found: other.to_string(),
                            expected: "number after '-' in match pattern".to_string(),
                            line: number_tok.span.line,
                            col: number_tok.span.col,
                        })
                    }
                }
            }
            TokenKind::Float(n) => {
                self.advance();
                Pattern::Literal(Literal::Float(*n))
            }
            TokenKind::String(s) => {
                self.advance();
                Pattern::Literal(Literal::String(s.clone()))
            }
            TokenKind::Bool(b) => {
                self.advance();
                Pattern::Literal(Literal::Bool(*b))
            }
            TokenKind::Gt | TokenKind::GtEq | TokenKind::Lt | TokenKind::LtEq => {
                let op = match pattern_tok.kind {
                    TokenKind::Gt => BinOp::Gt,
                    TokenKind::GtEq => BinOp::Ge,
                    TokenKind::Lt => BinOp::Lt,
                    TokenKind::LtEq => BinOp::Le,
                    _ => unreachable!(),
                };
                self.advance();
                let val = self.parse_expr_precedence(Precedence::Equality)?;
                Pattern::Compare {
                    op,
                    value: Box::new(val),
                }
            }
            _ => {
                return Err(ParseError::UnexpectedToken {
                    found: pattern_tok.kind.to_string(),
                    expected: "match pattern".to_string(),
                    line: pattern_tok.span.line,
                    col: pattern_tok.span.col,
                });
            }
        };

        let mut guard = None;
        if self.check(&TokenKind::If) {
            self.advance();
            guard = Some(self.parse_expr()?);
        }

        self.expect(TokenKind::FatArrow, "=>")?;
        self.skip_newlines();

        // T3.3 修订：分支体与普通表达式使用同一套括号消歧（`{ k: v }` 是 map 字面量，
        // `{ let a = 1; a }` 才是块），不再无条件把 `{` 当块。
        // 注意：`brace_starts_map()` 的契约是"游标已停在 `{` 之后"，
        // 因此这里先消费 `{` 再交给 `parse_brace_expr` 统一判定。
        let body = if self.check(&TokenKind::LBrace) {
            let lbrace = self.advance();
            self.parse_brace_expr(lbrace.span)?
        } else {
            self.parse_expr()?
        };

        if self.check(&TokenKind::Comma) {
            self.advance();
        }

        let span = pattern_tok.span.merge(body.span());
        Ok(MatchArm {
            pattern,
            guard,
            body,
            span,
        })
    }

    // ==========================================
    // Infix Expressions
    // ==========================================

    fn get_infix_precedence(&self, kind: &TokenKind) -> Precedence {
        match kind {
            TokenKind::Eq
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq => Precedence::Assign,

            TokenKind::QuestionQuestion => Precedence::NullCoalesce,
            TokenKind::PipePipe => Precedence::LogicalOr,
            TokenKind::AmpAmp => Precedence::LogicalAnd,
            TokenKind::Pipe => Precedence::BitOr,
            TokenKind::Caret => Precedence::BitXor,
            TokenKind::Amp => Precedence::BitAnd,
            TokenKind::EqEq | TokenKind::BangEq => Precedence::Equality,
            TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq => {
                Precedence::Relational
            }
            TokenKind::LtLt | TokenKind::GtGt => Precedence::Shift,
            TokenKind::Plus | TokenKind::Minus => Precedence::Term,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Precedence::Factor,
            TokenKind::PipeGreater => Precedence::Pipe,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::Dot | TokenKind::QuestionDot => {
                Precedence::Call
            }

            _ => Precedence::Lowest,
        }
    }

    fn parse_infix(&mut self, lhs: Expr) -> Result<Expr, ParseError> {
        let op_tok = self.advance();

        match op_tok.kind {
            // Assignment: right-associative
            TokenKind::Eq
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq => {
                self.skip_newlines();
                let rhs = self.parse_expr_precedence(Precedence::Assign)?;
                let assign_op = match op_tok.kind {
                    TokenKind::Eq => AssignOp::Assign,
                    TokenKind::PlusEq => AssignOp::AddAssign,
                    TokenKind::MinusEq => AssignOp::SubAssign,
                    TokenKind::StarEq => AssignOp::MulAssign,
                    TokenKind::SlashEq => AssignOp::DivAssign,
                    TokenKind::PercentEq => AssignOp::ModAssign,
                    _ => unreachable!(),
                };

                // Validate LHS
                match &lhs {
                    Expr::Ident(..) | Expr::Index { .. } | Expr::FieldAccess { .. } => {}
                    _ => {
                        return Err(ParseError::InvalidAssignmentTarget {
                            line: lhs.span().line,
                            col: lhs.span().col,
                        });
                    }
                }

                let span = lhs.span().merge(rhs.span());
                Ok(Expr::Assign {
                    target: Box::new(lhs),
                    op: assign_op,
                    value: Box::new(rhs),
                    span,
                })
            }

            TokenKind::PipeGreater => {
                self.skip_newlines();
                let rhs = self.parse_expr_precedence(Precedence::Pipe)?;
                let span = lhs.span().merge(rhs.span());
                Ok(Expr::Pipe {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                })
            }

            TokenKind::QuestionQuestion => {
                self.skip_newlines();
                let rhs = self.parse_expr_precedence(Precedence::NullCoalesce)?;
                let span = lhs.span().merge(rhs.span());
                Ok(Expr::NullCoalesce {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                })
            }

            // Function call
            TokenKind::LParen => {
                let mut args = Vec::new();
                self.skip_newlines();

                while !self.check(&TokenKind::RParen) && !self.is_eof() {
                    let arg = self.parse_expr()?;
                    args.push(arg);

                    self.skip_newlines();
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    } else {
                        break;
                    }
                }

                let rparen = self.expect(TokenKind::RParen, ")")?;
                let span = lhs.span().merge(rparen.span);
                Ok(Expr::Call {
                    callee: Box::new(lhs),
                    args,
                    span,
                })
            }

            // Index access
            TokenKind::LBracket => {
                self.skip_newlines();
                let index = self.parse_expr()?;
                let rbracket = self.expect(TokenKind::RBracket, "]")?;
                let span = lhs.span().merge(rbracket.span);
                Ok(Expr::Index {
                    target: Box::new(lhs),
                    index: Box::new(index),
                    span,
                })
            }

            // Field access or Suffix .await
            TokenKind::Dot => {
                if self.check(&TokenKind::Await) {
                    let await_tok = self.advance();
                    let span = lhs.span().merge(await_tok.span);
                    return Ok(Expr::Await {
                        expr: Box::new(lhs),
                        span,
                    });
                }
                let field_tok = self.expect_ident("field name")?;
                let field = match field_tok.kind {
                    TokenKind::Ident(s) => s,
                    _ => unreachable!(),
                };
                let span = lhs.span().merge(field_tok.span);
                Ok(Expr::FieldAccess {
                    target: Box::new(lhs),
                    field,
                    optional: false,
                    span,
                })
            }

            // Optional chaining access
            TokenKind::QuestionDot => {
                let field_tok = self.expect_ident("field name")?;
                let field = match field_tok.kind {
                    TokenKind::Ident(s) => s,
                    _ => unreachable!(),
                };
                let span = lhs.span().merge(field_tok.span);
                Ok(Expr::FieldAccess {
                    target: Box::new(lhs),
                    field,
                    optional: true,
                    span,
                })
            }

            // Binary arithmetic / logic / bitwise operators
            _ => {
                let bin_op = match op_tok.kind {
                    TokenKind::Plus => BinOp::Add,
                    TokenKind::Minus => BinOp::Sub,
                    TokenKind::Star => BinOp::Mul,
                    TokenKind::Slash => BinOp::Div,
                    TokenKind::Percent => BinOp::Mod,
                    TokenKind::EqEq => BinOp::Eq,
                    TokenKind::BangEq => BinOp::Ne,
                    TokenKind::Lt => BinOp::Lt,
                    TokenKind::LtEq => BinOp::Le,
                    TokenKind::Gt => BinOp::Gt,
                    TokenKind::GtEq => BinOp::Ge,
                    TokenKind::AmpAmp => BinOp::And,
                    TokenKind::PipePipe => BinOp::Or,
                    TokenKind::Amp => BinOp::BitAnd,
                    TokenKind::Pipe => BinOp::BitOr,
                    TokenKind::Caret => BinOp::BitXor,
                    TokenKind::LtLt => BinOp::Shl,
                    TokenKind::GtGt => BinOp::Shr,
                    _ => unreachable!(),
                };

                let op_prec = self.get_infix_precedence(&op_tok.kind);
                self.skip_newlines();
                let rhs = self.parse_expr_precedence(op_prec)?;
                let span = lhs.span().merge(rhs.span());
                Ok(Expr::Binary {
                    op: bin_op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                })
            }
        }
    }

    fn expect_ident(&mut self, desc: &str) -> Result<Token, ParseError> {
        let tok = self.peek();
        if matches!(tok.kind, TokenKind::Ident(_)) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                found: tok.kind.to_string(),
                expected: desc.to_string(),
                line: tok.span.line,
                col: tok.span.col,
            })
        }
    }
}
