pub mod compiler;
pub mod instruction;

pub use compiler::{CompileError, Compiler};
pub use instruction::{Constant, FunctionProto, GlobalWrite, Instruction, Reg, UpvalueDesc};

#[cfg(test)]
mod tests {
    use super::*;
    use ada_parser::Parser;

    #[test]
    fn test_compile_arithmetic() {
        let code = "let a = 10 + 20 * 3";
        let mut parser = Parser::from_source(code).expect("lex/parse failed");
        let program = parser.parse_program().expect("parse program failed");

        let proto = Compiler::compile_program(&program).expect("compile failed");
        assert!(!proto.instructions.is_empty());
        assert!(proto.register_count > 0);
    }

    #[test]
    fn test_compile_fn_and_call() {
        let code = r#"
            fn add(x, y) = x + y
            let res = add(1, 2)
        "#;
        let mut parser = Parser::from_source(code).expect("lex/parse failed");
        let program = parser.parse_program().expect("parse program failed");

        let proto = Compiler::compile_program(&program).expect("compile failed");
        assert_eq!(proto.child_protos.len(), 1);
        assert_eq!(proto.child_protos[0].name, "add");
        assert_eq!(proto.child_protos[0].arity, 2);
    }
}
