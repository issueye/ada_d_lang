use crate::value::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn create_time_module() -> Value {
    let mut module = HashMap::new();

    // time.now_ms() -> int (milliseconds since UNIX_EPOCH)
    module.insert(
        "now_ms".to_string(),
        Value::native_fn("time.now_ms", |_| {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            Ok(Value::Int(ms))
        }),
    );

    // time.now_secs() -> int (seconds since UNIX_EPOCH)
    module.insert(
        "now_secs".to_string(),
        Value::native_fn("time.now_secs", |_| {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Ok(Value::Int(secs))
        }),
    );

    // time.elapsed_ms(start_ms) -> int
    module.insert(
        "elapsed_ms".to_string(),
        Value::native_fn("time.elapsed_ms", |args| {
            let start = args.first().and_then(|a| a.as_int()).unwrap_or(0);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let elapsed = if now >= start { now - start } else { 0 };
            Ok(Value::Int(elapsed))
        }),
    );

    Value::map(module)
}
