//! 沙箱与宿主安全测试集（开发计划 M5 / T5.1–T5.6）。
//!
//! 验证目标（M5 退出标准）：
//! 1. 默认 strict 下脚本**读不到**宿主环境/时间/系统信息；
//! 2. 脚本**无法**耗尽宿主内存（超限返回专用错误，而不是 OOM 或 panic）；
//! 3. 脚本**无法**长时间占用宿主（墙钟预算）；
//! 4. 任何违规输入都不得让宿主 panic。

use ada_engine::{Engine, Error, NativeCallback, NativeModule, Sandbox, Value, VmError};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn strict() -> Engine {
    Engine::new()
}

fn eval_err(engine: &mut Engine, source: &str) -> Error {
    engine
        .eval::<Value>(source)
        .err()
        .unwrap_or_else(|| panic!("expected an error for {source:?}"))
}

/// 内存超限可能来自 VM 指令（`MemoryLimitExceeded`）或原生原型方法
/// （`NativeError`，因为闭包无法构造 VmError）；两者都必须被明确报告。
fn assert_memory_error(err: Error, context: &str) {
    match err {
        Error::Vm(VmError::MemoryLimitExceeded { .. }) => {}
        Error::Vm(VmError::NativeError(msg)) if msg.contains("memory limit") => {}
        other => panic!("expected a memory limit error for {context}, got {other:?}"),
    }
}

// ==================================================== T5.1 能力分级

#[test]
fn strict_hides_every_host_capability() {
    let mut engine = strict();
    for (source, name) in [
        ("time.now_ms()", "time"),
        ("time.elapsed_ms(0)", "time"),
        ("sys.os", "sys"),
        ("sys.version", "sys"),
        ("env.get(\"PATH\")", "env"),
        ("env.has(\"PATH\")", "env"),
        ("sleep_ms(1)", "sleep_ms"),
    ] {
        match eval_err(&mut engine, source) {
            Error::Vm(VmError::CapabilityDenied {
                name: denied,
                profile,
            }) => {
                assert_eq!(denied, name, "for {source}");
                assert_eq!(profile, "strict");
            }
            other => panic!("expected CapabilityDenied for {source}, got {other:?}"),
        }
    }
    // 未定义名与"被沙箱拒绝"必须可区分
    assert!(matches!(
        eval_err(&mut engine, "definitely_not_defined"),
        Error::Vm(VmError::UndefinedGlobal(_))
    ));
}

#[test]
fn strict_still_allows_pure_computation() {
    let mut engine = strict();
    let value: i64 = engine
        .eval(
            r#"
            let xs = [1, 2, 3, 4, 5] |> filter(x => x % 2 == 1) |> map(x => x * x) |> collect()
            xs |> sum()
            "#,
        )
        .unwrap();
    assert_eq!(value, 35);

    let json: String = engine
        .eval("json.stringify(json.parse(\"[1, 2, 3]\"))")
        .unwrap();
    assert_eq!(json, "[1,2,3]");

    let math: i64 = engine.eval("to_int(math.sqrt(144.0))").unwrap();
    assert_eq!(math, 12);

    // 任务调度是纯计算能力，strict 下可用（没有 sleep_ms，但 yield_now/await 可用）
    let tasks: i64 = engine
        .eval("fn f(x) = x * 2\n[spawn f(1), spawn f(2)].await_all() |> sum()")
        .unwrap();
    assert_eq!(tasks, 6);
}

#[test]
fn sandbox_profile_is_reported() {
    assert_eq!(strict().sandbox().kind.name(), "strict");
    assert_eq!(
        Engine::with_sandbox(Sandbox::standard())
            .sandbox()
            .kind
            .name(),
        "standard"
    );
    assert_eq!(Engine::permissive().sandbox().kind.name(), "permissive");

    let sandbox = Sandbox::standard();
    assert!(sandbox.allows_global("time"));
    assert!(!Sandbox::strict().allows_global("time"));
    assert!(!Sandbox::strict().allows_global("sleep_ms"));
    // 纯计算模块在任何 profile 下都可用
    assert!(Sandbox::strict().allows_global("json"));
    assert!(Sandbox::strict().allows_global("println"));
}

