//! AI 单页规格的可运行门禁（开发计划 T7.4）。
//!
//! `docs/AI_GUIDE.md` 中每个 ```` ```ada run ```` 代码块都由**真实 CLI** 执行，
//! 并与紧随其后的 ```` ```output ````（必须成功且输出一致）或 ```` ```error ````
//! （必须失败且错误消息包含给定片段）比对。这样"给模型的规格"不会与实现漂移。
//!
//! 同时校验 `examples/ai/*.ada` 的 golden 输出，并保证规格确实链接了这三个示例。

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn guide_path() -> PathBuf {
    workspace_root().join("docs/AI_GUIDE.md")
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

/// 去掉**公共前缀缩进**：Markdown 列表里的代码块会整体缩进。
///
/// 源码与期望输出都被同样处理，相对缩进不受影响；不处理的话，缩进会变成
/// "期望输出里多出来的空格"，让本来正确的示例误报失败。
fn dedent(text: &str) -> String {
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    if indent == 0 {
        return text.to_string();
    }
    text.lines()
        .map(|line| {
            if line.len() >= indent {
                &line[indent..]
            } else {
                line.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_cli(source: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ada_cli"))
        .arg("-e")
        .arg(source)
        .output()
        .expect("ada_cli should be runnable")
}

enum Expect {
    Output(String),
    Error(String),
}

struct Snippet {
    line: usize,
    source: String,
    expect: Expect,
}

/// 从规格里抽取 `ada run` 块及其声明的结果块。
fn guide_snippets() -> Vec<Snippet> {
    let text = std::fs::read_to_string(guide_path()).expect("docs/AI_GUIDE.md readable");
    let lines: Vec<&str> = text.lines().collect();
    let mut snippets = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim() != "```ada run" {
            i += 1;
            continue;
        }
        let start_line = i + 1;
        i += 1;
        let mut source = String::new();
        while i < lines.len() && lines[i].trim() != "```" {
            source.push_str(lines[i]);
            source.push('\n');
            i += 1;
        }
        i += 1; // 关闭围栏

        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        let fence = lines.get(i).map(|l| l.trim()).unwrap_or_default();
        let is_output = fence == "```output";
        let is_error = fence == "```error";
        assert!(
            is_output || is_error,
            "snippet at line {start_line} must be followed by a ```output or ```error block, found {fence:?}"
        );
        i += 1;
        let mut body = String::new();
        while i < lines.len() && lines[i].trim() != "```" {
            body.push_str(lines[i]);
            body.push('\n');
            i += 1;
        }
        i += 1; // 关闭围栏

        snippets.push(Snippet {
            line: start_line,
            source,
            expect: if is_output {
                Expect::Output(body)
            } else {
                Expect::Error(body)
            },
        });
    }

    snippets
}

#[test]
fn every_runnable_guide_snippet_matches_its_declared_result() {
    let snippets = guide_snippets();
    assert!(
        snippets.len() >= 12,
        "expected at least 12 runnable snippets in the AI guide, found {}",
        snippets.len()
    );

    for snippet in snippets {
        let source = dedent(&snippet.source);
        let output = run_cli(&source);
        let stdout = normalize(&String::from_utf8_lossy(&output.stdout));
        let stderr = normalize(&String::from_utf8_lossy(&output.stderr));
        match &snippet.expect {
            Expect::Output(expected) => {
                assert!(
                    output.status.success(),
                    "AI_GUIDE.md:{}: snippet failed but declares output\n--- source ---\n{}\n--- stderr ---\n{}",
                    snippet.line,
                    source,
                    stderr
                );
                assert_eq!(
                    stdout,
                    normalize(&dedent(expected)),
                    "AI_GUIDE.md:{}: output mismatch\n--- source ---\n{}",
                    snippet.line,
                    source
                );
            }
            Expect::Error(needle) => {
                let needle = normalize(&dedent(needle));
                assert!(
                    !output.status.success(),
                    "AI_GUIDE.md:{}: snippet succeeded but declares an error\n--- source ---\n{}\n--- stdout ---\n{}",
                    snippet.line,
                    source,
                    stdout
                );
                assert!(
                    stderr.contains(&needle),
                    "AI_GUIDE.md:{}: error message does not contain {needle:?}\n--- source ---\n{}\n--- stderr ---\n{}",
                    snippet.line,
                    source,
                    stderr
                );
            }
        }
    }
}

#[test]
fn ai_examples_match_their_golden_output() {
    let dir = workspace_root().join("examples/ai");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("examples/ai readable") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "ada") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let expected_path = dir.join(format!("{stem}.expected.txt"));
        assert!(
            expected_path.exists(),
            "examples/ai/{stem}.ada has no golden file ({}); generate it by running the example",
            expected_path.display()
        );

        let output = Command::new(env!("CARGO_BIN_EXE_ada_cli"))
            .arg(&path)
            .output()
            .expect("ada_cli runnable");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "examples/ai/{stem}.ada failed: {stderr}"
        );

        let actual = normalize(&String::from_utf8_lossy(&output.stdout));
        let expected =
            normalize(&std::fs::read_to_string(&expected_path).expect("golden readable"));
        assert_eq!(
            actual, expected,
            "examples/ai/{stem}.ada output diverged from its golden file"
        );
        checked += 1;
    }
    assert_eq!(checked, 3, "expected three AI examples");
}

#[test]
fn guide_links_every_ai_example() {
    let guide = std::fs::read_to_string(guide_path()).expect("guide readable");
    for name in ["transform.ada", "async.ada", "validate.ada"] {
        assert!(
            guide.contains(name),
            "AI_GUIDE.md no longer references examples/ai/{name}"
        );
        assert!(
            workspace_root().join("examples/ai").join(name).exists(),
            "examples/ai/{name} is missing"
        );
    }
}

#[test]
fn guide_covers_the_documented_pitfalls() {
    // 反直觉规则表必须仍然覆盖这些关键点（删掉任何一条都意味着规格退化）
    let guide = std::fs::read_to_string(guide_path()).expect("guide readable");
    for marker in [
        "let` 不可重新赋值",  // T3.1
        "续行",               // ASI 陷阱
        "`{ x }` 是 **map**", // D4
        "必须穷尽",           // T3.2
        "只接受数值",         // T3.5
        "小数位",             // T3.6
        "负索引",             // T3.5
        "O(n²)",              // 字符串拼接代价
        "CapabilityDenied",   // M5 沙箱
        "spawn",              // M4 并发
    ] {
        assert!(
            guide.contains(marker),
            "AI_GUIDE.md lost the pitfall marker {marker:?}"
        );
    }
}
