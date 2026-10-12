//! 语义规则测试集（开发计划 T3.7）。
//!
//! 覆盖 M3「语义规范收紧」的每一条规则，并与 `SPEC_LANGUAGE` / `SPEC_RUNTIME`
//! 的条文一一对应：
//! - T3.1 `let` 不可变性
//! - T3.2 `match` 非穷尽报错
//! - T3.4 插值片段的错误定位
//! - T3.5 数值比较/取模/负索引语义
//! - T3.6 浮点字符串化

use ada_bytecode::{CompileError, Compiler};
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lexing/parsing should succeed");
    let program = parser.parse_program().expect("parse should succeed");
    let proto = Compiler::compile_program(&program).expect("compilation should succeed");
    let mut vm = Vm::new();
    vm.execute(proto)
}

fn ok(source: &str) -> Value {
    run(source).unwrap_or_else(|e| panic!("expected success for {source:?}, got {e:?}"))
}

fn compile_err(source: &str) -> CompileError {
    let mut parser = Parser::from_source(source).expect("lexing/parsing should succeed");
    let program = parser.parse_program().expect("parse should succeed");
    Compiler::compile_program(&program)
        .err()
        .unwrap_or_else(|| panic!("expected a compile error for {source:?}"))
}

fn expect_type_error(source: &str, needle: &str) {
    match run(source) {
        Err(VmError::TypeError(msg)) => assert!(
            msg.contains(needle),
            "expected message containing {needle:?}, got {msg:?}"
        ),
        other => panic!("expected TypeError containing {needle:?} for {source:?}, got {other:?}"),
    }
}

// ============================================================ T3.1 不可变性

#[test]
fn plain_assignment_to_a_let_binding_is_rejected() {
    match compile_err("let a = 1\na = 2") {
        CompileError::AssignToImmutable(name) => assert_eq!(name, "a"),
        other => panic!("expected AssignToImmutable, got {other:?}"),
    }
}

#[test]
fn compound_assignment_to_a_let_binding_is_rejected() {
    match compile_err("let a = 1\na += 1") {
        CompileError::AssignToImmutable(name) => assert_eq!(name, "a"),
        other => panic!("expected AssignToImmutable, got {other:?}"),
    }
}

#[test]
fn mut_bindings_are_assignable() {
    assert_eq!(ok("mut a = 1\na = 2\na"), Value::Int(2));
    assert_eq!(ok("mut a = 1\na += 2\na"), Value::Int(3));
}

#[test]
fn parameters_loop_variables_and_match_bindings_are_assignable() {
    // 规则：只有 `let` 绑定受保护；参数、循环变量、match 绑定可重新赋值
    assert_eq!(ok("fn f(x) { x = x + 1\n    x }\nf(1)"), Value::Int(2));
    assert_eq!(
        ok("mut sum = 0\nfor i in 3 { sum += i }\nsum"),
        Value::Int(3)
    );
    assert_eq!(
        ok("mut v = 0\nmatch 7 { n => { v = n\n    v } }"),
        Value::Int(7)
    );
}

#[test]
fn immutable_capture_cannot_be_written_from_a_closure() {
    match compile_err("let n = 1\nlet f = () => { n += 1 }\nf()") {
        CompileError::AssignToImmutable(name) => assert_eq!(name, "n"),
        other => panic!("expected AssignToImmutable, got {other:?}"),
    }
}

#[test]
fn mutable_capture_can_be_written_from_a_closure() {
    assert_eq!(
        ok("mut n = 1\nlet f = () => { n += 1 }\nf()\nn"),
        Value::Int(2)
    );
}

#[test]
fn containers_behind_an_immutable_binding_stay_mutable() {
    // 与 JS `const` 一致：绑定不可重指向，但容器内容可变
    assert_eq!(ok("let a = [1, 2]\na[0] = 9\na[0]"), Value::Int(9));
    assert_eq!(ok("let m = { k: 1 }\nm.k = 5\nm.k"), Value::Int(5));
    assert_eq!(ok("let a = [1]\na.push(2)\na.len"), Value::Int(2));
}

// ============================================================ T3.2 match 穷尽

#[test]
fn non_exhaustive_match_raises_a_clear_error() {
    match run("match 99 { 1 => \"one\" }") {
        Err(VmError::NoMatch) => {}
        other => panic!("expected NoMatch, got {other:?}"),
    }
}

#[test]
fn wildcard_and_bare_binding_arms_are_irrefutable() {
    assert_eq!(
        ok("match 99 { 1 => \"one\", _ => \"other\" }"),
        Value::string("other")
    );
    assert_eq!(ok("match 42 { n => n }"), Value::Int(42));
}

#[test]
fn a_guarded_binding_arm_is_not_irrefutable() {
    // guard 失败时不再有兜底 arm → 显式报错
    match run("match 99 { n if n > 100 => n }") {
        Err(VmError::NoMatch) => {}
        other => panic!("expected NoMatch, got {other:?}"),
    }
    // guard 通过时正常返回
    assert_eq!(ok("match 99 { n if n > 10 => n }"), Value::Int(99));
}

#[test]
fn matched_arms_still_work_after_the_exhaustiveness_change() {
    let code = r#"
        fn grade(score) {
            match score {
                >= 90 => "A",
                >= 60 => "C",
                _ => "F"
            }
        }
        grade(95) + grade(70) + grade(10)
    "#;
    assert_eq!(ok(code), Value::string("ACF"));
}

