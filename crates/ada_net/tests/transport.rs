//! `ada_net` 的传输层集成测试（M9）。
//!
//! **全部 hermetic**：只用 `127.0.0.1` 上由测试自己起的临时服务，不解析任何外部域名、
//! 不访问公网。HTTP 服务端刻意用 `std::net::TcpListener` + 手写响应，而不是 hyper 服务端——
//! 这样可以精确构造"响应体超上限""重定向""畸形响应"这些边界，也不引入服务端框架。

use ada_net::{HttpMethod, HttpRequest, NetError, NetOp, NetPolicy, NetReply, NetRuntime};
use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

/// 测试用策略：放行 loopback、允许私有地址、短超时。
fn loopback_policy() -> Arc<NetPolicy> {
    Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_connect_timeout(Duration::from_secs(5))
            .with_io_timeout(Duration::from_secs(5)),
    )
}

fn runtime(policy: Arc<NetPolicy>) -> NetRuntime {
    NetRuntime::new(policy).expect("runtime 启动")
}

/// 起一个"读一次请求 → 写死响应 → 关闭"的临时 TCP 服务，返回其端口。
///
/// 用固定响应串而不是框架：请求内容对本测试无关紧要，重要的是响应形状可控。
fn serve_once(response: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // 读到请求头结束即可，不必读完 body。
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });
    port
}

fn http_response(status_line: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("{status_line}\r\n").into_bytes();
    for (name, value) in headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"connection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

fn get(url: &str) -> NetOp {
    NetOp::Http {
        request: HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
        },
    }
}

// ==================================================== TCP

#[test]
fn tcp_round_trip_over_loopback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).unwrap_or(0);
            let _ = stream.write_all(&buf[..n]); // echo
        }
    });

    let runtime = runtime(loopback_policy());
    let handle = match runtime.run_blocking(NetOp::TcpConnect {
        host: "127.0.0.1".to_string(),
        port,
    }) {
        Ok(NetReply::Connected { handle, .. }) => handle,
        other => panic!("expected a connection, got {other:?}"),
    };

    let written = runtime
        .run_blocking(NetOp::TcpWrite {
            handle,
            data: b"ping".to_vec(),
        })
        .expect("write");
    assert_eq!(written, NetReply::Written(4));

    let reply = runtime
        .run_blocking(NetOp::TcpRead {
            handle,
            max_bytes: 64,
        })
        .expect("read");
    assert_eq!(reply, NetReply::Bytes(b"ping".to_vec()));

    assert_eq!(
        runtime.run_blocking(NetOp::TcpClose { handle }).unwrap(),
        NetReply::Closed
    );
    // 关闭之后句柄失效——报错而不是静默成功
    assert!(matches!(
        runtime.run_blocking(NetOp::TcpRead {
            handle,
            max_bytes: 8
        }),
        Err(NetError::NoSuchHandle(_))
    ));
}

#[test]
fn tcp_connect_is_refused_for_a_host_outside_the_allowlist() {
    let runtime = runtime(Arc::new(NetPolicy::new())); // 空策略 = 全拒绝
    let err = runtime
        .run_blocking(NetOp::TcpConnect {
            host: "example.com".to_string(),
            port: 80,
        })
        .expect_err("白名单外的主机必须被拒绝");
    assert!(matches!(err, NetError::HostNotAllowed { .. }));
}

#[test]
fn tcp_read_times_out_instead_of_hanging() {
    // 服务端接受连接但从不回数据：读必须在超时后失败，而不是永远等待。
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let held = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(3));
        drop(stream);
    });

    let policy = Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_io_timeout(Duration::from_millis(200)),
    );
    let runtime = runtime(policy);
    let handle = match runtime
        .run_blocking(NetOp::TcpConnect {
            host: "127.0.0.1".to_string(),
            port,
        })
        .expect("connect")
    {
        NetReply::Connected { handle, .. } => handle,
        other => panic!("expected a connection, got {other:?}"),
    };

    let started = std::time::Instant::now();
    let err = runtime
        .run_blocking(NetOp::TcpRead {
            handle,
            max_bytes: 16,
        })
        .expect_err("读应当在超时后失败");
    assert!(matches!(err, NetError::Timeout(_)), "got {err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "超时应当很快生效，实际 {:?}",
        started.elapsed()
    );
    let _ = held.join();
}

// ==================================================== UDP

#[test]
fn udp_bind_requires_the_host_to_opt_in() {
    let runtime = runtime(loopback_policy()); // 没开 allow_bind
    let err = runtime
        .run_blocking(NetOp::UdpBind {
            host: "0.0.0.0".to_string(),
            port: 0,
        })
        .expect_err("默认不允许绑定本地端口");
    assert!(matches!(err, NetError::BindNotAllowed));
}

