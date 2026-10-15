//! 网络能力的脚本级测试（M9 / T9.2）。
//!
//! **全部 hermetic**：只用 `127.0.0.1` 上由测试自己起的临时服务，不解析外部域名、
//! 不访问公网。测试关心的是**脚本侧**的行为：默认拒绝、授权后可用、网络调用是真正的
//! 挂起点（能被其它任务交错），以及策略违规会报错而不是静默。

use ada_engine::{Engine, Error, NetPolicy, Sandbox, VmError};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// 授予 loopback 网络的沙箱（测试用；真实宿主应只放行自己需要的主机）。
fn loopback_sandbox() -> Sandbox {
    Sandbox::standard().with_net_policy(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .expect("host rules parse")
            .allow_private_addresses(true)
            .allow_bind(true)
            .with_connect_timeout(Duration::from_secs(5))
            .with_io_timeout(Duration::from_secs(5)),
    )
}

fn eval_err(engine: &mut Engine, source: &str) -> Error {
    match engine.eval_value(source) {
        Ok(value) => panic!("expected an error, got {value:?}"),
        Err(err) => err,
    }
}

/// 回显服务：读一次，写回同样的内容。
fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf) {
                let _ = stream.write_all(&buf[..n]);
                let _ = stream.flush();
            }
        }
    });
    port
}

/// 一次性 HTTP 服务：读请求，按 `delay` 延迟后回一段固定响应。
fn http_server(delay: Duration, body: String, connections: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut handlers = Vec::new();
        for _ in 0..connections {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            handlers.push({
                // 每个连接都要自己的一份 body：`move` 闭包会把它移走。
                let body = body.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    std::thread::sleep(delay);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                })
            });
        }
        for handler in handlers {
            let _ = handler.join();
        }
    });
    port
}

// ==================================================== 默认拒绝

#[test]
fn network_is_absent_until_the_host_grants_it() {
    let mut engine = Engine::with_sandbox(Sandbox::standard());
    // 未授权：模块名不算已知 → 与 `fs` 无根目录时同口径（unknown，而不是 denied）
    let err = eval_err(&mut engine, "import net from \"std:net\"\nnet");
    assert!(matches!(err, Error::Vm(VmError::TypeError(_))), "{err:?}");

    // 裸名字也拿不到：它是"被拒绝的能力"，而不是未定义变量
    let err = eval_err(&mut engine, "net");
    assert!(
        matches!(err, Error::Vm(VmError::CapabilityDenied { ref name, .. }) if name == "net"),
        "{err:?}"
    );

    // http 同理
    let err = eval_err(&mut engine, "http");
    assert!(
        matches!(err, Error::Vm(VmError::CapabilityDenied { ref name, .. }) if name == "http"),
        "{err:?}"
    );
}

#[test]
fn granting_network_makes_the_modules_importable() {
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    let value = engine
        .eval_value("import net from \"std:net\"\ntype_of(net)")
        .expect("net 应当可导入");
    assert_eq!(value.to_string(), "map");
    engine
        .eval_value("import http from \"std:http\"\ntype_of(http.get)")
        .expect("http 应当可导入");
}

#[test]
fn host_denial_wins_over_an_explicit_import() {
    // T8.15 的全局否决对网络同样生效：即使导入了也报 CapabilityDenied。
    let sandbox = loopback_sandbox().with_denied_module("net");
    let mut engine = Engine::with_sandbox(sandbox);
    let err = eval_err(&mut engine, "import net from \"std:net\"\nnet");
    assert!(
        matches!(err, Error::Vm(VmError::CapabilityDenied { ref name, .. }) if name == "net"),
        "{err:?}"
    );
}

// ==================================================== TCP

#[test]
fn tcp_round_trip_from_a_script() {
    let port = echo_server();
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    let source = format!(
        r#"
import net from "std:net"
let conn = net.tcp_connect("127.0.0.1", {port})
let written = conn.write("ping")
let reply = conn.read(64)
conn.close()
$"{{written}}:{{reply}}"
"#
    );
    let value = engine.eval_value(&source).expect("TCP 往返应当成功");
    assert_eq!(value.to_string(), "4:ping");
}

