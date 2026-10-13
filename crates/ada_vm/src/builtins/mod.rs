pub mod core;
pub mod crypto;
pub mod fs;
pub mod json;
pub mod math;
pub mod sys;
pub mod time;

use crate::sandbox::Sandbox;
use crate::value::Value;
use std::collections::HashMap;

/// 按沙箱配置注册内置函数与原生模块（M5 / T5.1）。
///
/// 被关闭的能力**不注册**（宿主能力不进入脚本可见的名字空间），
/// 由 VM 在访问时给出 `CapabilityDenied` 错误（见 `Vm::denied_globals`）。
pub fn register_all(globals: &mut HashMap<String, Value>, sandbox: &Sandbox) {
    // 1. Core global functions (I/O, type checks, conversions, assertions, math, iterators, async)
    core::register_core_globals(globals, sandbox);

    // 2. Iterator adapters (T2.1) — 覆盖 core 中同名的原生实现（如 sum/join）
    core::register_iterator_adapters(globals);

    // 3. Standard native modules（纯计算模块始终可用）
    globals.insert("json".to_string(), json::create_json_module(sandbox));
    globals.insert("math".to_string(), math::create_math_module());
    globals.insert("crypto".to_string(), crypto::create_crypto_module());

    // 4. 宿主能力模块：按 profile 逐项放行
    if sandbox.allow_time {
        globals.insert("time".to_string(), time::create_time_module());
    }
    if sandbox.allow_sys {
        globals.insert("sys".to_string(), sys::create_sys_module());
    }
    if sandbox.allow_env {
        globals.insert(
            "env".to_string(),
            sys::create_env_module(Some(&sandbox.env_allowlist)),
        );
    }
    // T7.5：`fs` 只在宿主显式给出受限根目录时注册
    if let Some(root) = &sandbox.fs_root {
        globals.insert(
            "fs".to_string(),
            fs::create_fs_module(root.clone(), sandbox.max_memory_bytes),
        );
    }
}

/// 构造一个内置模块值，供 `import ... from "std:<name>"` 在 VM 层注册（T8.14）。
///
/// 「导入即授权」（决策 D12）：以 permissive 构造，不受引擎 profile 限制。
/// 返回 `None` 表示未知模块；`fs` 需要宿主提供根目录，无根时同样返回 `None`。
pub fn std_module(name: &str, sandbox: &Sandbox) -> Option<Value> {
    let permissive = Sandbox::permissive();
    Some(match name {
        "json" => json::create_json_module(&permissive),
        "math" => math::create_math_module(),
        "crypto" => crypto::create_crypto_module(),
        "time" => time::create_time_module(),
        "sys" => sys::create_sys_module(),
        "env" => sys::create_env_module(None),
        "fs" => fs::create_fs_module(sandbox.fs_root.clone()?, None),
        _ => return None,
    })
}
