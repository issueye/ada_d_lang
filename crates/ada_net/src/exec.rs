//! 操作执行器（M9）：在驱动线程的 Tokio 运行时上落地 TCP / UDP。
//!
//! **连接表与 `Send` 约束**：`handle.spawn` 要求 future 为 `Send`，因此这里从不在
//! `await` 期间持有锁——所有操作都走「取出连接 → 异步操作 → 放回」的模式。表本身只在
//! 运行时线程上访问，锁仅用于跨线程发布句柄。

use crate::error::NetError;
use crate::op::{Handle, NetOp, NetReply};
use crate::policy::NetPolicy;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket, lookup_host};

/// 连接表里的一类宿主资源。
enum Conn {
    Tcp(TcpStream),
    Udp(UdpSocket),
}

#[derive(Default)]
struct Table {
    conns: HashMap<Handle, Conn>,
    next_handle: Handle,
}

/// 由 `NetRuntime` 与各操作共享的连接表 + TLS 配置。
#[derive(Clone)]
pub struct SharedState {
    inner: Arc<Mutex<Table>>,
    /// 客户端 TLS 配置（含信任根）。宿主可以注入自带 CA 的配置（企业私有 CA 场景），
    /// 测试也靠它信任自签证书。
    tls: Arc<rustls::ClientConfig>,
}

impl SharedState {
    pub fn new(tls: Arc<rustls::ClientConfig>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Table {
                conns: HashMap::new(),
                next_handle: 1,
            })),
            tls,
        }
    }

    pub fn tls(&self) -> &Arc<rustls::ClientConfig> {
        &self.tls
    }

    fn insert(&self, conn: Conn) -> Handle {
        let mut table = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let handle = table.next_handle;
        table.next_handle = table.next_handle.wrapping_add(1).max(1);
        table.conns.insert(handle, conn);
        handle
    }

    /// 取出连接（调用方负责放回或丢弃）。
    fn take(&self, handle: Handle) -> Result<Conn, NetError> {
        let mut table = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        table
            .conns
            .remove(&handle)
            .ok_or(NetError::NoSuchHandle(handle))
    }

    fn put_back(&self, handle: Handle, conn: Conn) {
        let mut table = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        table.conns.insert(handle, conn);
    }
}

/// 执行一个操作。所有失败都以 `Err` 返回，绝不 panic。
pub async fn execute(
    op: NetOp,
    policy: Arc<NetPolicy>,
    state: SharedState,
) -> Result<NetReply, NetError> {
    match op {
        NetOp::TcpConnect { host, port } => tcp_connect(&host, port, &policy, &state).await,
        NetOp::TcpRead { handle, max_bytes } => tcp_read(handle, max_bytes, &policy, &state).await,
        NetOp::TcpWrite { handle, data } => tcp_write(handle, data, &policy, &state).await,
        NetOp::TcpClose { handle } => tcp_close(handle, &state),
        NetOp::UdpBind { host, port } => udp_bind(&host, port, &policy, &state).await,
        NetOp::UdpSendTo {
            handle,
            host,
            port,
            data,
        } => udp_send_to(handle, &host, port, data, &policy, &state).await,
        NetOp::UdpRecvFrom { handle, max_bytes } => {
            udp_recv_from(handle, max_bytes, &policy, &state).await
        }
        NetOp::UdpClose { handle } => udp_close(handle, &state),
        NetOp::Http { request } => crate::http::execute(request, &policy, state.tls()).await,
    }
}

/// 解析主机名并**只保留策略放行的地址**。
///
/// 关键：连接用的是这里校验过的 `SocketAddr`，不再让 OS 重新解析（避免 DNS rebinding
/// 在"校验"与"连接"之间把结果换掉）。
async fn resolve_allowed(
    host: &str,
    port: u16,
    policy: &NetPolicy,
) -> Result<Vec<SocketAddr>, NetError> {
    let resolved: Vec<SocketAddr> = lookup_host((host, port))
        .await
        .map_err(|e| NetError::Io(format!("cannot resolve '{host}': {e}")))?
        .collect();
    if resolved.is_empty() {
        return Err(NetError::Io(format!("'{host}' resolved to no address")));
    }
    let mut allowed = Vec::new();
    let mut first_rejection: Option<NetError> = None;
    for addr in resolved {
        if let Err(err) = policy.check_address(addr.ip()) {
            first_rejection.get_or_insert(err);
        } else {
            allowed.push(addr);
        }
    }
    if allowed.is_empty() {
        return Err(first_rejection.unwrap_or(NetError::NotGranted));
    }
    Ok(allowed)
}

async fn tcp_connect(
    host: &str,
    port: u16,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    policy.check_host(host, port)?;
    let addrs = resolve_allowed(host, port, policy).await?;

    let connect_all = async {
        let mut last: Option<std::io::Error> = None;
        for addr in &addrs {
            match TcpStream::connect(addr).await {
                Ok(stream) => return Ok(stream),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no address to connect to")
        }))
    };

    let stream = tokio::time::timeout(policy.connect_timeout(), connect_all)
        .await
        .map_err(|_| NetError::Timeout(policy.connect_timeout()))??;
    let _ = stream.set_nodelay(true);
    let peer = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("{host}:{port}"));
    let handle = state.insert(Conn::Tcp(stream));
    Ok(NetReply::Connected { handle, peer })
}

