//! 插值字符串的错误定位与诊断质量（T3.4 / T8.1）。
//!
//! 插值片段（`$"{...}"` 里的代码）由**独立的子解析器**处理，因此必须把片段内的
//! 位置映射回源文件。T3.4 已经实现了映射（`Parser::from_fragment` + 词法错误偏移），
//! 本文件把实测行为逐条固定下来——包括多行片段与片段内的词法错误——并固定 T8.1
//! 补充的"这是插值边界"上下文。
//!
//! 之所以要专门测：这类偏移错了不会崩，只会指向错误的一行/一列，而位置是
//! 编辑器波浪线与 CI 报错的可信度基础（见 SPEC_RUNTIME §8.2）。

use ada_parser::{ParseError, Parser};

/// 解析并取出错误（解析必须失败）。
fn error(source: &str) -> ParseError {
    let mut parser = Parser::from_source(source).expect("lexing should succeed");
    match parser.parse_program() {
        Err(err) => err,
        Ok(_) => panic!("expected a parse error for {source:?}"),
    }
}

fn position(source: &str) -> (usize, usize) {
    error(source).position()
}

fn message(source: &str) -> String {
    error(source).to_string()
}

#[test]
fn single_line_fragment_positions_map_back_to_the_source() {
    // `$"x {1 +} y"`：片段是 `1 +`，其 EOF 落在 `}` 处（第 9 列）
    assert_eq!(position("$\"x {1 +} y\""), (1, 9));

    // 第二个片段：`$"{1} {2 +}"` → 片段起点更靠右，列号必须跟着走
    assert_eq!(position("$\"{1} {2 +}\""), (1, 11));
}

#[test]
fn fragment_positions_respect_earlier_lines() {
    let source = "let a = 1\nlet b = 2\nlet c = $\"ok {1 +} end\"\nc";
    // 第 3 行 `let c = $"ok {1 +} end"`：片段 `1 +` 的 EOF 在 `}` 处（第 18 列）
    assert_eq!(position(source), (3, 18));
}

#[test]
fn multiline_fragments_report_the_correct_line() {
    // 片段跨行：`$"a {1 +\n} b"`，片段内的 EOF 落在第 2 行第 1 列
    let source = "let s = $\"a {1 +\n} b\"\ns";
    assert_eq!(position(source), (2, 1));
}

#[test]
fn lexer_errors_inside_fragments_are_shifted() {
    // 片段内的词法错误也要跟着偏移：`$"{@}"` 的 `@` 在第 4 列
    assert_eq!(position("$\"{@}\""), (1, 4));
    // 靠后位置同样成立
    assert_eq!(position("$\"ab {@}\""), (1, 7));
}

#[test]
fn incomplete_interpolations_explain_the_boundary() {
    // T8.1：位置不变，但消息要说明"这是插值片段的结束"，否则用户看到的是 `}` 处
    // 的 `<EOF>`，很容易误解
    let same_line = message("$\"x {1 +} y\"");
    assert!(
        same_line.contains("interpolation"),
        "message should mention the interpolation boundary: {same_line}"
    );
    assert!(
        same_line.contains("expected expression"),
        "the original expectation must survive: {same_line}"
    );

    // 原期望记号不能被覆盖：这里真正缺的是 `)`
    let unbalanced_paren = message("$\"{(1}\"");
    assert!(
        unbalanced_paren.contains("expected )"),
        "the original expectation is `)`, not `}}`: {unbalanced_paren}"
    );
    assert!(
        unbalanced_paren.contains("interpolation"),
        "{unbalanced_paren}"
    );

    // 多行片段同样带上下文，且位置仍然指向第 2 行
    let multiline = error("let s = $\"a {1 +\n} b\"\ns");
    assert_eq!(multiline.position(), (2, 1));
    assert!(multiline.to_string().contains("interpolation"));
}

#[test]
fn valid_interpolations_still_parse() {
    for source in [
        "$\"plain\"",
        "let n = 2\n$\"n={n}\"",
        "let xs = [1]\n$\"{xs[0]} {1 + 2}\"",
        "let n = 1\n$\"{\n    n + 1\n}\"",
    ] {
        let mut parser = Parser::from_source(source).expect("lexing should succeed");
        parser
            .parse_program()
            .unwrap_or_else(|err| panic!("{source:?} should parse, got {err:?}"));
    }
}
