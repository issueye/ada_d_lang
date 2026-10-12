//! 错误携带源码位置（开发计划 T1.8）。
//!
//! 契约：
//! - `Engine::eval_diagnostic` 在失败时返回 `EvalFailure { error, position }`；
//! - 词法/语法错误用词法位置，编译错误用 `CompileError::span`，运行时错误用
//!   VM 记录的"失败指令"位置；
//! - 任务失败用**任务自己**的位置（`await` 那一行不是失败点）；
//! - `Engine::eval_value` 保持原签名（只返回 `Error`），老代码不受影响。

use ada_engine::{Engine, Error, Position, VmError};

fn failure(source: &str) -> (Error, Option<Position>) {
    let mut engine = Engine::new();
    // Engine::new() 是 strict（M5）；这些用例只做纯计算 + 并发，无需宿主能力
    let err = engine
        .eval_diagnostic(source)
        .expect_err("expected this program to fail");
    (err.error, err.position)
}

fn line_of(source: &str) -> Option<usize> {
    failure(source).1.map(|p| p.line)
}

#[test]
fn parse_and_lex_errors_report_their_position() {
    let (err, pos) = failure("let x = (1 +");
    assert!(matches!(err, Error::Parse(_)), "got {err:?}");
    assert_eq!(pos.map(|p| (p.line, p.col)), Some((1, 13)));
    assert_eq!(
        err.position(),
        pos,
        "Error::position agrees with EvalFailure"
    );

    let (err, pos) = failure("@@@");
    assert!(matches!(err, Error::Parse(_)));
    assert_eq!(pos.map(|p| (p.line, p.col)), Some((1, 1)));
}

#[test]
fn compile_errors_report_the_offending_line() {
    let (err, pos) = failure("let a = 1\nlet b = 2\na = 3");
    match &err {
        Error::Compile(compile_err) => {
            assert!(
                compile_err.span().is_some(),
                "compile error should carry a span: {compile_err:?}"
            );
        }
        other => panic!("expected a compile error, got {other:?}"),
    }
    assert_eq!(pos.map(|p| p.line), Some(3), "assignment is on line 3");
    assert_eq!(err.position(), pos);
}

#[test]
fn runtime_errors_point_at_the_failing_instruction() {
    assert_eq!(line_of("let a = 1\nlet b = 1 / 0"), Some(2));
    assert_eq!(line_of("let xs = [1, 2, 3]\nxs[9] + 1"), Some(2));
    assert_eq!(
        line_of("fn f(n) {\n    match n {\n        1 => 1\n    }\n}\nf(2)"),
        Some(2),
        "非穷尽 match 的 `MatchFail` 用的是 match 表达式自身的 span（第 2 行）"
    );
    assert_eq!(line_of("let a = 1\nnope"), Some(2));
}

#[test]
fn runtime_error_position_accounts_for_multiline_expressions() {
    // 注意：数组越界**读**返回 nil（不是错误），因此这里用 `+ 1` 触发类型错误
    let source = "let data = [\n    1,\n    2\n]\nlet bad = data[7] + 1\nbad";
    assert_eq!(line_of(source), Some(5));
}

#[test]
fn vm_errors_do_not_expose_positions_on_the_error_type_itself() {
    // 位置来自 VM 的失败指令，`Error` 不持有 VM 状态；因此 `Error::position()`
    // 对运行时错误返回 None，位置只经由 `EvalFailure` 提供（文档化行为）。
    let (err, pos) = failure("1 / 0");
    assert!(
        matches!(err, Error::Vm(VmError::TypeError(_))),
        "got {err:?}"
    );
    assert_eq!(pos.map(|p| p.line), Some(1));
    assert_eq!(err.position(), None);
}

#[test]
fn task_failures_point_inside_the_task_not_at_the_await() {
    let source = "fn boom() {\n    1 / 0\n}\nlet t = spawn boom()\nawait t";
    let (err, pos) = failure(source);
    assert!(
        matches!(err, Error::Vm(VmError::TaskFailed { .. })),
        "got {err:?}"
    );
    assert_eq!(
        pos.map(|p| p.line),
        Some(2),
        "the failure happened on line 2 inside boom()"
    );
}

#[test]
fn awaited_failures_are_attributed_to_the_failing_task() {
    // 两个任务：先 await 成功的那个，再 await 失败的那个
    let source = "fn ok() {\n    7\n}\nfn bad() {\n    1 / 0\n}\nlet a = spawn bad()\nlet b = spawn ok()\nawait b\nawait a";
    let (err, pos) = failure(source);
    assert!(
        matches!(err, Error::Vm(VmError::TaskFailed { .. })),
        "got {err:?}"
    );
    assert_eq!(pos.map(|p| p.line), Some(5), "bad() fails on line 5");
}

#[test]
fn adapter_callback_errors_point_at_the_call_site() {
    // 回调内部的错误会被适配器原样向上抛（此处是 TypeError）；因为回调是 Rust
    // 栈帧里的可重入调用，位置归到调用适配器的那一行
    let source = "let xs = [1, 2, 3]\nlet ys = xs.map(x => 1 / 0)\nys";
    let (err, pos) = failure(source);
    assert!(
        matches!(err, Error::Vm(VmError::TypeError(_))),
        "got {err:?}"
    );
    assert_eq!(pos.map(|p| p.line), Some(2));
}

#[test]
fn eval_value_keeps_the_error_only_signature() {
    // 兼容路径：老 API 仍返回 Error，且不 panic
    let mut engine = Engine::new();
    let err = engine.eval_value("let a = 1\na = 2").unwrap_err();
    assert!(matches!(err, Error::Compile(_)), "got {err:?}");
    assert!(
        err.position().is_some(),
        "compile errors know their position"
    );
}

#[test]
fn failure_display_includes_the_position() {
    let mut engine = Engine::new();
    let failure = engine.eval_diagnostic("let a = 1\n1 / 0").unwrap_err();
    let text = failure.to_string();
    assert!(
        text.starts_with("2:"),
        "display should start with line:col, got {text}"
    );
    assert!(text.contains("Division by zero"));
    // message() 给的是不带位置的人话（CLI 自己决定前缀）
    assert!(failure.message().contains("Division by zero"));
    assert!(!failure.message().starts_with("2:"));
    // 标准错误链
    use std::error::Error as _;
    assert!(failure.source().is_some());
}

#[test]
fn successful_evaluations_carry_no_position() {
    let mut engine = Engine::new();
    let value = engine.eval_diagnostic("1 + 1").expect("should succeed");
    assert_eq!(value.to_string(), "2");
}

#[test]
fn repeated_evaluations_do_not_leak_positions() {
    let mut engine = Engine::new();
    let first = engine.eval_diagnostic("1 / 0").unwrap_err();
    assert_eq!(first.position.map(|p| p.line), Some(1));

    assert!(engine.eval_diagnostic("40 + 2").is_ok());

    // 失败 → 成功 → 再失败：位置必须重新计算，而不是沿用上次的
    let again = engine
        .eval_diagnostic("let x = 1\nlet y = 2\n2 % 0")
        .unwrap_err();
    assert_eq!(again.position.map(|p| p.line), Some(3));
}
