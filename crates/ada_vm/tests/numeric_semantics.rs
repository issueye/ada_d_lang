//! 数值函数的口径统一（G-S6）与舍入函数命名（G-S7）。
//!
//! 背景：M3 把比较运算收紧成"非数值报错"（`5 < "a"` 不再静默 `false`），但
//! `abs`/`round`/`floor`/`ceil`/`min`/`max`/`sum`/`math.*` 仍对非数值**静默**返回 0
//! 或忽略该元素。静默恰好是最坏的选择：`sum([1, "2"])` 得到 1、`abs("x")` 得到 0，
//! 会让 AI 生成代码里的类型错误一路静默到结果里。
//!
//! 本文件把这些函数的契约固定为：**非数值一律报错**，且错误消息包含实际类型。

use ada_bytecode::{Compiler, FunctionProto};
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lex");
    let program = parser.parse_program().expect("parse");
    let proto: FunctionProto = Compiler::compile_program(&program).expect("compile");
    Vm::new().execute(proto)
}

fn ok(source: &str) -> Value {
    run(source).unwrap_or_else(|err| panic!("{source} should succeed, got {err}"))
}

/// 断言报错，且消息里带上了实际类型名。
fn assert_type_error(source: &str, expected_type: &str) {
    match run(source) {
        Err(VmError::TypeError(message)) | Err(VmError::NativeError(message)) => {
            assert!(
                message.contains(expected_type),
                "{source}: expected the message to name the type {expected_type:?}, got {message:?}"
            );
        }
        other => panic!("{source}: expected a type error, got {other:?}"),
    }
}

#[test]
fn non_numeric_arguments_are_rejected() {
    // 每个函数都必须拒绝字符串，并在消息里说明"got string"
    for source in [
        "abs(\"x\")",
        "round(\"x\")",
        "floor(\"x\")",
        "ceil(\"x\")",
        "math.sqrt(\"x\")",
        "math.log(\"x\")",
        "math.log10(\"x\")",
        "math.sin(\"x\")",
        "math.cos(\"x\")",
        "math.tan(\"x\")",
        "math.pow(\"x\", 2)",
        "math.pow(2, \"x\")",
        "math.round(\"x\")",
        "math.floor(\"x\")",
        "math.ceil(\"x\")",
    ] {
        assert_type_error(source, "string");
    }
}

#[test]
fn other_non_numeric_types_are_rejected_too() {
    assert_type_error("abs(nil)", "nil");
    assert_type_error("abs(true)", "bool");
    assert_type_error("abs([1])", "array");
    assert_type_error("abs({ a: 1 })", "map");
    // `map` 是适配器内建（函数值），不是数字
    assert_type_error("round(map)", "function");
}

#[test]
fn missing_arguments_are_rejected() {
    for source in [
        "abs()",
        "round()",
        "floor()",
        "ceil()",
        "math.sqrt()",
        "math.round()",
    ] {
        let err = run(source).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("nothing") || message.contains("expects"),
            "{source}: unexpected error {message:?}"
        );
    }
}

#[test]
fn collection_numeric_functions_reject_non_numeric_elements() {
    // sum 此前静默忽略非数值元素；min/max 此前静默跳过比较
    assert_type_error("sum([1, \"2\"])", "string");
    assert_type_error("sum([1, nil])", "nil");
    assert_type_error("min([1, \"a\"])", "string");
    assert_type_error("max([1, true])", "bool");
    assert_type_error("min(1, \"a\")", "string");
    // 管道形式走同一个实现
    assert_type_error("[1, \"2\"] |> sum()", "string");
}

#[test]
fn numeric_paths_still_behave_as_before() {
    assert_eq!(ok("abs(-3)"), Value::Int(3));
    assert_eq!(ok("abs(-3.5)"), Value::Float(3.5));
    assert_eq!(ok("round(1.5)"), Value::Int(2));
    assert_eq!(ok("round(-1.5)"), Value::Int(-2));
    assert_eq!(ok("floor(1.9)"), Value::Int(1));
    assert_eq!(ok("ceil(1.1)"), Value::Int(2));
    assert_eq!(ok("sum([1, 2, 3])"), Value::Int(6));
    assert_eq!(ok("sum([1, 2.5])"), Value::Float(3.5));
    assert_eq!(ok("sum([])"), Value::Int(0));
    assert_eq!(ok("min([3, 1, 2])"), Value::Int(1));
    assert_eq!(ok("max(3, 1, 2)"), Value::Int(3));
    assert_eq!(ok("math.sqrt(144.0)"), Value::Float(12.0));
    // 空集合仍然是 nil（不是错误）：只有"元素存在但类型不对"才算错误
    assert_eq!(ok("min([])"), Value::Nil);
    assert_eq!(ok("max([])"), Value::Nil);
}

#[test]
fn rounding_functions_are_available_in_both_namespaces() {
    // G-S7：舍入函数此前只在全局命名空间；math.* 别名补齐后两者等价
    assert_eq!(ok("round(1.6)"), ok("math.round(1.6)"));
    assert_eq!(ok("floor(1.6)"), ok("math.floor(1.6)"));
    assert_eq!(ok("ceil(1.2)"), ok("math.ceil(1.2)"));
    assert_eq!(ok("type_of(math.round)"), Value::string("function"));
    assert_eq!(ok("type_of(math.floor)"), Value::string("function"));
    assert_eq!(ok("type_of(math.ceil)"), Value::string("function"));
}
