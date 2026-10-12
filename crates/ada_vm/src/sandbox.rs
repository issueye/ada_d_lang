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

    // ---------------------------------------------------------- 查询

    /// 某个环境变量键是否被允许读取（未配置白名单时全部拒绝）。
    pub fn env_key_allowed(&self, key: &str) -> bool {
        self.allow_env && self.env_allowlist.iter().any(|k| k == key)
    }

    /// 是否暴露某个全局名（用于诊断与测试断言）。
    pub fn allows_global(&self, name: &str) -> bool {
        match name {
            "time" => self.allow_time,
            "env" => self.allow_env,
            "sys" => self.allow_sys,
            "sleep_ms" => self.allow_sleep,
            _ => true,
        }
    }
}
