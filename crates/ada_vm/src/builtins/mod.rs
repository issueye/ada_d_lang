pub mod core;
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
            sys::create_env_module(&sandbox.env_allowlist),
        );
    }
}
