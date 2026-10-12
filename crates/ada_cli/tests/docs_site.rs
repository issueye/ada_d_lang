//! 文档站结构门禁（开发计划 T7.3）。
//!
//! 规则：`docs/` 是 mdBook 的**单一源**——新增文档必须同时进入 `SUMMARY.md`，
//! `SUMMARY.md` 里的每一项都必须真实存在；能拿到 `mdbook` 时还要求**真实构建成功**。
//! 这样"文档站"不是另一份副本，而是同一批 Markdown 的另一种渲染。

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn docs_dir() -> PathBuf {
    workspace_root().join("docs")
}

/// 抽取 `SUMMARY.md` 中的相对链接目标（`- [标题](文件.md)` 形式）。
fn summary_entries() -> Vec<String> {
    let content = std::fs::read_to_string(docs_dir().join("SUMMARY.md")).expect("SUMMARY.md");
    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let Some(start) = trimmed.find("](") else {
            continue;
        };
        let rest = &trimmed[start + 2..];
        let Some(end) = rest.find(')') else { continue };
        let target = rest[..end].trim();
        if target.is_empty() || target.starts_with("http") || target.contains('#') {
            continue;
        }
        entries.push(target.to_string());
    }
    entries
}

#[test]
fn summary_lists_every_document_and_nothing_missing() {
    let docs = docs_dir();
    let mut listed: Vec<String> = summary_entries();
    listed.sort();

    let mut on_disk: Vec<String> = std::fs::read_dir(&docs)
        .expect("docs/ readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        // SUMMARY.md 自己就是目录，不需要在目录里再列一次
        .filter(|name| name != "SUMMARY.md")
        .collect();
    on_disk.sort();

    for entry in &listed {
        let path = docs.join(entry);
        assert!(
            path.exists(),
            "SUMMARY.md references {entry}, but {} does not exist",
            path.display()
        );
    }

    let missing: Vec<&String> = on_disk
        .iter()
        .filter(|name| !listed.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "这些文档没有出现在 docs/SUMMARY.md（文档站会看不到它们）：{missing:?}"
    );

    // 至少要覆盖到规范、白皮书与 AI 规格
    for required in [
        "SPEC_LANGUAGE.md",
        "SPEC_RUNTIME.md",
        "SPEC_STDLIB.md",
        "SPEC_ASYNC.md",
        "WHITEPAPER.md",
        "AI_GUIDE.md",
        "DEVELOPMENT_PLAN.md",
    ] {
        assert!(
            listed.iter().any(|e| e == required),
            "SUMMARY.md 缺少 {required}"
        );
    }
}

#[test]
fn book_config_points_at_the_docs_directory() {
    let config = std::fs::read_to_string(docs_dir().join("book.toml")).expect("docs/book.toml");
    assert!(config.contains("[book]"), "book.toml must declare [book]");
    assert!(
        config.contains("src = \".\""),
        "book.toml must use docs/ itself as the book source (single source of truth)"
    );
    // 产物目录必须在 docs/ 之外，避免"渲染结果混进源目录"
    assert!(
        config.contains("build-dir = \"../target/mdbook\""),
        "book.toml build-dir should live outside docs/"
    );
    assert!(config.contains("title ="), "book.toml needs a title");
}

#[test]
fn mdbook_builds_the_site_when_available() {
    let mdbook = Command::new("mdbook").arg("--version").output();
    match mdbook {
        Ok(output) if output.status.success() => {}
        _ => {
            // 本地/CI 未安装 mdbook 时跳过（CI 会安装，见 .github/workflows/ci.yml）
            println!("note: mdbook not found on PATH; skipping the real site build");
            return;
        }
    }

    let output = Command::new("mdbook")
        .arg("build")
        .arg(docs_dir())
        .output()
        .expect("mdbook should run");
    assert!(
        output.status.success(),
        "mdbook build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let site = workspace_root().join("target/mdbook");
    assert!(site.join("index.html").exists(), "site index.html missing");
    for entry in summary_entries() {
        // mdBook 把 README.md 渲染成 index.html
        let stem = entry.trim_end_matches(".md");
        let page_name = if stem == "README" {
            "index.html".to_string()
        } else {
            format!("{stem}.html")
        };
        let page = site.join(&page_name);
        assert!(
            page.exists(),
            "SUMMARY.md entry {entry} produced no {page_name}"
        );
    }
}
