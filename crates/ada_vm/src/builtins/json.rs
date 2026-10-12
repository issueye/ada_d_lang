use crate::sandbox::Sandbox;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub fn create_json_module(sandbox: &Sandbox) -> Value {
    let mut module = HashMap::new();

    // M5：`json.parse`/`stringify` 的规模由输入决定，宿主无法在 VM 侧为它们计费，
    // 因此按内存预算给字节数设上限，避免"一条指令吃掉全部预算"。
    let cap: Arc<Option<usize>> = Arc::new(sandbox.max_memory_bytes);

    // json.parse(str)
    let parse_cap = cap.clone();
    module.insert(
        "parse".to_string(),
        Value::native_fn("json.parse", move |args| {
            let str_val = args
                .first()
                .ok_or_else(|| "json.parse expects a string argument".to_string())?
                .to_string();

            if let Some(cap) = *parse_cap {
                if str_val.len() > cap {
                    return Err(format!(
                        "json.parse input ({} bytes) exceeds the sandbox memory budget ({} bytes)",
                        str_val.len(),
                        cap
                    ));
                }
            }

            let parsed: serde_json::Value =
                serde_json::from_str(&str_val).map_err(|e| format!("json.parse failed: {e}"))?;

            Ok(Value::from_json(&parsed))
        }),
    );

    // json.stringify(val, [pretty])
    let stringify_cap = cap;
    module.insert(
        "stringify".to_string(),
        Value::native_fn("json.stringify", move |args| {
            if args.is_empty() {
                return Ok(Value::string("null"));
            }
            let val = &args[0];
            let pretty = args.get(1).is_some_and(|p| p.is_truthy());

            let json_val = val.to_json();
            let res = if pretty {
                serde_json::to_string_pretty(&json_val)
            } else {
                serde_json::to_string(&json_val)
            }
            .map_err(|e| format!("json.stringify failed: {e}"))?;

            if let Some(cap) = *stringify_cap {
                if res.len() > cap {
                    return Err(format!(
                        "json.stringify output ({} bytes) exceeds the sandbox memory budget ({} bytes)",
                        res.len(),
                        cap
                    ));
                }
            }

            Ok(Value::string(res))
        }),
    );

    // json.valid(str)
    module.insert(
        "valid".to_string(),
        Value::native_fn("json.valid", |args| {
            if let Some(arg) = args.first() {
                let s = arg.to_string();
                let is_valid = serde_json::from_str::<serde_json::Value>(&s).is_ok();
                Ok(Value::Bool(is_valid))
            } else {
                Ok(Value::Bool(false))
            }
        }),
    );

    Value::map(module)
}
