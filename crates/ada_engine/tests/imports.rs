//! 文件 import 的集成测试（T8.7 / T8.8）。
//!
//! 覆盖：正常合并、无 root 拒绝、路径逃逸拒绝、文件缺失、具名导出缺失、
//! 循环依赖、`std:` 暂未支持、顶层副作用不执行。

use ada_engine::{Engine, Error, Value};
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("ada_import_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

#[test]
fn import_file_merges_declarations() {
    let dir = temp_dir("ok");
    write(&dir, "lib.ada", "fn double(x) = x * 2\n");
    write(
        &dir,
        "main.ada",
        "import { double } from \"lib.ada\"\ndouble(21)\n",
    );

    let mut engine = Engine::new();
    engine.set_import_root(dir.clone());
    let val = engine.eval_file(&dir.join("main.ada")).unwrap();
    assert_eq!(val.as_int(), Some(42));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_without_root_is_denied() {
    let dir = temp_dir("noroot");
    write(&dir, "lib.ada", "fn f() = 1\n");
    write(&dir, "main.ada", "import { f } from \"lib.ada\"\nf()\n");

    let mut engine = Engine::new();
    let err = engine.eval_file(&dir.join("main.ada")).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_path_escape_is_rejected() {
    let dir = temp_dir("escape");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    write(&dir, "outside.ada", "fn f() = 1\n");
    write(
        &sub,
        "main.ada",
        "import { f } from \"../outside.ada\"\nf()\n",
    );

    let mut engine = Engine::new();
    engine.set_import_root(sub.clone());
    let err = engine.eval_file(&sub.join("main.ada")).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_missing_file_errors() {
    let dir = temp_dir("missing");
    write(&dir, "main.ada", "import { f } from \"nope.ada\"\nf()\n");
    let mut engine = Engine::new();
    engine.set_import_root(dir.clone());
    let err = engine.eval_file(&dir.join("main.ada")).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_named_missing_export_errors() {
    let dir = temp_dir("missing_export");
    write(&dir, "lib.ada", "fn present() = 1\n");
    write(
        &dir,
        "main.ada",
        "import { absent } from \"lib.ada\"\nabsent()\n",
    );
    let mut engine = Engine::new();
    engine.set_import_root(dir.clone());
    let err = engine.eval_file(&dir.join("main.ada")).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn circular_import_is_rejected() {
    let dir = temp_dir("cycle");
    write(&dir, "a.ada", "import { b } from \"b.ada\"\nfn a() = 1\n");
    write(&dir, "b.ada", "import { a } from \"a.ada\"\nfn b() = 2\n");
    write(&dir, "main.ada", "import { a } from \"a.ada\"\na()\n");
    let mut engine = Engine::new();
    engine.set_import_root(dir.clone());
    let err = engine.eval_file(&dir.join("main.ada")).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn std_namespace_import_binds_the_whole_module() {
    let mut engine = Engine::new();
    let src = r#"import json from "std:json"
let m = json.parse("{\"a\": 41}")
m.a + 1"#;
    let val: i64 = engine.eval(src).unwrap();
    assert_eq!(val, 42);
}

#[test]
fn std_named_import_binds_the_function() {
    let mut engine = Engine::new();
    let src = r#"import { sqrt } from "std:math"
sqrt(16.0)"#;
    let val: f64 = engine.eval(src).unwrap();
    assert_eq!(val, 4.0);
}

#[test]
fn std_named_import_of_missing_export_errors() {
    let mut engine = Engine::new();
    let err = engine
        .eval::<Value>("import { nope } from \"std:json\"\nnope")
        .unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
}

#[test]
fn unknown_std_module_errors() {
    let mut engine = Engine::new();
    let err = engine
        .eval::<Value>("import x from \"std:nope\"\nx")
        .unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "got {err:?}");
}

#[test]
fn std_env_import_grants_read_all() {
    // 决策 D12「导入即授权」：即便引擎是 strict，import 后 env 也可读全部键
    let mut engine = Engine::new();
    let val = engine
        .eval::<Value>("import env from \"std:env\"\nenv.allowed()")
        .unwrap();
    match val {
        Value::Array(a) => {
            let items = a.read().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].to_string(), "*");
        }
        other => panic!("expected ['*'], got {other:?}"),
    }
}

#[test]
fn imported_top_level_side_effects_are_not_executed() {
    let dir = temp_dir("no_side_effects");
    // lib 顶层调用一个未定义函数：若被当成语句执行会报错；纯声明合并应丢弃它
    write(
        &dir,
        "lib.ada",
        "fn f() = 7\nthis_side_effect_would_fail()\n",
    );
    write(&dir, "main.ada", "import { f } from \"lib.ada\"\nf()\n");
    let mut engine = Engine::new();
    engine.set_import_root(dir.clone());
    let val = engine.eval_file(&dir.join("main.ada")).unwrap();
    assert_eq!(val.as_int(), Some(7));
    let _ = std::fs::remove_dir_all(&dir);
}
