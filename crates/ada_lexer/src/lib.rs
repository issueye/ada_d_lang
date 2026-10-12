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
}
