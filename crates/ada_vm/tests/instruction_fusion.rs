//! 指令融合的等价性（开发计划 T6.6）。
//!
//! 编译器把"一侧是整数字面量"的二元表达式编成单条 `BinaryConst`（省掉一条 `LoadInt`
//! 与一次派发）。代价是**多了一条执行路径**，因此这里用差分测试钉住等价性：
//! 同一个表达式写两遍——一次带字面量（走融合路径），一次把常量放进变量（走通用路径）
//! ——结果或错误消息必须逐字相同。
//!
//! 覆盖矩阵：12 种左值形态（含极值、浮点、字符串、布尔、nil、容器）× 11 个运算符 ×
//! 5 个常量。错误消息也参与比对，因为"静默错值"与"错误消息不一致"都是质量问题。

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};

/// 求值并把结果/错误都归一化成字符串，便于逐字比较。
fn eval_rendered(source: &str) -> Result<String, String> {
    let mut parser = Parser::from_source(source).expect("lex");
    let program = match parser.parse_program() {
        Ok(program) => program,
        Err(err) => return Err(format!("parse: {err}")),
    };
    let proto = match Compiler::compile_program(&program) {
        Ok(proto) => proto,
        Err(err) => return Err(format!("compile: {err}")),
    };
    match Vm::new().execute(proto) {
        Ok(value) => Ok(value.to_string()),
        Err(err) => Err(render_error(&err)),
    }
}

fn render_error(err: &VmError) -> String {
    err.to_string()
}

/// 融合版本：`<左值> <op> <常量字面量>`。
fn fused(left: &str, op: &str, constant: i64) -> String {
    format!("{left} {op} {constant}")
}

/// 通用版本：把常量放进变量，从而走原有的通用指令。
///
/// 注意结尾的 `;`：如果只靠换行分隔，`let k = 2` 后面紧跟以 `-` 开头的左值会被
/// ASI 续行规则拼成 `let k = 2 - 1 - k`（语义陷阱，见 AI 规格规则 2）。
fn generic(left: &str, op: &str, constant: i64) -> String {
    format!("let k = {constant};\n{left} {op} k")
}

