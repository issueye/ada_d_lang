//! HTTP/1.1 与 HTTP/2 客户端（M9，P3）。
//!
//! **刻意不用连接池**：每次请求开一条新连接，读完即关。理由是这个 crate 的定位是
//! "沙箱里的可预测行为"——池化会引入跨请求的隐藏状态（复用谁、何时过期），既难以审计，
//! 也让"每次请求都重新过白名单"的保证变复杂。代价是每次请求多一次握手，已记录在案。
//!
//! 沙箱相关的三件事都由我们自己实现，不交给库：
//! 1. **重定向**：默认不跟随；打开后每一跳都要重新过主机白名单，且禁止 `https → http` 降级；
//! 2. **响应体上限**：逐帧累加并在超限时立刻失败，不先分配再检查；
//! 3. **连接前的地址校验**：与 TCP 一致，只连校验过的 `SocketAddr`。

use crate::error::NetError;
use crate::op::{HttpMethod, HttpRequest, HttpResponse, NetReply};
use crate::policy::NetPolicy;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CONNECTION, CONTENT_LENGTH, HOST, LOCATION, TRANSFER_ENCODING};
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::Arc;
use tokio::net::TcpStream;
use url::Url;

/// 一次「连接 → 发一个请求 → 读完响应」的产物。
struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    protocol: &'static str,
    location: Option<String>,
}

/// 执行一次 HTTP 请求（含重定向策略）。
pub async fn execute(
    request: HttpRequest,
    policy: &NetPolicy,
    tls: &Arc<rustls::ClientConfig>,
) -> Result<NetReply, NetError> {
    let mut url = parse_url(&request.url)?;
    let mut method = request.method;
    let mut body = request.body.clone();
    let mut redirects_left = policy.max_redirects();
    let mut hops = 0u32;

    loop {
        // 每一跳都重新校验：这是防「重定向绕过白名单」的关键。
        let (host, port) = host_and_port(&url)?;
        policy.check_host(&host, port)?;

        let raw = send_once(&url, method, &request.headers, body.clone(), policy, tls).await?;

        let is_redirect = matches!(raw.status, 301 | 302 | 303 | 307 | 308);
        if !is_redirect {
            return Ok(NetReply::HttpResponse(HttpResponse {
                status: raw.status,
                headers: raw.headers,
                body: raw.body,
                final_url: url.to_string(),
                protocol: raw.protocol,
            }));
        }

        let Some(location) = raw.location else {
            // 3xx 但没有 Location：按最终响应对待，而不是猜。
            return Ok(NetReply::HttpResponse(HttpResponse {
                status: raw.status,
                headers: raw.headers,
                body: raw.body,
                final_url: url.to_string(),
                protocol: raw.protocol,
            }));
        };

        if redirects_left == 0 {
            return Err(NetError::TooManyRedirects(policy.max_redirects()));
        }
        redirects_left -= 1;
        hops += 1;

        let next = url
            .join(&location)
            .map_err(|e| NetError::Url(format!("invalid redirect target '{location}': {e}")))?;
        check_redirect(&url, &next)?;

        // 303，以及 301/302 上的 POST，都按惯例降级为 GET 并丢弃请求体。
        if raw.status == 303 || (matches!(raw.status, 301 | 302) && method == HttpMethod::Post) {
            method = HttpMethod::Get;
            body = None;
        }
        url = next;
        let _ = hops;
    }
}

fn parse_url(text: &str) -> Result<Url, NetError> {
    let url = Url::parse(text).map_err(|e| NetError::Url(e.to_string()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(NetError::Url(format!(
            "unsupported scheme '{other}' (only http and https)"
        ))),
    }
}

/// 重定向目标本身的合法性检查（与白名单检查分开，便于单独测试）。
///
/// 目前只有一条规则：**禁止从 https 降级到 http**。降级会把后续请求（连同认证头与
/// cookie）暴露在明网上，而"能改重定向的是对端"，所以这条必须由客户端兜住。
fn check_redirect(from: &Url, to: &Url) -> Result<(), NetError> {
    if from.scheme() == "https" && to.scheme() == "http" {
        return Err(NetError::Url(
            "refusing to follow a redirect that downgrades https to http".to_string(),
        ));
    }
    Ok(())
}

fn host_and_port(url: &Url) -> Result<(String, u16), NetError> {
    let host = url
        .host_str()
        .ok_or_else(|| NetError::Url(format!("url '{url}' has no host")))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| NetError::Url(format!("url '{url}' has no usable port")))?;
    Ok((host, port))
}

/// 建立连接并发一次请求。TLS 时按 ALPN 协商 h2 / http/1.1。
async fn send_once(
    url: &Url,
    method: HttpMethod,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
    policy: &NetPolicy,
    tls_config: &Arc<rustls::ClientConfig>,
) -> Result<RawResponse, NetError> {
    let (host, port) = host_and_port(url)?;
    if let Some(len) = body.as_ref().map(|b| b.len()) {
        policy.check_payload(len)?;
    }

    // 与 TCP 路径同源：只连校验过的地址。
    let addrs = {
        let resolved: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| NetError::Io(format!("cannot resolve '{host}': {e}")))?
            .collect();
        let allowed: Vec<std::net::SocketAddr> = resolved
            .into_iter()
            .filter(|addr| policy.check_address(addr.ip()).is_ok())
            .collect();
        if allowed.is_empty() {
            return Err(NetError::PrivateAddress { addr: host });
        }
        allowed
    };

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
    let tcp = tokio::time::timeout(policy.connect_timeout(), connect_all)
        .await
        .map_err(|_| NetError::Timeout(policy.connect_timeout()))??;
    let _ = tcp.set_nodelay(true);

    let request = build_request(url, method, headers, body)?;

    if url.scheme() == "https" {
        let connector = tokio_rustls::TlsConnector::from(Arc::clone(tls_config));
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| NetError::Tls(format!("invalid server name '{host}': {e}")))?;
        let tls = tokio::time::timeout(policy.io_timeout(), connector.connect(server_name, tcp))
            .await
            .map_err(|_| NetError::Timeout(policy.io_timeout()))?
            .map_err(|e| NetError::Tls(e.to_string()))?;

        let negotiated = tls
            .get_ref()
            .1
            .alpn_protocol()
            .map(|p| p.to_vec())
            .unwrap_or_default();
        if negotiated == b"h2" {
            send_h2(TokioIo::new(tls), request, policy).await
        } else {
            send_h1(TokioIo::new(tls), request, policy).await
        }
    } else {
        send_h1(TokioIo::new(tcp), request, policy).await
    }
}

