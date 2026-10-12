//! Ada-D 的最小 LSP 服务器（开发计划 T7.2）。
//!
//! **定位**：把编译器已经知道的诊断（词法 / 语法 / 编译错误，均带 `line:col`，见 T1.8）
//! 通过 LSP 送给编辑器，让写 Ada-D 时能实时看到波浪线。不做补全、跳转、重构。
//!
//! **只做静态诊断，不执行脚本**：`Engine::check` 只走解析 + 编译。这是刻意的安全选择——
//! 编辑器中每次输入都会触发诊断，若真的执行脚本，`sleep_ms`、宿主能力、沙箱限额都会
//! 影响编辑器（并且有副作用）。运行时错误因此不在本服务器范围内。
//!
//! 协议范围（最小可用子集，LSP 3.17）：
//!
//! | 方法 | 方向 | 处理 |
//! | :--- | :--- | :--- |
//! | `initialize` | 请求 | 回 `capabilities.textDocumentSync = 1`（全量同步）+ `serverInfo` |
//! | `initialized` | 通知 | 忽略 |
//! | `textDocument/didOpen` | 通知 | 存文本 → 发布诊断 |
//! | `textDocument/didChange` | 通知 | 用最后一次 change 的全文替换 → 发布诊断 |
//! | `textDocument/didClose` | 通知 | 丢弃文本 → 发布空诊断（清掉波浪线） |
//! | `shutdown` | 请求 | 回 `null` |
//! | `exit` | 通知 | 结束进程（未 `shutdown` 则以码 1 退出，符合规范） |
//! | 其他 | — | 请求回 `-32601`，通知忽略 |
//!
//! 逻辑（`handle` / `diagnose`）与 I/O（`run_stdio`）分离，因此可以在测试里直接驱动会话，
//! 不必每次都起进程。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

/// 一条诊断（行/列均从 1 开始，与编译器一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// 静态诊断：解析 + 编译，不执行脚本。
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    match ada_engine::Engine::check(source) {
        Ok(()) => Vec::new(),
        Err(failure) => {
            let (line, col) = failure.position.map(|p| (p.line, p.col)).unwrap_or((1, 1));
            vec![Diagnostic {
                line,
                col,
                message: failure.message(),
            }]
        }
    }
}

/// 会话状态：已打开文档的全文。
#[derive(Default)]
pub struct ServerState {
    documents: HashMap<String, String>,
    /// 会话是否已收到 `shutdown`（影响 `exit` 的退出码）
    pub shutdown_requested: bool,
}

impl ServerState {
    pub fn document(&self, uri: &str) -> Option<&str> {
        self.documents.get(uri).map(|text| text.as_str())
    }
}

/// 一次处理的结果：要发出的消息 + 是否该退出。
#[derive(Debug, Default)]
pub struct Handled {
    pub messages: Vec<Value>,
    pub exit: bool,
}

/// 处理一条入站消息（纯逻辑，便于测试）。
pub fn handle(state: &mut ServerState, message: &Value) -> Handled {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Handled {
            messages: vec![json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "capabilities": { "textDocumentSync": 1 },
                    "serverInfo": { "name": "ada-lsp", "version": env!("CARGO_PKG_VERSION") }
                }
            })],
            exit: false,
        },
        "initialized" => Handled::default(),
        "shutdown" => {
            state.shutdown_requested = true;
            Handled {
                messages: vec![json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null })],
                exit: false,
            }
        }
        "exit" => Handled {
            messages: Vec::new(),
            exit: true,
        },
        "textDocument/didOpen" => {
            let text_document = params.get("textDocument").cloned().unwrap_or(Value::Null);
            let uri = text_document
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let text = text_document
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if uri.is_empty() {
                return Handled::default();
            }
            state.documents.insert(uri.clone(), text);
            Handled {
                messages: vec![publish(&uri, state.document(&uri).unwrap_or_default())],
                exit: false,
            }
        }
        "textDocument/didChange" => {
            let uri = params
                .get("textDocument")
                .and_then(|doc| doc.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if uri.is_empty() {
                return Handled::default();
            }
            // 全量同步（textDocumentSync = 1）：取最后一个 change 的 text
            if let Some(changes) = params.get("contentChanges").and_then(Value::as_array) {
                if let Some(text) = changes
                    .iter()
                    .rev()
                    .find_map(|change| change.get("text").and_then(Value::as_str))
                {
                    state.documents.insert(uri.clone(), text.to_string());
                }
            }
            Handled {
                messages: vec![publish(&uri, state.document(&uri).unwrap_or_default())],
                exit: false,
            }
        }
        "textDocument/didClose" => {
            let uri = params
                .get("textDocument")
                .and_then(|doc| doc.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            state.documents.remove(&uri);
            Handled {
                messages: vec![publish(&uri, "")],
                exit: false,
            }
        }
        _ => {
            // 请求（有 id）要回错，通知（无 id）默默忽略
            match id {
                Some(id) => Handled {
                    messages: vec![json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("method not found: {method}") }
                    })],
                    exit: false,
                },
                None => Handled::default(),
            }
        }
    }
}