#[test]
fn standard_profile_exposes_time_and_sys() {
    let mut engine = Engine::with_sandbox(Sandbox::standard());
    assert!(engine.eval::<i64>("time.now_ms()").unwrap() > 0);
    assert_eq!(
        engine.eval::<String>("sys.os").unwrap(),
        std::env::consts::OS
    );
    assert!(engine.eval::<i64>("sleep_ms(1)").unwrap_or(0) == 0);
}

// ==================================================== T5.2 env 白名单

#[test]
fn env_whitelist_allows_only_listed_keys_and_never_reads_the_host_for_others() {
    // PATH 在宿主上一定存在：用它证明"未授权键根本没读宿主环境"
    assert!(std::env::var("PATH").is_ok(), "test precondition");

    let sandbox = Sandbox::strict().with_env_keys(["PATH"]);
    let mut engine = Engine::with_sandbox(sandbox);

    // 白名单内的键：与宿主一致
    let path: String = engine.eval("env.get(\"PATH\")").unwrap();
    assert_eq!(path, std::env::var("PATH").unwrap());
    assert!(engine.eval::<bool>("env.has(\"PATH\")").unwrap());

    // 白名单外的键：即使宿主存在也只能拿到 nil / false
    assert_eq!(
        engine
            .eval::<Value>("env.get(\"PATH_EXTRA_UNLISTED\")")
            .unwrap(),
        Value::Nil
    );
    assert!(engine.eval::<bool>("env.has(\"PATH\")").unwrap());
    assert!(!engine.eval::<bool>("env.has(\"UNLISTED_KEY\")").unwrap());

    // 允许宿主给默认值
    assert_eq!(
        engine
            .eval::<String>("env.get(\"UNLISTED_KEY\", \"fallback\")")
            .unwrap(),
        "fallback"
    );

    // 白名单可被脚本自省
    assert_eq!(
        engine
            .eval::<String>("env.allowed() |> join(\",\")")
            .unwrap(),
        "PATH"
    );
}

#[test]
fn env_module_with_an_empty_whitelist_reads_nothing() {
    let sandbox = Sandbox::permissive().with_env_keys(Vec::<String>::new());
    let mut engine = Engine::with_sandbox(sandbox);
    assert!(std::env::var("PATH").is_ok());
    assert_eq!(
        engine.eval::<Value>("env.get(\"PATH\")").unwrap(),
        Value::Nil
    );
    assert_eq!(
        engine
            .eval::<String>("env.allowed() |> join(\",\")")
            .unwrap(),
        ""
    );
}

#[test]
fn env_can_be_removed_entirely() {
    let mut engine = Engine::with_sandbox(Sandbox::standard().without_env());
    assert!(matches!(
        eval_err(&mut engine, "env.get(\"PATH\")"),
        Error::Vm(VmError::CapabilityDenied { .. })
    ));
}

// ==================================================== T5.3 内存预算

#[test]
fn memory_limit_rejects_a_huge_iterable_before_allocating() {
    let sandbox = Sandbox::strict().with_memory_limit(Some(64 * 1024));
    let mut engine = Engine::with_sandbox(sandbox);
    let start = Instant::now();
    match eval_err(&mut engine, "range(0, 100000000) |> collect()") {
        Error::Vm(VmError::MemoryLimitExceeded { limit, .. }) => assert_eq!(limit, 64 * 1024),
        other => panic!("expected MemoryLimitExceeded, got {other:?}"),
    }
    // 预检查必须发生在分配之前，因此应当瞬间返回
    assert!(
        start.elapsed() < Duration::from_millis(200),
        "rejection took {:?}; it likely allocated first",
        start.elapsed()
    );
}

#[test]
fn memory_limit_rejects_container_growth_in_a_loop() {
    let sandbox = Sandbox::strict().with_memory_limit(Some(32 * 1024));
    let mut engine = Engine::with_sandbox(sandbox);
    let code = r#"
        mut acc = []
        for i in 100000 { acc.push(i) }
        acc.len
    "#;
    assert_memory_error(eval_err(&mut engine, code), "push loop");
}

#[test]
fn memory_limit_rejects_string_amplification() {
    let sandbox = Sandbox::strict().with_memory_limit(Some(32 * 1024));
    let mut engine = Engine::with_sandbox(sandbox);
    let code = r#"
        mut s = ""
        for i in 20000 { s += "0123456789" }
        s.len
    "#;
    assert_memory_error(eval_err(&mut engine, code), "string += loop");
}

