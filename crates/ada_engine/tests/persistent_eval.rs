//! 多轮求值（REPL 语义）与燃料生命周期测试（开发计划 T2.4 / T2.6 / T2.7）。

use ada_engine::{Engine, Error, Value, VmError};

#[test]
fn top_level_bindings_persist_across_evaluations() {
    let mut engine = Engine::new();
    let first: i64 = engine.eval("let a = 41").unwrap();
    assert_eq!(first, 41);

    // T2.4：下一轮求值仍能看到上一轮的顶层绑定
    let second: i64 = engine.eval("a + 1").unwrap();
    assert_eq!(second, 42);

    let third: i64 = engine.eval("a = a + 1\na").unwrap();
    assert_eq!(third, 42);
    let fourth: i64 = engine.eval("a").unwrap();
    assert_eq!(fourth, 42);
}

#[test]
fn functions_defined_in_one_evaluation_are_callable_in_the_next() {
    let mut engine = Engine::new();
    engine
        .eval::<Value>("fn triple(x) = x * 3\ntriple(1)")
        .unwrap();
    let value: i64 = engine.eval("triple(14)").unwrap();
    assert_eq!(value, 42);
}

#[test]
fn native_modules_and_registered_functions_persist() {
    let mut engine = Engine::new();
    engine.register_fn("host_add", |args: &[Value]| {
        Ok(Value::Int(
            args[0].as_int().unwrap_or(0) + args[1].as_int().unwrap_or(0),
        ))
    });

    // 模块与宿主函数都可跨轮使用
    let a: i64 = engine.eval("json.parse(\"[1, 2, 3]\").len").unwrap();
    assert_eq!(a, 3);
    let b: i64 = engine.eval("host_add(40, 2)").unwrap();
    assert_eq!(b, 42);
}

#[test]
fn a_variable_shadowed_in_a_function_body_does_not_leak_to_globals() {
    // T2.4 的实现细节：只有模块顶层的绑定写全局；函数体内的局部变量不外泄
    let mut engine = Engine::new();
    engine
        .eval::<Value>("fn f() { let secret = 1\nsecret }\nf()")
        .unwrap();
    let err = engine.eval::<Value>("secret").unwrap_err();
    assert!(
        matches!(err, Error::Vm(VmError::UndefinedGlobal(ref name)) if name == "secret"),
        "expected UndefinedGlobal(secret), got {err:?}"
    );
}

#[test]
fn fuel_is_reloaded_on_every_evaluation() {
    let mut engine = Engine::new();
    engine.set_fuel(Some(50));

    let err = engine.eval::<Value>("while true {}").unwrap_err();
    assert!(matches!(err, Error::Vm(VmError::OutOfFuel)));

    // T2.6：同一次会话中后续求值必须继续可用
    let value: i64 = engine.eval("6 * 7").unwrap();
    assert_eq!(value, 42);
}

#[test]
fn remaining_fuel_is_observable_and_resettable() {
    let mut engine = Engine::new();
    engine.set_fuel(Some(10_000));
    engine
        .eval::<Value>("let xs = [1, 2, 3]\nxs |> sum()")
        .unwrap();

    let remaining = engine.remaining_fuel().expect("fuel should be tracked");
    assert!(remaining < 10_000, "fuel should have been consumed");
    assert_eq!(engine.get_fuel(), Some(10_000), "budget stays configured");

    engine.reset_fuel();
    assert_eq!(engine.remaining_fuel(), Some(10_000));
}

#[test]
fn without_fuel_no_limit_is_enforced() {
    let mut engine = Engine::new();
    assert_eq!(engine.get_fuel(), None);
    assert_eq!(engine.remaining_fuel(), None);
    let value: i64 = engine
        .eval("let s = 0\nfor i in range(0, 200) { s += i }\ns")
        .unwrap();
    assert_eq!(value, 19900);
}
