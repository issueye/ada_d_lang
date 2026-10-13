pub mod lexer;
pub mod token;

pub use lexer::{LexError, Lexer};
pub use token::{InterpPart, Token, TokenKind};

/// 语言关键字与保留字（含别名），供编辑器语法高亮、AI 提示模板等工具使用（T7.1/T7.4）。
///
/// **权威性来源**：`crates/ada_lexer/src/lexer.rs` 的实现。词法测试
/// `keywords_match_the_lexer` 会逐个验证"表内每个词都不被词法解析成标识符"，
/// 以及"典型非关键字仍被解析成标识符"，因此编辑器语法与文档无法悄悄漂移。
pub const KEYWORDS: &[&str] = &[
    // 声明
    "let", "mut", "fn", "async", // 异步
    "await", "spawn", // 控制流
    "if", "else", "match", "while", "for", "in", "return", "ret", "break", "continue",
    // 字面量
    "true", "false", "nil", "null",
];

/// 运算符与分隔符拼写（T7.1：编辑器语法与规范表格的共同来源）。
///
/// 注意 `$` 不在表内：它只在插值字符串前缀 `$"..."` 中出现，单独出现是词法错误。
pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "=", "==", "!=", "<", "<=", ">", ">=", "!", "&&", "||", "&", "|", "^",
    "~", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "->", "=>", "|>", "??", "?.", ".", "(", ")",
    "[", "]", "{", "}", ",", ":", ";",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 关键字表必须与词法实现一致（T7.1 的防漂移门禁：编辑器语法依赖本表）。
    #[test]
    fn keywords_match_the_lexer() {
        for word in KEYWORDS {
            let mut lexer = Lexer::new(word);
            let tokens = lexer.tokenize().expect("keyword should lex");
            let kind = &tokens[0].kind;
            assert!(
                !matches!(kind, TokenKind::Ident(_)),
                "'{word}' is listed in KEYWORDS but the lexer treats it as a plain identifier"
            );
        }

        // 反向：典型非关键字必须仍是标识符（防止表被误删）
        for word in ["fnx", "iff", "letter", "returns", "nill", "spawns"] {
            let mut lexer = Lexer::new(word);
            let tokens = lexer.tokenize().expect("identifier should lex");
            assert!(
                matches!(&tokens[0].kind, TokenKind::Ident(name) if name == word),
                "'{word}' should stay a plain identifier"
            );
        }
    }

    /// 运算符表必须都能被词法接受（T7.1：编辑器语法与规范表格的共同来源）。
    #[test]
    fn operators_are_lexable() {
        for op in OPERATORS {
            let mut lexer = Lexer::new(op);
            let tokens = lexer
                .tokenize()
                .unwrap_or_else(|e| panic!("operator {op:?} should lex, got {e:?}"));
            assert!(
                tokens.len() >= 2,
                "operator {op:?} produced no token before EOF"
            );
        }
    }

    #[test]
    fn test_simple_tokens() {
        let code = r#"
            let a = 10
            mut b = 20.5
            fn add(x, y) = x + y
        "#;
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().expect("Lexing failed");

        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Fn)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Let)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Mut)));
        assert!(tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Float(20.5))));
    }

    #[test]
    fn test_pipe_and_navigation() {
        let code = "data |> filter(x => x > 0) ?? fallback?.name";
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().expect("Lexing failed");

        assert!(tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::PipeGreater)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::FatArrow)));
        assert!(tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::QuestionQuestion)));
        assert!(tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::QuestionDot)));
    }

    #[test]
    fn test_interpolated_string() {
        let code = r#"$"User {name} has {roles.len()} roles""#;
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().expect("Lexing failed");

        assert_eq!(tokens.len(), 3); // InterpString, Newline, Eof
        if let TokenKind::InterpString(parts) = &tokens[0].kind {
            assert_eq!(parts.len(), 5);
            assert_eq!(parts[0], InterpPart::Lit("User ".to_string()));
            match &parts[1] {
                InterpPart::Code(code, _) => assert_eq!(code, "name"),
                _ => panic!("Expected Code part"),
            }
            assert_eq!(parts[2], InterpPart::Lit(" has ".to_string()));
            match &parts[3] {
                InterpPart::Code(code, _) => assert_eq!(code, "roles.len()"),
                _ => panic!("Expected Code part"),
            }
            assert_eq!(parts[4], InterpPart::Lit(" roles".to_string()));
        } else {
            panic!("Expected InterpString token");
        }
    }

    fn lex(source: &str) -> Result<Vec<Token>, LexError> {
        Lexer::new(source).tokenize()
    }

    #[test]
    fn 行注释被跳过不产生token() {
        let tokens = lex("// this is a comment\nlet x = 1").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(!tokens
            .iter()
            .any(|t| matches!(t.kind, TokenKind::Ident(ref s) if s.contains("comment"))));
    }

    #[test]
    fn 块注释被跳过且可跨行() {
        let tokens = lex("/* multi\nline\ncomment */ 42").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Int(42));
    }

    #[test]
    fn 未闭合块注释报错并指向开头() {
        let err = lex("/* never closed").unwrap_err();
        match err {
            LexError::UnterminatedBlockComment { line, col } => {
                assert_eq!(line, 1);
                assert_eq!(col, 1);
            }
            other => panic!("期望 UnterminatedBlockComment，得到 {other:?}"),
        }
    }

    #[test]
    fn 十六进制数字被解析为整数() {
        let tokens = lex("0xFF").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Int(255));
    }

    #[test]
    fn 二进制数字被解析为整数() {
        let tokens = lex("0b1010").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Int(10));
    }

    #[test]
    fn 数字下划线分隔被忽略() {
        let tokens = lex("1_000_000").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Int(1_000_000));
    }

    #[test]
    fn 浮点数字被解析() {
        let tokens = lex("2.5").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Float(2.5));
    }

    #[test]
    fn 科学计数法浮点被解析() {
        let tokens = lex("1.5e3").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Float(1500.0));
    }

    #[test]
    fn 无效十六进制数字报错() {
        let err = lex("0xGG").unwrap_err();
        assert!(
            matches!(err, LexError::InvalidNumber { .. }),
            "得到 {err:?}"
        );
    }

    #[test]
    fn 非法字符报错并指向行列() {
        let err = lex("@").unwrap_err();
        match err {
            LexError::UnexpectedChar { ch, line, col } => {
                assert_eq!(ch, '@');
                assert_eq!(line, 1);
                assert_eq!(col, 1);
            }
            other => panic!("期望 UnexpectedChar，得到 {other:?}"),
        }
    }

    #[test]
    fn 单独的问号是词法错误() {
        let err = lex("?").unwrap_err();
        assert!(
            matches!(err, LexError::UnexpectedChar { ch: '?', .. }),
            "得到 {err:?}"
        );
    }

    #[test]
    fn 未闭合字符串报错() {
        let err = lex("\"abc").unwrap_err();
        assert!(
            matches!(err, LexError::UnterminatedString { .. }),
            "得到 {err:?}"
        );
    }

    #[test]
    fn 字符串转义序列被正确解析() {
        let tokens = lex("\"a\\nb\\t c\"").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::String("a\nb\t c".to_string()));
    }

    #[test]
    fn asi在可结束语句token后插入换行() {
        let tokens = lex("1\n2").unwrap();
        let newlines = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Newline))
            .count();
        assert_eq!(newlines, 2);
    }

    #[test]
    fn asi在运算符后不插入换行() {
        let tokens = lex("1 +\n2").unwrap();
        let newlines = tokens
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::Newline))
            .count();
        assert_eq!(newlines, 1);
    }

    #[test]
    fn 源码末尾自动补全换行token() {
        let tokens = lex("let x = 1").unwrap();
        let last = &tokens[tokens.len() - 2];
        assert!(matches!(last.kind, TokenKind::Newline));
        assert!(matches!(tokens[tokens.len() - 1].kind, TokenKind::Eof));
    }

    #[test]
    fn tokenkind_display_覆盖代表性种类() {
        assert_eq!(TokenKind::Int(5).to_string(), "5");
        assert_eq!(TokenKind::Float(1.5).to_string(), "1.5");
        assert_eq!(TokenKind::String("hi".into()).to_string(), "\"hi\"");
        assert_eq!(TokenKind::Bool(true).to_string(), "true");
        assert_eq!(TokenKind::Nil.to_string(), "nil");
        assert_eq!(TokenKind::Ident("x".into()).to_string(), "x");
        assert_eq!(TokenKind::Plus.to_string(), "+");
        assert_eq!(TokenKind::PipeGreater.to_string(), "|>");
        assert_eq!(TokenKind::FatArrow.to_string(), "=>");
        assert_eq!(TokenKind::Newline.to_string(), "\\n");
        assert_eq!(TokenKind::Eof.to_string(), "<EOF>");
    }

    #[test]
    fn token_new_记录种类与位置() {
        let span = ada_ast::Span::new(0, 3, 1, 1);
        let tok = Token::new(TokenKind::Int(7), span);
        assert_eq!(tok.kind, TokenKind::Int(7));
        assert_eq!(tok.span, span);
    }

    #[test]
    fn 标识符可包含下划线与数字() {
        let tokens = lex("foo_bar123").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("foo_bar123".to_string()));
    }

    #[test]
    fn 关键字别名被识别() {
        let ret = lex("ret").unwrap();
        assert!(matches!(ret[0].kind, TokenKind::Return));
        let null = lex("null").unwrap();
        assert!(matches!(null[0].kind, TokenKind::Nil));
    }

    #[test]
    fn lexerror_position_各变体返回正确位置() {
        let cases = [
            LexError::UnexpectedChar {
                ch: '#',
                line: 2,
                col: 3,
            },
            LexError::UnterminatedString { line: 4, col: 5 },
            LexError::UnterminatedBlockComment { line: 6, col: 7 },
            LexError::InvalidNumber {
                raw: "0xZ".into(),
                line: 8,
                col: 9,
            },
            LexError::UnterminatedInterpExpr { line: 10, col: 11 },
        ];
        let expected = [(2, 3), (4, 5), (6, 7), (8, 9), (10, 11)];
        for (err, pos) in cases.iter().zip(expected) {
            assert_eq!(err.position(), pos);
        }
    }

    #[test]
    fn 插值字符串可为纯字面量() {
        let tokens = lex("$\"hello\"").unwrap();
        if let TokenKind::InterpString(parts) = &tokens[0].kind {
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0], InterpPart::Lit("hello".to_string()));
        } else {
            panic!("期望 InterpString");
        }
    }

    #[test]
    fn 空插值字符串产生空parts() {
        let tokens = lex("$\"\"").unwrap();
        if let TokenKind::InterpString(parts) = &tokens[0].kind {
            assert!(parts.is_empty());
        } else {
            panic!("期望 InterpString");
        }
    }

    #[test]
    fn 未闭合插值表达式报错() {
        let err = lex("$\"{x\"").unwrap_err();
        assert!(
            matches!(err, LexError::UnterminatedInterpExpr { .. }),
            "得到 {err:?}"
        );
    }

    #[test]
    fn 插值中的转义花括号视为字面量() {
        let tokens = lex("$\"a\\{b\"").unwrap();
        if let TokenKind::InterpString(parts) = &tokens[0].kind {
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0], InterpPart::Lit("a{b".to_string()));
        } else {
            panic!("期望 InterpString");
        }
    }
}
