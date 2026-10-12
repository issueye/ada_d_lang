//! `fs` 模块的能力门禁与路径逃逸防护（开发计划 T7.5）。
//!
//! 这是本轮最需要"敌意输入"的部分：一旦宿主开了 `fs`，脚本就能碰到真实文件系统，
//! 因此测试不只验证"能读写"，更要验证**逃不掉**——`..`、绝对路径、符号链接、
//! 前缀混淆（`/root` vs `/root-evil`）都必须被拒绝，而且拒绝要**报错**而不是
//! 静默返回 `false`/`nil`（静默失败会把攻击变成"看起来没事"）。

use ada_engine::{Engine, Error, Sandbox, Value, VmError};
use std::path::{Path, PathBuf};

/// 每个测试用独立临时目录，避免并行测试互相干扰。
fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ada_fs_sandbox_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp root");
    dir.canonicalize().expect("canonicalize temp root")
}

fn engine_with_root(root: &Path) -> Engine {
    Engine::with_sandbox(Sandbox::strict().with_fs_root(root))
}

fn expect_native_error(result: Result<Value, Error>, needle: &str) {
    match result {
        Err(Error::Vm(VmError::NativeError(message))) => assert!(
            message.contains(needle),
            "expected an error containing {needle:?}, got {message:?}"
        ),
        other => panic!("expected NativeError containing {needle:?}, got {other:?}"),
    }
}

#[test]
fn fs_is_denied_without_an_explicit_root() {
    // 三档默认 profile 都不暴露 fs：能力"不注册"，访问得到 CapabilityDenied
    for sandbox in [
        Sandbox::strict(),
        Sandbox::standard(),
        Sandbox::permissive(),
    ] {
        let mut engine = Engine::with_sandbox(sandbox.clone());
        let err = engine
            .eval_value("fs.read_text(\"x.txt\")")
            .expect_err("fs must be denied");
        match err {
            Error::Vm(VmError::CapabilityDenied { name, .. }) => assert_eq!(name, "fs"),
            other => panic!(
                "expected CapabilityDenied for fs ({:?}), got {other:?}",
                sandbox.kind
            ),
        }
    }
}

#[test]
fn write_read_list_round_trip_inside_the_root() {
    let root = temp_root("roundtrip");
    let mut engine = engine_with_root(&root);

    // 模块不会隐式建目录：父目录不存在时写入必须失败
    expect_native_error(
        engine.eval_value("fs.write_text(\"notes/a.txt\", \"hello\")"),
        "fs.write_text",
    );

    std::fs::create_dir_all(root.join("notes")).expect("mkdir notes");
    std::fs::create_dir_all(root.join("sub")).expect("mkdir sub");
    assert_eq!(
        engine
            .eval::<Value>("fs.write_text(\"notes/a.txt\", \"hello\")")
            .expect("write"),
        Value::Bool(true)
    );
    engine
        .eval_value("fs.write_text(\"sub/b.txt\", \"world\")")
        .expect("write nested");

    assert_eq!(
        engine
            .eval::<String>("fs.read_text(\"sub/b.txt\")")
            .unwrap(),
        "world"
    );
    assert!(engine.eval::<bool>("fs.exists(\"sub/b.txt\")").unwrap());
    assert!(engine.eval::<bool>("fs.exists(\"sub\")").unwrap());
    assert!(engine.eval::<bool>("fs.is_dir(\"sub\")").unwrap());
    assert!(!engine.eval::<bool>("fs.is_dir(\"sub/b.txt\")").unwrap());

    // 缺失文件 → nil（与数组/map 的"缺就是 nil"一致）
    assert!(engine
        .eval::<Value>("fs.read_text(\"nope.txt\")")
        .unwrap()
        .is_nil());
    assert!(!engine.eval::<bool>("fs.exists(\"nope.txt\")").unwrap());

    // list 按名称排序（保证脚本输出确定）
    assert_eq!(
        engine
            .eval::<String>("fs.list(\"sub\") |> join(\",\")")
            .unwrap(),
        "b.txt"
    );
    assert_eq!(
        engine
            .eval::<String>("fs.list(\".\") |> join(\",\")")
            .unwrap(),
        "notes,sub"
    );

    // remove：存在 → true，重复 → false，目录 → 报错
    assert!(engine.eval::<bool>("fs.remove(\"sub/b.txt\")").unwrap());
    assert!(!engine.eval::<bool>("fs.remove(\"sub/b.txt\")").unwrap());
    expect_native_error(engine.eval_value("fs.remove(\"sub\")"), "is a directory");
}

