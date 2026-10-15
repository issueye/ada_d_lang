//! 网络授权策略（M9，对应决策 D13）。
//!
//! 策略是**唯一**的网络准入判定点，由宿主构造后交给沙箱。核心立场与 `fs` 一致：
//! **默认拒绝**——没有策略（`None`）时模块根本不存在；有策略但没有白名单条目时
//! 任何主机都不放行。
//!
//! 三层防线，缺一不可：
//! 1. **主机白名单**（大小写不敏感；`*.suffix` 只匹配子域，不匹配 suffix 本身）；
//! 2. **解析后地址检查**：拒绝私有/保留地址段（防 DNS rebinding 指向内网），
//!    并且连的是**校验过的那个 `SocketAddr`**，不再解析一次（避免 TOCTOU）；
//! 3. **逐操作的硬超时与字节上限**，把无界等待与放大变成有界失败。

use crate::error::NetError;
use std::net::IpAddr;
use std::time::Duration;

/// 默认单次载荷上限：与 `fs` 的 `IO_LIMIT_BYTES` 保持一致（1 MiB）。
pub const DEFAULT_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
/// 默认连接超时。
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 默认单次读/写超时。
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// 一条主机放行规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRule {
    /// `None` 表示放行任意主机（规则的 host 部分是 `*`）。
    host: Option<String>,
    /// `true` 时 `host` 是后缀，匹配任意子域（`*.example.com` 匹配 `a.example.com`）。
    suffix: bool,
    /// `None` 表示该主机的任意端口都放行。
    port: Option<u16>,
}

impl HostRule {
    /// 解析 `host`、`host:port`、`*.suffix`、`*.suffix:port` 或 `*`。
    ///
    /// 大小写不敏感（DNS 主机名本身不区分大小写），并统一去掉尾部点号。
    pub fn parse(text: &str) -> Result<Self, NetError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(NetError::Url("empty host rule".to_string()));
        }
        // 端口在最后一个冒号之后；注意 IPv6 字面量带冒号，因此只接受带方括号的形态。
        let (host_part, port) = split_port(text)?;
        if host_part == "*" {
            return Ok(HostRule {
                host: None,
                suffix: false,
                port,
            });
        }
        let (suffix, host) = match host_part.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, host_part),
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return Err(NetError::Url(format!("invalid host rule '{text}'")));
        }
        Ok(HostRule {
            host: Some(host),
            suffix,
            port,
        })
    }

    /// 该规则的主机部分是否放行 `host`（**不考虑端口**）。
    pub fn matches_host(&self, host: &str) -> bool {
        let Some(rule_host) = &self.host else {
            return true; // `*`
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.suffix {
            // 只匹配子域：`*.example.com` 命中 `a.example.com`，但不命中 `example.com`
            // 也不命中 `notexample.com`（后缀前必须是点）。
            host.len() > rule_host.len()
                && host.ends_with(rule_host.as_str())
                && host.as_bytes()[host.len() - rule_host.len() - 1] == b'.'
        } else {
            host == *rule_host
        }
    }

    /// 该规则是否放行 `host:port`。
    pub fn matches(&self, host: &str, port: u16) -> bool {
        if let Some(rule_port) = self.port {
            if rule_port != port {
                return false;
            }
        }
        self.matches_host(host)
    }
}

/// 拆出 `host[:port]`；带方括号的 IPv6（`[::1]:80`）也支持。
fn split_port(text: &str) -> Result<(&str, Option<u16>), NetError> {
    if let Some(rest) = text.strip_prefix('[') {
        let (inside, after) = rest
            .split_once(']')
            .ok_or_else(|| NetError::Url(format!("unterminated IPv6 literal in '{text}'")))?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(parse_port(p, text)?),
            None if after.is_empty() => None,
            None => return Err(NetError::Url(format!("invalid host rule '{text}'"))),
        };
        return Ok((inside, port));
    }
    match text.rsplit_once(':') {
        // 多个冒号且没有方括号 → 裸 IPv6 字面量，不支持（要求写成 [::1]）。
        Some((_, p)) if p.chars().all(|c| c.is_ascii_digit()) => {
            Ok((&text[..text.len() - p.len() - 1], Some(parse_port(p, text)?)))
        }
        Some(_) => Err(NetError::Url(format!(
            "bare IPv6 literals are not supported in host rules; write '[::1]:{text}'"
        ))),
        None => Ok((text, None)),
    }
}

fn parse_port(text: &str, whole: &str) -> Result<u16, NetError> {
    text.parse::<u16>()
        .map_err(|_| NetError::Url(format!("invalid port in host rule '{whole}'")))
}

/// 网络授权策略。构造后不可变，通过 `Arc` 在运行时与各操作间共享。
#[derive(Debug, Clone)]
pub struct NetPolicy {
    allowed: Vec<HostRule>,
    connect_timeout: Duration,
    io_timeout: Duration,
    max_payload_bytes: usize,
    max_redirects: u32,
    allow_private_addresses: bool,
    allow_bind: bool,
}

