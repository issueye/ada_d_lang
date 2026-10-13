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

    // 3. `fs`（T7.5）：需要宿主提供受限根目录——**根目录本身即授权**，因此按沙箱
    //    注册（同时支持 `import fs from "std:fs"`，二者等价）。
    if let Some(root) = &sandbox.fs_root {
        globals.insert(
            "fs".to_string(),
            fs::create_fs_module(root.clone(), sandbox.max_memory_bytes),
        );
    }

    // 4. 其它内置模块（T8.14，决策 D12）：**默认不再注册**。脚本必须显式
    //    `import X from "std:X"`——由编译器生成 ImportModule/ImportNamed 指令，
    //    VM 通过 `std_module` 以 permissive（完全授权）注册。
    //    沙箱 profile 的 allow_* 不再决定模块可见性（仅用于诊断信息）。
}

/// 构造一个内置模块值，供 `import ... from "std:<name>"` 在 VM 层注册（T8.14）。
///
/// 「导入即授权」（决策 D12）：以 permissive 构造，不受引擎 profile 限制。
/// 返回 `None` 表示未知模块；`fs` 需要宿主提供根目录，无根时同样返回 `None`。
pub fn std_module(name: &str, sandbox: &Sandbox) -> Option<Value> {
    // D12：「导入即授权」——能力全开（permissive）；但同时保留引擎的内存预算，
    // 否则模块内部的资源检查（如 json.parse 的输入上限）会被绕过。
    let granted = Sandbox::permissive().with_memory_limit(sandbox.max_memory_bytes);
    Some(match name {
        "json" => json::create_json_module(&granted),
        "math" => math::create_math_module(),
        "crypto" => crypto::create_crypto_module(),
        "time" => time::create_time_module(),
        "sys" => sys::create_sys_module(),
        "env" => sys::create_env_module(None),
        "fs" => fs::create_fs_module(sandbox.fs_root.clone()?, None),
        _ => return None,
    })
}
