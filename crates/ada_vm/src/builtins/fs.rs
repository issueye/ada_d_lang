//! `fs` 模块（T7.5）：**受限根目录**上的文件读写。
//!
//! # 安全模型
//!
//! 这个模块只在宿主显式调用 [`Sandbox::with_fs_root`](crate::sandbox::Sandbox::with_fs_root)
//! 时才注册（默认三档 profile 都没有 `fs`，访问会得到 `CapabilityDenied`）。
//! 一旦开启，脚本给出的任何路径都必须落在根目录内：
//!
//! 1. **禁用路径语法**：空路径、含 NUL 的路径、以及任何 `..` 组件直接拒绝
//!    （词法层面的第一道闸，避免依赖后续规范化）。
//! 2. **规范化后再验证**（真正的那道闸）：对已存在的目标直接 `canonicalize`；
//!    对尚不存在的目标（写文件）规范化其**父目录**，再拼上文件名。符号链接因此
//!    会被解析出来，指向根目录之外的链接会被拒绝。
//! 3. **前缀校验**：规范化结果必须以规范化后的根目录开头（按路径组件比较，
//!    避免 `/tmp/root-evil` 命中 `/tmp/root` 这类前缀混淆）。
//!
//! 另外：单次读写有字节上限（与内存预算取小），目录列举有条目上限，且任何失败
//! 都返回普通错误而不是 panic。

use crate::value::Value;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// 单次 I/O 的字节上限（内存预算更小则取预算）。
const IO_LIMIT_BYTES: usize = 1024 * 1024;
/// `fs.list` 返回的最大条目数。
const LIST_LIMIT: usize = 10_000;

