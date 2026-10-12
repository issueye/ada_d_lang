use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub fn create_sys_module() -> Value {
    let mut module = HashMap::new();

    // sys.version -> string
    module.insert(
        "version".to_string(),
        Value::string(env!("CARGO_PKG_VERSION")),
    );

    // sys.os -> string ("windows", "linux", "macos", etc.)
    module.insert("os".to_string(), Value::string(std::env::consts::OS));

    // sys.arch -> string ("x86_64", "aarch64", etc.)
    module.insert("arch".to_string(), Value::string(std::env::consts::ARCH));

    Value::map(module)
}

pub fn create_env_module(allowlist: &[String]) -> Value {
    let mut module = HashMap::new();
    let allowlist: Arc<Vec<String>> = Arc::new(allowlist.to_vec());

    // M5 / T5.2：未列入白名单的键**不读宿主环境**，直接返回 nil / false
    let allowed = allowlist.clone();
    module.insert(
        "allowed".to_string(),
        Value::native_fn("env.allowed", move |_| {
            Ok(Value::array(
                allowed.iter().map(|k| Value::string(k.clone())).collect(),
            ))
        }),
    );

    // env.get(key, [default]) -> string or default or nil
    let allowed = allowlist.clone();
    module.insert(
        "get".to_string(),
        Value::native_fn("env.get", move |args| {
            let key = args.first().map(|a| a.to_string()).unwrap_or_default();
            if !allowed.contains(&key) {
                return Ok(args.get(1).cloned().unwrap_or(Value::Nil));
            }
            match std::env::var(&key) {
                Ok(val) => Ok(Value::string(val)),
                Err(_) => Ok(args.get(1).cloned().unwrap_or(Value::Nil)),
            }
        }),
    );

    // env.has(key) -> bool
    let allowed = allowlist;
    module.insert(
        "has".to_string(),
        Value::native_fn("env.has", move |args| {
            let key = args.first().map(|a| a.to_string()).unwrap_or_default();
            if !allowed.contains(&key) {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(std::env::var(&key).is_ok()))
        }),
    );

    Value::map(module)
}
