//! 编辑器支持的门禁（开发计划 T7.1）。
//!
//! `editors/vscode/` 里的 TextMate 语法是**手写高亮规则**，最容易与语言实现脱节。
//! 这里把它的关键字/运算符列表与 `ada_lexer` 的权威表做**集合比对**，
//! 并校验扩展清单声明的路径真实存在，从而让"高亮悄悄失效"变成一次性失败。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn vscode_dir() -> PathBuf {
    workspace_root().join("editors/vscode")
}

fn grammar() -> serde_json::Value {
    let text = std::fs::read_to_string(vscode_dir().join("syntaxes/ada.tmLanguage.json"))
        .expect("grammar file readable");
    serde_json::from_str(&text).expect("grammar must be valid JSON")
}

fn json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

/// 从 `\b(a|b|c)\b` 形式的正则里取出单词集合。
fn words_in_alternation(pattern: &str) -> BTreeSet<String> {
    let start = pattern.find('(').expect("alternation group");
    let end = pattern[start..]
        .find(')')
        .map(|i| start + i)
        .expect("closing paren");
    pattern[start + 1..end]
        .split('|')
        .map(|word| word.to_string())
        .collect()
}

/// 拆分正则的顶层备选分支：尊重 `\` 转义与 `[...]` 字符类
/// （字符类里的 `|` 是字面量，例如 `[-+*/%<>=!&|^~]`）。
///
/// 返回去掉正则转义后的字面量；字符类会展开成单个字符。
fn literal_alternatives(pattern: &str) -> BTreeSet<String> {
    let mut alternatives: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut in_class = false;

    for ch in pattern.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => {
                in_class = true;
                current.push(ch);
            }
            ']' => {
                in_class = false;
                current.push(ch);
            }
            '|' if !in_class => alternatives.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    alternatives.push(current);

    let mut out = BTreeSet::new();
    for alternative in alternatives {
        let trimmed = alternative.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            for ch in trimmed[1..trimmed.len() - 1].chars() {
                out.insert(ch.to_string());
            }
        } else {
            // 去掉分组括号等纯结构记号
            let cleaned = trimmed.replace("(?:", "").replace(')', "");
            if !cleaned.is_empty() {
                out.insert(cleaned);
            }
        }
    }
    out
}

#[test]
fn grammar_keywords_match_the_lexer() {
    let grammar = grammar();
    // 关键字分布在两条规则里：控制流/声明在 keywords，字面量在 constants
    let mut in_grammar = words_in_alternation(
        grammar["repository"]["keywords"]["match"]
            .as_str()
            .expect("keyword pattern"),
    );
    in_grammar.extend(words_in_alternation(
        grammar["repository"]["constants"]["match"]
            .as_str()
            .expect("constants pattern"),
    ));
    let authoritative: BTreeSet<String> =
        ada_lexer::KEYWORDS.iter().map(|k| k.to_string()).collect();

    let missing: Vec<&String> = authoritative.difference(&in_grammar).collect();
    let extra: Vec<&String> = in_grammar.difference(&authoritative).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "VSCode 语法与 ada_lexer::KEYWORDS 不一致：\n  语法缺少 {missing:?}\n  语法多出 {extra:?}\n\
         （请同步 editors/vscode/syntaxes/ada.tmLanguage.json）"
    );
}

#[test]
fn grammar_operators_match_the_lexer() {
    let grammar = grammar();
    // 运算符与分隔符分布在两条规则里：operators（含多字符组合）与 punctuation（括号/逗号等）
    let mut in_grammar: BTreeSet<String> = BTreeSet::new();
    for section in ["operators", "punctuation"] {
        let patterns = grammar["repository"][section]["patterns"]
            .as_array()
            .unwrap_or_else(|| panic!("{section} patterns"));
        for pattern in patterns {
            let regex = pattern["match"].as_str().expect("operator match");
            in_grammar.extend(literal_alternatives(regex));
        }
    }

    let authoritative: BTreeSet<String> =
        ada_lexer::OPERATORS.iter().map(|o| o.to_string()).collect();
    let missing: Vec<&String> = authoritative.difference(&in_grammar).collect();
    let extra: Vec<&String> = in_grammar.difference(&authoritative).collect();
    assert!(
        missing.is_empty(),
        "VSCode 语法缺少这些运算符：{missing:?}\n（请同步 editors/vscode/syntaxes/ada.tmLanguage.json）"
    );
    assert!(
        extra.is_empty(),
        "VSCode 语法含有语言里不存在的运算符：{extra:?}\n（请同步或从 ada_lexer::OPERATORS 中补上）"
    );
}

#[test]
fn extension_manifest_paths_exist_and_cover_ada_files() {
    let manifest = json(&vscode_dir().join("package.json"));
    let languages = manifest["contributes"]["languages"]
        .as_array()
        .expect("languages contribution");
    assert_eq!(languages[0]["id"], "ada");
    let extensions: Vec<&str> = languages[0]["extensions"]
        .as_array()
        .expect("extensions array")
        .iter()
        .filter_map(|e| e.as_str())
        .collect();
    assert!(
        extensions.contains(&".ada"),
        "language contribution must claim .ada files, got {extensions:?}"
    );

    let config_path = languages[0]["configuration"]
        .as_str()
        .expect("language configuration path");
    assert!(
        vscode_dir().join(config_path).exists(),
        "language-configuration path {config_path} does not exist"
    );

    let grammars = manifest["contributes"]["grammars"]
        .as_array()
        .expect("grammars contribution");
    let grammar_path = grammars[0]["path"].as_str().expect("grammar path");
    assert!(
        vscode_dir().join(grammar_path).exists(),
        "grammar path {grammar_path} does not exist"
    );
    assert_eq!(grammars[0]["scopeName"], "source.ada");

    // 语法文件的 scopeName 必须与清单一致（否则 VSCode 不会应用高亮）
    assert_eq!(grammar()["scopeName"], "source.ada");
}

#[test]
fn grammar_covers_the_language_surface() {
    let grammar = grammar();
    let repository = &grammar["repository"];
    // 语言里"必须能高亮"的几类记号
    for key in [
        "comments",
        "strings",
        "interpolated-strings",
        "numbers",
        "keywords",
        "constants",
        "operators",
        "punctuation",
    ] {
        assert!(
            !repository[key].is_null(),
            "grammar repository 缺少 {key} 规则"
        );
    }
    // 插值字符串必须复用整份语法（`$"{...}"` 内部也是 Ada-D 代码）
    let interp = repository["interpolated-strings"]["patterns"]
        .as_array()
        .expect("interpolation patterns");
    assert!(
        interp.iter().any(|p| p["patterns"]
            .as_array()
            .is_some_and(|inner| inner.iter().any(|i| i["include"] == "$self"))),
        "插值表达式内部应包含 $self 以复用整份语法"
    );
    // 块注释成对
    let comments = repository["comments"]["patterns"]
        .as_array()
        .expect("comment patterns");
    assert!(
        comments
            .iter()
            .any(|p| p["begin"] == "/\\*" && p["end"] == "\\*/"),
        "缺少块注释规则"
    );
}