#[test]
fn memory_limit_also_caps_json_input() {
    let sandbox = Sandbox::strict().with_memory_limit(Some(1024));
    let mut engine = Engine::with_sandbox(sandbox);
    let big = "1".repeat(4096);
    let source = format!("json.parse(\"{big}\")");
    match eval_err(&mut engine, &source) {
        Error::Vm(VmError::NativeError(msg)) => {
            assert!(msg.contains("memory budget"), "message: {msg}");
        }
        other => panic!("expected NativeError about the budget, got {other:?}"),
    }
}

#[test]
fn memory_accounting_is_reset_per_evaluation_and_does_not_poison_the_engine() {
    let sandbox = Sandbox::strict().with_memory_limit(Some(16 * 1024));
    let mut engine = Engine::with_sandbox(sandbox);
    assert!(matches!(
        eval_err(&mut engine, "range(0, 1000000) |> collect()"),
        Error::Vm(VmError::MemoryLimitExceeded { .. })
    ));
    // 上一次超限不应影响下一次求值
    assert_eq!(engine.eval::<i64>("1 + 1").unwrap(), 2);
    assert!(engine.memory_used() < 16 * 1024);
    // 同一份预算内正常的小规模分配仍然可用
    assert_eq!(
        engine
            .eval::<i64>("[1, 2, 3] |> map(x => x * 2) |> sum()")
            .unwrap(),
        12
    );
}

#[test]
fn generous_budget_allows_normal_work() {
    let sandbox = Sandbox::permissive();
    let mut engine = Engine::with_sandbox(sandbox);
    assert_eq!(
        engine
            .eval::<i64>("range(0, 10000) |> filter(x => x % 2 == 0) |> count()")
            .unwrap(),
        5000
    );
}

// ==================================================== T5.4 墙钟预算

#[test]
fn wall_clock_limit_interrupts_an_infinite_loop() {
    let sandbox = Sandbox::strict().with_wall_clock_limit(Some(200));
    let mut engine = Engine::with_sandbox(sandbox);
    let start = Instant::now();
    match eval_err(&mut engine, "while true {}") {
        Error::Vm(VmError::WallClockLimitExceeded { limit_ms }) => assert_eq!(limit_ms, 200),
        other => panic!("expected WallClockLimitExceeded, got {other:?}"),
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(150) && elapsed < Duration::from_secs(2),
        "wall-clock check fired at {elapsed:?}"
    );
}

#[test]
fn wall_clock_limit_also_covers_suspension_waits() {
    // 5 秒的 sleep 在 150ms 预算下必须被中断，而不是真的等 5 秒
    let sandbox = Sandbox::standard().with_wall_clock_limit(Some(150));
    let mut engine = Engine::with_sandbox(sandbox);
    let start = Instant::now();
    match eval_err(&mut engine, "sleep_ms(5000)") {
        Error::Vm(VmError::WallClockLimitExceeded { .. }) => {}
        other => panic!("expected WallClockLimitExceeded, got {other:?}"),
    }
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "suspension wait ignored the wall-clock budget ({:?})",
        start.elapsed()
    );
}

#[test]
fn fuel_and_wall_clock_are_independent() {
    // fuel 先耗尽 → OutOfFuel（不受墙钟影响）
    let sandbox = Sandbox::strict().with_fuel(Some(1000));
    let mut engine = Engine::with_sandbox(sandbox);
    assert!(matches!(
        eval_err(&mut engine, "while true {}"),
        Error::Vm(VmError::OutOfFuel)
    ));
}

#[test]
fn task_limit_rejects_task_storms() {
    let sandbox = Sandbox::standard().with_task_limit(Some(8));
    let mut engine = Engine::with_sandbox(sandbox);
    let code = r#"
        fn slow() { sleep_ms(1000) }
        mut tasks = []
        for i in 40 { tasks.push(spawn slow()) }
        tasks.len
    "#;
    match eval_err(&mut engine, code) {
        Error::Vm(VmError::TaskLimitExceeded { limit }) => assert_eq!(limit, 8),
        other => panic!("expected TaskLimitExceeded, got {other:?}"),
    }
}

// ==================================================== T5.5 宿主注入

