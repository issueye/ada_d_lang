//! VM 层 std: 导入测试（T8.14）：证明 `import` 经编译器/VM 生效，不再依赖 Engine。

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lex/parse");
    let program = parser.parse_program().expect("parse program");
    let proto = Compiler::compile_program(&program).expect("compile");
    let mut vm = Vm::new();
    vm.execute(proto)
}

#[test]
fn vm_level_namespace_import() {
    let src = "import math from \"std:math\"\nto_int(math.sqrt(81.0))";
    assert_eq!(run(src).unwrap(), Value::Int(9));
}

#[test]
fn vm_level_named_import() {
    let src = "import { sqrt } from \"std:math\"\nto_int(sqrt(49.0))";
    assert_eq!(run(src).unwrap(), Value::Int(7));
}

#[test]
fn vm_level_unknown_module_errors() {
    let src = "import x from \"std:nope\"\nx";
    let err = run(src).unwrap_err();
    assert!(matches!(err, VmError::TypeError(_)), "got {err:?}");
}
