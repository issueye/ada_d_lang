//! 宿主 panic 兜底（开发计划 T1.7）。
//!
//! 契约：宿主注册的原生回调/自定义模块一旦 panic，**不得**把 panic 穿透到调用方；
//! 引擎把它转成 `VmError::HostPanic`，脚本求值失败但进程与 `Engine` 的调用方都活着。
//!
//! 注意：panic 发生在宿主代码里，可能已经污染 `RwLock`（中毒）。因此收到该错误后
//! 应当**丢弃并重建** `Engine`——测试只断言"不崩溃 + 错误种类"，不保证引擎可继续使用。

use ada_engine::{Engine, Error, VmError};
use ada_vm::Value;

#[test]
fn panicking_native_function_becomes_an_error() {
    let mut engine = Engine::new();
    engine.register_fn("boom", |_args| panic!("host bug"));

    let err = engine
        .eval_diagnostic("boom()")
        .expect_err("a panicking host callback must surface as an error");
    let message = err.error.to_string();
    assert!(
        message.contains("Host function panicked"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("host bug"),
        "the panic message should be preserved: {message}"
    );
    assert!(matches!(err.error, Error::Vm(VmError::HostPanic { .. })));
    // 位置仍指向调用点（T1.8 与 T1.7 协同）
    assert_eq!(err.position.map(|p| p.line), Some(1));
}

#[test]
fn panicking_callback_inside_an_adapter_is_contained() {
    let mut engine = Engine::new();
    engine.register_fn("explode", |_args| -> Result<Value, String> {
        panic!("adapter host bug")
    });

    let err = engine
        .eval_diagnostic("let xs = [1, 2]\nlet ys = xs.map(x => explode(x))\nys")
        .expect_err("panic inside an adapter callback must not escape");
    assert!(
        matches!(err.error, Error::Vm(VmError::HostPanic { .. })),
        "got {err:?}"
    );
    assert_eq!(err.position.map(|p| p.line), Some(2));
}

#[test]
fn panicking_custom_module_function_is_contained() {
    struct Boom;
    impl ada_engine::NativeModule for Boom {
        fn name(&self) -> &str {
            "boommod"
        }
        fn functions(&self) -> Vec<(String, ada_engine::NativeCallback)> {
            vec![(
                "go".to_string(),
                std::sync::Arc::new(|_args: &[Value]| -> Result<Value, String> {
                    panic!("module host bug")
                }),
            )]
        }
    }

    let mut engine = Engine::new();
    engine.register_module(Boom).expect("module registration");

    let err = engine
        .eval_diagnostic("boommod.go()")
        .expect_err("panic in a custom module must not escape");
    let message = err.error.to_string();
    assert!(message.contains("Host function panicked"), "got {message}");
    assert!(message.contains("module host bug"), "got {message}");
}

#[test]
fn panic_inside_a_spawned_task_is_contained() {
    let mut engine = Engine::new();
    engine.register_fn("boom", |_args| panic!("task host bug"));

    // 任务体内调用会 panic 的宿主函数，并在主任务里 await 它
    let err = engine
        .eval_diagnostic("fn work() {\n    boom()\n}\nlet t = spawn work()\nawait t")
        .expect_err("a panic inside a task must not escape the engine");
    let message = err.error.to_string();
    assert!(
        message.contains("Host function panicked"),
        "unexpected error: {message}"
    );
    assert!(message.contains("task host bug"), "got {message}");
    // 展开不会清空帧栈，因此仍能给出任务内的失败位置
    assert_eq!(err.position.map(|p| p.line), Some(2));
}

#[test]
fn normal_native_errors_are_unaffected() {
    // 对照组：返回 Err 的回调仍然只产生普通 NativeError（不需要重建引擎）
    let mut engine = Engine::new();
    engine.register_fn("fail", |_args| Err("business error".to_string()));

    let err = engine.eval_diagnostic("fail()").expect_err("should fail");
    match &err.error {
        Error::Vm(VmError::NativeError(message)) => assert_eq!(message, "business error"),
        other => panic!("expected NativeError, got {other:?}"),
    }

    // 同一引擎继续可用
    let value = engine
        .eval_diagnostic("1 + 1")
        .expect("engine still usable");
    assert_eq!(value.to_string(), "2");
}