const OPS: [&str; 11] = ["+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">="];

const CONSTANTS: [i64; 5] = [0, 1, 2, -3, 7];

#[test]
fn fused_and_generic_paths_agree() {
    // 左值形态覆盖：整数（含 i64 边界）、浮点（含 0.0）、字符串、布尔、nil、容器
    let lefts = [
        "1",
        "-1",
        "0",
        "9223372036854775807",
        "-9223372036854775807",
        "1.5",
        "-2.25",
        "\"s\"",
        "\"\"",
        "true",
        "nil",
        "[1, 2]",
        "{ a: 1 }",
    ];

    let mut checked = 0;
    for left in lefts {
        for op in OPS {
            for constant in CONSTANTS {
                let fused_source = fused(left, op, constant);
                let generic_source = generic(left, op, constant);
                let a = eval_rendered(&fused_source);
                let b = eval_rendered(&generic_source);
                assert_eq!(
                    a, b,
                    "融合路径与通用路径不一致\n  融合: {fused_source}\n  通用: {generic_source}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 600, "expected a large matrix, checked {checked}");
}

#[test]
fn literals_on_the_left_are_mirrored_correctly() {
    // 字面量在左侧时只有可交换/可镜像的运算符才融合：`2 < x` ≡ `x > 2`
    let lefts = ["1", "-1", "3", "2.5", "\"s\"", "nil"];
    for left in lefts {
        for (fused_op, generic_op) in [("<", ">"), ("<=", ">="), (">", "<"), (">=", "<=")] {
            // `2 <op> <left>`（字面量在左，走镜像融合）对比
            // `let k = 2;` + `<left> <mirrored> k`（走通用路径）
            let fused_source = format!("2 {fused_op} {left}");
            let generic_source = format!("let k = 2;\n{left} {generic_op} k");
            assert_eq!(
                eval_rendered(&fused_source),
                eval_rendered(&generic_source),
                "镜像不一致: {fused_source} vs {generic_source}"
            );
        }
    }

    // 可交换运算符：`2 + x` 与 `x + 2` 等价
    for left in ["1", "2.5", "\"s\""] {
        assert_eq!(
            eval_rendered(&format!("2 + {left}")),
            eval_rendered(&format!("{left} + 2")),
            "加法应当可交换: {left}"
        );
        assert_eq!(
            eval_rendered(&format!("2 == {left}")),
            eval_rendered(&format!("{left} == 2")),
            "相等应当可交换: {left}"
        );
    }

    // 不可镜像的运算符不融合，但结果必须正确（常量在左侧时退回通用路径）
    assert_eq!(eval_rendered("2 - 1").unwrap(), "1");
    assert_eq!(eval_rendered("2 - 5").unwrap(), "-3");
    assert_eq!(eval_rendered("6 / 2").unwrap(), "3");
    assert_eq!(eval_rendered("7 % 2").unwrap(), "1");
}

#[test]
fn constants_outside_i32_are_not_fused_but_stay_correct() {
    // 超出 i32 的字面量不能用 `BinaryConst`（value 是 i32），退回通用路径
    assert_eq!(eval_rendered("1 + 5000000000").unwrap(), "5000000001");
    assert_eq!(eval_rendered("5000000000 > 1").unwrap(), "true");
    assert_eq!(
        eval_rendered("9223372036854775807 - 1").unwrap(),
        "9223372036854775806"
    );
}

#[test]
fn extreme_values_and_errors_match_the_generic_path() {
    // i64::MIN 由表达式构造（字面量写不出来），再与常量 -1 做除法/取模：
    // 两条路径都必须给出同样的溢出错误
    let build_min = "let m = -9223372036854775807 - 1;\n";
    let fused_only = format!("{build_min}m / -1");
    let generic_only = format!("{build_min}let k = -1;\nm / k");
    assert_eq!(
        eval_rendered(&fused_only),
        eval_rendered(&generic_only),
        "i64::MIN / -1 的错误必须一致"
    );
    assert!(eval_rendered(&fused_only).unwrap_err().contains("overflow"));

    let fused_mod = format!("{build_min}m % -1");
    let generic_mod = format!("{build_min}let k = -1;\nm % k");
    assert_eq!(eval_rendered(&fused_mod), eval_rendered(&generic_mod));

    // 除零 / 取模零：消息必须逐字一致
    assert_eq!(eval_rendered("5 / 0"), eval_rendered("let k = 0;\n5 / k"));
    assert_eq!(eval_rendered("5 % 0"), eval_rendered("let k = 0;\n5 % k"));
    assert!(eval_rendered("5 / 0")
        .unwrap_err()
        .contains("Division by zero"));
    assert!(eval_rendered("5 % 0")
        .unwrap_err()
        .contains("Modulo by zero"));

    // 整数溢出：加法与乘法
    let huge = "9223372036854775807";
    assert_eq!(
        eval_rendered(&format!("{huge} + 1")),
        eval_rendered(&format!("let k = 1;\n{huge} + k"))
    );
    assert!(eval_rendered(&format!("{huge} + 1"))
        .unwrap_err()
        .contains("overflow"));
}

#[test]
fn fusion_reduces_the_instruction_count() {
    // 融合的直接收益：同样语义的表达式指令更少（确定性、可度量）
    fn instruction_count(source: &str) -> usize {
        let mut parser = Parser::from_source(source).expect("lex");
        let program = parser.parse_program().expect("parse");
        Compiler::compile_program(&program)
            .expect("compile")
            .instructions
            .len()
    }

    let with_literals = instruction_count("let x = 5\nx * 2 + 1");
    let with_variables = instruction_count("let x = 5\nlet a = 2\nlet b = 1\nx * a + b");
    // 字面量版本应至少少两条 `LoadInt`
    assert!(
        with_literals + 2 <= with_variables,
        "fusion should remove the LoadInt instructions: literals={with_literals}, variables={with_variables}"
    );

    // 数值循环体（T6.1 基准里的形状）同样受益
    let loop_body = "mut total = 0\nfor i in 200000 { total += i * 2 + 1 }\ntotal";
    assert_eq!(
        eval_rendered(loop_body).unwrap(),
        "40000000000",
        "融合后语义不变"
    );
    let _ = Value::Nil; // 保持 use 不报未使用
}

#[test]
fn condition_fusion_reduces_loop_work_without_changing_semantics() {
    // T6.7：`while i < 1000`（比较 + 常量 → 一条融合跳转）对比等价但**不融合**的写法
    // `while !(i >= 1000)`（外层是 `Not`，条件不是比较表达式）与 `i += one`（右侧是变量）。
    // 两者语义必须一致，而融合形式的指令更少——这是确定性证据，不受机器负载影响。
    let fused_source =
        "mut i = 0\nmut total = 0\nwhile i < 1000 {\n    total += i\n    i += 1\n}\ntotal";
    let unfused_source = "mut i = 0\nmut total = 0\nlet one = 1\nlet limit = 1000\nwhile !(i >= limit) {\n    total += i\n    i += one\n}\ntotal";

    // 语义一致：sum(0..999) = 499_500
    assert_eq!(eval_rendered(fused_source).unwrap(), "499500");
    assert_eq!(eval_rendered(unfused_source).unwrap(), "499500");

    fn instruction_count(source: &str) -> usize {
        let mut parser = Parser::from_source(source).expect("lex");
        let program = parser.parse_program().expect("parse");
        Compiler::compile_program(&program)
            .expect("compile")
            .instructions
            .len()
    }

    let fused = instruction_count(fused_source);
    let unfused = instruction_count(unfused_source);
    assert!(
        fused + 4 <= unfused,
        "融合形式应明显更少指令：fused={fused}, unfused={unfused}"
    );
}

#[test]
fn conditional_edge_cases_still_work() {
    // 融合路径不能破坏原有的条件语义：真值判定、短路、空 else、break/continue、
    // 非数值比较报错、NaN
    assert_eq!(
        eval_rendered("if 1 < 2 { \"y\" } else { \"n\" }").unwrap(),
        "y"
    );
    assert_eq!(
        eval_rendered("if 1 > 2 { \"y\" } else { \"n\" }").unwrap(),
        "n"
    );
    assert_eq!(
        eval_rendered("if \"a\" == \"a\" { 1 } else { 0 }").unwrap(),
        "1"
    );
    assert_eq!(eval_rendered("if nil { 1 } else { 2 }").unwrap(), "2");
    assert_eq!(
        eval_rendered("if 0 == 0 && 1 < 2 { \"both\" } else { \"no\" }").unwrap(),
        "both"
    );
    assert_eq!(
        eval_rendered("if 0 != 0 || 1 < 2 { \"either\" } else { \"no\" }").unwrap(),
        "either"
    );

    // 非数值有序比较：错误消息必须与通用路径一致
    assert_eq!(
        eval_rendered("if 1 < \"a\" { 1 } else { 2 }"),
        eval_rendered("let k = \"a\";\nif 1 < k { 1 } else { 2 }")
    );
    assert!(eval_rendered("if 1 < \"a\" { 1 } else { 2 }")
        .unwrap_err()
        .contains("only numbers are orderable"));

    // NaN：所有有序比较为 false（走 else 分支），两条路径一致
    let nan = "let nan = 0.0 / 0.0;\n";
    assert_eq!(
        eval_rendered(&format!("{nan}if nan < 1.0 {{ 1 }} else {{ 2 }}")),
        eval_rendered(&format!(
            "{nan}let k = 1.0;\nif nan < k {{ 1 }} else {{ 2 }}"
        ))
    );

    // break / continue 在融合条件的循环里照常工作
    assert_eq!(
        eval_rendered("mut i = 0\nwhile i < 100 {\n    i += 1\n    if i == 5 { break }\n}\ni")
            .unwrap(),
        "5"
    );
    assert_eq!(
        eval_rendered("mut i = 0\nmut n = 0\nwhile i < 10 {\n    i += 1\n    if i % 2 == 0 { continue }\n    n += 1\n}\nn").unwrap(),
        "5"
    );

    // 嵌套 if/else 链
    assert_eq!(
        eval_rendered("let x = 7\nif x < 5 { \"s\" } else if x < 10 { \"m\" } else { \"l\" }")
            .unwrap(),
        "m"
    );
}
