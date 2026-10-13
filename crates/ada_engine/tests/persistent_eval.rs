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

    // T8.3：`let` 绑定跨轮次同样不可改写（此前会静默写入全局表）
    let rejected = engine.eval::<Value>("a = a + 1").unwrap_err();
    assert!(
        matches!(&rejected, Error::Vm(VmError::TypeError(msg))
            if msg.contains("immutable variable 'a'")),
        "expected an immutability error, got {rejected:?}"
    );
    assert_eq!(
        engine.eval::<i64>("a").unwrap(),
        41,
        "value must be unchanged"
    );

    // `mut` 声明则允许跨轮写入
    engine.eval::<Value>("mut total = 1").unwrap();
    assert_eq!(engine.eval::<i64>("total += 41\ntotal").unwrap(), 42);
}

#[test]
fn immutability_is_enforced_within_and_across_evaluations() {
    let mut engine = Engine::new();
    // 同一程序内：`let` 绑定不可写（T3.1，编译期报错）
    let err = engine.eval::<Value>("let b = 1\nb = 2").unwrap_err();
    assert!(matches!(err, Error::Compile(_)), "got {err:?}");

    // 跨轮求值（T8.3，缺口 G-L17 已修复）：上一轮的 `let` 落到全局表时带上了
    // 不可变标记，因此再赋值报运行时错误而不是静默改写
    engine.eval::<Value>("let c = 1").unwrap();
    let rejected = engine.eval::<Value>("c = 5").unwrap_err();
    assert!(
        matches!(&rejected, Error::Vm(VmError::TypeError(msg))
            if msg.contains("declared with `let` in an earlier evaluation")),
        "expected a cross-evaluation immutability error, got {rejected:?}"
    );
    assert_eq!(engine.eval::<i64>("c").unwrap(), 1);

    // 重新用 `mut` 声明可以夺回可写性（同名 `let` 声明则继续锁定）
    engine.eval::<Value>("mut c = 10").unwrap();
    assert_eq!(engine.eval::<i64>("c = 11\nc").unwrap(), 11);
}

#[test]
fn builtin_and_module_names_cannot_be_overwritten_by_assignment() {
    let mut engine = Engine::new();
    // D12：模块不再预注册（`json` 在 import 前是自由名）；只有核心内建函数受保护
    for source in ["len = 1", "println = 1"] {
        let err = engine.eval::<Value>(source).unwrap_err();
        assert!(
            matches!(&err, Error::Vm(VmError::TypeError(msg))
                if msg.contains("built-in or host-injected")),
            "{source}: expected a builtin-protection error, got {err:?}"
        );
    }
    // 模块仍然可用
    assert_eq!(
        engine
            .eval::<Value>("import json from \"std:json\"\njson.valid(\"{}\")")
            .unwrap(),
        Value::Bool(true)
    );

    // **声明**则可以接管内建名：`mut sum = 0` 是 AI 规格里的常见写法
    assert_eq!(
        engine
            .eval::<i64>("mut sum = 0\nfor i in range(1, 5) { sum += i }\nsum")
            .unwrap(),
        10
    );
    // 接管之后，这个名字就归脚本了（跨轮次也可写）
    assert_eq!(engine.eval::<i64>("sum = 100\nsum").unwrap(), 100);

    // 宿主注入的全局同样受保护，但宿主自身可以再次注入
    engine.register_fn("host_fn", |_| Ok(Value::Int(7)));
    assert_eq!(engine.eval::<i64>("host_fn()").unwrap(), 7);
    let err = engine.eval::<Value>("host_fn = 1").unwrap_err();
    assert!(
        matches!(&err, Error::Vm(VmError::TypeError(msg))
        if msg.contains("built-in or host-injected")),
        "got {err:?}"
    );
    engine.set_global("host_fn", Value::Int(9));
    assert_eq!(
        engine.eval::<i64>("host_fn").unwrap(),
        9,
        "宿主再次注入应当生效"
    );
    // 注入之后脚本依然不能改写它
    assert!(engine.eval::<Value>("host_fn = 1").is_err());
}

#[test]
fn function_names_are_immutable_bindings() {
    let mut engine = Engine::new();
    // 函数名与 `let` 一样是不可变绑定（同程序内由编译器拦下）
    let err = engine.eval::<Value>("fn f() = 1\nf = 2").unwrap_err();
    assert!(matches!(err, Error::Compile(_)), "got {err:?}");

    // 重新声明是允许的（遮蔽，不报错）
    assert_eq!(
        engine.eval::<i64>("fn g() = 1\nfn g() = 2\ng()").unwrap(),
        2
    );
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
    let a: i64 = engine
        .eval("import json from \"std:json\"\njson.parse(\"[1, 2, 3]\").len")
        .unwrap();
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
        .eval("mut s = 0\nfor i in range(0, 200) { s += i }\ns")
        .unwrap();
    assert_eq!(value, 19900);
}
