//! 迭代器适配器与显式迭代协议测试（开发计划 T2.1 / T2.3）。

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

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

// ------------------------------------------------------- T2.1 计划验收用例

#[test]
fn plan_acceptance_pipeline_returns_165() {
    let code = r#"
        range(1, 1000)
            |> filter(x => x % 2 != 0)
            |> map(x => x * x)
            |> take(5)
            |> sum()
    "#;
    assert_eq!(ok(code), Value::Int(165));
}

// ------------------------------------------------------------- 方法形式

#[test]
fn method_form_adapters_work_on_arrays() {
    assert_eq!(ints("[1, 2, 3].map(x => x * 2).collect()"), vec![2, 4, 6]);
    assert_eq!(
        ints("[1, 2, 3, 4].filter(x => x > 2).collect()"),
        vec![3, 4]
    );
    assert_eq!(ints("[1, 2, 3, 4].take(2).collect()"), vec![1, 2]);
    assert_eq!(ints("[1, 2, 3, 4].skip(2).collect()"), vec![3, 4]);
    assert_eq!(ints("[1, 3, 2].rev().collect()"), vec![2, 3, 1]);
    assert_eq!(ok("[1, 2, 3].count()"), Value::Int(3));
    assert_eq!(ok("[1, 2, 3].sum()"), Value::Int(6));
}

#[test]
fn method_form_adapters_work_on_range_iterators() {
    assert_eq!(ints("range(1, 4).collect()"), vec![1, 2, 3]);
    assert_eq!(ok("range(1, 10).count()"), Value::Int(9));
    assert_eq!(ints("range(0, 10).skip(7).collect()"), vec![7, 8, 9]);
    assert_eq!(
        ints("range(0, 10).filter(x => x % 3 == 0).collect()"),
        vec![0, 3, 6, 9]
    );
    assert_eq!(ok("range(1, 5).sum()"), Value::Int(10));
}

#[test]
fn method_form_adapters_work_on_strings_and_maps() {
    // 字符串按字符迭代
    assert_eq!(
        ints("\"hello\".map(ch => 1).collect()"),
        vec![1, 1, 1, 1, 1]
    );
    assert_eq!(ok("\"abc\".rev().join(\"\")"), Value::string("cba"));
    // map 按 [key, value] 迭代
    let entries = ok("{ a: 1, b: 2 }.enumerate().count()");
    assert_eq!(entries, Value::Int(2));
    assert_eq!(ok("{ a: 1, b: 2 }.map(kv => kv[1]).sum()"), Value::Int(3));
}

// ------------------------------------------------------------- 全局/管道形式

#[test]
fn global_form_adapters_work_with_pipes_and_direct_calls() {
    assert_eq!(ints("map([1, 2, 3], x => x + 1)"), vec![2, 3, 4]);
    assert_eq!(ints("[1, 2, 3] |> map(x => x + 1)"), vec![2, 3, 4]);
    assert_eq!(ints("filter([1, 2, 3, 4], x => x % 2 == 0)"), vec![2, 4]);
    assert_eq!(ints("take(range(0, 100), 3)"), vec![0, 1, 2]);
    assert_eq!(ok("count([1, 2, 3])"), Value::Int(3));
    assert_eq!(ok("join([\"a\", \"b\"], \"-\")"), Value::string("a-b"));
    assert_eq!(ok("sum([1, 2, 3])"), Value::Int(6));
}

#[test]
fn adapters_chain_and_compose() {
    let code = r#"
        "hello world".split(" ")
            |> map(w => w.len)
            |> filter(n => n > 4)
            |> collect()
    "#;
    assert_eq!(ints(code), vec![5, 5]);
}

// --------------------------------------------------------- find / any / all

#[test]
fn find_any_all_semantics() {
    assert_eq!(ok("[1, 2, 3, 4].find(x => x > 2)"), Value::Int(3));
    assert_eq!(ok("[1, 2].find(x => x > 9)"), Value::Nil);
    assert_eq!(ok("[1, 2, 3].any(x => x > 2)"), Value::Bool(true));
    assert_eq!(ok("[1, 2, 3].any(x => x > 9)"), Value::Bool(false));
    assert_eq!(ok("[1, 2, 3].all(x => x > 0)"), Value::Bool(true));
    assert_eq!(ok("[1, 2, 3].all(x => x > 1)"), Value::Bool(false));
    // 空集合的边界：any 为 false，all 为 true
    assert_eq!(ok("[].any(x => true)"), Value::Bool(false));
    assert_eq!(ok("[].all(x => false)"), Value::Bool(true));
}