/// 创建 `fs` 模块，所有操作限制在 `root` 之下。
pub fn create_fs_module(root: PathBuf, memory_limit: Option<usize>) -> Value {
    // 根目录只规范化一次；失败时模块仍然注册，但每次调用都会报出同一个错误
    // （比"悄悄不注册"更容易诊断，也不会 panic）。
    let canonical_root: Result<PathBuf, String> = root.canonicalize().map_err(|err| {
        format!(
            "fs root '{}' is not an accessible directory: {err}",
            root.display()
        )
    });
    let limit = memory_limit.map_or(IO_LIMIT_BYTES, |cap| cap.min(IO_LIMIT_BYTES));

    let mut module = HashMap::new();

    // fs.root -> string：让脚本/测试能看清自己被限制在哪
    module.insert(
        "root".to_string(),
        Value::string(
            canonical_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| root.display().to_string()),
        ),
    );

    let resolve = {
        let canonical_root = canonical_root.clone();
        move |user_path: &str| -> Result<PathBuf, String> {
            match &canonical_root {
                Ok(root) => resolve_within(root, user_path),
                Err(err) => Err(err.clone()),
            }
        }
    };

    // fs.read_text(path) -> string | nil（文件不存在时 nil）
    {
        let resolve = resolve.clone();
        module.insert(
            "read_text".to_string(),
            Value::native_fn("fs.read_text", move |args| {
                let path = string_arg(args, "fs.read_text")?;
                let resolved = resolve(&path)?;
                match std::fs::metadata(&resolved) {
                    Ok(meta) if meta.is_dir() => {
                        return Err(format!("fs.read_text: '{path}' is a directory"));
                    }
                    Ok(meta) if meta.len() as usize > limit => {
                        return Err(format!(
                            "fs.read_text: '{path}' is {} bytes, over the {limit}-byte limit",
                            meta.len()
                        ));
                    }
                    Ok(_) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Value::Nil)
                    }
                    Err(err) => return Err(format!("fs.read_text: {err}")),
                }
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => Ok(Value::string(text)),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Value::Nil),
                    Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                        Err(format!("fs.read_text: '{path}' is not valid UTF-8"))
                    }
                    Err(err) => Err(format!("fs.read_text: {err}")),
                }
            }),
        );
    }

    // fs.write_text(path, text) -> true
    {
        let resolve = resolve.clone();
        module.insert(
            "write_text".to_string(),
            Value::native_fn("fs.write_text", move |args| {
                let path = string_arg(args, "fs.write_text")?;
                let text = args
                    .get(1)
                    .ok_or_else(|| "fs.write_text expects (path, text)".to_string())?
                    .to_string();
                if text.len() > limit {
                    return Err(format!(
                        "fs.write_text: {} bytes over the {limit}-byte limit",
                        text.len()
                    ));
                }
                let resolved = resolve(&path)?;
                std::fs::write(&resolved, text.as_bytes())
                    .map_err(|err| format!("fs.write_text: {err}"))?;
                Ok(Value::Bool(true))
            }),
        );
    }

    // fs.exists(path) -> bool
    {
        let resolve = resolve.clone();
        module.insert(
            "exists".to_string(),
            Value::native_fn("fs.exists", move |args| {
                let path = string_arg(args, "fs.exists")?;
                // 越界路径在这里同样报错，而不是返回 false——静默 false 会掩盖攻击
                let resolved = resolve(&path)?;
                Ok(Value::Bool(resolved.exists()))
            }),
        );
    }

    // fs.is_dir(path) -> bool
    {
        let resolve = resolve.clone();
        module.insert(
            "is_dir".to_string(),
            Value::native_fn("fs.is_dir", move |args| {
                let path = string_arg(args, "fs.is_dir")?;
                let resolved = resolve(&path)?;
                Ok(Value::Bool(resolved.is_dir()))
            }),
        );
    }

    // fs.list(path) -> [string]（按名称排序，保证脚本输出确定）
    {
        let resolve = resolve.clone();
        module.insert(
            "list".to_string(),
            Value::native_fn("fs.list", move |args| {
                let path = string_arg(args, "fs.list")?;
                let resolved = resolve(&path)?;
                let entries =
                    std::fs::read_dir(&resolved).map_err(|err| format!("fs.list: {err}"))?;
                let mut names: Vec<String> = entries
                    .take(LIST_LIMIT)
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name().to_string_lossy().to_string())
                    .collect();
                names.sort();
                Ok(Value::array(names.into_iter().map(Value::string).collect()))
            }),
        );
    }

    // fs.remove(path) -> bool（仅文件；不存在返回 false）
    {
        let resolve = resolve.clone();
        module.insert(
            "remove".to_string(),
            Value::native_fn("fs.remove", move |args| {
                let path = string_arg(args, "fs.remove")?;
                let resolved = resolve(&path)?;
                match std::fs::metadata(&resolved) {
                    Ok(meta) if meta.is_dir() => Err(format!(
                        "fs.remove: '{path}' is a directory (only files can be removed)"
                    )),
                    Ok(_) => {
                        std::fs::remove_file(&resolved)
                            .map_err(|err| format!("fs.remove: {err}"))?;
                        Ok(Value::Bool(true))
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        Ok(Value::Bool(false))
                    }
                    Err(err) => Err(format!("fs.remove: {err}")),
                }
            }),
        );
    }

    Value::map(module)
}

fn string_arg(args: &[Value], who: &str) -> Result<String, String> {
    match args.first() {
        Some(value) if !value.is_nil() => Ok(value.to_string()),
        _ => Err(format!("{who} expects a path string")),
    }
}

/// 把脚本给的路径解析成根目录内的绝对路径；任何逃逸尝试都返回错误。
pub fn resolve_within(canonical_root: &Path, user_path: &str) -> Result<PathBuf, String> {
    if user_path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if user_path.contains('\0') {
        return Err("path must not contain NUL".to_string());
    }

    let candidate = Path::new(user_path);

    // 第一道闸：词法上拒绝 `..`（无论它出现在哪里）
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!(
            "path '{user_path}' escapes the fs root (parent components are not allowed)"
        ));
    }

    // 相对路径挂到根上；绝对路径保持原样，交给后面的前缀校验判定
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        canonical_root.join(candidate)
    };

    // 第二道闸：规范化（解析符号链接）后必须仍在根目录内
    let normalized = canonicalize_deepest(&joined)?;
    if !within(canonical_root, &normalized) {
        return Err(format!(
            "path '{user_path}' escapes the fs root '{}'",
            canonical_root.display()
        ));
    }
    Ok(normalized)
}