impl Default for NetPolicy {
    fn default() -> Self {
        Self {
            allowed: Vec::new(),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            io_timeout: DEFAULT_IO_TIMEOUT,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
            max_redirects: 0,
            allow_private_addresses: false,
            allow_bind: false,
        }
    }
}

impl NetPolicy {
    /// 空策略：**不放行任何主机**。宿主必须显式 `allow_hosts`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 放行一批主机规则（`host` / `host:port` / `*.suffix` / `*`）。
    pub fn allow_hosts<I, S>(mut self, rules: I) -> Result<Self, NetError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for rule in rules {
            self.allowed.push(HostRule::parse(rule.as_ref())?);
        }
        Ok(self)
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn with_io_timeout(mut self, timeout: Duration) -> Self {
        self.io_timeout = timeout;
        self
    }

    pub fn with_max_payload_bytes(mut self, bytes: usize) -> Self {
        self.max_payload_bytes = bytes;
        self
    }

    pub fn with_max_redirects(mut self, redirects: u32) -> Self {
        self.max_redirects = redirects;
        self
    }

    /// 允许私有/保留地址（默认**不允许**）。测试与内网场景才应打开。
    pub fn allow_private_addresses(mut self, allow: bool) -> Self {
        self.allow_private_addresses = allow;
        self
    }

    /// 允许脚本**绑定本地端口**（默认**不允许**）。
    ///
    /// UDP 必须有本地端口才能收发，因此绑定本质上带服务端性质；TCP 侧本轮根本不提供
    /// `listen`。默认关闭，意味着"只做客户端"是默认姿态。
    pub fn allow_bind(mut self, allow: bool) -> Self {
        self.allow_bind = allow;
        self
    }

