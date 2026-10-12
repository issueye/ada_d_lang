//! Golden 脚本测试（开发计划 M2 退出标准）。
//!
//! `examples/showcase.ada` 汇总了 SPEC_LANGUAGE §5–§8、SPEC_STDLIB §2–§6、
//! SPEC_ASYNC §2 的规范示例；其**确定性**输出与 `examples/showcase.expected.txt`
//! 逐行比对。任何一个规范示例跑不出预期结果，这里都会失败——这是防止
//! "文档承诺 > 实现"（风险 R1）的最后一道闸门。
//!
//! 更新方式：确认输出变化是**有意**的语义变更后，重新生成期望文件：
//! `cargo run -p ada_cli -- examples/showcase.ada > examples/showcase.expected.txt`

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

fn run_script(relative: &str) -> std::process::Output {
    let script = workspace_root().join(relative);
    assert!(script.exists(), "script not found: {}", script.display());
    Command::new(env!("CARGO_BIN_EXE_ada_cli"))
        .arg(&script)
        .output()
        .expect("ada_cli should be runnable")
}

#[test]
fn showcase_script_matches_golden_output() {
    let output = run_script("examples/showcase.ada");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "examples/showcase.ada failed to run: {stderr}"
    );

    let actual = normalize(&String::from_utf8_lossy(&output.stdout));
    let golden_path = workspace_root().join("examples/showcase.expected.txt");
    let expected = normalize(
        &std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", golden_path.display())),
    );

    if actual != expected {
        let diff: Vec<String> = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .filter(|(_, (e, a))| e != a)
            .map(|(i, (e, a))| format!("  line {}:\n    expected: {e}\n    actual:   {a}", i + 1))
            .collect();
        panic!(
            "showcase output diverged from golden file ({} lines expected, {} actual)\n{}",
            expected.lines().count(),
            actual.lines().count(),
            if diff.is_empty() {
                "  (length mismatch; see raw output)".to_string()
            } else {
                diff.join("\n")
            }
        );
    }
}

#[test]
fn demo_script_runs_successfully() {
    // demo.ada 含 time/sleep 输出，故只断言成功退出，不做逐行比对
    let output = run_script("examples/demo.ada");
    assert!(
        output.status.success(),
        "examples/demo.ada failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn showcase_covers_every_spec_section() {
    // 防止有人在更新 showcase 时删掉某个规范小节的覆盖
    let script = std::fs::read_to_string(workspace_root().join("examples/showcase.ada"))
        .expect("showcase.ada readable");
    for marker in [
        "-- variables --",
        "-- functions --",
        "-- control flow --",
        "-- indexing --",
        "-- closures --",
        "-- match --",
        "-- navigation --",
        "-- equality --",
        "-- stdlib globals --",
        "-- stdlib modules --",
        "-- prototypes --",
        "-- iteration --",
        "-- async syntax --",
    ] {
        assert!(
            script.contains(marker),
            "showcase.ada lost the '{marker}' section"
        );
    }
}