#[test]
fn parent_components_cannot_escape_the_root() {
    let root = temp_root("escape_parent");
    let outside = root.parent().expect("parent").join("ada_fs_outside.txt");
    std::fs::write(&outside, "secret").expect("write outside");

    let mut engine = engine_with_root(&root);
    for attempt in [
        "../ada_fs_outside.txt",
        "sub/../../ada_fs_outside.txt",
        "..",
    ] {
        let source = format!("fs.read_text(\"{attempt}\")");
        expect_native_error(engine.eval_value(&source), "escapes the fs root");
    }
    // 写路径同样受限
    expect_native_error(
        engine.eval_value("fs.write_text(\"../evil.txt\", \"x\")"),
        "escapes the fs root",
    );
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn absolute_paths_outside_the_root_are_rejected() {
    let root = temp_root("escape_absolute");
    let mut engine = engine_with_root(&root);
    let outside = if cfg!(windows) {
        "C:\\Windows\\win.ini"
    } else {
        "/etc/passwd"
    };
    // 绝对路径在词法上可能合法（无 `..`），必须靠规范化 + 前缀校验拦住
    let source = format!("fs.read_text(\"{}\")", outside.replace('\\', "\\\\"));
    expect_native_error(engine.eval_value(&source), "escapes the fs root");
}

#[test]
fn prefix_confusion_sibling_directory_is_rejected() {
    let base = temp_root("prefix");
    let root = base.join("root");
    let evil = base.join("root-evil");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::create_dir_all(&evil).expect("evil");
    std::fs::write(evil.join("secret.txt"), "nope").expect("write");

    let root = root.canonicalize().expect("canonicalize root");
    let mut engine = engine_with_root(&root);
    let attempt = evil.join("secret.txt").display().to_string();
    let source = format!("fs.read_text(\"{}\")", attempt.replace('\\', "\\\\"));
    expect_native_error(engine.eval_value(&source), "escapes the fs root");
}

#[test]
fn symlinks_pointing_outside_are_rejected() {
    let root = temp_root("symlink");
    let outside_file = std::env::temp_dir().join("ada_fs_symlink_target.txt");
    std::fs::write(&outside_file, "secret").expect("write target");

    let link = root.join("link.txt");
    let created = make_file_symlink(&outside_file, &link);
    if !created {
        // Windows 上创建符号链接需要开发者模式/管理员权限：拿不到就跳过这一条
        println!("note: 无法创建符号链接，跳过该用例（Windows 权限限制）");
        let _ = std::fs::remove_file(&outside_file);
        return;
    }

    let mut engine = engine_with_root(&root);
    expect_native_error(
        engine.eval_value("fs.read_text(\"link.txt\")"),
        "escapes the fs root",
    );
    let _ = std::fs::remove_file(&outside_file);
}

#[cfg(windows)]
fn make_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(not(windows))]
fn make_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn oversized_io_is_refused() {
    // 内存预算 4 KiB → fs 的单次 I/O 上限取二者的较小值
    let root = temp_root("oversize");
    let mut engine = Engine::with_sandbox(
        Sandbox::strict()
            .with_fs_root(&root)
            .with_memory_limit(Some(4 * 1024)),
    );
    let big = "x".repeat(8 * 1024);
    let source = format!("fs.write_text(\"big.txt\", \"{big}\")");
    expect_native_error(engine.eval_value(&source), "over the");
    assert!(
        !root.join("big.txt").exists(),
        "oversized write must not happen"
    );

    std::fs::write(root.join("big.txt"), big).expect("write big file directly");
    expect_native_error(engine.eval_value("fs.read_text(\"big.txt\")"), "over the");
}

#[test]
fn malformed_paths_are_refused() {
    let root = temp_root("malformed");
    let mut engine = engine_with_root(&root);
    expect_native_error(engine.eval_value("fs.read_text(\"\")"), "must not be empty");
    // 目录不能当文件读
    std::fs::create_dir_all(root.join("dir")).expect("mkdir");
    expect_native_error(engine.eval_value("fs.read_text(\"dir\")"), "is a directory");
    // 参数缺失
    expect_native_error(engine.eval_value("fs.read_text()"), "expects a path string");
    // 注：含 NUL 的路径在 `builtins::fs` 的单元测试里覆盖（Ada-D 字符串语法
    // 不便直接表达 NUL 字节）
}

#[test]
fn unusable_root_fails_loudly_instead_of_silently_allowing() {
    let missing = std::env::temp_dir().join("ada_fs_root_that_does_not_exist");
    let _ = std::fs::remove_dir_all(&missing);
    let mut engine = Engine::with_sandbox(Sandbox::strict().with_fs_root(&missing));
    expect_native_error(
        engine.eval_value("fs.read_text(\"x.txt\")"),
        "not an accessible directory",
    );
}

#[test]
fn module_is_visible_and_reports_its_root() {
    let root = temp_root("module_shape");
    let mut engine = engine_with_root(&root);
    assert_eq!(engine.eval::<String>("type_of(fs)").unwrap(), "map");
    let reported = engine.eval::<String>("fs.root").unwrap();
    assert_eq!(
        PathBuf::from(&reported)
            .canonicalize()
            .expect("canonicalize"),
        root,
        "fs.root should be the canonical root"
    );
}
