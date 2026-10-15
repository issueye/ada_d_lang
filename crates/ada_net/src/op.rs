//! 网络操作与结果的类型定义（M9）。
//!
//! 这是 VM 与 `ada_net` 之间的边界类型：VM 提交 `NetOp`，运行时回投 `NetReply`。
//! 句柄用 `u64` 表示（运行时内部映射到真实的 socket），因此这里不含任何 tokio 类型，
//! 便于 VM 侧持有与比较。

/// 一次网络操作的标识（VM 用它把完成事件匹配回挂起的任务）。
pub type OpId = u64;

/// 网络句柄（TCP 连接 / UDP socket）的标识。
pub type Handle = u64;

/// HTTP 请求方法。只支持这几种，避免把任意 token 透传给对端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl HttpMethod {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "PATCH" => Some(HttpMethod::Patch),
            "DELETE" => Some(HttpMethod::Delete),
            "HEAD" => Some(HttpMethod::Head),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
        }
    }
}

/// 一次 HTTP 请求（相对 URL 由调用方补全为绝对 URL 后传入）。
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// 一次 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// 响应体字节（受 `NetPolicy::max_payload_bytes` 限制）。
    pub body: Vec<u8>,
    /// 跟随重定向后的最终 URL（未跟随时即请求 URL）。
    pub final_url: String,
    /// 协商出的协议版本：`"http/1.1"` 或 `"h2"`。
    pub protocol: &'static str,
}

/// 提交给运行时的操作。
#[derive(Debug, Clone)]
pub enum NetOp {
    TcpConnect { host: String, port: u16 },
    TcpRead { handle: Handle, max_bytes: usize },
    TcpWrite { handle: Handle, data: Vec<u8> },
    TcpClose { handle: Handle },
    UdpBind { host: String, port: u16 },
    UdpSendTo { handle: Handle, host: String, port: u16, data: Vec<u8> },
    UdpRecvFrom { handle: Handle, max_bytes: usize },
    UdpClose { handle: Handle },
    Http { request: HttpRequest },
}

impl NetOp {
    /// 操作名（诊断与错误消息用）。
    pub fn name(&self) -> &'static str {
        match self {
            NetOp::TcpConnect { .. } => "tcp_connect",
            NetOp::TcpRead { .. } => "tcp_read",
            NetOp::TcpWrite { .. } => "tcp_write",
            NetOp::TcpClose { .. } => "tcp_close",
            NetOp::UdpBind { .. } => "udp_bind",
            NetOp::UdpSendTo { .. } => "udp_send_to",
            NetOp::UdpRecvFrom { .. } => "udp_recv_from",
            NetOp::UdpClose { .. } => "udp_close",
            NetOp::Http { .. } => "http_request",
        }
    }
}

/// 操作成功的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetReply {
    Connected { handle: Handle, peer: String },
    /// 读到的字节；空表示对端已关闭（EOF）。
    Bytes(Vec<u8>),
    /// 实际写出的字节数。
    Written(usize),
    Closed,
    Bound { handle: Handle, local: String },
    /// UDP 收到的数据报及其来源。
    Datagram { data: Vec<u8>, from: String },
    HttpResponse(HttpResponse),
}
