use crate::value::Value;
use std::collections::HashMap;

pub fn create_json_module() -> Value {
    let mut module = HashMap::new();

    // json.parse(str)
    module.insert(
        "parse".to_string(),
        Value::native_fn("json.parse", |args| {
            let str_val = args
                .first()
                .ok_or_else(|| "json.parse expects a string argument".to_string())?
                .to_string();

            let parsed: serde_json::Value =
                serde_json::from_str(&str_val).map_err(|e| format!("json.parse failed: {e}"))?;

            Ok(Value::from_json(&parsed))
        }),
    );

    // json.stringify(val, [pretty])
    module.insert(
        "stringify".to_string(),
        Value::native_fn("json.stringify", |args| {
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
