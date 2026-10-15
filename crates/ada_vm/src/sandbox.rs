//! 沙箱配置（开发计划 M5 / T5.1–T5.4）。
//!
//! 设计原则：**宿主能力默认最小化**。VM 本身（`Vm::new`）仍注册全部内置模块，
//! 以便直接使用 VM 的场合保持既有行为；嵌入门面 `Engine::new()` 默认使用
//! [`Sandbox::strict`]，只有宿主显式选择的 profile 才会把 `time`/`env`/`sys`
//! 这类宿主能力交给脚本。
//!
//! 三档 profile：
//!
//! | profile | 允许 | 限额 |
//! | :--- | :--- | :--- |
//! | `strict()` | 纯计算 + `json`/`math` + 全部语言特性与任务；**无** `time`/`env`/`sys`/`sleep_ms` | 内存 16 MiB、墙钟 5s、任务 128 |
//! | `standard()` | 上述 + `time` + `sys` + `env`（**键白名单**）+ `sleep_ms` | 内存 64 MiB、任务 512（无墙钟上限） |
//! | `permissive()` | 全部（等效旧行为，仅供受信场景/测试） | 无 |
//!
//! 内存与墙钟都是**近似**约束（见 SPEC_RUNTIME §7.3）：内存按容器/字符串的分配
//! 规模累计，墙钟按指令循环与调度等待处的时钟检查；目标是"无法在不被拒绝的前提下
//! 耗尽宿主资源"，而不是精确的分配器计量。

use std::path::PathBuf;

/// profile 名称，便于诊断与文档对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    /// 默认：仅纯计算 + json/math
    Strict,
    /// 受信宿主：再加 time/sys/env（白名单）/sleep
    Standard,
    /// 不做能力限制
    Permissive,
    /// 宿主自定义
    Custom,
}

impl ProfileKind {
    pub fn name(self) -> &'static str {
        match self {
            ProfileKind::Strict => "strict",
            ProfileKind::Standard => "standard",
            ProfileKind::Permissive => "permissive",
            ProfileKind::Custom => "custom",
        }
    }
}

/// 沙箱配置：能力开关 + 资源限额。
#[derive(Clone, Debug)]
pub struct Sandbox {
    pub kind: ProfileKind,
    /// 是否暴露 `time` 模块
    pub allow_time: bool,
    /// 是否暴露 `env` 模块（键仍需通过 `env_allowlist`）
    pub allow_env: bool,
    /// `env` 允许读取的键；空列表 = 全部拒绝（模块存在但读不到任何键）
    pub env_allowlist: Vec<String>,
    /// 是否暴露 `sys` 模块（`version`/`os`/`arch`）
    pub allow_sys: bool,
    /// 是否允许 `sleep_ms`（挂起等待会占用宿主墙钟）
    pub allow_sleep: bool,
    /// 内存预算（字节，近似）
    pub max_memory_bytes: Option<usize>,
    /// 单次求值的墙钟上限（毫秒）
    pub max_wall_clock_ms: Option<u64>,
    /// 同时存活的任务数上限
    pub max_tasks: Option<usize>,
    /// 指令燃料预算（`None` = 不限制）
    pub fuel: Option<u64>,
    /// `fs` 模块允许访问的根目录（`None` = **不暴露 `fs` 模块**）。
    ///
    /// 这是 T7.5 的核心安全开关：脚本里的所有路径都被解析并限制在该目录之下，
    /// 既不能用 `..`、也不能用绝对路径或符号链接逃逸（见 `builtins/fs.rs`）。
    pub fs_root: Option<PathBuf>,
    /// 宿主**全局否决**的内置模块名（D12 安全缓解，T8.15）。
    ///
    /// 决策 D12「导入即授权」使 `std:` 导入绕过 profile 的能力开关；本字段是
    /// 宿主保留的**最后防线**：列在这里的模块名，即使脚本显式
    /// `import X from "std:X"` 也会被拒绝（`CapabilityDenied`）。
    ///
    /// **默认为空**，即维持 D12 现状（导入即可用）。宿主可借此在保留
    /// 「显式导入」语义的同时，收回对特定模块的最终控制权。
    pub denied_modules: Vec<String>,
    /// 网络授权策略（`None` = **不暴露 `net` / `http` 模块**）。M9 / 决策 D13。
    ///
    /// 与 `fs_root` 同构：这是宿主显式授予的能力，默认**不存在**。策略本体
    /// （主机白名单、硬超时、载荷上限、重定向策略、私有地址开关）定义在
    /// `ada_net::NetPolicy` 中；沙箱只负责"给不给"，不重复实现判定。
    pub net_policy: Option<std::sync::Arc<ada_net::NetPolicy>>,
    /// 网络客户端 TLS 配置（`None` = 用默认：webpki 公共根证书 + ring 后端）。
    ///
    /// 宿主需要信任**私有 CA**（企业内网、自签证书）时在此注入。刻意**不提供**
    /// "跳过证书校验"的开关——那种开关一旦存在就会被误用。
    pub net_tls: Option<std::sync::Arc<ada_net::rustls::ClientConfig>>,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::strict()
    }
}

