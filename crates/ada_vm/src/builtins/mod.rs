pub mod core;
pub mod json;
pub mod math;
pub mod sys;
pub mod time;

use crate::value::Value;
use std::collections::HashMap;

/// Registers all standard built-in functions and native modules into the VM global scope.
pub fn register_all(globals: &mut HashMap<String, Value>) {
    // 1. Core global functions (I/O, type checks, conversions, assertions, math, iterators, async)
    core::register_core_globals(globals);

    // 2. Iterator adapters (T2.1) — 覆盖 core 中同名的原生实现（如 sum/join）
    core::register_iterator_adapters(globals);

    // 3. Standard native modules (json, math, time, sys, env)
    globals.insert("json".to_string(), json::create_json_module());
    globals.insert("math".to_string(), math::create_math_module());
    globals.insert("time".to_string(), time::create_time_module());
    globals.insert("sys".to_string(), sys::create_sys_module());
    globals.insert("env".to_string(), sys::create_env_module());
}