fn build_request(
    url: &Url,
    method: HttpMethod,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<Request<Full<Bytes>>, NetError> {
    let mut builder = Request::builder()
        .method(method.as_str())
        .uri(url.as_str());
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        // 这些头由传输层负责，脚本不得覆盖（否则会破坏分帧）。
        if matches!(
            lower.as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
        ) {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_str());
    }
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("user-agent")) {
        builder = builder.header("user-agent", "ada-d/0.1");
    }
    let body = match body {
        Some(bytes) => Full::new(Bytes::from(bytes)),
        None => Full::new(Bytes::new()),
    };
    builder
        .body(body)
        .map_err(|e| NetError::Http(e.to_string()))
}

async fn send_h1<T>(
    io: TokioIo<T>,
    request: Request<Full<Bytes>>,
    policy: &NetPolicy,
) -> Result<RawResponse, NetError>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| NetError::Http(e.to_string()))?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });

    let response = tokio::time::timeout(policy.io_timeout(), sender.send_request(request))
        .await
        .map_err(|_| NetError::Timeout(policy.io_timeout()))?
        .map_err(|e| NetError::Http(e.to_string()))?;
    let raw = read_response(response, "http/1.1", policy).await;
    driver.abort();
    raw
}

async fn send_h2<T>(
    io: TokioIo<T>,
    request: Request<Full<Bytes>>,
    policy: &NetPolicy,
) -> Result<RawResponse, NetError>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), io)
            .await
            .map_err(|e| NetError::Http(e.to_string()))?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });

    let response = tokio::time::timeout(policy.io_timeout(), sender.send_request(request))
        .await
        .map_err(|_| NetError::Timeout(policy.io_timeout()))?
        .map_err(|e| NetError::Http(e.to_string()))?;
    let raw = read_response(response, "h2", policy).await;
    driver.abort();
    raw
}

/// 读取响应，逐帧累加并对上限做**前检查**。
async fn read_response(
    response: Response<Incoming>,
    protocol: &'static str,
    policy: &NetPolicy,
) -> Result<RawResponse, NetError> {
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let _ = (CONNECTION, CONTENT_LENGTH, TRANSFER_ENCODING, HOST);

    let limit = policy.max_payload_bytes();
    let mut body = Vec::new();
    let mut stream = response.into_body();
    while let Some(frame) = stream.frame().await {
        let frame = frame.map_err(|e| NetError::Http(e.to_string()))?;
        if let Some(chunk) = frame.data_ref() {
            if body.len() + chunk.len() > limit {
                // 已经超过上限：立刻失败，不再继续读入内存。
                return Err(NetError::TooLarge {
                    size: body.len() + chunk.len(),
                    limit,
                });
            }
            body.extend_from_slice(chunk);
        }
    }

    Ok(RawResponse {
        status,
        headers,
        body,
        protocol,
        location,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_and_https_schemes_are_accepted() {
        assert!(parse_url("http://example.com/x").is_ok());
        assert!(parse_url("https://example.com/x").is_ok());
        for bad in ["ftp://example.com/x", "file:///etc/passwd", "gopher://x/1"] {
            assert!(matches!(parse_url(bad), Err(NetError::Url(_))), "{bad}");
        }
    }

    #[test]
    fn default_ports_follow_the_scheme() {
        let (host, port) = host_and_port(&parse_url("http://example.com/x").unwrap()).unwrap();
        assert_eq!((host.as_str(), port), ("example.com", 80));
        let (host, port) = host_and_port(&parse_url("https://example.com/x").unwrap()).unwrap();
        assert_eq!((host.as_str(), port), ("example.com", 443));
        let (host, port) = host_and_port(&parse_url("https://example.com:8443/x").unwrap()).unwrap();
        assert_eq!((host.as_str(), port), ("example.com", 8443));
    }

    /// 降级必须被拒绝：对端能控制 Location，客户端必须兜住凭据泄露。
    #[test]
    fn https_to_http_downgrade_is_refused_but_other_hops_are_allowed() {
        let https = parse_url("https://example.com/a").unwrap();
        let downgraded = parse_url("http://example.com/a").unwrap();
        assert!(matches!(
            check_redirect(&https, &downgraded),
            Err(NetError::Url(_))
        ));

        // http → https 升级、https → https 换站，都应当放行（白名单另行检查）
        let http = parse_url("http://example.com/a").unwrap();
        assert!(check_redirect(&http, &https).is_ok());
        assert!(check_redirect(&https, &parse_url("https://other.test/b").unwrap()).is_ok());
        assert!(check_redirect(&http, &parse_url("http://other.test/b").unwrap()).is_ok());
    }
}
