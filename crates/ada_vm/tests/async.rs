//! 异步与并发运行时测试（开发计划 M4 / T4.1–T4.6）。
//!
//! 并发模型是**单线程协作式事件循环**（决策 D2）：任务只在挂起点让出，
//! `sleep_ms` 用真实时钟但由事件循环统一等待，因此并发睡眠只花最长时间。
//! 这里同时断言**正确性**与**可观测的并发收益**（否则"有调度器"就无从验证）。

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};
use std::time::{Duration, Instant};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lexing/parsing should succeed");
    let program = parser.parse_program().expect("parse should succeed");
    let proto = Compiler::compile_program(&program).expect("compilation should succeed");
    let mut vm = Vm::new();
    vm.execute(proto)
}

fn ok(source: &str) -> Value {
    run(source).unwrap_or_else(|e| panic!("expected success for {source:?}, got {e:?}"))
}

fn string(source: &str) -> String {
    match ok(source) {
        Value::String(s) => s.to_string(),
        other => panic!("expected string for {source:?}, got {other:?}"),
    }
}

fn ints(source: &str) -> Vec<i64> {
    match ok(source) {
        Value::Array(items) => items
            .read()
            .unwrap()
            .iter()
            .map(|v| v.as_int().expect("int"))
            .collect(),
        other => panic!("expected array for {source:?}, got {other:?}"),
    }
}

fn elapsed_ms(source: &str) -> (Value, u128) {
    let start = Instant::now();
    let value = ok(source);
    (value, start.elapsed().as_millis())
}

// ============================================================ T4.1 惰性任务

#[test]
fn spawn_does_not_execute_the_call_at_the_spawn_point() {
    let code = r#"
        mut log = ""
        fn work(tag) { log += tag
            1 }
        let t = spawn work("a")
        let before = log
        let value = t.await
        $"{before}|{log}|{value}"
    "#;
    // spawn 之后任务体尚未执行（before 为空），await 之后才执行
    assert_eq!(string(code), "|a|1");
}

#[test]
fn task_states_progress_from_queued_to_done() {
    let code = r#"
        fn work() = 7
        let t = spawn work()
        let early = t.state
        t.await
        $"{early}->{t.state}"
    "#;
    assert_eq!(string(code), "queued->done");
    // 完成后的任务反复 await 得到同一个值
    assert_eq!(
        ok("fn work() = 7\nlet t = spawn work()\nt.await\nt.await"),
        Value::Int(7)
    );
}

#[test]
fn spawn_of_a_plain_expression_is_already_done() {
    let code = r#"
        let t = spawn 50
        $"{t.state}:{t.await}"
    "#;
    assert_eq!(string(code), "done:50");
}

#[test]
fn await_on_a_non_task_passes_the_value_through() {
    assert_eq!(ok("(await 21)"), Value::Int(21));
}

#[test]
fn spawning_with_wrong_arity_fails_at_await_with_task_context() {
    let err = run("fn f(a, b) = a + b\nlet t = spawn f(1)\nt.await").unwrap_err();
    match err {
        VmError::TaskFailed { label, source } => {
            assert_eq!(label, "f");
            assert!(
                matches!(*source, VmError::ArityMismatch { .. }),
                "got {source:?}"
            );
        }
        other => panic!("expected TaskFailed, got {other:?}"),
    }
}

// ======================================================= T4.2/T4.3 真并发

#[test]
fn concurrent_sleeps_overlap_instead_of_adding_up() {
    let (_, concurrent) = elapsed_ms(
        r#"
        [spawn sleep_ms(200), spawn sleep_ms(200)].await_all()
        "#,
    );
    // 验收标准：2 个各 200ms 的任务总耗时 < 250ms（串行会是 ~400ms）
    assert!(
        concurrent < 250,
        "two 200ms tasks took {concurrent}ms; they are not overlapping"
    );
    // 同时必须真的等待了（而不是被跳过）
    assert!(
        concurrent >= 190,
        "expected a real 200ms wait, got {concurrent}ms"
    );
}

#[test]
fn many_concurrent_sleeps_still_cost_one_delay() {
    let (_, elapsed) = elapsed_ms(
        r#"
        [spawn sleep_ms(150), spawn sleep_ms(150), spawn sleep_ms(150), spawn sleep_ms(150)].await_all()
        "#,
    );
    assert!(elapsed < 300, "four 150ms tasks took {elapsed}ms");
}

#[test]
fn serial_sleeps_still_add_up() {
    let (_, serial) = elapsed_ms("sleep_ms(150)\nsleep_ms(150)");
    // 对照组：顺序睡眠确实是累加的（证明上面的重叠不是"根本没等"）
    assert!(serial >= 290, "two serial 150ms sleeps took {serial}ms");
}