#[test]
fn host_registered_functions_are_available_in_strict() {
    let mut engine = strict();
    engine.register_fn("host_secret", |args| {
        Ok(Value::Int(
            args.first().and_then(|a| a.as_int()).unwrap_or(0) * 2,
        ))
    });
    // 宿主显式注入的能力不受 profile 限制
    assert_eq!(engine.eval::<i64>("host_secret(21)").unwrap(), 42);
}

struct GeometryModule;

impl NativeModule for GeometryModule {
    fn name(&self) -> &str {
        "geometry"
    }

    fn functions(&self) -> Vec<(String, NativeCallback)> {
        vec![
            (
                "area".to_string(),
                Arc::new(|args: &[Value]| {
                    let w = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
                    let h = args.get(1).and_then(|a| a.as_float()).unwrap_or(0.0);
                    Ok(Value::Float(w * h))
                }),
            ),
            (
                "name".to_string(),
                Arc::new(|_| Ok(Value::string("geometry"))),
            ),
        ]
    }
}

#[test]
fn custom_native_module_is_callable_and_conflicts_are_rejected() {
    let mut engine = strict();
    engine.register_module(GeometryModule).unwrap();

    assert_eq!(engine.eval::<f64>("geometry.area(3.0, 4.0)").unwrap(), 12.0);
    assert_eq!(
        engine.eval::<String>("geometry.name()").unwrap(),
        "geometry"
    );
    // 模块是 map，因此也能整体传递与管道调用
    assert_eq!(
        engine
            .eval::<String>("geometry |> to_str |> type_of")
            .unwrap(),
        "string"
    );

    // 与已有全局同名 → 明确报错，避免静默替换能力
    match engine.register_module(GeometryModule) {
        Err(Error::ModuleConflict(name)) => assert_eq!(name, "geometry"),
        other => panic!("expected ModuleConflict, got {other:?}"),
    }
    // 也不能覆盖内置模块（否则脚本会以为自己在用标准库）
    struct MathShadow;
    impl NativeModule for MathShadow {
        fn name(&self) -> &str {
            "math"
        }
        fn functions(&self) -> Vec<(String, NativeCallback)> {
            vec![("sqrt".to_string(), Arc::new(|_| Ok(Value::Int(0))))]
        }
    }
    assert!(matches!(
        engine.register_module(MathShadow),
        Err(Error::ModuleConflict(_))
    ));
}

// ==================================================== 宿主健壮性

#[test]
fn hostile_scripts_return_errors_instead_of_panicking_the_host() {
    let sandbox = Sandbox::standard()
        .with_memory_limit(Some(256 * 1024))
        .with_wall_clock_limit(Some(300));
    let mut engine = Engine::with_sandbox(sandbox);

    let hostile = [
        "1 / 0",
        "1 % 0",
        "1 < \"a\"",
        "let a = [1]\na[999999999] = 1",
        "json.parse(\"{{{ not json\")",
        "let f = () => f()\nf()",
        "range(0, 100000000) |> collect()",
        "mut s = \"\"\nfor i in 100000 { s += \"x\" }",
        "match 1 { 2 => 3 }",
        "nil.field",
        "let m = {}\nfor i in 1000000 { m[to_str(i)] = i }",
    ];
    for source in hostile {
        // 必须返回 Err；绝不能 panic（panic 会让这个测试直接失败）
        assert!(
            engine.eval::<Value>(source).is_err(),
            "expected an error for hostile snippet: {source}"
        );
    }

    // 一连串敌意输入之后，引擎仍然可用
    assert_eq!(engine.eval::<i64>("40 + 2").unwrap(), 42);
}

#[test]
fn permissive_profile_preserves_pre_m5_behaviour() {
    // 兼容性检查：不设限时旧的宿主能力仍全部可用
    let mut engine = Engine::permissive();
    assert!(engine.eval::<i64>("time.now_ms()").unwrap() > 0);
    assert_eq!(
        engine.eval::<String>("sys.os").unwrap(),
        std::env::consts::OS
    );
    assert_eq!(engine.eval::<Value>("sleep_ms(1)").unwrap(), Value::Nil);
    assert_eq!(
        engine
            .eval::<i64>("range(0, 50000) |> filter(x => x % 2 == 0) |> count()")
            .unwrap(),
        25000
    );
}