// ============================================================ T3.5 数值语义

#[test]
fn modulo_accepts_mixed_numeric_types() {
    assert_eq!(ok("5.0 % 2"), Value::Float(1.0));
    assert_eq!(ok("5 % 2.0"), Value::Float(1.0));
    assert_eq!(ok("5 % 2"), Value::Int(1));
    expect_type_error("1 % 0", "Modulo by zero");
    expect_type_error("1 % \"x\"", "Cannot modulo");
}

#[test]
fn ordered_comparisons_reject_non_numeric_operands() {
    expect_type_error("5 < \"a\"", "Cannot compare");
    expect_type_error("nil > 0", "Cannot compare");
    expect_type_error("true <= false", "Cannot compare");
    // 数值混合比较仍然成立
    assert_eq!(ok("to_int(1 < 2.5)"), Value::Int(1));
}

#[test]
fn nan_comparisons_are_all_false() {
    // IEEE：NaN 与任何值的有序比较均为 false
    assert_eq!(ok("to_int((0.0 / 0.0) < 1.0)"), Value::Int(0));
    assert_eq!(ok("to_int((0.0 / 0.0) <= 1.0)"), Value::Int(0));
    assert_eq!(ok("to_int((0.0 / 0.0) > 1.0)"), Value::Int(0));
    assert_eq!(ok("to_int((0.0 / 0.0) >= 1.0)"), Value::Int(0));
}

#[test]
fn float_division_by_zero_keeps_ieee_semantics() {
    // 整数除零报错；浮点遵循 IEEE（inf / NaN），已在规范中明确
    assert_eq!(ok("1.0 / 0.0"), Value::Float(f64::INFINITY));
    expect_type_error("1 / 0", "Division by zero");
}

#[test]
fn negative_indexes_count_from_the_end() {
    assert_eq!(ok("[10, 20, 30][-1]"), Value::Int(30));
    assert_eq!(ok("[10, 20, 30][-3]"), Value::Int(10));
    assert_eq!(ok("\"abc\"[-1]"), Value::string("c"));
    assert_eq!(ok("[10, 20][-3]"), Value::Nil);
    assert_eq!(ok("\"abc\"[-9]"), Value::Nil);
}

#[test]
fn negative_index_assignment_targets_the_tail() {
    assert_eq!(ok("mut a = [1, 2, 3]\na[-1] = 9\na[2]"), Value::Int(9));
    match run("mut a = [1]\na[-2] = 0") {
        Err(VmError::IndexOutOfBounds { index, len }) => {
            assert_eq!((index, len), (-2, 1));
        }
        other => panic!("expected IndexOutOfBounds, got {other:?}"),
    }
}

// ============================================================ T3.6 浮点显示

#[test]
fn whole_floats_print_with_a_decimal_point() {
    assert_eq!(ok("to_str(1.0)"), Value::string("1.0"));
    assert_eq!(ok("to_str(1.5)"), Value::string("1.5"));
    assert_eq!(ok("to_str(-2.0)"), Value::string("-2.0"));
    assert_eq!(ok("to_str(2.0 * 3.0)"), Value::string("6.0"));
    // 整数不受影响
    assert_eq!(ok("to_str(1)"), Value::string("1"));
    // 超出精确整数范围的浮点沿用科学计数法（不展开成长串）
    assert_eq!(ok("to_str(math.pow(10.0, 300.0))"), Value::string("1e300"));
}

// ============================================================ T3.4 插值定位

#[test]
fn errors_inside_string_interpolation_point_at_the_real_source_line() {
    let source = "let a = 1\nlet s = $\"pre {a +} post\"\n";
    // 注意：插值片段是在 parse_program 阶段解析的，词法阶段不会报错
    let mut parser = Parser::from_source(source).expect("tokenizing should succeed");
    let err = match parser.parse_program() {
        Err(err) => err,
        Ok(_) => panic!("interpolation should fail to parse"),
    };
    let (line, col) = match err {
        ada_parser::ParseError::UnexpectedToken { line, col, .. } => (line, col),
        ada_parser::ParseError::UnexpectedEof { line, col, .. } => (line, col),
        other => panic!("unexpected error kind: {other:?}"),
    };
    // 修复前片段被独立解析，错误会落在第 1 行；现在应指向第 2 行的插值内部
    assert_eq!(line, 2, "error should be reported on the interpolated line");
    assert!(
        col >= 16,
        "error column {col} should point inside `{{a +}}`"
    );
}

// ============================================================ T3.3 `{ x }` 判定

#[test]
fn brace_with_a_bare_identifier_is_a_map_not_a_block() {
    // 规则（决策 D4）：`{ x }` 是 map 字面量简写；要写块表达式请先放一条语句
    assert_eq!(ok("let x = 5\ntype_of({ x })"), Value::string("map"));
    assert_eq!(ok("let x = 5\n{ x }.x"), Value::Int(5));
    // 块形式：以语句开头时按块解析，末尾表达式为块值
    assert_eq!(ok("let v = { let a = 1\n    a + 1 }\nv"), Value::Int(2));
}

// ============================================================ 交叉回归

#[test]
fn valid_programs_from_earlier_milestones_still_run() {
    let code = r#"
        let threshold = 3
        let xs = [1, 2, 3, 4, 5] |> filter(x => x > threshold) |> collect()
        mut total = 0
        for x in xs { total += x }
        total
    "#;
    assert_eq!(ok(code), Value::Int(9));
}
