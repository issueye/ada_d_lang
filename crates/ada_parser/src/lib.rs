pub mod error;
pub mod parser;

pub use error::ParseError;
pub use parser::Parser;

#[cfg(test)]
mod tests {
    use super::*;
    use ada_ast::*;

    #[test]
    fn test_variable_and_function_parsing() {
        let code = r#"
            let a = 10
            mut count = 0
            count += 1
            fn add(x, y) = x + y
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 4);
        assert!(
            matches!(program.stmts[0], Stmt::Let { ref name, is_mut: false, .. } if name == "a")
        );
        assert!(
            matches!(program.stmts[1], Stmt::Let { ref name, is_mut: true, .. } if name == "count")
        );
        assert!(matches!(
            program.stmts[2],
            Stmt::Expr {
                expr: Expr::Assign {
                    op: AssignOp::AddAssign,
                    ..
                },
                ..
            }
        ));
        assert!(
            matches!(program.stmts[3], Stmt::Fn { ref name, ref params, .. } if name == "add" && params.len() == 2)
        );
    }

    #[test]
    fn test_pipe_and_closure() {
        let code = r#"
            let result = data
                |> filter(x => x > 0)
                |> map(x => x * 2)
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        if let Stmt::Let {
            init: Some(Expr::Pipe { lhs, rhs, .. }),
            ..
        } = &program.stmts[0]
        {
            assert!(matches!(**lhs, Expr::Pipe { .. }));
            assert!(matches!(**rhs, Expr::Call { .. }));
        } else {
            panic!("Expected Pipe expression in let init");
        }
    }

    #[test]
    fn test_optional_chaining_and_null_coalescing() {
        let code = r#"
            let val = user?.profile?.email ?? "default@domain.com"
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        if let Stmt::Let {
            init: Some(Expr::NullCoalesce { lhs, .. }),
            ..
        } = &program.stmts[0]
        {
            assert!(matches!(**lhs, Expr::FieldAccess { optional: true, .. }));
        } else {
            panic!("Expected NullCoalesce expression");
        }
    }

    #[test]
    fn test_map_and_array_literals() {
        let code = r#"
            let obj = {
                name: "Ada",
                roles: ["dev", "admin"],
            }
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        if let Stmt::Let {
            init: Some(Expr::Map { entries, .. }),
            ..
        } = &program.stmts[0]
        {
            assert_eq!(entries.len(), 2);
        } else {
            panic!("Expected Map literal");
        }
    }

    #[test]
    fn test_match_expression() {
        let code = r#"
            let grade = match score {
                >= 90 => "A",
                >= 80 => "B",
                _     => "C"
            }
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        if let Stmt::Let {
            init: Some(Expr::Match { arms, .. }),
            ..
        } = &program.stmts[0]
        {
            assert_eq!(arms.len(), 3);
        } else {
            panic!("Expected Match expression");
        }
    }

    #[test]
    fn test_async_await_spawn() {
        let code = r#"
            async fn fetch_user(id) {
                let res = http.get(id).await
                res.data
            }
            let task = spawn fetch_user(42)
            let user = await task
        "#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 3);
        assert!(matches!(program.stmts[0], Stmt::Fn { is_async: true, .. }));
        if let Stmt::Let {
            init: Some(Expr::Spawn { .. }),
            ..
        } = &program.stmts[1]
        {
            // ok
        } else {
            panic!("Expected spawn");
        }
        if let Stmt::Let {
            init: Some(Expr::Await { .. }),
            ..
        } = &program.stmts[2]
        {
            // ok
        } else {
            panic!("Expected await");
        }
    }

    #[test]
    fn test_namespace_import() {
        let code = r#"import json from "std:json""#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        assert!(matches!(
            &program.stmts[0],
            Stmt::Import { kind: ImportKind::Namespace(name), path, .. }
                if name == "json" && path == "std:json"
        ));
    }

    #[test]
    fn test_named_import() {
        let code = r#"import { parse, stringify } from "std:json""#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");

        assert_eq!(program.stmts.len(), 1);
        if let Stmt::Import {
            kind: ImportKind::Named(names),
            path,
            ..
        } = &program.stmts[0]
        {
            assert_eq!(names, &vec!["parse".to_string(), "stringify".to_string()]);
            assert_eq!(path, "std:json");
        } else {
            panic!("Expected named import");
        }
    }

    #[test]
    fn test_named_import_from_file() {
        let code = r#"import { a } from "lib/helpers.ada""#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        let program = parser.parse_program().expect("Parse failed");
        assert!(matches!(
            &program.stmts[0],
            Stmt::Import { kind: ImportKind::Named(names), path, .. }
                if names == &vec!["a".to_string()] && path == "lib/helpers.ada"
        ));
    }

    #[test]
    fn test_import_without_from_is_error() {
        let code = r#"import { a } "std:json""#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        assert!(parser.parse_program().is_err());
    }

    #[test]
    fn test_import_with_non_string_path_is_error() {
        let code = r#"import json from json"#;
        let mut parser = Parser::from_source(code).expect("Lex failed");
        assert!(parser.parse_program().is_err());
    }
}