#[test]
fn tcp_read_returns_nil_at_eof() {
    let port = echo_server();
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    // 服务端在回写后关闭连接，第二次读应当得到 nil（而不是空字符串或报错）
    let source = format!(
        r#"
import net from "std:net"
let conn = net.tcp_connect("127.0.0.1", {port})
conn.write("x")
let first = conn.read(64)
sleep_ms(50)
let second = conn.read(64)
conn.close()
$"{{first}}:{{second}}"
"#
    );
    let value = engine.eval_value(&source).expect("读应当成功");
    assert_eq!(value.to_string(), "x:nil");
}

#[test]
fn connecting_to_a_host_outside_the_allowlist_fails() {
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    let err = eval_err(
        &mut engine,
        "import net from \"std:net\"\nnet.tcp_connect(\"example.com\", 80)",
    );
    match err {
        Error::Vm(VmError::NativeError(message)) => {
            assert!(message.contains("not allowed"), "message: {message}")
        }
        other => panic!("expected a policy error, got {other:?}"),
    }
}

// ==================================================== HTTP

#[test]
fn http_get_returns_a_response_map() {
    let port = http_server(Duration::ZERO, "hello".to_string(), 1);
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    let source = format!(
        r#"
import http from "std:http"
let res = http.get("http://127.0.0.1:{port}/x")
let kind = res.headers["content-type"]
$"{{res.status}}:{{res.body}}:{{kind}}:{{res.ok}}"
"#
    );
    let value = engine.eval_value(&source).expect("HTTP GET 应当成功");
    assert_eq!(value.to_string(), "200:hello:text/plain:true");
}

#[test]
fn http_response_body_respects_the_payload_cap() {
    let port = http_server(Duration::ZERO, "x".repeat(4096), 1);
    let sandbox = Sandbox::standard().with_net_policy(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_max_payload_bytes(512),
    );
    let mut engine = Engine::with_sandbox(sandbox);
    let err = eval_err(
        &mut engine,
        &format!("import http from \"std:http\"\nhttp.get(\"http://127.0.0.1:{port}/big\")"),
    );
    match err {
        Error::Vm(VmError::NativeError(message)) => {
            assert!(message.contains("exceeds"), "message: {message}")
        }
        other => panic!("expected a payload-cap error, got {other:?}"),
    }
}

// ==================================================== 挂起点语义

/// 网络调用必须是**真正的挂起点**：两个并发请求 + 一个 sleep 要能真正重叠。
///
/// 服务端对每个连接都延迟 300ms 才回。若网络调用是阻塞的，两次请求会串行 ≈600ms；
/// 只有它们是挂起点，总耗时才会 ≈300ms。这与 `async.rs` 里 `sleep_ms` 的重叠用例同构。
#[test]
fn concurrent_requests_overlap_instead_of_adding_up() {
    const DELAY: Duration = Duration::from_millis(300);
    let port = http_server(DELAY, "ok".to_string(), 2);
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    let source = format!(
        r#"
import http from "std:http"
fn fetch() {{
    http.get("http://127.0.0.1:{port}/a").body
}}
let a = spawn fetch()
let b = spawn fetch()
let both = [a, b].await_all()
both |> join(",")
"#
    );

    let started = Instant::now();
    let value = engine.eval_value(&source).expect("并发请求应当成功");
    let elapsed = started.elapsed();

    assert_eq!(value.to_string(), "ok,ok");
    assert!(
        elapsed >= DELAY,
        "必须真的等过服务端的延迟，实测 {elapsed:?}"
    );
    assert!(
        elapsed < DELAY * 2,
        "两个请求应当重叠（< {}ms），实测 {elapsed:?}",
        DELAY.as_millis() * 2
    );
}

/// 适配器回调内无法挂起（`rust_depth > 0`）：网络调用退化为同步执行，但结果正确。
#[test]
fn network_inside_an_adapter_callback_still_returns_the_right_result() {
    let port = http_server(Duration::ZERO, "hi".to_string(), 1);
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    // `map` 的回调是 Rust 栈帧 —— 这里验证"退化路径"不会算错、也不会报"无法挂起"。
    let source = format!(
        r#"
import http from "std:http"
[1].map(x => http.get("http://127.0.0.1:{port}/a").body) |> join("")
"#
    );
    let value = engine
        .eval_value(&source)
        .expect("回调内的网络调用应当退化为阻塞并成功");
    assert_eq!(value.to_string(), "hi");
}

