//! `http` 模块（M9）：HTTP/1.1 与 HTTP/2 客户端。
//!
//! 三个入口，都是 `Value::Async`（真正的挂起点）：
//!
//! ```text
//! http.get(url [, headers])
//! http.post(url, body [, headers])
//! http.request(method, url, body [, headers])
//! ```
//!
//! `headers` 是 `map[string, string]`。返回一个响应 map：
//! `status` / `headers` / `body` / `protocol` / `url` / `ok`。
//!
//! ⚠️ **`body` 是 UTF-8 字符串**（非 UTF-8 字节按 `U+FFFD` 替换）。二进制响应本轮
//! 只保证"拿到长度与状态码"；原始字节访问列为后续任务（见 SPEC_STDLIB §8 的缺口）。
//!
//! 传输细节（白名单、硬超时、响应体上限、重定向策略）全部由 `ada_net::NetPolicy`
//! 强制，脚本侧无法放宽。

use crate::value::{AsyncKind, Value};
use std::collections::HashMap;

/// 构造 `http` 模块。只有宿主授予了 `NetPolicy` 时才会被注册。
pub fn create_http_module() -> Value {
    let mut module: HashMap<String, Value> = HashMap::new();

    // 三个入口各有独立的 `AsyncKind`：实参形状不同（`get` 的第二个实参是 headers，
    // 而 `post` 的第二个是 body），共用一个 kind 在 1–2 个实参时就分不清了。
    module.insert(
        "get".to_string(),
        Value::Async {
            kind: AsyncKind::HttpGet,
            receiver: None,
        },
    );
    module.insert(
        "post".to_string(),
        Value::Async {
            kind: AsyncKind::HttpPost,
            receiver: None,
        },
    );
    module.insert(
        "request".to_string(),
        Value::Async {
            kind: AsyncKind::HttpRequest,
            receiver: None,
        },
    );

    Value::map(module)
}