async fn tcp_read(
    handle: Handle,
    max_bytes: usize,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    let mut conn = state.take(handle)?;
    let result = match &mut conn {
        Conn::Tcp(stream) => {
            let cap = max_bytes.clamp(1, policy.max_payload_bytes());
            let mut buf = vec![0u8; cap];
            match tokio::time::timeout(policy.io_timeout(), stream.read(&mut buf)).await {
                Err(_) => Err(NetError::Timeout(policy.io_timeout())),
                Ok(Err(err)) => Err(NetError::from(err)),
                Ok(Ok(read)) => {
                    buf.truncate(read);
                    Ok(NetReply::Bytes(buf))
                }
            }
        }
        _ => Err(NetError::WrongHandleKind {
            handle,
            expected: "tcp",
        }),
    };
    state.put_back(handle, conn);
    result
}

async fn tcp_write(
    handle: Handle,
    data: Vec<u8>,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    policy.check_payload(data.len())?;
    let mut conn = state.take(handle)?;
    let result = match &mut conn {
        Conn::Tcp(stream) => {
            match tokio::time::timeout(policy.io_timeout(), stream.write_all(&data)).await {
                Err(_) => Err(NetError::Timeout(policy.io_timeout())),
                Ok(Err(err)) => Err(NetError::from(err)),
                Ok(Ok(())) => Ok(NetReply::Written(data.len())),
            }
        }
        _ => Err(NetError::WrongHandleKind {
            handle,
            expected: "tcp",
        }),
    };
    state.put_back(handle, conn);
    result
}

fn tcp_close(handle: Handle, state: &SharedState) -> Result<NetReply, NetError> {
    let conn = state.take(handle)?;
    match conn {
        // TcpStream 的析构会关闭 socket；显式 shutdown 让对端立刻看到 FIN。
        Conn::Tcp(stream) => {
            drop(stream);
            Ok(NetReply::Closed)
        }
        other => {
            state.put_back(handle, other);
            Err(NetError::WrongHandleKind {
                handle,
                expected: "tcp",
            })
        }
    }
}

async fn udp_bind(
    _host: &str,
    port: u16,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    if !policy.binding_allowed() {
        return Err(NetError::BindNotAllowed);
    }
    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .map_err(NetError::from)?;
    let local = socket
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("0.0.0.0:{port}"));
    let handle = state.insert(Conn::Udp(socket));
    Ok(NetReply::Bound { handle, local })
}

async fn udp_send_to(
    handle: Handle,
    host: &str,
    port: u16,
    data: Vec<u8>,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    policy.check_host(host, port)?;
    policy.check_payload(data.len())?;
    let addrs = resolve_allowed(host, port, policy).await?;

    let mut conn = state.take(handle)?;
    let result = match &mut conn {
        Conn::Udp(socket) => {
            let send = async {
                let mut last: Option<std::io::Error> = None;
                for addr in &addrs {
                    match socket.send_to(&data, addr).await {
                        Ok(written) => return Ok(written),
                        Err(err) => last = Some(err),
                    }
                }
                Err(last.unwrap_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no address to send to")
                }))
            };
            match tokio::time::timeout(policy.io_timeout(), send).await {
                Err(_) => Err(NetError::Timeout(policy.io_timeout())),
                Ok(Err(err)) => Err(NetError::from(err)),
                Ok(Ok(written)) => Ok(NetReply::Written(written)),
            }
        }
        _ => Err(NetError::WrongHandleKind {
            handle,
            expected: "udp",
        }),
    };
    state.put_back(handle, conn);
    result
}

async fn udp_recv_from(
    handle: Handle,
    max_bytes: usize,
    policy: &NetPolicy,
    state: &SharedState,
) -> Result<NetReply, NetError> {
    let mut conn = state.take(handle)?;
    let result = match &mut conn {
        Conn::Udp(socket) => {
            let cap = max_bytes.clamp(1, policy.max_payload_bytes());
            let mut buf = vec![0u8; cap];
            match tokio::time::timeout(policy.io_timeout(), socket.recv_from(&mut buf)).await {
                Err(_) => Err(NetError::Timeout(policy.io_timeout())),
                Ok(Err(err)) => Err(NetError::from(err)),
                Ok(Ok((read, from))) => {
                    buf.truncate(read);
                    Ok(NetReply::Datagram {
                        data: buf,
                        from: from.to_string(),
                    })
                }
            }
        }
        _ => Err(NetError::WrongHandleKind {
            handle,
            expected: "udp",
        }),
    };
    state.put_back(handle, conn);
    result
}

fn udp_close(handle: Handle, state: &SharedState) -> Result<NetReply, NetError> {
    let conn = state.take(handle)?;
    match conn {
        Conn::Udp(socket) => {
            drop(socket);
            Ok(NetReply::Closed)
        }
        other => {
            state.put_back(handle, other);
            Err(NetError::WrongHandleKind {
                handle,
                expected: "udp",
            })
        }
    }
}
