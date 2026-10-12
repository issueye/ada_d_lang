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

/// 取数值参数：非数值报错（G-S6 口径统一，不再静默按 0 处理）。
fn number_arg(args: &[Value], who: &str) -> Result<f64, String> {
    let value = args
        .first()
        .ok_or_else(|| format!("{who}: expected a number, got nothing"))?;
    value
        .as_float()
        .ok_or_else(|| format!("{who}: expected a number, got {}", value.type_name()))
}

/// 一元数学函数（`sqrt`/`log`/`log10`/`sin`/`cos`/`tan`）。
fn unary_math(args: &[Value], who: &str, op: fn(f64) -> f64) -> Result<Value, String> {
    Ok(Value::Float(op(number_arg(args, who)?)))
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
        Value::native_fn("math.sqrt", |args| unary_math(args, "math.sqrt", f64::sqrt)),
    );

    // math.pow(x, y)
    module.insert(
        "pow".to_string(),
        Value::native_fn("math.pow", |args| {
            if args.len() < 2 {
                return Err("math.pow requires (base, exp)".into());
            }
            let base = args[0].as_float().ok_or_else(|| {
                format!("math.pow: expected a number, got {}", args[0].type_name())
            })?;
            let exp = args[1].as_float().ok_or_else(|| {
                format!("math.pow: expected a number, got {}", args[1].type_name())
            })?;
            Ok(Value::Float(base.powf(exp)))
        }),
    );

    // math.log(x) / math.log10(x)
    module.insert(
        "log".to_string(),
        Value::native_fn("math.log", |args| unary_math(args, "math.log", f64::ln)),
    );
    module.insert(
        "log10".to_string(),
        Value::native_fn("math.log10", |args| {
            unary_math(args, "math.log10", f64::log10)
        }),
    );

    // math.sin/cos/tan(rad)
    module.insert(
        "sin".to_string(),
        Value::native_fn("math.sin", |args| unary_math(args, "math.sin", f64::sin)),
    );
    module.insert(
        "cos".to_string(),
        Value::native_fn("math.cos", |args| unary_math(args, "math.cos", f64::cos)),
    );
    module.insert(
        "tan".to_string(),
        Value::native_fn("math.tan", |args| unary_math(args, "math.tan", f64::tan)),
    );

    // G-S7：舍入函数此前只以全局名提供，命名不一致；这里补上 math.* 别名
    // （全局 `round`/`floor`/`ceil` 仍然保留，不构成破坏性变更）。
    for (name, op) in [
        ("round", f64::round as fn(f64) -> f64),
        ("floor", f64::floor),
        ("ceil", f64::ceil),
    ] {
        let qualified = format!("math.{name}");
        module.insert(
            name.to_string(),
            Value::native_fn(qualified.clone(), move |args| {
                let number = number_arg(args, &qualified)?;
                Ok(Value::Int(op(number) as i64))
            }),
        );
    }

    // math.random() -> [0.0, 1.0)
    module.insert(
        "random".to_string(),
        Value::native_fn("math.random", |_| Ok(Value::Float(next_random_f64()))),
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
            // 用 i128 计算区间宽度：i64 全域输入也不会溢出 panic
            let span = (max as i128 - min as i128 + 1) as u128;
            let offset = (next_random_u64() as u128) % span;
            Ok(Value::Int((min as i128 + offset as i128) as i64))
        }),
    );

    Value::map(module)
}