impl Sandbox {
    /// 默认档：仅纯计算 + `json`/`math`。
    pub fn strict() -> Self {
        Self {
            kind: ProfileKind::Strict,
            allow_time: false,
            allow_env: false,
            env_allowlist: Vec::new(),
            allow_sys: false,
            allow_sleep: false,
            max_memory_bytes: Some(16 * 1024 * 1024),
            max_wall_clock_ms: Some(5_000),
            max_tasks: Some(128),
            fuel: None,
            fs_root: None,
            denied_modules: Vec::new(),
            net_policy: None,
            net_tls: None,
        }
    }

    /// 受信宿主档：允许 `time`/`sys`/`env`(白名单)/`sleep_ms`。
    pub fn standard() -> Self {
        Self {
            kind: ProfileKind::Standard,
            allow_time: true,
            allow_env: true,
            env_allowlist: Vec::new(),
            allow_sys: true,
            allow_sleep: true,
            max_memory_bytes: Some(64 * 1024 * 1024),
            max_wall_clock_ms: None,
            max_tasks: Some(512),
            fuel: None,
            fs_root: None,
            denied_modules: Vec::new(),
            net_policy: None,
            net_tls: None,
        }
    }

    /// 受信场景/兼容旧行为：不做能力与资源限制。
    pub fn permissive() -> Self {
        Self {
            kind: ProfileKind::Permissive,
            allow_time: true,
            allow_env: true,
            env_allowlist: Vec::new(),
            allow_sys: true,
            allow_sleep: true,
            max_memory_bytes: None,
            max_wall_clock_ms: None,
            max_tasks: None,
            fuel: None,
            fs_root: None,
            denied_modules: Vec::new(),
            net_policy: None,
            net_tls: None,
        }
    }

    // ---------------------------------------------------------- builder

    pub fn with_time(mut self, allow: bool) -> Self {
        self.kind = ProfileKind::Custom;
        self.allow_time = allow;
        self
    }

    pub fn with_sys(mut self, allow: bool) -> Self {
        self.kind = ProfileKind::Custom;
        self.allow_sys = allow;
        self
    }

    pub fn with_sleep(mut self, allow: bool) -> Self {
        self.kind = ProfileKind::Custom;
        self.allow_sleep = allow;
        self
    }

