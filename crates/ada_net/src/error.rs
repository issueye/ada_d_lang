//! 网络能力的错误模型（M9）。
//!
//! 每条错误都对应沙箱策略里的一个具体拒绝理由或一次有界失败。脚本侧统一表现为
//! `VmError::NativeError`（原生回调的错误通道），宿主侧可直接读这些变体。
//!
//! 注意「被拒绝」与「未知模块」在 VM 层是可区分的（`CapabilityDenied` vs `TypeError`），
//! 这里的 `NetError` 只描述**已经获得网络权限之后**的失败。

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// 主机不在白名单内。
    #[error("host '{host}' is not allowed by the network policy")]
    HostNotAllowed { host: String },

    /// 端口在该主机上未被放行。
    #[error("port {port} is not allowed for host '{host}'")]
    PortNotAllowed { host: String, port: u16 },

    /// 解析结果落在私有/保留地址段，而策略不允许（SSRF 缓解）。
    #[error("address '{addr}' is in a private or reserved range, which the network policy forbids")]
    PrivateAddress { addr: String },

    /// 超出了本次操作的硬超时。
    #[error("network operation timed out after {0:?}")]
    Timeout(Duration),

    /// 载荷超过上限（响应体、数据报、写入缓冲）。
    #[error("payload of {size} bytes exceeds the {limit}-byte limit")]
    TooLarge { size: usize, limit: usize },

    /// 重定向次数超过策略上限。
    #[error("too many redirects (limit is {0})")]
    TooManyRedirects(u32),

    /// URL 无法解析（相对于策略允许的形状）。
    #[error("invalid url: {0}")]
    Url(String),

    /// TLS 握手或证书校验失败。
    #[error("tls error: {0}")]
    Tls(String),

    /// 协议层错误（畸形 HTTP 响应等）。
    #[error("http error: {0}")]
    Http(String),

    /// 底层 I/O 错误。
    #[error("io error: {0}")]
    Io(String),

    /// 句柄不存在（已关闭或从未创建）。
    #[error("network handle {0} is not open")]
    NoSuchHandle(u64),

    /// 句柄类型与操作不匹配。
    #[error("network handle {handle} is not a {expected} handle")]
    WrongHandleKind { handle: u64, expected: &'static str },

    /// 宿主没有授予网络能力（正常路径下模块根本不存在，这里是兜底）。
    #[error("network access is not granted by the host")]
    NotGranted,

    /// 策略不允许绑定本地端口（UDP 需要端口才能收发，故默认关闭）。
    #[error("binding a local port is not allowed by the network policy")]
    BindNotAllowed,
}

impl From<std::io::Error> for NetError {
    fn from(err: std::io::Error) -> Self {
        NetError::Io(err.to_string())
    }
}
