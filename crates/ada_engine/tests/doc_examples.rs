//! 文档示例可运行性测试（开发计划 §6「文档测试」、风险 R1 缓解措施）。
//!
//! 本文件把 README 与 SPEC_* 规范中标注为"可运行 / 可直接编译"的示例固化为测试。
//! 一旦规范文档里的示例与实际行为脱节，这里会立刻失败，从而阻止文档漂移。

use ada_engine::Engine;

fn eval_i64(source: &str) -> i64 {
    let mut engine = Engine::new();
    engine.eval(source).expect("example should run")
}

fn eval_string(source: &str) -> String {
    let mut engine = Engine::new();
    engine.eval(source).expect("example should run")
}

// -------------------------------------------------- README「快速开始」脚本示例

#[test]
fn readme_quick_start_eval_example() {
    assert_eq!(eval_i64("let arr = [10, 20, 30]\narr |> sum()"), 60);
}

// ------------------------------------------------ README / SPEC_RUNTIME 嵌入示例

#[test]
fn spec_runtime_embedding_example_with_exported_value() {
    use ada_engine::Value; // T0.7：Value 现由 ada_engine 直接 re-export

    let mut engine = Engine::new();
    engine.set_fuel(Some(50_000));
    engine.register_fn("double", |args: &[Value]| {
        Ok(Value::Int(args[0].as_int().unwrap_or(0) * 2))
    });

    let n: i64 = engine.eval("double(21)").unwrap();
    assert_eq!(n, 42);
}

#[test]
fn spec_runtime_tax_example() {
    use ada_engine::Value;

    let mut engine = Engine::new();
    engine.set_fuel(Some(50_000));
    engine.register_fn("calculate_tax", |args: &[Value]| {
        let income = args.first().and_then(|v| v.as_float()).unwrap_or(0.0);
        let rate = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.1);
        Ok(Value::Float(income * rate))
    });

    let tax: f64 = engine
        .eval("let t = calculate_tax(10000.0, 0.2)\nt")
        .unwrap();
    assert_eq!(tax, 2000.0);
}

// ------------------------------------------------------ SPEC_LANGUAGE 语法示例

#[test]
fn spec_language_pipe_and_desugaring_examples() {
    assert_eq!(eval_i64("fn add3(a, b, c) = a + b + c\n1 |> add3(2, 3)"), 6);
    assert_eq!(eval_i64("fn inc(x) = x + 1\n41 |> inc"), 42);
    // 管道优先级高于 `+`：1 + 2 |> f() == 1 + f(2)
    assert_eq!(eval_i64("fn f(x) = x * 10\n1 + 2 |> f()"), 21);
}

#[test]
fn spec_language_match_example() {
    let grade =
        eval_string("let score = 85\nmatch score { >= 90 => \"A\", >= 80 => \"B\", _ => \"C\" }");
    assert_eq!(grade, "B");
}

#[test]
fn spec_language_interpolation_example() {
    let msg = eval_string(
        "let name = \"Ada\"\nlet roles = [\"admin\", \"dev\"]\n$\"User {name} has {roles.len} roles\"",
    );
    assert_eq!(msg, "User Ada has 2 roles");
}

#[test]
fn spec_language_safe_navigation_example() {
    let code = r#"
        let user = { name: "Ada", config: { timeout: 3000 } }
        let timeout = user?.config?.timeout ?? 5000
        let missing = user?.missing?.item ?? "fallback"
        $"{timeout}/{missing}"
    "#;
    assert_eq!(eval_string(code), "3000/fallback");
}

#[test]
fn spec_language_async_syntax_example() {
    // 当前为同步求值语义；此处只锁定语法与结果，运行时语义见 SPEC_ASYNC §3
    assert_eq!(
        eval_i64("async fn fast(x) = x * 2\nlet t = spawn fast(21)\nt.await"),
        42
    );
}

// -------------------------------------------------------- SPEC_STDLIB 标准库示例

#[test]
fn spec_stdlib_json_example() {
    let name = eval_string(
        "let user = json.parse(\"{\\\"id\\\": 101, \\\"name\\\": \\\"Ada\\\", \\\"tags\\\": [\\\"rust\\\"]}\")\nuser.name",
    );
    assert_eq!(name, "Ada");
}

#[test]
fn spec_stdlib_math_and_time_examples() {
    assert_eq!(eval_i64("to_int(math.sqrt(256.0))"), 16);
    assert_eq!(eval_i64("to_int(math.pow(2.0, 8.0))"), 256);
    assert_eq!(eval_string("sys.os"), std::env::consts::OS);
}

#[test]
fn spec_stdlib_iteration_examples() {
    assert_eq!(
        eval_i64("mut s = 0\nfor i in range(0, 5) { s += i }\ns"),
        10
    );
    assert_eq!(eval_i64("mut s = 0\nfor i in 5 { s += i }\ns"), 10);
    assert_eq!(
        eval_i64("mut s = 0\nfor [k, v] in { a: 1, b: 2 } { s += v }\ns"),
        3
    );
    assert_eq!(
        eval_i64("mut s = 0\nfor [i, v] in [10, 20].enumerate() { s += v }\ns"),
        30
    );
    assert_eq!(
        eval_string("mut out = \"\"\nfor ch in \"Ada\" { out += ch }\nout"),
        "Ada"
    );
}

#[test]
fn spec_stdlib_iterator_adapter_examples() {
    // SPEC_STDLIB §6.2 计划验收用例
    let code = r#"
        let result = range(1, 1000)
            |> filter(x => x % 2 != 0)
            |> map(x => x * x)
            |> take(5)
            |> sum()
        result
    "#;
    assert_eq!(eval_i64(code), 165);

    // SPEC_STDLIB §6 方法形式 + §6.3 双入口
    assert_eq!(eval_i64("[1, 2, 3].map(x => x * 2).sum()"), 12);
    assert_eq!(
        eval_string("[1, 2, 3, 4].filter(x => x % 2 == 0).collect() |> join(\"-\")"),
        "2-4"
    );
}

#[test]
fn spec_language_top_level_binding_persists_across_evaluations() {
    // SPEC_LANGUAGE §5.1：模块顶层绑定跨轮可见（T2.4）
    let mut engine = Engine::new();
    let a: i64 = engine.eval("let a = 41").unwrap();
    assert_eq!(a, 41);
    let b: i64 = engine.eval("a + 1").unwrap();
    assert_eq!(b, 42);
}

#[test]
fn spec_stdlib_string_and_array_prototypes() {
    assert_eq!(eval_string("\"  Ada  \".trim().to_upper()"), "ADA");
    assert_eq!(eval_string("\"abcdef\".slice(1, 4)"), "bcd");
    assert_eq!(eval_string("\"a,b,c\".split(\",\").join(\"-\")"), "a-b-c");
    assert_eq!(eval_i64("mut a = [10, 20]\na.push(30)\na.len"), 3);
    assert_eq!(eval_i64("[3, 1, 2].rev()[0]"), 2);
    assert_eq!(
        eval_i64("mut m = { a: 1, b: 2 }\nto_int(m.contains_key(\"a\"))"),
        1
    );
}
