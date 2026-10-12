use crate::value::Value;
use std::collections::HashMap;

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

pub fn create_env_module() -> Value {
    let mut module = HashMap::new();

    // env.get(key, [default]) -> string or default or nil
    module.insert(
        "get".to_string(),
        Value::native_fn("env.get", |args| {
            let key = args.first().map(|a| a.to_string()).unwrap_or_default();
            match std::env::var(&key) {
                Ok(val) => Ok(Value::string(val)),
                Err(_) => {
                    if let Some(default_val) = args.get(1) {
                        Ok(default_val.clone())
                    } else {
                        Ok(Value::Nil)
                    }
                }
            }
        }),
    );

    // env.has(key) -> bool
    module.insert(
        "has".to_string(),
        Value::native_fn("env.has", |args| {
            let key = args.first().map(|a| a.to_string()).unwrap_or_default();
            Ok(Value::Bool(std::env::var(&key).is_ok()))
        }),
    );

    Value::map(module)
}
