//! 方法分派缓存（T6.8）的正确性。
//!
//! 缓存把「字段名 → (接收者, 已绑定闭包)」记住，命中时直接复用闭包。这类优化最容易
//! 出错的地方不是速度，而是**安全**：
//!
//! 1. 复用必须限定在**同一个接收者对象**上（否则 `xs.push` 会被绑到 `ys`）；
//! 2. 闭包捕获的是容器本身，因此缓存后写入依然实时可见；
//! 3. 不能抢走 map 的**自有键**（T7.5 的规则）；
//! 4. 方法作为值取出来用（`let f = xs.push`）仍然绑在 xs 上；
//! 5. 非原型方法名（用户键、适配器名）不进入缓存。
//!
//! 这里逐条固定。性能证据见 SPEC_RUNTIME §11.3（受控 A/B：54.8 ms → 26.6 ms）。

use ada_bytecode::{Compiler, FunctionProto};
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

fn run(source: &str) -> Result<Value, VmError> {
    let mut parser = Parser::from_source(source).expect("lex");
    let program = parser.parse_program().expect("parse");
    let proto: FunctionProto = Compiler::compile_program(&program).expect("compile");
    Vm::new().execute(proto)
}

fn ok(source: &str) -> Value {
    run(source).unwrap_or_else(|err| panic!("{source} should succeed, got {err}"))
}

fn ok_string(source: &str) -> String {
    ok(source).to_string()
}

#[test]
fn repeated_calls_on_the_same_receiver_stay_live() {
    // 缓存复用同一个闭包，但它捕获的是容器 Arc，因此写入必须实时生效
    assert_eq!(
        ok_string("mut xs = []\nfor i in 5 { xs.push(i) }\nxs |> join(\",\")"),
        "0,1,2,3,4"
    );
    assert_eq!(
        ok_string("mut xs = [1]\nfor i in 3 { xs.push(i) }\nxs.reverse()\nxs |> join(\",\")"),
        "2,1,0,1"
    );
    assert_eq!(
        ok("mut xs = [1, 2, 3]\nxs.pop()\nxs.pop()\nxs.len"),
        Value::Int(1)
    );
}

#[test]
fn different_receivers_never_share_a_method() {
    // 缓存最危险的失效模式：把 xs 的方法用到 ys 上
    let interleaved = r#"mut xs = []
mut ys = []
xs.push(1)
ys.push(2)
xs.push(3)
$"xs={xs |> join(",")} ys={ys |> join(",")}""#;
    assert_eq!(ok_string(interleaved), "xs=1,3 ys=2");

    // 两个 map：contains_key 必须各问各的（同一个字段名、不同接收者）
    let two_maps = r#"mut a = { k: 1 }
mut b = { j: 2 }
$"{a.contains_key("k")}|{b.contains_key("k")}|{b.contains_key("j")}""#;
    assert_eq!(ok_string(two_maps), "true|false|true");

    // 交错使用字符串与数组
    let mixed = r#"mut xs = []
mut s = "a"
xs.push(1)
let upper = s.to_upper()
xs.push(2)
$"{upper}-{xs.len}-{s.to_upper()}""#;
    assert_eq!(ok_string(mixed), "A-2-A");
}

#[test]
fn map_own_keys_still_win_over_method_names() {
    // T7.5 的规则不受缓存影响：自有键优先；函数值键也不会被原生方法顶替
    assert_eq!(ok("{ contains: 42 }.contains"), Value::Int(42));
    assert_eq!(ok("{ remove: 5 }.remove"), Value::Int(5));
    assert_eq!(ok("{ merge: \"m\" }.merge"), Value::string("m"));
    assert_eq!(
        ok("let m = { push: x => x * 2 }\nm.push(21)"),
        Value::Int(42)
    );
    // 适配器名同样让位于自有键
    assert_eq!(ok("{ map: 7 }.map"), Value::Int(7));
}

#[test]
fn methods_taken_as_values_stay_bound_to_their_receiver() {
    let two_bound = r#"mut xs = []
mut ys = []
let fx = xs.push
let fy = ys.push
fx(1)
fy(2)
fx(3)
$"xs={xs |> join(",")} ys={ys |> join(",")}""#;
    assert_eq!(ok_string(two_bound), "xs=1,3 ys=2");

    assert_eq!(
        ok_string("mut xs = []\nlet f = xs.push\nfor i in 3 { f(i) }\nxs |> join(\",\")"),
        "0,1,2"
    );
}

#[test]
fn string_methods_and_iterators_are_unaffected() {
    assert_eq!(ok_string("\"  Ab \".trim().to_lower()"), "ab");
    assert_eq!(ok_string("\"a,b,c\".split(\",\") |> join(\"-\")"), "a-b-c");
    assert_eq!(ok("[\"x\", \"y\"].contains(\"y\")"), Value::Bool(true));
    assert_eq!(ok("[1, 2, 3].iter().next()"), Value::Int(1));
    // 迭代器方法连续取值（iter/next 在白名单里）
    assert_eq!(
        ok_string("let it = [1, 2].iter()\n$\"{it.next()},{it.next()}\""),
        "1,2"
    );
}
