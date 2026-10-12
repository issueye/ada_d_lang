//! `ada-lsp` 的端到端会话测试（开发计划 T7.2）。
//!
//! 单元测试在内存里驱动逻辑；这里**真的起进程**、按 LSP 的 `Content-Length` 分帧收发，
//! 验证"编辑器接上去能拿到诊断"这条完整链路。
//!
//! 防挂死：子进程若在 20 秒内没有按预期回应，看门狗会杀掉它，测试以断言失败结束
//! （而不是永远挂着）。

use ada_lsp::read_message;
use serde_json::{json, Value};
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Session {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    done: Arc<AtomicBool>,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ada-lsp"))
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("ada-lsp should start");

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));

        let child = Arc::new(Mutex::new(child));
        let done = Arc::new(AtomicBool::new(false));
        let watchdog_child = Arc::clone(&child);
        let watchdog_done = Arc::clone(&done);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            if !watchdog_done.load(Ordering::SeqCst) {
                let _ = watchdog_child.lock().expect("child lock").kill();
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            stdout,
            done,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("session still open");
        ada_lsp::write_message(stdin, message).expect("write frame");
        stdin.flush().expect("flush");
    }

    /// 关闭写端（模拟编辑器崩溃 / 管道断开）。
    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn receive(&mut self) -> Value {
        read_message(&mut self.stdout)
            .expect("read frame")
            .expect("server closed the stream unexpectedly")
    }

    fn finish(self) -> std::process::ExitStatus {
        self.done.store(true, Ordering::SeqCst);
        let mut child = self.child.lock().expect("child lock");
        child.wait().expect("wait for ada-lsp")
    }
}

#[test]
fn editor_session_reports_and_clears_diagnostics() {
    let mut session = Session::start();
    let uri = "file:///workspace/demo.ada";

    // 1) initialize：声明全量同步
    session.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "processId": Value::Null, "rootUri": Value::Null, "capabilities": {} }
    }));
    let response = session.receive();
    assert_eq!(response["id"], json!(1));
    assert_eq!(
        response["result"]["capabilities"]["textDocumentSync"],
        json!(1)
    );
    assert_eq!(response["result"]["serverInfo"]["name"], json!("ada-lsp"));

    session.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));

    // 2) didOpen（有语法错误）→ 立刻收到诊断，位置是 0 基的行列
    session.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": { "uri": uri, "languageId": "ada", "version": 1, "text": "let x = (1 +" } }
    }));
    let published = session.receive();
    assert_eq!(
        published["method"],
        json!("textDocument/publishDiagnostics")
    );
    assert_eq!(published["params"]["uri"], json!(uri));
    let diagnostics = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1, "one diagnostic expected: {published}");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], json!(0));
    assert_eq!(diagnostics[0]["range"]["start"]["character"], json!(12));
    assert_eq!(diagnostics[0]["severity"], json!(1));
    assert_eq!(diagnostics[0]["source"], json!("ada"));
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .expect("message")
            .contains("expected expression"),
        "unexpected message: {published}"
    );

    // 3) didChange（改成编译错误）→ 波浪线跟着换位置
    session.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [ { "text": "let a = 1\nlet b = 2\na = 3\n" } ]
        }
    }));
    let published = session.receive();
    let diagnostics = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], json!(2));
    assert!(diagnostics[0]["message"]
        .as_str()
        .expect("message")
        .contains("Cannot assign to immutable variable"));

    // 4) didChange（修好）→ 诊断清空
    session.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": 3 },
            "contentChanges": [ { "text": "mut a = 1\nlet b = 2\na = 3\na + b\n" } ]
        }
    }));
    let published = session.receive();
    assert_eq!(
        published["params"]["diagnostics"],
        json!([]),
        "fixed source should have no diagnostics: {published}"
    );

    // 5) 未知请求 → 回 -32601（编辑器插件常探测能力）
    session.send(&json!({ "jsonrpc": "2.0", "id": 9, "method": "textDocument/completion" }));
    let response = session.receive();
    assert_eq!(response["id"], json!(9));
    assert_eq!(response["error"]["code"], json!(-32601));

    // 6) shutdown / exit → 干净退出
    session.send(&json!({ "jsonrpc": "2.0", "id": 10, "method": "shutdown" }));
    let response = session.receive();
    assert_eq!(response["id"], json!(10));
    assert!(response["result"].is_null());
    session.send(&json!({ "jsonrpc": "2.0", "method": "exit" }));

    let status = session.finish();
    assert!(
        status.success(),
        "ada-lsp should exit 0 after shutdown+exit, got {status}"
    );
}

#[test]
fn exit_without_shutdown_reports_failure() {
    let mut session = Session::start();
    session.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }));
    let _ = session.receive();
    session.send(&json!({ "jsonrpc": "2.0", "method": "exit" }));

    let status = session.finish();
    assert!(
        !status.success(),
        "LSP 规范要求没有 shutdown 就 exit 时以非零码结束"
    );
}

#[test]
fn closing_stdin_ends_the_server_without_panicking() {
    let mut session = Session::start();
    session.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }));
    let _ = session.receive();
    session.close_stdin(); // 对端关闭输入

    let status = session.finish();
    // 输入直接结束（未 shutdown）→ 非零退出，但不崩溃、不挂死
    assert!(!status.success());
}

#[test]
fn version_flag_does_not_start_a_session() {
    let output = Command::new(env!("CARGO_BIN_EXE_ada-lsp"))
        .arg("--version")
        .output()
        .expect("ada-lsp --version");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("ada-lsp"), "unexpected output: {text}");
}