    pub fn binding_allowed(&self) -> bool {
        self.allow_bind
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn io_timeout(&self) -> Duration {
        self.io_timeout
    }

    pub fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    pub fn max_redirects(&self) -> u32 {
        self.max_redirects
    }

    pub fn private_addresses_allowed(&self) -> bool {
        self.allow_private_addresses
    }

    /// 单次操作的**总时长上界**。
    ///
    /// 这个值有一个结构性用途，不只是诊断：VM 把网络调用做成挂起点后，"操作是否总会
    /// 结算"就成了调度器不死锁的前提。运行时用这个上界把每个操作整体包一层 `timeout`，
    /// 从而保证**每个提交的操作都会在有限时间内产生恰好一个完成事件**——包括 DNS 解析、
    /// 读响应体这些本身没有单独超时的步骤。
    pub fn operation_budget(&self) -> Duration {
        self.connect_timeout + self.io_timeout * 2
    }

    /// 白名单是否为空（空即全拒绝）。用于自省与诊断。
    pub fn is_deny_all(&self) -> bool {
        self.allowed.is_empty()
    }

    /// 主机 + 端口是否放行。这是第一层防线。
    pub fn check_host(&self, host: &str, port: u16) -> Result<(), NetError> {
        if self.allowed.iter().any(|rule| rule.matches(host, port)) {
            return Ok(());
        }
        // 区分「主机不认识」与「主机认识但端口不放行」，让错误可诊断。
        if self.allowed.iter().any(|rule| rule.matches_host(host)) {
            return Err(NetError::PortNotAllowed {
                host: host.to_string(),
                port,
            });
        }
        Err(NetError::HostNotAllowed {
            host: host.to_string(),
        })
    }

    /// 解析后的地址是否放行。这是第二层防线（防 DNS rebinding 指向内网）。
    pub fn check_address(&self, addr: IpAddr) -> Result<(), NetError> {
        if self.allow_private_addresses || is_public_address(addr) {
            Ok(())
        } else {
            Err(NetError::PrivateAddress {
                addr: addr.to_string(),
            })
        }
    }

    /// 校验载荷长度。这是第三层防线的一部分。
    pub fn check_payload(&self, size: usize) -> Result<(), NetError> {
        if size > self.max_payload_bytes {
            Err(NetError::TooLarge {
                size,
                limit: self.max_payload_bytes,
            })
        } else {
            Ok(())
        }
    }
}

/// 该地址是否属于**可公开路由**的范围。
///
/// 刻意不用 `IpAddr::is_global()`：它在标准库里仍是 unstable（`ip` feature），而且这里的
/// 被拒范围是 SSRF 防线的核心判定，写清楚比"调一个方法"更重要——评审时应当能一眼看出
/// 每个网段为什么被挡。逐条对应 IANA 特殊用途地址登记表。
pub fn is_public_address(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            let [a, b, c, ..] = v4.octets();
            let blocked = a == 0                        // 0.0.0.0/8    "本网络"
                || a == 10                              // 10/8         私有
                || a == 127                             // 127/8        回环
                || (a == 100 && (64..=127).contains(&b)) // 100.64/10   运营商级 NAT
                || (a == 169 && b == 254)               // 169.254/16   链路本地
                || (a == 172 && (16..=31).contains(&b)) // 172.16/12    私有
                || (a == 192 && b == 0 && c == 0)       // 192.0.0/24   IETF 保留
                || (a == 192 && b == 0 && c == 2)       // 192.0.2/24   TEST-NET-1
                || (a == 192 && b == 88 && c == 99)     // 192.88.99/24 6to4 中继
                || (a == 192 && b == 168)               // 192.168/16   私有
                || (a == 198 && (b == 18 || b == 19))   // 198.18/15    基准测试
                || (a == 198 && b == 51 && c == 100)    // 198.51.100/24 TEST-NET-2
                || (a == 203 && b == 0 && c == 113)     // 203.0.113/24 TEST-NET-3
                || a >= 224; //                            组播(224/4) + 保留(240/4，含广播)
            !blocked
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() || v6.is_loopback() || v6.is_multicast() {
                return false;
            }
            let segments = v6.segments();
            // IPv4 映射(::ffff:a.b.c.d)与兼容(::a.b.c.d)：按内嵌的 v4 判定
            if segments[..5] == [0, 0, 0, 0, 0] && (segments[5] == 0 || segments[5] == 0xffff) {
                let embedded = std::net::Ipv4Addr::new(
                    (segments[6] >> 8) as u8,
                    segments[6] as u8,
                    (segments[7] >> 8) as u8,
                    segments[7] as u8,
                );
                return is_public_address(IpAddr::V4(embedded));
            }
            let blocked = segments[0] & 0xffc0 == 0xfe80        // fe80::/10 链路本地
                || segments[0] & 0xfe00 == 0xfc00               // fc00::/7  唯一本地
                || (segments[0] == 0x2001 && segments[1] == 0x0db8); // 2001:db8::/32 文档
            !blocked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_host_rule_matches_only_that_host_any_case() {
        let rule = HostRule::parse("Example.COM").unwrap();
        assert!(rule.matches("example.com", 80));
        assert!(rule.matches("EXAMPLE.com", 80));
        assert!(rule.matches("example.com.", 443));
        assert!(!rule.matches("a.example.com", 80));
        assert!(!rule.matches("example.com.evil.test", 80));
    }

    #[test]
    fn wildcard_rule_matches_subdomains_but_not_the_apex() {
        let rule = HostRule::parse("*.example.com").unwrap();
        assert!(rule.matches("a.example.com", 443));
        assert!(rule.matches("a.b.example.com", 443));
        assert!(!rule.matches("example.com", 443));
        // 关键：后缀前必须是点，否则 `notexample.com` 会被误放行。
        assert!(!rule.matches("notexample.com", 443));
    }

    #[test]
    fn port_scoped_rule_rejects_other_ports() {
        let policy = NetPolicy::new().allow_hosts(["api.test:443"]).unwrap();
        assert!(policy.check_host("api.test", 443).is_ok());
        assert!(matches!(
            policy.check_host("api.test", 80),
            Err(NetError::PortNotAllowed { .. })
        ));
        assert!(matches!(
            policy.check_host("other.test", 443),
            Err(NetError::HostNotAllowed { .. })
        ));
    }

    #[test]
    fn empty_policy_denies_everything() {
        let policy = NetPolicy::new();
        assert!(policy.is_deny_all());
        assert!(matches!(
            policy.check_host("example.com", 443),
            Err(NetError::HostNotAllowed { .. })
        ));
    }

    #[test]
    fn bare_star_allows_any_host() {
        let policy = NetPolicy::new().allow_hosts(["*"]).unwrap();
        assert!(policy.check_host("anything.test", 8080).is_ok());
        assert!(!policy.is_deny_all());
    }

    #[test]
    fn private_addresses_are_rejected_unless_opted_in() {
        let strict = NetPolicy::new();
        assert!(strict.check_address("127.0.0.1".parse().unwrap()).is_err());
        assert!(strict.check_address("10.0.0.1".parse().unwrap()).is_err());
        assert!(strict.check_address("192.168.1.1".parse().unwrap()).is_err());
        assert!(strict.check_address("::1".parse().unwrap()).is_err());
        assert!(strict.check_address("93.184.216.34".parse().unwrap()).is_ok());

        let relaxed = NetPolicy::new().allow_private_addresses(true);
        assert!(relaxed.check_address("127.0.0.1".parse().unwrap()).is_ok());
    }

    #[test]
    fn ipv6_literal_rules_parse_with_brackets() {
        let rule = HostRule::parse("[::1]:8080").unwrap();
        assert!(rule.matches("::1", 8080));
        assert!(!rule.matches("::1", 80));
    }

    #[test]
    fn payload_limit_is_enforced() {
        let policy = NetPolicy::new().with_max_payload_bytes(8);
        assert!(policy.check_payload(8).is_ok());
        assert!(matches!(
            policy.check_payload(9),
            Err(NetError::TooLarge { .. })
        ));
    }
}
