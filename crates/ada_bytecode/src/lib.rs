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

    fn compile_source(source: &str) -> Result<FunctionProto, CompileError> {
        let mut parser = Parser::from_source(source).expect("lex/parse failed");
        let program = parser.parse_program().expect("parse program failed");
        Compiler::compile_program(&program)
    }

    #[test]
    fn reg_display_格式化为r加数字() {
        assert_eq!(Reg(0).to_string(), "R0");
        assert_eq!(Reg(42).to_string(), "R42");
        assert_eq!(Reg(u16::MAX).to_string(), "R65535");
    }

    #[test]
    fn constant_各变体可克隆且可比较() {
        let consts = [
            Constant::Int(7),
            Constant::Float(1.5),
            Constant::String("s".into()),
            Constant::Bool(false),
            Constant::Nil,
        ];
        for c in &consts {
            assert_eq!(c, &c.clone());
        }
        assert_ne!(Constant::Int(1), Constant::Int(2));
        assert_ne!(Constant::Nil, Constant::Bool(false));
    }

    #[test]
    fn instruction_代表性变体可克隆且可比较() {
        let insts = [
            Instruction::LoadInt {
                dst: Reg(0),
                val: 5,
            },
            Instruction::Add {
                dst: Reg(2),
                lhs: Reg(0),
                rhs: Reg(1),
            },
            Instruction::BinaryConst {
                dst: Reg(2),
                op: ada_ast::BinOp::Mul,
                src: Reg(0),
                value: 3,
            },
            Instruction::Jump { offset: -1 },
            Instruction::Return { src: None },
            Instruction::MatchFail,
        ];
        for i in &insts {
            assert_eq!(i, &i.clone());
        }
        assert_ne!(insts[0], insts[1]);
    }

    #[test]
    fn globalwrite_三种写入种类互不相等且可复制() {
        let kinds = [
            GlobalWrite::Assign,
            GlobalWrite::DeclareMut,
            GlobalWrite::DeclareImmutable,
        ];
        for (i, a) in kinds.iter().enumerate() {
            let copied = *a;
            assert_eq!(*a, copied);
            for (j, b) in kinds.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn upvaluedesc_父局部与父捕获可比较() {
        assert_eq!(UpvalueDesc::ParentLocal(0), UpvalueDesc::ParentLocal(0));
        assert_ne!(UpvalueDesc::ParentLocal(0), UpvalueDesc::ParentUpvalue(0));
        assert_ne!(UpvalueDesc::ParentLocal(0), UpvalueDesc::ParentLocal(1));
    }

    #[test]
    fn function_proto_new_初始字段为空() {
        let proto = FunctionProto::new("main", 0);
        assert_eq!(proto.name, "main");
        assert_eq!(proto.arity, 0);
        assert!(!proto.is_async);
        assert_eq!(proto.register_count, 0);
        assert!(proto.instructions.is_empty());
        assert!(proto.constants.is_empty());
        assert!(proto.names.is_empty());
        assert!(proto.child_protos.is_empty());
        assert!(proto.spans.is_empty());
        assert!(proto.upvalues.is_empty());
    }

    #[test]
    fn add_constant_对重复常量去重返回同一索引() {
        let mut proto = FunctionProto::new("f", 0);
        let a = proto.add_constant(Constant::Int(42));
        let b = proto.add_constant(Constant::String("x".into()));
        let a_again = proto.add_constant(Constant::Int(42));
        assert_eq!(a, a_again);
        assert_ne!(a, b);
        assert_eq!(proto.constants.len(), 2);
    }

    #[test]
    fn add_name_对重复名字去重返回同一索引() {
        let mut proto = FunctionProto::new("f", 0);
        let a = proto.add_name("foo");
        let b = proto.add_name("bar");
        let a_again = proto.add_name("foo");
        assert_eq!(a, a_again);
        assert_ne!(a, b);
        assert_eq!(proto.names, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn 赋值给不可变变量返回编译错误() {
        let err = compile_source("let x = 1\nx = 2").unwrap_err();
        assert!(
            matches!(err, CompileError::AssignToImmutable { .. }),
            "期望 AssignToImmutable，得到 {err:?}"
        );
    }

    #[test]
    fn 循环外使用break返回编译错误() {
        let err = compile_source("break").unwrap_err();
        assert!(
            matches!(err, CompileError::LoopKeywordOutsideLoop { .. }),
            "期望 LoopKeywordOutsideLoop，得到 {err:?}"
        );
    }

    #[test]
    fn 循环外使用continue返回编译错误() {
        let err = compile_source("continue").unwrap_err();
        assert!(
            matches!(err, CompileError::LoopKeywordOutsideLoop { .. }),
            "期望 LoopKeywordOutsideLoop，得到 {err:?}"
        );
    }

    #[test]
    fn 编译错误span指向出错位置() {
        let err = compile_source("let x = 1\nx = 2").unwrap_err();
        let s = err.span().expect("AssignToImmutable 应带位置");
        assert!(s.line >= 1);
        assert!(s.col >= 1);
    }

    #[test]
    fn 异步函数原型标记is_async() {
        let proto = compile_source("async fn f() = 1\nf()").unwrap();
        assert_eq!(proto.child_protos.len(), 1);
        assert!(proto.child_protos[0].is_async);
        assert_eq!(proto.child_protos[0].name, "f");
    }

    #[test]
    fn while循环产生跳转指令() {
        let proto = compile_source("mut i = 0\nwhile i < 3 { i += 1 }").unwrap();
        let has_jump = proto.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Jump { .. }
                    | Instruction::JumpIfFalse { .. }
                    | Instruction::JumpIfTrue { .. }
                    | Instruction::JumpIfNotCompare { .. }
                    | Instruction::JumpIfNotCompareConst { .. }
            )
        });
        assert!(has_jump, "while 循环应产生跳转指令");
    }

    #[test]
    fn for循环产生迭代指令() {
        let proto = compile_source("for x in [1, 2, 3] { x }").unwrap();
        let has_iter = proto.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::IterInit { .. } | Instruction::IterNext { .. }
            )
        });
        assert!(has_iter, "for 循环应产生迭代指令");
    }

    #[test]
    fn 算术表达式产生运算指令() {
        let proto = compile_source("1 + 2 * 3").unwrap();
        let has_arith = proto
            .instructions
            .iter()
            .any(|i| matches!(i, Instruction::Add { .. } | Instruction::BinaryConst { .. }));
        assert!(has_arith, "算术表达式应产生加法或常量融合指令");
    }

    #[test]
    fn 顶层let写入全局名表() {
        let proto = compile_source("let greeting = 1").unwrap();
        assert!(
            proto.names.iter().any(|n| n == "greeting"),
            "顶层 let 的名字应进入全局名表，得到 {:?}",
            proto.names
        );
        let has_set_global = proto
            .instructions
            .iter()
            .any(|i| matches!(i, Instruction::SetGlobal { .. }));
        assert!(has_set_global, "顶层 let 应产生 SetGlobal");
    }

    #[test]
    fn 函数参数占用前若干寄存器() {
        let proto = compile_source("fn add3(a, b, c) = a\nadd3(1, 2, 3)").unwrap();
        let child = &proto.child_protos[0];
        assert_eq!(child.arity, 3);
        assert!(child.register_count >= 3, "寄存器数应至少覆盖 3 个参数");
    }

    #[test]
    fn 嵌套函数编译产生子原型() {
        let proto =
            compile_source("fn outer() {\n  fn inner() = 1\n  inner()\n}\nouter()").unwrap();
        assert_eq!(proto.child_protos.len(), 1);
        assert_eq!(proto.child_protos[0].name, "outer");
        assert_eq!(proto.child_protos[0].child_protos.len(), 1);
        assert_eq!(proto.child_protos[0].child_protos[0].name, "inner");
    }
}
