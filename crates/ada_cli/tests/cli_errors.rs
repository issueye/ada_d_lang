//! CLI 错误输出格式门禁（开发计划 T0.6）。
//!
//! 契约：错误必须是**单行**，形如
//! - `file:line:col: error: <消息>`（词法/语法错误，位置可用时）
//! - `file: error: <消息>`（编译/运行时错误，当前没有 span，见 T1.8）
//!
//! 工具与编辑器（以及 T7.2 的 LSP）依赖这个稳定格式，因此这里逐条固定住它。

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ada_cli"))
        .args(args)
        .output()
        .expect("ada_cli should be runnable");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
    )
}

fn assert_single_line_error(stderr: &str) -> String {
    let trimmed = stderr.trim_end();
    assert!(
        !trimmed.is_empty(),
        "expected an error message on stderr, got nothing"
    );
    assert_eq!(
        trimmed.lines().count(),
        1,
        "error output must be a single line, got:\n{trimmed}"
    );
    trimmed.to_string()
}

#[test]
fn parse_errors_report_line_and_column() {
    let (ok, stdout, stderr) = run(&["-e", "let x = (1 +"]);
    assert!(!ok, "syntax error must fail");
    assert!(stdout.is_empty());
    let line = assert_single_line_error(&stderr);
    assert!(
        line.starts_with("<eval>:1:13: error: "),
        "unexpected format: {line}"
    );
    assert!(
        line.contains("expected expression"),
        "message should keep the expectation: {line}"
    );
}

#[test]
fn lex_errors_report_position_and_reason() {
    let (ok, _, stderr) = run(&["-e", "@@@"]);
    assert!(!ok);
    let line = assert_single_line_error(&stderr);
    assert_eq!(line, "<eval>:1:1: error: Unexpected character '@'");
}

#[test]
fn file_errors_use_the_file_name_as_the_source() {
    let dir = std::env::temp_dir().join("ada_cli_error_format");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = dir.join("broken.ada");
    std::fs::write(&script, "let a = 1\nlet b = (2 +\n").expect("write script");

    let (ok, _, stderr) = run(&[script.to_str().unwrap()]);
    assert!(!ok);
    let line = assert_single_line_error(&stderr);
    assert!(
        line.starts_with(&format!("{}:3:1: error: ", script.display())),
        "unexpected format: {line}"
    );

    let _ = std::fs::remove_file(&script);
}

#[test]
fn compile_and_runtime_errors_use_the_display_message() {
    // 编译期：`let` 不可变
    let (ok, _, stderr) = run(&["-e", "let a = 1\na = 2"]);
    assert!(!ok);
    let line = assert_single_line_error(&stderr);
    assert_eq!(
        line,
        "<eval>: error: Cannot assign to immutable variable 'a' (declare it with `mut`)"
    );

    // 运行时：非穷尽 match
    let (ok, _, stderr) = run(&["-e", "match 99 { 1 => \"one\" }"]);
    assert!(!ok);
    let line = assert_single_line_error(&stderr);
    assert!(
        line.starts_with("<eval>: error: No `match` arm matched"),
        "unexpected format: {line}"
    );

    // 沙箱能力
    let (ok, _, stderr) = run(&["--strict", "-e", "time.now_ms()"]);
    assert!(!ok);
    let line = assert_single_line_error(&stderr);
    assert_eq!(
        line,
        "<eval>: error: Capability 'time' is not available in the 'strict' sandbox profile"
    );
}

#[test]
fn successful_runs_print_only_program_output() {
    let (ok, stdout, stderr) = run(&["-e", "println(\"hi\")\n1 + 1"]);
    assert!(ok);
    assert_eq!(stdout, "hi\n2\n");
    assert!(stderr.is_empty(), "stderr should be empty: {stderr}");
}

#[test]
fn errors_do_not_include_debug_style_payloads() {
    // Debug 形式（`Error: Vm(NoMatch)` / `Compile(AssignToImmutable("a"))`）不应再出现在面向用户的输出里
    for source in ["match 99 { 1 => \"one\" }", "let a = 1\na = 2"] {
        let (_, _, stderr) = run(&["-e", source]);
        for forbidden in ["Compile(", "Vm(", "Parse(", "Some(", "{"] {
            assert!(
                !stderr.contains(forbidden),
                "stderr leaked a debug payload ({forbidden}) for {source:?}: {stderr}"
            );
        }
    }
}

#[test]
fn error_format_matches_the_documented_shape() {
    // 文档中承诺的正则形状：可选 `file:` + 可选 `line:col:` + `error: ` + 消息
    let cases: Vec<Vec<&str>> = vec![
        vec!["-e", "let x = (1 +"],
        vec!["-e", "@@@"],
        vec!["-e", "1 / 0"],
    ];
    let workspace_docs = workspace_root().join("docs/SPEC_RUNTIME.md");
    let docs = std::fs::read_to_string(&workspace_docs).expect("SPEC_RUNTIME readable");
    assert!(
        docs.contains("error: <消息>") && docs.contains("file:line:col"),
        "SPEC_RUNTIME §8.1 should document the CLI error format"
    );

    for args in cases {
        let (_, _, stderr) = run(&args);
        let line = assert_single_line_error(&stderr);
        let has_error_marker = line.contains(": error: ");
        assert!(has_error_marker, "missing `: error: ` marker in {line}");
    }
}