#[test]
fn concurrent_results_are_collected_in_argument_order() {
    let code = r#"
        fn slow() { sleep_ms(60)
            "slow" }
        fn fast() { sleep_ms(10)
            "fast" }
        let a = spawn slow()
        let b = spawn fast()
        [a, b].await_all() |> join(",")
    "#;
    // await_all 按**实参顺序**收集（不是完成顺序）
    assert_eq!(string(code), "slow,fast");
}

#[test]
fn await_any_returns_the_task_that_settles_first() {
    let code = r#"
        fn slow() { sleep_ms(60)
            "slow" }
        fn fast() { sleep_ms(10)
            "fast" }
        let a = spawn slow()
        let b = spawn fast()
        [a, b].await_any()
    "#;
    assert_eq!(string(code), "fast");
    // await_race 是 await_any 的旧名
    let code = code.replace("await_any", "await_race");
    assert_eq!(string(&code), "fast");
}

#[test]
fn yield_now_interleaves_tasks_deterministically() {
    let code = r#"
        mut log = ""
        fn worker(tag) {
            for i in 2 { log += tag
                yield_now() }
            tag
        }
        let t1 = spawn worker("a")
        let t2 = spawn worker("b")
        [t1, t2].await_all()
        log
    "#;
    assert_eq!(string(code), "abab");
}

#[test]
fn the_same_concurrent_script_is_deterministic_across_runs() {
    let code = r#"
        mut log = ""
        fn worker(tag, delay) {
            for i in 2 { sleep_ms(delay)
                log += tag }
            tag
        }
        let t1 = spawn worker("a", 20)
        let t2 = spawn worker("b", 5)
        let all = [t1, t2].await_all()
        $"{log}|{all}"
    "#;
    let first = string(code);
    let second = string(code);
    assert_eq!(first, second, "scheduler ordering must be deterministic");
    assert_eq!(first, "bbaa|[a, b]");
}

#[test]
fn a_task_can_spawn_another_task() {
    let code = r#"
        fn inner(x) { sleep_ms(5)
            x * 2 }
        fn outer(x) {
            let t = spawn inner(x)
            t.await + 1
        }
        let top = spawn outer(10)
        top.await
    "#;
    assert_eq!(ok(code), Value::Int(21));
}

// ==================================================== T4.6 生命周期契约

#[test]
fn cancelling_a_queued_task_reports_cancelled() {
    let code = r#"
        fn slow() { sleep_ms(100)
            1 }
        let t = spawn slow()
        let cancelled = t.cancel()
        $"{cancelled}:{t.state}"
    "#;
    assert_eq!(string(code), "true:cancelled");

    match run("fn slow() { sleep_ms(100)\n    1 }\nlet t = spawn slow()\nt.cancel()\nt.await") {
        Err(VmError::TaskCancelled(label)) => assert_eq!(label, "slow"),
        other => panic!("expected TaskCancelled, got {other:?}"),
    }
}

#[test]
fn cancelling_a_finished_task_is_a_noop() {
    let code = r#"
        fn f() = 1
        let t = spawn f()
        t.await
        let second = t.cancel()
        $"{second}:{t.state}"
    "#;
    assert_eq!(string(code), "false:done");
}

#[test]
fn a_failed_task_stops_the_program_at_await() {
    let code = r#"
        fn boom() { 1 / 0 }
        let t = spawn boom()
        "unreachable: " + to_str(t.await)
    "#;
    match run(code) {
        Err(VmError::TaskFailed { label, source }) => {
            assert_eq!(label, "boom");
            assert!(matches!(*source, VmError::TypeError(_)), "got {source:?}");
        }
        other => panic!("expected TaskFailed, got {other:?}"),
    }
}

#[test]
fn a_failed_task_in_await_all_propagates() {
    match run("fn boom() = 1 / 0\n[spawn boom(), spawn 1].await_all()") {
        Err(VmError::TaskFailed { label, .. }) => assert_eq!(label, "boom"),
        other => panic!("expected TaskFailed, got {other:?}"),
    }
}

#[test]
fn awaiting_itself_is_reported_as_a_deadlock() {
    // 主任务与任务互相等待：调度器必须报死锁而不是挂起宿主
    let code = r#"
        fn self_wait() { me.await }
        let me = spawn self_wait()
        me.await
    "#;
    match run(code) {
        Err(VmError::TaskDeadlock) => {}
        other => panic!("expected TaskDeadlock, got {other:?}"),
    }
}

#[test]
fn unfinished_tasks_are_abandoned_when_the_script_ends() {
    let (value, elapsed) = elapsed_ms(
        r#"
        spawn sleep_ms(200)
        "done"
    "#,
    );
    // 主任务不等待后台任务：脚本立刻返回，不会被 200ms 的任务拖住
    assert_eq!(value, Value::string("done"));
    assert!(
        elapsed < 100,
        "script waited for an abandoned task ({elapsed}ms)"
    );
}

