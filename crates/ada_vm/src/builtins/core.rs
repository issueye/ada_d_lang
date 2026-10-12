use crate::sandbox::Sandbox;
use crate::value::{AdapterKind, AsyncKind, IteratorState, Value};
use std::collections::HashMap;

pub fn register_core_globals(globals: &mut HashMap<String, Value>, sandbox: &Sandbox) {
    // -------------------------------------------------------------
    // 1. I/O & Debugging
    // -------------------------------------------------------------
    globals.insert(
        "println".to_string(),
        Value::native_fn("println", |args| {
            let out: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            println!("{}", out.join(" "));
            Ok(Value::Nil)
        }),
    );

    globals.insert(
        "print".to_string(),
        Value::native_fn("print", |args| {
            let out: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            print!("{}", out.join(" "));
            Ok(Value::Nil)
        }),
    );

    globals.insert(
        "eprintln".to_string(),
        Value::native_fn("eprintln", |args| {
            let out: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            eprintln!("{}", out.join(" "));
            Ok(Value::Nil)
        }),
    );

    // -------------------------------------------------------------
    // 2. Type Checking & Introspection
    // -------------------------------------------------------------
    globals.insert(
        "type_of".to_string(),
        Value::native_fn("type_of", |args| {
            if args.is_empty() {
                return Ok(Value::string("nil"));
            }
            let t = match &args[0] {
                Value::Nil => "nil",
                Value::Bool(_) => "bool",
                Value::Int(_) => "int",
                Value::Float(_) => "float",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Map(_) => "map",
                Value::Closure(_)
                | Value::NativeFn(_)
                | Value::Intrinsic(_)
                | Value::Async { .. } => "function",
                Value::Task(_) => "task",
                Value::Iterator(_) => "iterator",
                // 内部值：不应出现在用户可见路径，降级为底层类型名
                Value::Cell(cell) => match &*cell.read().unwrap() {
                    Value::Bool(_) => "bool",
                    Value::Int(_) => "int",
                    Value::Float(_) => "float",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Map(_) => "map",
                    Value::Task(_) => "task",
                    Value::Iterator(_) => "iterator",
                    _ => "function",
                },
            };
            Ok(Value::string(t))
        }),
    );

    globals.insert(
        "is_nil".to_string(),
        Value::native_fn("is_nil", |args| {
            Ok(Value::Bool(args.first().is_none_or(|v| v.is_nil())))
        }),
    );

    globals.insert(
        "is_num".to_string(),
        Value::native_fn("is_num", |args| {
            let is_num = matches!(args.first(), Some(Value::Int(_) | Value::Float(_)));
            Ok(Value::Bool(is_num))
        }),
    );

    globals.insert(
        "is_str".to_string(),
        Value::native_fn("is_str", |args| {
            let is_str = matches!(args.first(), Some(Value::String(_)));
            Ok(Value::Bool(is_str))
        }),
    );

    globals.insert(
        "is_arr".to_string(),
        Value::native_fn("is_arr", |args| {
            let is_arr = matches!(args.first(), Some(Value::Array(_)));
            Ok(Value::Bool(is_arr))
        }),
    );

    globals.insert(
        "is_map".to_string(),
        Value::native_fn("is_map", |args| {
            let is_map = matches!(args.first(), Some(Value::Map(_)));
            Ok(Value::Bool(is_map))
        }),
    );

    globals.insert(
        "is_fn".to_string(),
        Value::native_fn("is_fn", |args| {
            let is_fn = matches!(
                args.first(),
                Some(Value::Closure(_) | Value::NativeFn(_) | Value::Intrinsic(_))
            );
            Ok(Value::Bool(is_fn))
        }),
    );

    globals.insert(
        "is_task".to_string(),
        Value::native_fn("is_task", |args| {
            let is_task = matches!(args.first(), Some(Value::Task(_)));
            Ok(Value::Bool(is_task))
        }),
    );

    globals.insert(
        "is_iterator".to_string(),
        Value::native_fn("is_iterator", |args| {
            let is_iterator = matches!(args.first(), Some(Value::Iterator(_)));
            Ok(Value::Bool(is_iterator))
        }),
    );

    // -------------------------------------------------------------
    // 3. Type Conversion
    // -------------------------------------------------------------
    globals.insert(
        "to_int".to_string(),
        Value::native_fn("to_int", |args| {
            if let Some(arg) = args.first() {
                match arg {
                    Value::Int(n) => Ok(Value::Int(*n)),
                    Value::Float(f) => Ok(Value::Int(*f as i64)),
                    Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                    Value::String(s) => {
                        let trimmed = s.trim();
                        if let Ok(i) = trimmed.parse::<i64>() {
                            Ok(Value::Int(i))
                        } else if let Ok(f) = trimmed.parse::<f64>() {
                            Ok(Value::Int(f as i64))
                        } else {
                            Ok(Value::Nil)
                        }
                    }
                    _ => Ok(Value::Nil),
                }
            } else {
                Ok(Value::Nil)
            }
        }),
    );

    globals.insert(
        "to_float".to_string(),
        Value::native_fn("to_float", |args| {
            if let Some(arg) = args.first() {
                match arg {
                    Value::Float(f) => Ok(Value::Float(*f)),
                    Value::Int(n) => Ok(Value::Float(*n as f64)),
                    Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
                    Value::String(s) => {
                        if let Ok(f) = s.trim().parse::<f64>() {
                            Ok(Value::Float(f))
                        } else {
                            Ok(Value::Nil)
                        }
                    }
                    _ => Ok(Value::Nil),
                }
            } else {
                Ok(Value::Nil)
            }
        }),
    );

    globals.insert(
        "to_str".to_string(),
        Value::native_fn("to_str", |args| {
            if let Some(arg) = args.first() {
                Ok(Value::string(arg.to_string()))
            } else {
                Ok(Value::string(""))
            }
        }),
    );

    globals.insert(
        "to_bool".to_string(),
        Value::native_fn("to_bool", |args| {
            let b = args.first().is_some_and(|a| a.is_truthy());
            Ok(Value::Bool(b))
        }),
    );

    // -------------------------------------------------------------
    // 4. Assertions & Testing
    // -------------------------------------------------------------
    globals.insert(
        "assert".to_string(),
        Value::native_fn("assert", |args| {
            let cond = args.first().is_some_and(|a| a.is_truthy());
            if !cond {
                let msg = args
                    .get(1)
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "Assertion failed".into());
                return Err(msg);
            }
            Ok(Value::Bool(true))
        }),
    );

    globals.insert(
        "assert_eq".to_string(),
        Value::native_fn("assert_eq", |args| {
            if args.len() < 2 {
                return Err("assert_eq requires 2 arguments".into());
            }
            if args[0] != args[1] {
                let msg = args.get(2).map(|m| m.to_string()).unwrap_or_else(|| {
                    format!("assert_eq failed: left={:?}, right={:?}", args[0], args[1])
                });
                return Err(msg);
            }
            Ok(Value::Bool(true))
        }),
    );

    // -------------------------------------------------------------
    // 5. Math Functions
    // -------------------------------------------------------------
    globals.insert(
        "abs".to_string(),
        Value::native_fn("abs", |args| match args.first() {
            Some(Value::Int(n)) => n
                .checked_abs()
                .map(Value::Int)
                .ok_or_else(|| "abs: integer overflow (i64::MIN)".to_string()),
            Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
            _ => Ok(Value::Int(0)),
        }),
    );

    globals.insert(
        "min".to_string(),
        Value::native_fn("min", |args| {
            if args.is_empty() {
                return Ok(Value::Nil);
            }
            // Support passing array as single argument: min([1, 2, 3])
            let items: Vec<Value> = if args.len() == 1 {
                if let Value::Array(arr) = &args[0] {
                    arr.read().unwrap().clone()
                } else {
                    return Ok(args[0].clone());
                }
            } else {
                args.to_vec()
            };

            if items.is_empty() {
                return Ok(Value::Nil);
            }
            let mut min_val = items[0].clone();
            for it in items.iter().skip(1) {
                if let (Some(a), Some(b)) = (it.as_float(), min_val.as_float()) {
                    if a < b {
                        min_val = it.clone();
                    }
                }
            }
            Ok(min_val)
        }),
    );

    globals.insert(
        "max".to_string(),
        Value::native_fn("max", |args| {
            if args.is_empty() {
                return Ok(Value::Nil);
            }
            let items: Vec<Value> = if args.len() == 1 {
                if let Value::Array(arr) = &args[0] {
                    arr.read().unwrap().clone()
                } else {
                    return Ok(args[0].clone());
                }
            } else {
                args.to_vec()
            };

            if items.is_empty() {
                return Ok(Value::Nil);
            }
            let mut max_val = items[0].clone();
            for it in items.iter().skip(1) {
                if let (Some(a), Some(b)) = (it.as_float(), max_val.as_float()) {
                    if a > b {
                        max_val = it.clone();
                    }
                }
            }
            Ok(max_val)
        }),
    );

    globals.insert(
        "clamp".to_string(),
        Value::native_fn("clamp", |args| {
            if args.len() < 3 {
                return Err("clamp requires (val, min, max)".into());
            }
            let val = args[0].as_float().ok_or("clamp: val must be number")?;
            let min = args[1].as_float().ok_or("clamp: min must be number")?;
            let max = args[2].as_float().ok_or("clamp: max must be number")?;

            let res = val.max(min).min(max);
            if matches!(args[0], Value::Int(_)) {
                Ok(Value::Int(res as i64))
            } else {
                Ok(Value::Float(res))
            }
        }),
    );

    globals.insert(
        "round".to_string(),
        Value::native_fn("round", |args| {
            if let Some(arg) = args.first() {
                if let Some(f) = arg.as_float() {
                    return Ok(Value::Int(f.round() as i64));
                }
            }
            Ok(Value::Int(0))
        }),
    );

    globals.insert(
        "floor".to_string(),
        Value::native_fn("floor", |args| {
            if let Some(arg) = args.first() {
                if let Some(f) = arg.as_float() {
                    return Ok(Value::Int(f.floor() as i64));
                }
            }
            Ok(Value::Int(0))
        }),
    );

    globals.insert(
        "ceil".to_string(),
        Value::native_fn("ceil", |args| {
            if let Some(arg) = args.first() {
                if let Some(f) = arg.as_float() {
                    return Ok(Value::Int(f.ceil() as i64));
                }
            }
            Ok(Value::Int(0))
        }),
    );

    // -------------------------------------------------------------
    // 6. Range & Collections
    // -------------------------------------------------------------
    globals.insert(
        "len".to_string(),
        Value::native_fn("len", |args| {
            if args.is_empty() {
                return Ok(Value::Int(0));
            }
            match &args[0] {
                Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::Array(arr) => Ok(Value::Int(arr.read().unwrap().len() as i64)),
                Value::Map(m) => Ok(Value::Int(m.read().unwrap().len() as i64)),
                _ => Ok(Value::Int(0)),
            }
        }),
    );

    globals.insert(
        "range".to_string(),
        Value::native_fn("range", |args| {
            if args.is_empty() {
                return Ok(Value::iterator(IteratorState::Range {
                    current: 0,
                    end: 0,
                    step: 1,
                }));
            }
            let (start, end, step) = if args.len() == 1 {
                (0, args[0].as_int().unwrap_or(0), 1)
            } else if args.len() == 2 {
                (
                    args[0].as_int().unwrap_or(0),
                    args[1].as_int().unwrap_or(0),
                    1,
                )
            } else {
                (
                    args[0].as_int().unwrap_or(0),
                    args[1].as_int().unwrap_or(0),
                    args[2].as_int().unwrap_or(1),
                )
            };
            Ok(Value::iterator(IteratorState::Range {
                current: start,
                end,
                step,
            }))
        }),
    );

    globals.insert(
        "push".to_string(),
        Value::native_fn("push", |args| {
            if args.len() < 2 {
                return Err("push expects (array, value)".to_string());
            }
            if let Value::Array(arr) = &args[0] {
                arr.write().unwrap().push(args[1].clone());
                Ok(Value::Nil)
            } else {
                Err("push called on non-array".to_string())
            }
        }),
    );

    // -------------------------------------------------------------
    // 7. Async Primitives（M4：由 VM 调度器实现，可挂起）
    //    M5：`sleep_ms` 会占用宿主墙钟，因此在沙箱中被 `allow_sleep` 控制
    // -------------------------------------------------------------
    let async_op = |kind| Value::Async {
        kind,
        receiver: None,
    };
    if sandbox.allow_sleep {
        globals.insert("sleep_ms".to_string(), async_op(AsyncKind::Sleep));
    }
    globals.insert("yield_now".to_string(), async_op(AsyncKind::YieldNow));
    globals.insert("await_all".to_string(), async_op(AsyncKind::AwaitAll));
    globals.insert("await_any".to_string(), async_op(AsyncKind::AwaitAny));
    globals.insert("timeout".to_string(), async_op(AsyncKind::Timeout));
    // 旧名为 await_race，语义等同 await_any
    globals.insert("await_race".to_string(), async_op(AsyncKind::AwaitAny));
}

/// 注册迭代器适配器（T2.1）为全局函数，使管道形式可用：
/// `xs |> filter(f) |> map(g) |> take(3) |> collect()`。
///
/// 方法形式（`xs.filter(f)`）由 VM 的字段派发绑定接收者，见 `Vm::invoke_adapter`。
/// 这些名字会覆盖 `register_core_globals` 中同名的原生实现（如旧的 `sum`）。
pub fn register_iterator_adapters(globals: &mut HashMap<String, Value>) {
    for kind in AdapterKind::all() {
        globals.insert(kind.name().to_string(), Value::intrinsic(*kind));
    }
}
