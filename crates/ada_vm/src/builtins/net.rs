//! `net` 模块（M9）：TCP 客户端与 UDP。
//!
//! **模块的成员不是普通的原生函数，而是 `Value::Async`**——因为网络操作是真正的挂起点
//! （复用 `sleep_ms` 的那套机制），而原生回调是无法挂起的 `Fn(&[Value]) -> Result<Value, String>`。
//!
//! 句柄模型：`tcp_connect` 挂起结束后写回一个 **map**，其成员是绑定了句柄的 `Value::Async`：
//!
//! ```text
//! conn = net.tcp_connect("example.com", 80)   // -> map
//! conn.write("GET / HTTP/1.1\r\n\r\n")        // -> 挂起点，写回字节数
//! conn.read(4096)                             // -> 挂起点，写回字符串（EOF 时 nil）
//! conn.close()
//! ```
//!
//! **句柄不可伪造**：脚本没有任何字面量能构造 `Value::Async`，而 `TcpRead` 这些
//! `AsyncKind` 也从不注册为全局名。因此拿到句柄的唯一途径是 `tcp_connect` 的返回值，
//! "传一个随手编的 id 去读别人的连接"这条路不存在。

use crate::value::{AsyncKind, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// 构造连接 map：把每个方法作为绑定了句柄的 `Value::Async` 放进去。
fn handle_map(kind: HandleKind, handle: u64, peer: String, methods: &[(&str, AsyncKind)]) -> Value {
    let receiver = Arc::new(Value::Int(handle as i64));
    let mut map: HashMap<String, Value> = HashMap::new();
    map.insert("handle".to_string(), Value::Int(handle as i64));
    map.insert(
        "kind".to_string(),
        Value::string(match kind {
            HandleKind::Tcp => "tcp",
            HandleKind::Udp => "udp",
        }),
    );
    map.insert("peer".to_string(), Value::string(peer));
    for (name, method_kind) in methods {
        map.insert(
            (*name).to_string(),
            Value::Async {
                kind: *method_kind,
                receiver: Some(Arc::clone(&receiver)),
            },
        );
    }
    Value::map(map)
}

#[derive(Clone, Copy)]
pub enum HandleKind {
    Tcp,
    Udp,
}

/// TCP 连接句柄。
pub fn tcp_connection(handle: u64, peer: String) -> Value {
    handle_map(
        HandleKind::Tcp,
        handle,
        peer,
        &[
            ("read", AsyncKind::TcpRead),
            ("write", AsyncKind::TcpWrite),
            ("close", AsyncKind::TcpClose),
        ],
    )
}

/// UDP socket 句柄。
pub fn udp_socket(handle: u64, local: String) -> Value {
    let value = handle_map(
        HandleKind::Udp,
        handle,
        local.clone(),
        &[
            ("send_to", AsyncKind::UdpSendTo),
            ("recv_from", AsyncKind::UdpRecvFrom),
            ("close", AsyncKind::UdpClose),
        ],
    );
    // 绑定到临时端口（`:0`）时脚本必须能知道自己分到了哪个端口，否则无法被联系。
    // 单独给出 `port` 比让脚本去切 `peer` 字符串可靠。
    let port = local
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<i64>().ok())
        .unwrap_or(0);
    if let Value::Map(entries) = &value {
        entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert("port".to_string(), Value::Int(port));
    }
    value
}

/// 构造 `net` 模块。只有宿主授予了 `NetPolicy` 时才会被注册（见 `builtins/mod.rs`）。
pub fn create_net_module() -> Value {
    let mut module: HashMap<String, Value> = HashMap::new();

    // 模块级函数：接收者为空，参数就是实参。
    module.insert(
        "tcp_connect".to_string(),
        Value::Async {
            kind: AsyncKind::TcpConnect,
            receiver: None,
        },
    );
    module.insert(
        "udp_bind".to_string(),
        Value::Async {
            kind: AsyncKind::UdpBind,
            receiver: None,
        },
    );
    module.insert("protocols".to_string(), Value::array(vec![
        Value::string("tcp"),
        Value::string("udp"),
        Value::string("http"),
    ]));

    Value::map(module)
}
