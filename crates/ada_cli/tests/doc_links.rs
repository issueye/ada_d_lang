//! 文档链接门禁（开发计划 T0.4 / §11 治理规则第 5 条）。
//!
//! 扫描根 `README.md` 与 `docs/` 下的全部 Markdown，校验：
//! 1. 相对链接指向的文件存在；
//! 2. 带锚点的链接在目标文件中有对应标题（按 GitHub 的 slug 规则计算）。
//!
//! 任何不达标的链接都会让本测试失败，防止文档整合后留下死链。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 复刻 GitHub slugger：小写 → 去掉标点 → 空白转 `-`。
fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.trim().to_lowercase().chars() {
        let c = ch as u32;
        if (0x2000..=0x206F).contains(&c) || (0x2E00..=0x2E7F).contains(&c) {
            continue;
        }
        if matches!(
            ch,
            '\\' | '\''
                | '!'
                | '"'
                | '#'
                | '$'
                | '%'
                | '&'
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | '.'
                | '/'
                | ':'
                | ';'
                | '<'
                | '='
                | '>'
                | '?'
                | '@'
                | '['
                | ']'
                | '^'
                | '`'
                | '{'
                | '|'
                | '}'
                | '~'
        ) {
            continue;
        }
        if ch.is_whitespace() {
            out.push('-');
            continue;
        }
        out.push(ch);
    }
    out
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

/// 待检查的文档：根目录下的 Markdown（README / CHANGELOG 等）+ docs/ 下所有 .md
fn doc_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }
    }
    collect_markdown(&root.join("docs"), &mut files);
    files.sort();
    files
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn headings(content: &str) -> HashSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                return None;
            }
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            if !(1..=6).contains(&hashes) {
                return None;
            }
            let title = trimmed[hashes..].strip_prefix(' ')?;
            Some(slug(title))
        })
        .collect()
}

/// 提取 Markdown 内联链接目标（含图片链接），跳过代码块内的伪链接。
fn link_targets(content: &str) -> Vec<(usize, String)> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b']' && bytes[i + 1] == b'(' {
                let start = i + 2;
                if let Some(end_offset) = line[start..].find(')') {
                    let target = line[start..start + end_offset].trim();
                    if !target.is_empty() {
                        targets.push((idx + 1, target.to_string()));
                    }
                    i = start + end_offset + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
    targets
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("file:")
}

/// 归一化 `a/../b` 之类的相对路径，不要求目标已存在。
fn normalize(base: &Path, relative: &str) -> PathBuf {
    let joined = base.join(relative);
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            other => parts.push(other.as_os_str().to_os_string()),
        }
    }
    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    path
}

#[test]
fn documentation_links_and_anchors_resolve() {
    let root = workspace_root();
    let files = doc_files(&root);
    assert!(
        files.len() >= 8,
        "expected at least the root README plus 7 docs, found {}",
        files.len()
    );

    let mut anchors: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut contents: HashMap<PathBuf, String> = HashMap::new();
    for file in &files {
        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        anchors.insert(file.clone(), headings(&content));
        contents.insert(file.clone(), content);
    }

    let mut checked = 0usize;
    let mut broken = Vec::new();

    for file in &files {
        let content = &contents[file];
        let base = file.parent().expect("file has a parent directory");
        for (line_no, target) in link_targets(content) {
            if is_external(&target) {
                continue;
            }
            checked += 1;
            let (path_part, anchor) = match target.split_once('#') {
                Some((path, anchor)) => (path, Some(anchor.to_string())),
                None => (target.as_str(), None),
            };

            if path_part.is_empty() {
                if let Some(anchor) = anchor {
                    if !anchors[file].contains(&anchor) {
                        broken.push(format!(
                            "{}:{line_no}: same-file anchor missing: {target}",
                            file.display()
                        ));
                    }
                }
                continue;
            }

            let resolved = normalize(base, path_part);
            let Some(target_anchors) = anchors.get(&resolved) else {
                broken.push(format!(
                    "{}:{line_no}: file missing: {target}",
                    file.display()
                ));
                continue;
            };
            if let Some(anchor) = anchor {
                if !target_anchors.contains(&anchor) {
                    broken.push(format!(
                        "{}:{line_no}: anchor missing: {target}",
                        file.display()
                    ));
                }
            }
        }
    }

    assert!(
        checked >= 80,
        "expected to check at least 80 links, found {checked}"
    );
    assert!(
        broken.is_empty(),
        "broken documentation links ({}):\n{}",
        broken.len(),
        broken.join("\n")
    );
}

#[test]
fn spec_documents_do_not_contain_roadmaps() {
    // §11 治理规则第 2 条：路线图/里程碑只允许出现在 DEVELOPMENT_PLAN.md
    let root = workspace_root();
    for entry in std::fs::read_dir(root.join("docs")).expect("docs/ should exist") {
        let path = entry.expect("readable dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !path.extension().is_some_and(|e| e == "md") || name == "DEVELOPMENT_PLAN.md" {
            continue;
        }
        let content = std::fs::read_to_string(&path).expect("doc readable");
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            // 只禁止"路线图章节标题"，正文中引用开发计划不算违规
            if trimmed.starts_with('#')
                && (trimmed.contains("路线图")
                    || trimmed.contains("Roadmap")
                    || trimmed.contains("里程碑"))
            {
                panic!(
                    "{}:{}: 规范文档不应包含路线图章节：{trimmed}",
                    path.display(),
                    line_no + 1
                );
            }
        }
    }
}
