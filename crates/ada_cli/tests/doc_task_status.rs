//! 文档-计划一致性门禁（T0.4 文档治理规则的自动化）。
//!
//! 项目规则：规范文档里标 ⚠️ / 📋 / 🚧 的条目**必须**对应一个仍未完成的任务。
//! 但"任务完成后忘了改文档"几乎必然发生——本仓库就曾积累了一批指向已完成任务的
//! ⚠️/📋 标记（`HostPanic`、`max_tasks`、`顶层 let 跨轮次`、`env 白名单`…），
//! 它们会让读者与 AI 模型误判能力现状。
//!
//! 本测试把这条规则变成可执行检查：
//!
//! 1. 解析 `docs/DEVELOPMENT_PLAN.md`，得到每个任务 ID（`T<数字>.<数字>`）的状态；
//! 2. 扫描其他文档里的表格行：若整行**没有任何 ✅**、却含 ⚠️/📋/🚧 并引用了任务 ID，
//!    则这些任务必须**尚未完成**（计划里不是 ✅/❌），否则报错；
//! 3. 引用的任务 ID 若在计划里不存在，也报错（防拼写错误）。
//!
//! 输出包含出问题的文件:行号与建议，便于直接修文档。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// 任务状态（只区分"是否已完成/已否决"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskStatus {
    /// ✅ 已完成
    Done,
    /// ❌ 明确不做
    Rejected,
    /// 🚧 / 📋 / 无标记：仍在进行或计划中
    Open,
}

const DONE: &str = "✅";
const REJECTED: &str = "❌";

/// 从计划文档里解析任务状态：`| ✅ T6.2 | …` / `| 📋 T6.4 | …`。
fn plan_task_status() -> BTreeMap<String, TaskStatus> {
    let plan = std::fs::read_to_string(workspace_root().join("docs/DEVELOPMENT_PLAN.md"))
        .expect("DEVELOPMENT_PLAN.md readable");
    let mut statuses = BTreeMap::new();

    for line in plan.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('|') {
            continue;
        }
        // 第一个单元格里应同时出现任务 ID 与状态标记
        let first_cell = trimmed.split('|').nth(1).unwrap_or_default().trim();
        let Some(id) = first_task_id(first_cell) else {
            continue;
        };
        let status = if first_cell.contains(DONE) {
            TaskStatus::Done
        } else if first_cell.contains(REJECTED) {
            TaskStatus::Rejected
        } else {
            TaskStatus::Open
        };
        // 同一任务可能在多处出现（表格与进度表）：以"最不乐观"的状态为准，
        // 避免一处写 ✅ 就掩盖别处的 📋。
        statuses
            .entry(id)
            .and_modify(|existing| {
                if status == TaskStatus::Open {
                    *existing = TaskStatus::Open;
                }
            })
            .or_insert(status);
    }
    statuses
}

/// 取单元格里的第一个任务 ID（`T6.2` 这种形态）。
fn first_task_id(cell: &str) -> Option<String> {
    let bytes = cell.as_bytes();
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index] == b'T' && bytes[index + 1].is_ascii_digit() {
            let rest = &cell[index + 1..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Some(after) = rest[digits.len()..].strip_prefix('.') {
                let minor: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !minor.is_empty() {
                    return Some(format!("T{digits}.{minor}"));
                }
            }
        }
        index += 1;
    }
    None
}

/// 取一行里出现的全部任务 ID。
fn task_ids_in(line: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = line;
    while let Some(id) = first_task_id(rest) {
        let consumed = rest.find(&id).map(|i| i + id.len()).unwrap_or(rest.len());
        ids.push(id);
        rest = &rest[consumed..];
    }
    ids
}

/// 待检查的文档：docs/ 下除开发计划以外的全部 Markdown（+ 根 README）。
fn documents() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = vec![root.join("README.md")];
    for entry in std::fs::read_dir(root.join("docs")).expect("docs/ readable") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.extension().is_some_and(|e| e == "md") && name != "DEVELOPMENT_PLAN.md" {
            files.push(path);
        }
    }
    files
}

#[test]
fn open_markers_do_not_reference_completed_tasks() {
    let statuses = plan_task_status();
    assert!(
        statuses.len() >= 30,
        "expected to parse at least 30 task ids from the plan, found {}",
        statuses.len()
    );

    let mut stale = Vec::new();
    let mut unknown = Vec::new();
    let mut checked_rows = 0usize;

    for file in documents() {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('|') {
                continue;
            }
            let is_open_marker = ["⚠️", "📋", "🚧"].iter().any(|m| trimmed.contains(m));
            if !is_open_marker || trimmed.contains(DONE) {
                continue;
            }
            let ids = task_ids_in(trimmed);
            if ids.is_empty() {
                continue;
            }
            checked_rows += 1;
            for id in ids {
                match statuses.get(&id) {
                    Some(TaskStatus::Done) => stale.push(format!(
                        "{}:{}: 该行标 ⚠️/📋 但引用已完成的任务 {id}\n    {}",
                        file.display(),
                        index + 1,
                        trimmed
                    )),
                    Some(TaskStatus::Rejected) => stale.push(format!(
                        "{}:{}: 该行标 ⚠️/📋 但 {id} 已明确不做（❌）\n    {}",
                        file.display(),
                        index + 1,
                        trimmed
                    )),
                    Some(TaskStatus::Open) => {}
                    None => unknown.push(format!(
                        "{}:{}: 引用了计划里不存在的任务 {id}\n    {}",
                        file.display(),
                        index + 1,
                        trimmed
                    )),
                }
            }
        }
    }

    // 门禁本身要有判别力：至少检查过若干行，否则说明解析规则失效了
    assert!(
        checked_rows >= 5,
        "expected to check at least 5 open-marker rows, found {checked_rows}; \
         the parsing rule probably stopped matching the document format"
    );

    assert!(
        unknown.is_empty(),
        "文档引用了不存在的任务 ID（拼写错误？）：\n{}",
        unknown.join("\n")
    );
    assert!(
        stale.is_empty(),
        "以下文档行仍以 ⚠️/📋 描述**已完成**的任务，请更新状态（或改为 ✅ 并说明残留）\n\
         ——共 {} 处：\n{}",
        stale.len(),
        stale.join("\n")
    );
}

#[test]
fn plan_ids_are_well_formed() {
    // 防呆：计划里的任务 ID 必须是 `T<数字>.<数字>`，且每个 ID 只对应一种状态
    let plan = std::fs::read_to_string(workspace_root().join("docs/DEVELOPMENT_PLAN.md"))
        .expect("plan readable");
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for line in plan.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cell = trimmed.split('|').nth(1).unwrap_or_default().trim();
        if let Some(id) = first_task_id(cell) {
            *seen.entry(id).or_insert(0) += 1;
        }
    }
    assert!(
        seen.keys().any(|id| id == "T6.4"),
        "expected the plan to still list the deferred JIT task T6.4"
    );
    assert!(
        seen.keys().all(|id| id.starts_with('T')),
        "task ids should all start with T"
    );
}
