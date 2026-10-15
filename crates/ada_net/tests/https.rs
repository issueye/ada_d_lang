//! HTTPS 与 HTTP/2 的端到端测试（M9 / T9.3）。
//!
//! **hermetic**：服务端是测试自己起的 rustls + hyper，证书是仓库里的自签
//! `localhost` 证书（`fixtures/`）。不访问公网、不依赖外部 CA。
//!
//! 为什么要单独测这一组：P0 只证明了 `ring` 能链接并运行、明文 HTTP/1.1 能跑通；
//! **TLS 握手 + ALPN 协商 + HTTP/2** 这条路径此前一次都没真正执行过。它是
//! "HTTPS 可用"这个结论的唯一证据。
//!
//! 证书是"测试 CA + localhost 叶子"的两级结构，而不是单张自签证书：rustls 会正确拒绝
//! 把 CA 证书当叶子用（`CaUsedAsEndEntity`），而自签叶子当作信任根同样不被接受。
//! CA 私钥没有入库（只在生成时用过一次），重新生成：
//!
//! ```text
//! openssl req -x509 -newkey rsa:2048 -nodes -keyout ca-key.pem -out ca-cert.pem \
//!   -days 36500 -subj "//CN=Ada-D Test CA" \
//!   -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
//! openssl req -newkey rsa:2048 -nodes -keyout localhost-key.pem -out localhost.csr -subj "//CN=localhost"
//! openssl x509 -req -in localhost.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial \
//!   -out localhost-cert.pem -days 36500 -extfile leaf.ext
//! # leaf.ext: basicConstraints=critical,CA:FALSE / extendedKeyUsage=serverAuth /
//! #            subjectAltName=DNS:localhost,IP:127.0.0.1
//! ```

use ada_net::{HttpMethod, HttpRequest, NetError, NetOp, NetPolicy, NetReply, NetRuntime};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::Arc;
use std::time::Duration;

const CA_PEM: &[u8] = include_bytes!("fixtures/ca-cert.pem");
const CERT_PEM: &[u8] = include_bytes!("fixtures/localhost-cert.pem");
const KEY_PEM: &[u8] = include_bytes!("fixtures/localhost-key.pem");

fn test_policy() -> Arc<NetPolicy> {
    Arc::new(
        NetPolicy::new()
            .allow_hosts(["localhost"])
            .unwrap()
            .allow_private_addresses(true) // localhost 解析到 127.0.0.1/::1
            .with_connect_timeout(Duration::from_secs(5))
            .with_io_timeout(Duration::from_secs(5)),
    )
}

/// 只信任测试自签证书的客户端配置。
///
/// 默认配置只信任 webpki 的公共根证书集，因此**必须**能额外注入信任根——这同时是
/// 宿主使用私有 CA 的正当需求（不是为了测试才加的开关）。
fn trusting_test_cert() -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut std::io::BufReader::new(CA_PEM)) {
        roots.add(cert.expect("测试 CA 可解析")).expect("加入信任根");
    }
    build_client_config(roots)
}

fn build_client_config(roots: rustls::RootCertStore) -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

fn server_tls_config(alpn: &[&[u8]]) -> Arc<rustls::ServerConfig> {
    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(CERT_PEM))
        .collect::<Result<_, _>>()
        .expect("证书链");
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(KEY_PEM))
        .expect("私钥可解析")
        .expect("存在私钥");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("服务端证书配置");
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Arc::new(config)
}

/// 起一个 TLS 服务端并在**同一个**运行时里跑完一次连接。
///
/// 注意：处理器必须作为 future 交给这里的 `block_on`，不能在处理器内部再建运行时
/// （嵌套 `block_on` 会 panic）。
fn tls_server<F, Fut>(alpn: &[&[u8]], serve_one: F) -> u16
where
    F: FnOnce(tokio_rustls::server::TlsStream<tokio::net::TcpStream>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let config = server_tls_config(alpn);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("server runtime");
        runtime.block_on(async move {
            listener.set_nonblocking(true).ok();
            let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = tokio_rustls::TlsAcceptor::from(config);
            let Ok(tls) = acceptor.accept(stream).await else {
                return;
            };
            serve_one(tls).await;
        });
    });
    port
}

async fn hello_h1(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .header(hyper::header::CONNECTION, "close")
        .body(Full::new(Bytes::from_static(b"secure")))
        .unwrap())
}

async fn hello_h2(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // h2 里 `connection` 是非法头，不能带。
    Ok(Response::builder()
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .body(Full::new(Bytes::from_static(b"secure")))
        .unwrap())
}

fn https_get(port: u16) -> NetOp {
    NetOp::Http {
        request: HttpRequest {
            method: HttpMethod::Get,
            url: format!("https://localhost:{port}/x"),
            headers: Vec::new(),
            body: None,
        },
    }
}

/// ALPN 协商出 h2 时走 HTTP/2 —— 同时验证 TLS 与 HTTP/2 两条路径。
#[test]
fn https_request_negotiates_http2_via_alpn() {
    let port = tls_server(&[b"h2", b"http/1.1"], |tls| async move {
        let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(tls), service_fn(hello_h2))
            .await;
    });

    let runtime = NetRuntime::with_tls_config(test_policy(), trusting_test_cert()).unwrap();
    let reply = runtime
        .run_blocking(https_get(port))
        .expect("HTTPS 请求应当成功");
    match reply {
        NetReply::HttpResponse(response) => {
            assert_eq!(response.status, 200);
            assert_eq!(response.body, b"secure");
            assert_eq!(response.protocol, "h2", "ALPN 应当协商出 HTTP/2");
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

/// 服务端只支持 HTTP/1.1 时，同一份客户端配置应当回落到 h1。
#[test]
fn https_request_falls_back_to_http1_when_h2_is_unavailable() {
    let port = tls_server(&[b"http/1.1"], |tls| async move {
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(tls), service_fn(hello_h1))
            .await;
    });

    let runtime = NetRuntime::with_tls_config(test_policy(), trusting_test_cert()).unwrap();
    let reply = runtime
        .run_blocking(https_get(port))
        .expect("HTTPS 请求应当成功");
    match reply {
        NetReply::HttpResponse(response) => {
            assert_eq!(response.status, 200);
            assert_eq!(response.body, b"secure");
            assert_eq!(response.protocol, "http/1.1");
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

/// 自签证书**不被默认信任集接受**：证明我们真的在校验证书链，
/// 而不是"连上就算成功"。
#[test]
fn a_self_signed_server_is_rejected_by_the_default_trust_store() {
    let port = tls_server(&[b"h2", b"http/1.1"], |tls| async move {
        let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(tls), service_fn(hello_h2))
            .await;
    });

    // 默认配置（webpki 公共根证书），不注入测试证书
    let runtime = NetRuntime::new(test_policy()).unwrap();
    let err = runtime
        .run_blocking(https_get(port))
        .expect_err("自签证书必须被拒绝");
    assert!(matches!(err, NetError::Tls(_)), "expected a TLS error, got {err:?}");
}