#[test]
fn udp_send_and_receive_from_a_script() {
    let mut engine = Engine::with_sandbox(loopback_sandbox());
    // 绑定一个临时端口，从宿主侧发一个数据报过去，再在脚本里收。
    let source = r#"
import net from "std:net"
let sock = net.udp_bind(0)
sock.port
"#;
    let bound = engine.eval_value(source).expect("udp_bind 应当成功");
    let port: u16 = bound.to_string().parse().expect("端口可解析");

    let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    peer.send_to(b"dgram", ("127.0.0.1", port)).unwrap();

    let value = engine
        .eval_value(
            r#"
let got = sock.recv_from(64)
$"{{got.data}}:{{got.sender}}"
"#,
        )
        .expect("recv_from 应当成功");
    let text = value.to_string();
    assert!(text.starts_with("dgram:127.0.0.1:"), "got {text}");
}

// ==================================================== HTTPS 与私有 CA

/// 最小的 TLS 服务端：握手后读一次请求，回一段写死的 HTTP/1.1 响应。
///
/// 刻意不用 hyper：这里要验证的是**宿主注入私有 CA** 那条链路，服务端越简单越好。
fn tls_server_once(alpn: &[&[u8]]) -> u16 {
    // TLS 流是异步的：这里要用 tokio 的扩展 trait，而不是 std::io 的同名 trait。
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(TEST_CERT))
        .collect::<Result<_, _>>()
        .expect("证书链");
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(TEST_KEY))
        .expect("私钥可解析")
        .expect("存在私钥");
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("服务端证书配置");
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();

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
            let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
            let Ok(mut tls) = acceptor.accept(stream).await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = tls.read(&mut buf).await;
            let _ = tls
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 7\r\nconnection: close\r\n\r\nsecure!",
                )
                .await;
            let _ = tls.flush().await;
        });
    });
    port
}

const TEST_CERT: &[u8] = include_bytes!("../../ada_net/tests/fixtures/localhost-cert.pem");
const TEST_KEY: &[u8] = include_bytes!("../../ada_net/tests/fixtures/localhost-key.pem");
const TEST_CA: &[u8] = include_bytes!("../../ada_net/tests/fixtures/ca-cert.pem");

/// 只信任测试 CA 的客户端配置。
fn test_ca_tls_config() -> std::sync::Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut std::io::BufReader::new(TEST_CA)) {
        roots.add(cert.expect("CA 可解析")).expect("加入信任根");
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    std::sync::Arc::new(config)
}

/// 宿主注入私有 CA 后，脚本里的 HTTPS 请求应当能成功——这正是企业内网/自签场景
/// 的唯一出路（默认信任集只有 webpki 公共根证书）。
#[test]
fn https_works_when_the_host_injects_a_private_ca() {
    let port = tls_server_once(&[b"http/1.1"]);
    let sandbox = Sandbox::standard()
        .with_net_policy(
            NetPolicy::new()
                .allow_hosts(["localhost"])
                .unwrap()
                .allow_private_addresses(true),
        )
        .with_net_tls_config(test_ca_tls_config());
    let mut engine = Engine::with_sandbox(sandbox);
    let value = engine
        .eval_value(&format!(
            "import http from \"std:http\"\nhttp.get(\"https://localhost:{port}/x\").body"
        ))
        .expect("注入私有 CA 后 HTTPS 应当成功");
    assert_eq!(value.to_string(), "secure!");
}

/// 同一台服务端，**不**注入私有 CA 时必须失败：证明上一条的成功来自注入的信任根，
/// 而不是"我们其实没在校验证书"。
#[test]
fn https_fails_without_the_private_ca() {
    let port = tls_server_once(&[b"http/1.1"]);
    // 与上一条同样的网络授权，但**不**注入私有 CA——差别只在这里。
    let sandbox = Sandbox::standard().with_net_policy(
        NetPolicy::new()
            .allow_hosts(["localhost"])
            .unwrap()
            .allow_private_addresses(true),
    );
    let mut engine = Engine::with_sandbox(sandbox);
    let err = eval_err(
        &mut engine,
        &format!("import http from \"std:http\"\nhttp.get(\"https://localhost:{port}/x\")"),
    );
    match err {
        Error::Vm(VmError::NativeError(message)) => {
            assert!(message.contains("tls"), "message: {message}")
        }
        other => panic!("expected a TLS error, got {other:?}"),
    }
}