    /// 允许 `env` 模块，并只放行给定键（调用会覆盖已有白名单）。
    pub fn with_env_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.kind = ProfileKind::Custom;
        self.allow_env = true;
        self.env_allowlist = keys.into_iter().map(Into::into).collect();
        self
    }

    /// 完全关闭 `env` 模块。
    pub fn without_env(mut self) -> Self {
        self.kind = ProfileKind::Custom;
        self.allow_env = false;
        self.env_allowlist.clear();
        self
    }

    pub fn with_memory_limit(mut self, bytes: Option<usize>) -> Self {
        self.kind = ProfileKind::Custom;
        self.max_memory_bytes = bytes;
        self
    }

    pub fn with_wall_clock_limit(mut self, ms: Option<u64>) -> Self {
        self.kind = ProfileKind::Custom;
        self.max_wall_clock_ms = ms;
        self
    }

    pub fn with_task_limit(mut self, tasks: Option<usize>) -> Self {
        self.kind = ProfileKind::Custom;
        self.max_tasks = tasks;
        self
    }

    pub fn with_fuel(mut self, fuel: Option<u64>) -> Self {
        self.kind = ProfileKind::Custom;
        self.fuel = fuel;
        self
    }

    /// 暴露 `fs` 模块，并把脚本可见的文件系统限制在 `root` 之下（T7.5）。
    ///
    /// `root` 应当是**已存在**的目录；否则模块会注册但每次调用都返回错误
    /// （可见的失败，而不是静默放行或 panic）。所有路径都会被规范化并验证仍在
    /// 根目录内：`..`、绝对路径与符号链接都无法逃逸。
    pub fn with_fs_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.kind = ProfileKind::Custom;
        self.fs_root = Some(root.into());
        self
    }

    /// 关闭 `fs` 模块（默认行为）。
    pub fn without_fs(mut self) -> Self {
        self.kind = ProfileKind::Custom;
        self.fs_root = None;
        self
    }

    /// 授予网络能力（M9 / 决策 D13）：传入完整策略后，`net` 与 `http` 模块才存在。
    ///
    /// 这是**唯一**的授权入口——与 `fs` 一样，"能力"必须由宿主显式给出，profile
    /// 本身从不开网络。策略本体见 `ada_net::NetPolicy`。
    pub fn with_net_policy(mut self, policy: ada_net::NetPolicy) -> Self {
        self.kind = ProfileKind::Custom;
        self.net_policy = Some(std::sync::Arc::new(policy));
        self
    }

    /// 便捷入口：放行一组主机规则（`host` / `host:port` / `*.suffix` / `*`）后授予网络。
    ///
    /// 其余参数走 `NetPolicy` 的默认值（连接/读写各 10s、载荷 1 MiB、不跟随重定向、
    /// 拒绝私有地址、不允许绑定本地端口）。需要更细的控制请直接用
    /// [`Sandbox::with_net_policy`]。
    pub fn with_net_allowed_hosts<I, S>(self, hosts: I) -> Result<Self, ada_net::NetError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let policy = ada_net::NetPolicy::new().allow_hosts(hosts)?;
        Ok(self.with_net_policy(policy))
    }

    /// 收回网络能力（默认行为）。
    pub fn without_net(mut self) -> Self {
        self.kind = ProfileKind::Custom;
        self.net_policy = None;
        self.net_tls = None;
        self
    }

    /// 注入网络客户端的 TLS 配置（信任根 / ALPN 偏好）。
    ///
    /// 用于宿主需要信任**私有 CA** 的场景：默认配置只信任 webpki 的公共根证书集。
    /// 必须与 [`Sandbox::with_net_policy`] 一起用（没有网络授权时该配置无效）。
    pub fn with_net_tls_config(
        mut self,
        config: std::sync::Arc<ada_net::rustls::ClientConfig>,
    ) -> Self {
        self.kind = ProfileKind::Custom;
        self.net_tls = Some(config);
        self
    }

    /// 当前是否已授予网络能力。
    pub fn net_granted(&self) -> bool {
        self.net_policy.is_some()
    }

    /// 全局否决一个内置模块（D12 安全缓解，T8.15）：即使脚本显式
    /// `import X from "std:X"` 也会被拒绝。
    ///
    /// 这是宿主在「导入即授权」语义下保留的最后防线。重复添加同一名字是幂等的。
    pub fn with_denied_module(mut self, name: impl Into<String>) -> Self {
        self.kind = ProfileKind::Custom;
        let name = name.into();
        if !self.denied_modules.contains(&name) {
            self.denied_modules.push(name);
        }
        self
    }

    /// 批量全局否决内置模块，语义同 [`Sandbox::with_denied_module`]。
    pub fn with_denied_modules<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for name in names {
            self = self.with_denied_module(name);
        }
        self
    }

    // ---------------------------------------------------------- 查询

    /// 某个环境变量键是否被允许读取（未配置白名单时全部拒绝）。
    pub fn env_key_allowed(&self, key: &str) -> bool {
        self.allow_env && self.env_allowlist.iter().any(|k| k == key)
    }

    /// 某个内置模块是否被宿主全局否决（D12 安全缓解，T8.15）。
    pub fn module_denied(&self, name: &str) -> bool {
        self.denied_modules.iter().any(|m| m == name)
    }

    /// 是否暴露某个全局名（用于诊断与测试断言）。
    pub fn allows_global(&self, name: &str) -> bool {
        match name {
            "time" => self.allow_time,
            "env" => self.allow_env,
            "sys" => self.allow_sys,
            "sleep_ms" => self.allow_sleep,
            "fs" => self.fs_root.is_some(),
            // M9 / D13：网络与 fs 同构——没有宿主授予的策略就不存在。
            "net" | "http" => self.net_policy.is_some(),
            _ => true,
        }
    }
}