/// 构造 `textDocument/publishDiagnostics` 通知。
fn publish(uri: &str, source: &str) -> Value {
    let diagnostics: Vec<Value> = diagnose(source)
        .into_iter()
        .map(|diagnostic| {
            let line = diagnostic.line.saturating_sub(1);
            let character = diagnostic.col.saturating_sub(1);
            // 高亮一个字符；行尾时向前收一格，避免越界
            let line_len = source
                .lines()
                .nth(line)
                .map(|text| text.chars().count())
                .unwrap_or(0);
            let end_character = (character + 1).min(line_len.max(character));
            json!({
                "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": end_character }
                },
                "severity": 1,
                "source": "ada",
                "message": diagnostic.message
            })
        })
        .collect();

    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    })
}

// ---------------------------------------------------------------- stdio 传输

/// 读一条 LSP 消息；`Ok(None)` 表示对端关闭（EOF）。
pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let read = reader.read_line(&mut header)?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let Some(length) = content_length else {
        // 没有长度头：忽略这条消息（不 panic）
        return Ok(Some(Value::Null));
    };
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    // 非法 JSON 也不致命：交给调用方当作 Null 处理
    Ok(Some(serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// 写一条 LSP 消息（`Content-Length` 分帧）。
pub fn write_message(writer: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// 在 stdio 上跑服务器，直到收到 `exit` 或输入结束。
///
/// 返回是否"干净退出"（收到过 `shutdown`）。
pub fn serve(input: impl Read, output: impl Write) -> std::io::Result<bool> {
    let mut reader = BufReader::new(input);
    let mut writer = output;
    let mut state = ServerState::default();

    while let Some(message) = read_message(&mut reader)? {
        let handled = handle(&mut state, &message);
        for reply in handled.messages {
            write_message(&mut writer, &reply)?;
        }
        if handled.exit {
            return Ok(state.shutdown_requested);
        }
    }
    // 输入结束但没收到 exit：按规范属于异常终止
    Ok(false)
}

/// `ada-lsp` 主入口使用的 stdio 版本。
pub fn run_stdio() -> std::io::Result<bool> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(messages: &[Value]) -> String {
        serde_json::to_string(messages).expect("serializable")
    }

    #[test]
    fn clean_source_has_no_diagnostics() {
        assert!(diagnose("let a = 1\na + 1").is_empty());
    }

    #[test]
    fn syntax_error_reports_line_and_column() {
        let diagnostics = diagnose("let x = (1 +");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!((diagnostics[0].line, diagnostics[0].col), (1, 13));
        assert!(diagnostics[0].message.contains("expected expression"));
    }

    #[test]
    fn compile_error_reports_its_line() {
        let diagnostics = diagnose("let a = 1\nlet b = 2\na = 3");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Cannot assign to immutable variable"));
    }

    #[test]
    fn did_not_run_side_effects() {
        // 静态检查绝不执行：这里有无限循环 + 打印，若被执行会挂住/产生输出
        assert!(diagnose("while true { println(\"nope\") }").is_empty());
    }

    #[test]
    fn initialize_advertises_full_sync() {
        let mut state = ServerState::default();
        let handled = handle(
            &mut state,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        );
        assert!(!handled.exit);
        let text = text_of(&handled.messages);
        assert!(text.contains("\"textDocumentSync\":1"), "{text}");
        assert!(text.contains("ada-lsp"), "{text}");
    }

    #[test]
    fn did_open_publishes_a_diagnostic_at_the_right_position() {
        let mut state = ServerState::default();
        let handled = handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": "file:///a.ada", "text": "let a = 1\nlet b = 1 / 0\n" } }
            }),
        );
        // 除零是运行时错误 → 静态检查不报（文档化行为）
        let text = text_of(&handled.messages);
        assert!(text.contains("\"diagnostics\":[]"), "{text}");

        let handled = handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": "file:///b.ada", "text": "let x = (1 +" } }
            }),
        );
        let text = text_of(&handled.messages);
        assert!(text.contains("\"line\":0"), "LSP 行号从 0 开始：{text}");
        assert!(text.contains("\"character\":12"), "{text}");
        assert_eq!(state.document("file:///b.ada"), Some("let x = (1 +"));
    }

    #[test]
    fn did_change_replaces_the_text_and_clears_diagnostics() {
        let mut state = ServerState::default();
        let uri = "file:///c.ada";
        handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "text": "let x = (1 +" } }
            }),
        );
        let handled = handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": uri },
                    "contentChanges": [ { "text": "let x = 1 + 1\nx" } ]
                }
            }),
        );
        assert_eq!(state.document(uri), Some("let x = 1 + 1\nx"));
        let text = text_of(&handled.messages);
        assert!(text.contains("\"diagnostics\":[]"), "{text}");
    }

    #[test]
    fn did_close_clears_diagnostics() {
        let mut state = ServerState::default();
        let uri = "file:///d.ada";
        handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "text": "let x = (1 +" } }
            }),
        );
        let handled = handle(
            &mut state,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": uri } }
            }),
        );
        assert!(state.document(uri).is_none());
        assert!(text_of(&handled.messages).contains("\"diagnostics\":[]"));
    }

    #[test]
    fn unknown_request_gets_method_not_found() {
        let mut state = ServerState::default();
        let handled = handle(
            &mut state,
            &json!({ "jsonrpc": "2.0", "id": 7, "method": "textDocument/completion" }),
        );
        assert!(text_of(&handled.messages).contains("-32601"));
    }

    #[test]
    fn unknown_notification_is_ignored() {
        let mut state = ServerState::default();
        let handled = handle(
            &mut state,
            &json!({ "jsonrpc": "2.0", "method": "$/setTrace", "params": {} }),
        );
        assert!(handled.messages.is_empty());
    }

    #[test]
    fn malformed_params_do_not_panic() {
        let mut state = ServerState::default();
        // 缺 uri / 类型不对 / 空 params：都不能 panic
        for message in [
            json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {} }),
            json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": 3 } }),
            json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": { "textDocument": { "uri": "u" }, "contentChanges": "nope" } }),
            json!({ "jsonrpc": "2.0", "method": "shutdown" }),
            json!({ "jsonrpc": "2.0", "method": "exit" }),
        ] {
            let _ = handle(&mut state, &message);
        }
    }

    #[test]
    fn shutdown_then_exit_is_clean() {
        let mut state = ServerState::default();
        assert!(!handle(&mut state, &json!({ "id": 1, "method": "shutdown" })).exit);
        assert!(state.shutdown_requested);
        assert!(handle(&mut state, &json!({ "method": "exit" })).exit);
    }

    #[test]
    fn framing_round_trips() {
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        let mut buffer: Vec<u8> = Vec::new();
        write_message(&mut buffer, &message).expect("write");
        let mut reader = std::io::BufReader::new(std::io::Cursor::new(buffer));
        let decoded = read_message(&mut reader).expect("read").expect("message");
        assert_eq!(decoded, message);

        // EOF
        let mut empty = std::io::BufReader::new(std::io::Cursor::new(Vec::<u8>::new()));
        assert!(read_message(&mut empty).expect("no error").is_none());
    }

    #[test]
    fn garbage_input_does_not_panic() {
        // 有长度头但不是 JSON
        let raw = b"Content-Length: 5\r\n\r\n{{{{{";
        let mut reader = std::io::BufReader::new(std::io::Cursor::new(raw.to_vec()));
        assert_eq!(read_message(&mut reader).expect("read"), Some(Value::Null));

        // 没有长度头
        let raw = b"X-Whatever: 1\r\n\r\n";
        let mut reader = std::io::BufReader::new(std::io::Cursor::new(raw.to_vec()));
        assert_eq!(read_message(&mut reader).expect("read"), Some(Value::Null));
    }

    #[test]
    fn serve_handles_a_whole_session() {
        let mut input: Vec<u8> = Vec::new();
        write_message(
            &mut input,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        )
        .unwrap();
        write_message(
            &mut input,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": "file:///e.ada", "text": "let x = (1 +" } }
            }),
        )
        .unwrap();
        write_message(
            &mut input,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }),
        )
        .unwrap();
        write_message(&mut input, &json!({ "jsonrpc": "2.0", "method": "exit" })).unwrap();

        let mut output: Vec<u8> = Vec::new();
        let clean = serve(std::io::Cursor::new(input), &mut output).expect("serve");
        assert!(clean, "shutdown before exit should be reported as clean");

        // 输出是完整的 LSP 帧序列
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("\"textDocumentSync\":1"), "{text}");
        assert!(text.contains("publishDiagnostics"), "{text}");
        assert!(text.contains("expected expression"), "{text}");
        assert!(text.contains("\"id\":2"), "{text}");
    }
}
