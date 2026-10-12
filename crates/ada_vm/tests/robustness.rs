//! 稳健性回归测试（开发计划 T1.5）。
//!
//! 目标：**任何脚本输入都不得让宿主进程 panic**（开发计划 §5 门禁第 3 条）。
//! 本文件逐个覆盖 2026-10 审计中发现的 panic 路径；任何一个用例触发 panic
//! 都会让测试进程崩溃，从而使本测试失败。

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lexing/parsing should succeed");
    let program = parser.parse_program().expect("parse should succeed");
    let proto = Compiler::compile_program(&program).expect("compilation should succeed");
    let mut vm = Vm::new();
    vm.execute(proto)
}

fn expect_ok(source: &str) -> Value {
    run(source).unwrap_or_else(|e| panic!("expected success for {source:?}, got {e:?}"))
}

fn expect_type_error(source: &str, needle: &str) {
    match run(source) {
        Err(VmError::TypeError(msg)) => {
            assert!(
                msg.contains(needle),
                "expected message containing {needle:?}, got {msg:?}"
            );
        }
        other => panic!("expected TypeError containing {needle:?} for {source:?}, got {other:?}"),
    }
}

// ---------------------------------------------------------------- T1.1 取模零

#[test]
fn modulo_by_zero_is_an_error_not_a_panic() {
    expect_type_error("10 % 0", "Modulo by zero");
}

#[test]
fn modulo_of_negative_integer_min_by_minus_one_is_an_error() {
    // i64::MIN % -1 在 Rust 中会 overflow panic
    expect_type_error("let a = -9223372036854775807 - 1\na % -1", "overflow");
}

#[test]
fn division_of_integer_min_by_minus_one_is_an_error() {
    expect_type_error("let a = -9223372036854775807 - 1\na / -1", "overflow");
}

// ------------------------------------------------------------ T1.2 整数溢出

#[test]
fn integer_addition_overflow_is_an_error_not_a_panic() {
    expect_type_error("9223372036854775807 + 1", "overflow");
}

#[test]
fn integer_subtraction_overflow_is_an_error_not_a_panic() {
    expect_type_error("-9223372036854775807 - 2", "overflow");
}

#[test]
fn integer_multiplication_overflow_is_an_error_not_a_panic() {
    expect_type_error("9223372036854775807 * 2", "overflow");
}

#[test]
fn integer_negation_overflow_is_an_error_not_a_panic() {
    // 注意用括号开始新语句：`-` 是续写算子，否则会被并入上一行（见下方 ASI 用例）
    expect_type_error("let a = -9223372036854775807 - 1\n(-a)", "overflow");
}

#[test]
fn newline_before_minus_continues_the_expression_asi_rule() {
    // SPEC_LANGUAGE §2.6：`-` 属于续写算子，换行不会切断语句，
    // 故 `let a = 1` + 新行 `-a` 实际被解析为 `let a = 1 - a`
    match run("let a = 1\n-a") {
        Err(VmError::UndefinedGlobal(name)) => assert_eq!(name, "a"),
        other => panic!("expected UndefinedGlobal(a), got {other:?}"),
    }
    // 用括号显式断开后行为符合直觉
    assert_eq!(expect_ok("let a = 1\n(-a)"), Value::Int(-1));
}

// -------------------------------------------------------------- T1.3 移位越界

#[test]
fn shift_left_out_of_range_is_an_error_not_a_panic() {
    expect_type_error("1 << 64", "Shift amount out of range");
}

#[test]
fn shift_left_by_negative_amount_is_an_error_not_a_panic() {
    expect_type_error("1 << -1", "Shift amount out of range");
}

#[test]
fn shift_right_out_of_range_is_an_error_not_a_panic() {
    expect_type_error("1 >> 64", "Shift amount out of range");
}

// ---------------------------------------------- T1.4 / T2.5 UTF-8 字符索引与长度

#[test]
fn string_index_is_char_based_and_out_of_range_yields_nil() {
    assert_eq!(expect_ok("\"语言\"[0]"), Value::string("语"));
    assert_eq!(expect_ok("\"语言\"[1]"), Value::string("言"));
    assert_eq!(expect_ok("\"Ada 语言\"[4]"), Value::string("语"));
    assert_eq!(expect_ok("\"abc\"[5]"), Value::Nil);
    // T3.5：负索引从末尾计数（数组与字符串一致，与 `.slice` 对齐）
    assert_eq!(expect_ok("\"abc\"[-1]"), Value::string("c"));
    assert_eq!(expect_ok("\"abc\"[-4]"), Value::Nil);
}

#[test]
fn string_len_counts_characters_and_len_bytes_counts_bytes() {
    assert_eq!(expect_ok("len(\"Ada 语言\")"), Value::Int(6));
    assert_eq!(expect_ok("\"Ada 语言\".len"), Value::Int(6));
    assert_eq!(expect_ok("\"Ada 语言\".len_bytes"), Value::Int(10));
    assert_eq!(expect_ok("len(\"abc\")"), Value::Int(3));
}

// ------------------------------------------------------- T1.6 其他审计发现

#[test]
fn min_and_max_on_empty_array_return_nil_without_panicking() {
    assert_eq!(expect_ok("min([])"), Value::Nil);
    assert_eq!(expect_ok("max([])"), Value::Nil);
    // 正常路径不受守卫影响
    assert_eq!(expect_ok("min([3, 1, 2])"), Value::Int(1));
    assert_eq!(expect_ok("max([3, 1, 2])"), Value::Int(3));
}

// ------------------------------------------------- 修复未破坏正常算术与语义

#[test]
fn valid_arithmetic_still_works_at_the_boundaries() {
    assert_eq!(
        expect_ok("9223372036854775807 - 1"),
        Value::Int(i64::MAX - 1)
    );
    assert_eq!(expect_ok("-9223372036854775807 - 1"), Value::Int(i64::MIN));
    assert_eq!(expect_ok("1 << 62"), Value::Int(1i64 << 62));
    assert_eq!(expect_ok("1024 >> 3"), Value::Int(128));
    assert_eq!(expect_ok("10 % 3"), Value::Int(1));
    assert_eq!(expect_ok("10 / 4"), Value::Float(2.5));
    assert_eq!(expect_ok("10 / 5"), Value::Int(2));
}

#[test]
fn division_by_zero_still_reports_the_original_error() {
    expect_type_error("10 / 0", "Division by zero");
}

#[test]
fn recursion_and_iteration_are_unaffected() {
    let code = r#"
        fn fib(n) { if n <= 1 { n } else { fib(n - 1) + fib(n - 2) } }
        mut total = 0
        for i in range(1, 6) { total += i }
        fib(10) + total
    "#;
    assert_eq!(expect_ok(code), Value::Int(70));
}
