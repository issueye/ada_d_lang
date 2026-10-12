//! 编辑器客户端 ↔ `ada-lsp` 的互通测试（开发计划 T7.2）。
//!
//! 单元测试验证协议逻辑，`session.rs` 验证真实进程的分帧，这里再往前一步：
//! 用**随扩展一起发布的**零依赖 LSP 客户端（`editors/vscode/lsp-client.js`）
//! 去驱动真实的 `ada-lsp` 二进制，确认"编辑器那条路"确实能拿到诊断。
//!
//! 需要 `node`；没有则跳过（打印说明），CI 的 ubuntu 与开发机都自带。

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn vscode_client_core_talks_to_the_real_server() {
    if !node_available() {
        println!("note: node 不可用，跳过编辑器客户端互通测试");
        return;
    }

    let driver = workspace_root().join("crates/ada_lsp/tests/fixtures/node_client_driver.js");
    assert!(driver.exists(), "driver missing: {}", driver.display());
    // 客户端核心是扩展的一部分，必须存在
    assert!(workspace_root()
        .join("editors/vscode/lsp-client.js")
        .exists());

    let client_core = workspace_root().join("editors/vscode/lsp-client.js");
    assert!(
        client_core.exists(),
        "the shipped client core is missing: {}",
        client_core.display()
    );

    let output = Command::new("node")
        .arg(&driver)
        .arg(env!("CARGO_BIN_EXE_ada-lsp"))
        .arg(&client_core)
        .output()
        .expect("node driver should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "driver failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );

    let report: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|err| panic!("driver did not print JSON ({err}): {stdout}"));

    // 1) 语法错误：0 基行 0、列 12
    let syntax = report["syntax"].as_array().expect("syntax diagnostics");
    assert_eq!(syntax.len(), 1, "expected one syntax diagnostic: {report}");
    assert_eq!(syntax[0]["range"]["start"]["line"], 0);
    assert_eq!(syntax[0]["range"]["start"]["character"], 12);
    assert_eq!(syntax[0]["severity"], 1);
    assert_eq!(syntax[0]["source"], "ada");
    assert!(syntax[0]["message"]
        .as_str()
        .expect("message")
        .contains("expected expression"));

    // 2) 编译错误：第 3 行（0 基 2）
    let compile = report["compile"].as_array().expect("compile diagnostics");
    assert_eq!(
        compile.len(),
        1,
        "expected one compile diagnostic: {report}"
    );
    assert_eq!(compile[0]["range"]["start"]["line"], 2);
    assert!(compile[0]["message"]
        .as_str()
        .expect("message")
        .contains("Cannot assign to immutable variable"));

    // 3) 修好后清空
    assert_eq!(
        report["fixed"],
        serde_json::json!([]),
        "fixed document should publish an empty list: {report}"
    );

    // 客户端确实收到了 3 次发布
    assert_eq!(report["publishes"], 3);
}
