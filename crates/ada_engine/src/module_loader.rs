//! 文件 import 的加载与声明合并（T8.7 / T8.8）。
//!
//! 语义（见 docs/plans/2026-10-13-import-and-structure-like-design.md）：
//! - **相对当前文件解析**：`import { a } from "./lib.ada"` 相对被导入文件的目录。
//! - **root 边界**：规范化后必须仍在 import root 内，`..` 逃逸被拒绝。
//! - **纯声明合并**：只吸收被导入文件的顶层 `fn`/`let` 声明，**不执行**其顶层副作用
//!   （顶层表达式语句被丢弃）。
//! - **缓存 + 循环检测**：同一文件只加载一次；`A -> B -> A` 报错。
//!
//! v1 的可见性为「合并被导入文件的全部顶层声明」（具名列表用于**校验导出存在**）；
//! 「仅具名可见」的严格隔离留作后续增强（见设计文档 §3.8 的说明）。

use crate::Error;
use ada_ast::{ImportKind, Program, Span, Stmt};
use ada_parser::Parser;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) struct ModuleLoader {
    /// import 根目录（未配置时文件 import 一律拒绝）
    root: Option<PathBuf>,
    /// 已加载文件 -> 其顶层声明（其自身 import 已解析并合并）
    cache: HashMap<PathBuf, Vec<Stmt>>,
    /// 正在加载的文件（循环检测）
    loading: HashSet<PathBuf>,
}

impl ModuleLoader {
    pub(crate) fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            cache: HashMap::new(),
            loading: HashSet::new(),
        }
    }

    /// 把 `program` 里的 import 展开：返回「被导入文件的可声明语句 + 当前文件去掉 import 后的语句」。
    ///
    /// `current_dir` 是 `program` 所在文件的目录（用于解析相对路径）。
    pub(crate) fn resolve(
        &mut self,
        program: Program,
        current_dir: &Path,
    ) -> Result<Program, Error> {
        let mut merged: Vec<Stmt> = Vec::new();

        for stmt in &program.stmts {
            if let Stmt::Import { kind, path, span } = stmt {
                if let Some(module) = path.strip_prefix("std:") {
                    return Err(import_err(
                        format!("std module imports are not yet supported ('std:{module}')"),
                        *span,
                    ));
                }
                let decls = self.load_file(path, current_dir, *span)?;
                if let ImportKind::Named(names) = kind {
                    for name in names {
                        let found = decls
                            .iter()
                            .any(|d| top_level_name(d).as_deref() == Some(name.as_str()));
                        if !found {
                            return Err(import_err(
                                format!("module '{path}' has no exported declaration '{name}'"),
                                *span,
                            ));
                        }
                    }
                }
                merged.extend(decls);
            }
        }

        for stmt in program.stmts {
            if !matches!(stmt, Stmt::Import { .. }) {
                merged.push(stmt);
            }
        }

        Ok(Program { stmts: merged })
    }

    fn load_file(
        &mut self,
        import_path: &str,
        current_dir: &Path,
        span: Span,
    ) -> Result<Vec<Stmt>, Error> {
        let root_raw = self.root.as_ref().ok_or_else(|| {
            import_err(
                format!(
                    "file import '{import_path}' is not allowed: no import root configured \
                     (use Engine::set_import_root or CLI --import-root)"
                ),
                span,
            )
        })?;
        let root = root_raw
            .canonicalize()
            .map_err(|e| import_err(format!("import root is unusable: {e}"), span))?;

        let candidate = current_dir.join(import_path);
        let canonical = candidate
            .canonicalize()
            .map_err(|_| import_err(format!("import file not found: {import_path}"), span))?;

        if !canonical.starts_with(&root) {
            return Err(import_err(
                format!("import path escapes root: {import_path}"),
                span,
            ));
        }

        if self.loading.contains(&canonical) {
            return Err(import_err(
                format!("circular import detected: {}", canonical.display()),
                span,
            ));
        }
        if let Some(decls) = self.cache.get(&canonical) {
            return Ok(decls.clone());
        }

        let source = std::fs::read_to_string(&canonical)
            .map_err(|e| import_err(format!("cannot read '{import_path}': {e}"), span))?;
        let program = Parser::from_source(&source)
            .and_then(|mut parser| parser.parse_program())
            .map_err(Error::Parse)?;

        self.loading.insert(canonical.clone());
        let dir = canonical
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| current_dir.to_path_buf());
        let resolved = self.resolve(program, &dir)?;
        self.loading.remove(&canonical);

        // 纯声明合并：只保留顶层声明，丢弃顶层副作用（表达式语句等）
        let decls: Vec<Stmt> = resolved.stmts.into_iter().filter(is_declaration).collect();
        self.cache.insert(canonical, decls.clone());
        Ok(decls)
    }
}

/// 声明性语句：仅这些会被跨文件吸收（不触发顶层副作用）。
fn is_declaration(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Fn { .. } | Stmt::Let { .. })
}

/// 顶层声明的名字（用于具名导入校验）。
fn top_level_name(stmt: &Stmt) -> Option<String> {
    match stmt {
        Stmt::Fn { name, .. } | Stmt::Let { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn import_err(message: String, span: Span) -> Error {
    Error::Import {
        message,
        line: span.line,
        col: span.col,
    }
}