#[test]
fn udp_send_and_recv_over_loopback() {
    let policy = Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .allow_bind(true)
            .with_io_timeout(Duration::from_secs(5)),
    );
    let runtime = runtime(policy);
    let (handle, local) = match runtime
        .run_blocking(NetOp::UdpBind {
            host: "0.0.0.0".to_string(),
            port: 0,
        })
        .expect("bind")
    {
        NetReply::Bound { handle, local } => (handle, local),
        other => panic!("expected a bound socket, got {other:?}"),
    };
    let local: std::net::SocketAddr = local.parse().expect("bound 地址可解析");

    // 用原生 socket 给脚本的 UDP 句柄发一个数据报，再从脚本侧收。
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    // 绑的是 0.0.0.0，因此要往回环地址发（而不是往 0.0.0.0 发）。
    peer.send_to(b"dgram", ("127.0.0.1", local.port())).unwrap();

    let reply = runtime
        .run_blocking(NetOp::UdpRecvFrom {
            handle,
            max_bytes: 64,
        })
        .expect("recv");
    match reply {
        NetReply::Datagram { data, from } => {
            assert_eq!(data, b"dgram");
            assert!(from.starts_with("127.0.0.1:"), "from = {from}");
        }
        other => panic!("expected a datagram, got {other:?}"),
    }
}

// ==================================================== HTTP

#[test]
fn http_get_returns_status_headers_and_body() {
    let port = serve_once(http_response(
        "HTTP/1.1 200 OK",
        &[("content-type", "text/plain")],
        b"hello",
    ));
    let runtime = runtime(loopback_policy());
    let reply = runtime
        .run_blocking(get(&format!("http://127.0.0.1:{port}/x")))
        .expect("http get");
    match reply {
        NetReply::HttpResponse(response) => {
            assert_eq!(response.status, 200);
            assert_eq!(response.body, b"hello");
            assert_eq!(response.protocol, "http/1.1");
            assert!(response
                .headers
                .iter()
                .any(|(name, value)| name == "content-type" && value == "text/plain"));
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

#[test]
fn http_body_over_the_cap_fails_instead_of_being_allocated() {
    let big = vec![b'x'; 4096];
    let port = serve_once(http_response("HTTP/1.1 200 OK", &[], &big));
    let policy = Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_max_payload_bytes(1024),
    );
    let runtime = runtime(policy);
    let err = runtime
        .run_blocking(get(&format!("http://127.0.0.1:{port}/big")))
        .expect_err("超过上限的响应体必须失败");
    assert!(matches!(err, NetError::TooLarge { .. }), "got {err:?}");
}

#[test]
fn http_redirect_is_not_followed_by_default() {
    let port = serve_once(http_response(
        "HTTP/1.1 302 Found",
        &[("location", "http://127.0.0.1:1/elsewhere")],
        b"",
    ));
    let runtime = runtime(loopback_policy()); // max_redirects 默认 0
    let err = runtime
        .run_blocking(get(&format!("http://127.0.0.1:{port}/r")))
        .expect_err("默认不跟随重定向");
    assert!(matches!(err, NetError::TooManyRedirects(0)), "got {err:?}");
}

#[test]
fn http_redirect_target_must_pass_the_allowlist_again() {
    // 允许 127.0.0.1，但重定向指向白名单外的 example.com：必须被拦下。
    let port = serve_once(http_response(
        "HTTP/1.1 302 Found",
        &[("location", "http://example.com/steal")],
        b"",
    ));
    let policy = Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_max_redirects(3),
    );
    let runtime = runtime(policy);
    let err = runtime
        .run_blocking(get(&format!("http://127.0.0.1:{port}/r")))
        .expect_err("重定向目标必须重新过白名单");
    assert!(matches!(err, NetError::HostNotAllowed { .. }), "got {err:?}");
}

#[test]
fn http_redirect_to_an_unreachable_allowed_port_reports_io_not_success() {
    // 第二跳落在白名单内（127.0.0.1）但没人监听：必须是 IO/超时错误，绝不能算成功。
    // （https→http 的降级拒绝是纯规则，已在 http.rs 的单元测试里覆盖。）
    let port = serve_once(http_response(
        "HTTP/1.1 302 Found",
        &[("location", "http://127.0.0.1:1/nowhere")],
        b"",
    ));
    let policy = Arc::new(
        NetPolicy::new()
            .allow_hosts(["127.0.0.1"])
            .unwrap()
            .allow_private_addresses(true)
            .with_max_redirects(3),
    );
    let runtime = runtime(policy);
    let err = runtime
        .run_blocking(get(&format!("http://127.0.0.1:{port}/r")))
        .expect_err("第二跳连不上，应当报错而不是成功");
    assert!(
        matches!(err, NetError::Io(_) | NetError::Timeout(_)),
        "got {err:?}"
    );
}

#[test]
fn http_rejects_unsupported_schemes() {
    let runtime = runtime(loopback_policy());
    let err = runtime
        .run_blocking(get("ftp://127.0.0.1/file"))
        .expect_err("只支持 http/https");
    assert!(matches!(err, NetError::Url(_)), "got {err:?}");
}
