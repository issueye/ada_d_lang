pub mod lexer;
pub mod token;

pub use lexer::{LexError, Lexer};
pub use token::{InterpPart, Token, TokenKind};

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Float(20.5))));
    }

    #[test]
    fn test_pipe_and_navigation() {
        let code = "data |> filter(x => x > 0) ?? fallback?.name";
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().expect("Lexing failed");

        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::PipeGreater)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::FatArrow)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::QuestionQuestion)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::QuestionDot)));
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
