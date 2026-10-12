use crate::value::Value;
use std::cell::Cell;
use std::collections::HashMap;
use std::time::SystemTime;

thread_local! {
    static RNG_STATE: Cell<u64> = Cell::new({
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x853c49e6748fea9b);
        if seed == 0 { 0x853c49e6748fea9b } else { seed }
    });
}

fn next_random_u64() -> u64 {
    RNG_STATE.with(|cell| {
        let mut x = cell.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        cell.set(x);
        x
    })
}

fn next_random_f64() -> f64 {
    // 53 bits of precision
    (next_random_u64() >> 11) as f64 / (1u64 << 53) as f64
}

pub fn create_math_module() -> Value {
    let mut module = HashMap::new();

    // Constants
    module.insert("PI".to_string(), Value::Float(std::f64::consts::PI));
    module.insert("E".to_string(), Value::Float(std::f64::consts::E));
    module.insert("INF".to_string(), Value::Float(f64::INFINITY));

    // math.sqrt(x)
    module.insert(
        "sqrt".to_string(),
        Value::native_fn("math.sqrt", |args| {
            let x = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
            Ok(Value::Float(x.sqrt()))
        }),
    );

    // math.pow(x, y)
    module.insert(
        "pow".to_string(),
        Value::native_fn("math.pow", |args| {
            if args.len() < 2 {
                return Err("math.pow requires (base, exp)".into());
            }
            let x = args[0].as_float().unwrap_or(0.0);
            let y = args[1].as_float().unwrap_or(0.0);
            Ok(Value::Float(x.powf(y)))
        }),
    );

    // math.log(x)
    module.insert(
        "log".to_string(),
        Value::native_fn("math.log", |args| {
            let x = args.first().and_then(|a| a.as_float()).unwrap_or(1.0);
            Ok(Value::Float(x.ln()))
        }),
    );

    // math.log10(x)
    module.insert(
        "log10".to_string(),
        Value::native_fn("math.log10", |args| {
            let x = args.first().and_then(|a| a.as_float()).unwrap_or(1.0);
            Ok(Value::Float(x.log10()))
        }),
    );

    // math.sin(rad)
    module.insert(
        "sin".to_string(),
        Value::native_fn("math.sin", |args| {
            let rad = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
            Ok(Value::Float(rad.sin()))
        }),
    );

    // math.cos(rad)
    module.insert(
        "cos".to_string(),
        Value::native_fn("math.cos", |args| {
            let rad = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
            Ok(Value::Float(rad.cos()))
        }),
    );

    // math.tan(rad)
    module.insert(
        "tan".to_string(),
        Value::native_fn("math.tan", |args| {
            let rad = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
            Ok(Value::Float(rad.tan()))
        }),
    );

    // math.random() -> [0.0, 1.0)
    module.insert(
        "random".to_string(),
        Value::native_fn("math.random", |_| {
            Ok(Value::Float(next_random_f64()))
        }),
    );

    // math.random_int(min, max) -> [min, max]
    module.insert(
        "random_int".to_string(),
        Value::native_fn("math.random_int", |args| {
            if args.len() < 2 {
                return Err("math.random_int requires (min, max)".into());
            }
            let min = args[0].as_int().unwrap_or(0);
            let max = args[1].as_int().unwrap_or(100);
            if min >= max {
                return Ok(Value::Int(min));
            }
            let span = (max - min + 1) as u64;
            let val = min + (next_random_u64() % span) as i64;
            Ok(Value::Int(val))
        }),
    );

    Value::map(module)
}