#[test]
fn tasks_do_not_leak_between_executions() {
    let mut vm = Vm::new();
    let run_once = |vm: &mut Vm, src: &str| -> Value {
        let mut parser = Parser::from_source(src).expect("parse input");
        let program = parser.parse_program().expect("program");
        let proto = Compiler::compile_program(&program).expect("compile");
        vm.execute(proto).expect("execute")
    };
    // 第一段脚本留下未完成的任务与定时器
    let first = run_once(&mut vm, "spawn sleep_ms(500)\n1");
    assert_eq!(first, Value::Int(1));
    // 第二段脚本不应看到上一段的任务，也不应被其定时器影响
    let (second, elapsed) = {
        let start = Instant::now();
        let value = run_once(&mut vm, "is_task(nil)");
        (value, start.elapsed().as_millis())
    };
    assert_eq!(second, Value::Bool(false));
    assert!(
        elapsed < 100,
        "previous script's timers leaked ({elapsed}ms)"
    );
}

#[test]
fn timeout_cancels_a_slow_task_and_returns_nil() {
    let code = r#"import time from "std:time"

        fn slow() { sleep_ms(400)
            "done" }
        let t = spawn slow()
        let start = time.now_ms()
        let value = t.timeout(50)
        $"{value}:{time.now_ms() - start}:{t.state}"
    "#;
    let result = string(code);
    let parts: Vec<&str> = result.split(':').collect();
    assert_eq!(parts[0], "nil", "timeout should yield nil");
    let elapsed: i64 = parts[1].parse().expect("elapsed");
    assert!(
        (40..200).contains(&elapsed),
        "timeout should fire near 50ms, took {elapsed}ms"
    );
    assert_eq!(parts[2], "cancelled");
}

#[test]
fn timeout_returns_the_value_when_the_task_finishes_in_time() {
    assert_eq!(
        string("fn quick() { sleep_ms(10)\n    \"done\" }\nlet t = spawn quick()\nt.timeout(300)"),
        "done"
    );
    // 全局形式 timeout(task, ms) 与方法形式等价
    assert_eq!(
        string("fn quick() { sleep_ms(5)\n    \"ok\" }\nlet t = spawn quick()\ntimeout(t, 300)"),
        "ok"
    );
    // 已结算的任务不需要挂起
    assert_eq!(ok("let t = spawn 7\nt.timeout(1000)"), Value::Int(7));
}

// ==================================================== 文档化的降级行为

#[test]
fn sleeping_inside_an_adapter_callback_blocks_but_still_works() {
    // 适配器回调内部是 Rust 栈帧，无法保存现场，因此 sleep_ms 退化为真实阻塞
    // （SPEC_ASYNC §4.3 记录该限制）；结果仍然正确。
    let start = Instant::now();
    let value = ok("[1, 2].map(x => { sleep_ms(5)\n    x * 10 })");
    let elapsed = start.elapsed();
    let items: Vec<i64> = match value {
        Value::Array(items) => items
            .read()
            .unwrap()
            .iter()
            .map(|v| v.as_int().expect("int"))
            .collect(),
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(items, vec![10, 20]);
    assert!(
        elapsed >= Duration::from_millis(8),
        "expected two blocking 5ms sleeps, took {elapsed:?}"
    );
}

#[test]
fn async_functions_and_await_syntax_still_work() {
    let code = r#"
        async fn fetch(x) = x * 2
        let t = spawn fetch(21)
        t.await
    "#;
    assert_eq!(ok(code), Value::Int(42));
    // 前缀 await 与后缀 .await 等价
    assert_eq!(ok("async fn f() = 5\nawait spawn f()"), Value::Int(5));
}

#[test]
fn await_all_of_an_empty_or_fully_settled_list_does_not_suspend() {
    assert_eq!(
        ints("mut out = []\nlet r = out.await_all()\nr"),
        Vec::<i64>::new()
    );
    // 全部已就绪时直接在原地完成，不需要调度
    let (value, elapsed) = elapsed_ms("[spawn 1, spawn 2, spawn 3].await_all()");
    let items: Vec<i64> = match value {
        Value::Array(items) => items
            .read()
            .unwrap()
            .iter()
            .map(|v| v.as_int().expect("int"))
            .collect(),
        other => panic!("expected array, got {other:?}"),
    };
    assert_eq!(items, vec![1, 2, 3]);
    assert!(elapsed < 50, "ready tasks should not suspend ({elapsed}ms)");
}

#[test]
fn is_task_and_type_of_report_tasks() {
    let code = r#"
        let t = spawn 1
        $"{is_task(t)}:{type_of(t)}:{is_task(1)}"
    "#;
    assert_eq!(string(code), "true:task:false");
}
