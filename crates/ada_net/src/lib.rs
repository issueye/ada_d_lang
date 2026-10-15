//! Ada-D 的网络传输层（M9）。
//!
//! 这个 crate 提供**纯 Rust 的宿主侧网络能力**，不认识 `ada_vm::Value`——脚本类型到
//! `Value` 的包装在 `ada_vm::builtins::{net,http}` 里完成。这样 hyper/tokio 的类型
//! 不会渗进 VM 的核心文件，依赖方向也保持单向（`ada_net` 低于 `ada_vm`）。
//!
//! 设计约束（见 docs/SPEC_RUNTIME §10 与 docs/DEVELOPMENT_PLAN §10 的决策 D13/D14）：
//! - **默认拒绝**：没有宿主授予的 [`NetPolicy`] 时，脚本侧的模块根本不存在。
//! - **硬超时**：连接与单次读写都有上限，把"挂几分钟"变成"有界失败"。
//! - **有界载荷**：响应体、数据报、写入缓冲都有字节上限。
//! - **地址校验**：连的是校验过的 `SocketAddr`，私有/保留段默认拒绝（防 DNS rebinding）。
//! - Tokio 只承载 socket/HTTP，跑在独立驱动线程上（[`NetRuntime`]）；字节码执行仍是
//!   单线程，任务调度顺序不变（决策 D14）。

mod error;
mod exec;
mod http;
mod op;
mod policy;
mod runtime;

pub use error::NetError;
pub use op::{Handle, HttpMethod, HttpRequest, HttpResponse, NetOp, NetReply, OpId};
pub use policy::{HostRule, NetPolicy};
pub use runtime::NetRuntime;

/// 重新导出 `rustls`：宿主构造自己的 TLS 配置（私有 CA、额外信任根）时需要它的类型，
/// 而 `ada_vm` / `ada_engine` 不应各自再声明一份 rustls 依赖。
pub use rustls;

use std::sync::Arc;

/// 构建使用 `ring` 后端的 rustls 客户端配置。
///
/// `aws-lc-rs` 在本项目不可用（需要 cmake 与 NASM，开发机没有），因此显式指定 `ring`。
/// 显式传入 provider 而不是安装进程级默认值：默认值是全局状态，会让并发测试互相影响。
pub fn tls_client_config() -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring 提供了安全的默认协议版本")
        .with_root_certificates(roots)
        .with_no_client_auth();
    // ALPN 顺序即偏好：优先 HTTP/2，其次 HTTP/1.1。
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper::{Request, Response};
    use hyper_util::rt::TokioIo;
    use std::time::Duration;

    /// P0 闸门（一）：证明 `ring` 的 native 代码能在本机 MSVC 上**链接并执行**。
    ///
    /// 只 `rustls::ClientConfig::builder` 不足以证明——那是纯 Rust 的类型装配，
    /// 链接器不会因此拉进 `ring` 的目标文件。这里调用 provider 的安全随机源，
    /// 它必然落到 `ring` 的 `SystemRandom` 上。
    #[test]
    fn ring_native_code_links_and_runs() {
        let provider = rustls::crypto::ring::default_provider();
        let mut buf = [0u8; 32];
        provider
            .secure_random
            .fill(&mut buf)
            .expect("ring 的随机源可用");
        assert_ne!(buf, [0u8; 32], "随机源应当写出非零字节");

        // 客户端配置也必须能装配（使用同一 provider 与 webpki 根证书）
        let config = tls_client_config();
        assert_eq!(config.alpn_protocols.len(), 2);
    }

    /// P0 闸门（二）：证明 tokio + hyper 的 HTTP/1.1 在 loopback 上真的能跑通。
    ///
    /// 用单连接握手而不是 `hyper_util` 的连接池客户端：池化会让服务端连接保持打开，
    /// 等待 `serve_connection` 返回就会死锁。整个用例再套一层超时，保证永不挂住测试套件。
    #[tokio::test]
    async fn hyper_and_tokio_round_trip_over_loopback() {
        let work = async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service_fn(hello))
                    .await;
            });

            let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
                .await
                .unwrap();
            let conn_task = tokio::spawn(conn);

            let request = Request::builder()
                .uri(format!("http://{addr}/ping"))
                .header(hyper::header::HOST, addr.to_string())
                .body(Full::new(Bytes::new()))
                .unwrap();
            let response = sender.send_request(request).await.unwrap();
            assert_eq!(response.status(), 200);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(&body[..], b"pong");

            drop(sender);
            let _ = conn_task.await;
            let _ = server.await;
        };

        tokio::time::timeout(Duration::from_secs(20), work)
            .await
            .expect("loopback 往返应在超时前完成");
    }

    /// 服务端回 `Connection: close`，让单连接在响应后自然收尾。
    async fn hello(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
        Ok(Response::builder()
            .header(hyper::header::CONNECTION, "close")
            .body(Full::new(Bytes::from_static(b"pong")))
            .unwrap())
    }
}