#[test]
fn adapters_pass_metadata_through_callback_arity_errors() {
    // 回调参数个数不匹配 → 干净错误（不是 panic）
    let err = run("[1, 2].map((a, b) => a)").unwrap_err();
    assert!(
        matches!(
            err,
            VmError::ArityMismatch {
                expected: 2,
                got: 1
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn adapter_arity_and_type_errors_are_clean() {
    // 缺少回调
    let err = run("map([1, 2])").unwrap_err();
    assert!(matches!(err, VmError::NativeError(_)), "got {err:?}");
    // 非可迭代接收者：字段派发不绑定适配器，调用时报错
    let err = run("true.filter(x => x)").unwrap_err();
    assert!(matches!(err, VmError::NotCallable(_)), "got {err:?}");
}

// --------------------------------------------------------- T2.3 迭代协议

#[test]
fn explicit_iter_and_next_protocol() {
    assert_eq!(
        ok("let it = range(5, 7).iter()\nit.next() + it.next()"),
        Value::Int(11)
    );
    // 耗尽后返回 nil
    assert_eq!(ok("let it = [1].iter()\nit.next()\nit.next()"), Value::Nil);
    assert_eq!(ok("type_of([1, 2].iter())"), Value::string("iterator"));
    // 显式迭代器同样支持适配器
    assert_eq!(
        ints("[1, 2, 3].iter().map(x => x * 10).collect()"),
        vec![10, 20, 30]
    );
}

#[test]
fn chars_with_idx_provides_index_char_pairs() {
    let pairs = ok("\"ab\".chars_with_idx()");
    if let Value::Array(items) = pairs {
        let items = items.read().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].to_string(), "[1, b]");
    } else {
        panic!("expected array");
    }
}

#[test]
fn iterator_adapters_consume_iterator_state() {
    // 与 .next() 一致：迭代器被适配器消费后即耗尽
    let code = r#"
        let it = [1, 2, 3].iter()
        let taken = it.take(2).collect()
        taken
    "#;
    assert_eq!(ints(code), vec![1, 2]);
}

// --------------------------------------------------- 派发优先级与类型内省

#[test]
fn map_keys_win_over_new_adapter_method_names() {
    // 适配器名字在 0.1.0 之前不存在，因此 map 上的同名键保持优先
    assert_eq!(ok("{ map: 7 }.map"), Value::Int(7));
    assert_eq!(ok("{ sum: 1, a: 2 }.sum"), Value::Int(1));
    // 既有原型方法名（len/keys/...）保持原有优先级
    assert_eq!(ok("{ len: 9 }.len"), Value::Int(1));
    assert_eq!(ok("{ keys: 1 }.keys()[0]"), Value::string("keys"));
}

#[test]
fn adapters_are_functions_for_introspection() {
    assert_eq!(ok("type_of(map)"), Value::string("function"));
    assert_eq!(ok("is_fn(filter)"), Value::Bool(true));
    assert_eq!(ok("type_of([1].map)"), Value::string("function"));
}

// -------------------------------------------- 顺带修复的 panic 路径回归

#[test]
fn sum_overflow_and_abs_overflow_are_clean_errors() {
    let err = run("[9223372036854775807, 1].sum()").unwrap_err();
    assert!(
        matches!(err, VmError::TypeError(ref m) if m.contains("overflow")),
        "got {err:?}"
    );

    let err = run("abs(-9223372036854775807 - 1)").unwrap_err();
    assert!(matches!(err, VmError::NativeError(_)), "got {err:?}");
}

#[test]
fn random_int_over_full_i64_range_does_not_panic() {
    let value = ok("math.random_int(-9223372036854775807, 9223372036854775807)");
    assert!(value.as_int().is_some(), "got {value:?}");
}