/// 规范化"最深的已存在祖先"，再拼回剩余部分。
///
/// 这样写新文件（目标还不存在）也能得到解析过符号链接的路径。
fn canonicalize_deepest(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();

    loop {
        match existing.canonicalize() {
            Ok(canonical) => {
                let mut result = canonical;
                for name in tail.iter().rev() {
                    result.push(name);
                }
                return Ok(result);
            }
            Err(err) => {
                if existing.parent().is_none() {
                    return Err(format!("cannot resolve path: {err}"));
                }
                let name = existing
                    .file_name()
                    .ok_or_else(|| format!("cannot resolve path: {err}"))?
                    .to_os_string();
                tail.push(name);
                existing = existing
                    .parent()
                    .ok_or_else(|| format!("cannot resolve path: {err}"))?
                    .to_path_buf();
            }
        }
    }
}

/// `path` 是否位于 `root` 之下（按组件比较，避免前缀混淆）。
fn within(root: &Path, path: &Path) -> bool {
    let mut root_components = root.components();
    let mut path_components = path.components();
    loop {
        match (root_components.next(), path_components.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(expected), Some(actual)) if expected == actual => continue,
            (Some(_), Some(_)) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ada_fs_test_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp root");
        dir.canonicalize().expect("canonicalize")
    }

    #[test]
    fn accepts_paths_inside_the_root() {
        let root = temp_root("inside");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        std::fs::write(root.join("sub/a.txt"), "hi").expect("write");

        let resolved = resolve_within(&root, "sub/a.txt").expect("should resolve");
        assert!(resolved.starts_with(&root));
        assert_eq!(std::fs::read_to_string(resolved).unwrap(), "hi");

        // 根目录自身
        assert!(resolve_within(&root, ".").is_ok());
    }

    #[test]
    fn rejects_parent_components() {
        let root = temp_root("parent");
        for attempt in ["../outside.txt", "sub/../../outside.txt", ".."] {
            let err = resolve_within(&root, attempt).expect_err("must reject");
            assert!(err.contains("escapes the fs root"), "{attempt}: {err}");
        }
    }

    #[test]
    fn rejects_absolute_paths_outside_the_root() {
        let root = temp_root("absolute");
        let outside = if cfg!(windows) {
            "C:\\Windows\\win.ini"
        } else {
            "/etc/passwd"
        };
        let err = resolve_within(&root, outside).expect_err("must reject");
        assert!(err.contains("escapes the fs root"), "{err}");
    }

    #[test]
    fn rejects_empty_and_nul_paths() {
        let root = temp_root("empty");
        assert!(resolve_within(&root, "").is_err());
        assert!(resolve_within(&root, "a\0b").is_err());
    }

    #[test]
    fn allows_writing_a_file_that_does_not_exist_yet() {
        let root = temp_root("newfile");
        let resolved = resolve_within(&root, "brand/new.txt").expect("resolve");
        assert!(resolved.starts_with(&root));
        assert!(!resolved.exists());
    }

    #[test]
    fn rejects_prefix_confusion() {
        // `/tmp/root` vs `/tmp/root-evil` 不能被当成"在根内"
        let base = temp_root("prefix");
        let root = base.join("root");
        let evil = base.join("root-evil");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&evil).unwrap();
        std::fs::write(evil.join("secret.txt"), "nope").unwrap();

        let root = root.canonicalize().unwrap();
        let err = resolve_within(&root, &evil.join("secret.txt").display().to_string())
            .expect_err("must reject");
        assert!(err.contains("escapes the fs root"), "{err}");
    }
}
