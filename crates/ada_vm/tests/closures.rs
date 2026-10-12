//! 闭包 upvalue 捕获测试（开发计划 T2.2）。
//!
//! 覆盖：只读捕获、可写捕获（Cell 共享）、逃逸闭包、多级传递捕获、
//! 每轮循环变量独立捕获、与迭代器适配器的组合，以及不泄漏内部 Cell。

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

// ------------------------------------------------------------ 计划验收用例

#[test]
fn plan_acceptance_captures_outer_parameter() {
    let code = r#"
        fn outer(a) {
            let g = x => x + a
            g(5)
        }
        outer(10)
    "#;
    assert_eq!(ok(code), Value::Int(15));
}

#[test]
fn multi_level_capture_through_a_non_referencing_middle_layer() {
    // 中间层 l2 自己不引用 x，只把它继续传给 l3
    let code = r#"
        fn l1() {
            let x = 10
            fn l2() {
                fn l3() = x + 5
                l3()
            }
            l2()
        }
        l1()
    "#;
    assert_eq!(ok(code), Value::Int(15));
}

// ------------------------------------------------------------ 可写捕获

#[test]
fn captured_variable_is_shared_and_writable() {
    let code = r#"
        mut count = 0
        let inc = () => { count += 1 }
        inc()
        inc()
        count
    "#;
    assert_eq!(ok(code), Value::Int(2));
}

#[test]
fn two_closures_share_the_same_slot() {
    let code = r#"
        mut n = 0
        let bump = () => { n += 1 }
        let read = () => n
        bump()
        bump()
        bump()
        read()
    "#;
    assert_eq!(ok(code), Value::Int(3));
}

#[test]
fn named_function_can_write_an_outer_local() {
    let code = r#"
        mut total = 0
        fn add(x) { total += x
            total }
        add(5)
        add(7)
        total
    "#;
    assert_eq!(ok(code), Value::Int(12));
}

#[test]
fn captured_parameter_is_writable_too() {
    let code = r#"
        fn make(a) {
            let bump = () => { a += 1
                a }
            bump()
            bump()
        }
        make(10)
    "#;
    assert_eq!(ok(code), Value::Int(12));
}

// ------------------------------------------------------------ 逃逸与嵌套

#[test]
fn closure_escapes_its_defining_scope() {
    let code = r#"
        fn make_adder(n) { x => x + n }
        let add5 = make_adder(5)
        let add10 = make_adder(10)
        add5(1) + add10(1)
    "#;
    assert_eq!(ok(code), Value::Int(17));
}

#[test]
fn nested_closures_capture_through_multiple_levels() {
    let code = r#"
        fn make() {
            let base = 100
            x => { let mid = x * 2
                () => base + mid }
        }
        let outer = make()
        let inner = outer(5)
        inner()
    "#;
    assert_eq!(ok(code), Value::Int(110));
}

#[test]
fn loop_variable_gets_a_fresh_slot_per_iteration() {
    let code = r#"
        let grabs = []
        for i in range(0, 3) {
            grabs.push((() => i))
        }
        [grabs[0](), grabs[1](), grabs[2]()]
    "#;
    assert_eq!(ints(code), vec![0, 1, 2]);
}

#[test]
fn inner_parameter_shadows_outer_captured_name() {
    let code = r#"
        fn f(a) {
            let g = a => a * 3
            g(2) + a
        }
        f(10)
    "#;
    assert_eq!(ok(code), Value::Int(16));
}

// ------------------------------------------------- 与迭代器适配器的组合

#[test]
fn adapters_can_use_captured_thresholds() {
    let code = r#"
        let threshold = 3
        [1, 2, 3, 4, 5] |> filter(x => x > threshold) |> collect()
    "#;
    assert_eq!(ints(code), vec![4, 5]);

    let code = r#"
        let factor = 3
        [1, 2, 3] |> map(x => x * factor) |> collect()
    "#;
    assert_eq!(ints(code), vec![3, 6, 9]);
}

#[test]
fn adapter_callbacks_can_accumulate_into_a_captured_slot() {
    let code = r#"
        mut total = 0
        [1, 2, 3, 4].map(x => { total += x
            x })
        total
    "#;
    assert_eq!(ok(code), Value::Int(10));
}

#[test]
fn closures_without_captures_still_use_the_plain_closure_path() {
    let code = r#"
        let double = x => x * 2
        [1, 2, 3].map(double) |> collect()
    "#;
    assert_eq!(ints(code), vec![2, 4, 6]);
}

// ----------------------------------------------------- 不泄漏内部 Cell

#[test]
fn captured_variables_do_not_leak_cell_values() {
    let code = r#"
        let n = 7
        let read = () => n
        $"{read()}/{type_of(n)}/{type_of(read)}"
    "#;
    assert_eq!(ok(code), Value::string("7/int/function"));

    // 经适配器取出后仍是普通值
    let code = r#"
        let items = []
        for i in range(0, 2) { items.push((() => i)) }
        items |> map(f => f()) |> collect() |> join("-")
    "#;
    assert_eq!(ok(code), Value::string("0-1"));
}

#[test]
fn json_round_trip_of_captured_values_is_plain() {
    let code = r#"
        let cfg = { retries: 3 }
        let get = () => cfg
        json.stringify(get())
    "#;
    assert_eq!(ok(code), Value::string("{\"retries\":3}"));
}
