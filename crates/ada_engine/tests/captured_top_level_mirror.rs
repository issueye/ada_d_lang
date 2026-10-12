//! 被闭包捕获的**顶层绑定**，其全局镜像的同步性（缺口 G-L16，任务 T8.5）。
//!
//! 现状由两个测试固定，两者形状不同、行为也不同——这正是缺口难以被发现的原因：
//!
//! - 顶层声明与闭包在**同一个编译单元**里：闭包捕获的是局部 `Cell`，而全局槽里存的是
//!   声明时刻的**快照**，于是两份值会各自演化（本文件第一个测试记录该现状）。
//! - 声明与闭包在**不同的求值**里（典型 REPL 逐行输入）：闭包体内对 `c` 的引用在编译期
//!   解析为**全局名**，读写都落在全局表上，因此是一致的一（第二个测试保护这一点，
//!   T8.5 的修复不能破坏它）。
//!
//! ⚠️ 第一个测试里的断言记录的是**当前行为**，不是期望语义：修复 T8.5 时应把它们改成
//! "镜像跟随闭包写入"（`1`、`2`、`1`…），并把本文件与 SPEC_LANGUAGE §5.1 的说明同步更新。

use ada_engine::{Engine, Value};

/// 需求：同一编译单元内声明 + 闭包 → 全局镜像应跟随闭包写入。
/// 现状：镜像停留在声明时刻的值，闭包在自己的 Cell 上继续累加（两者分叉）。
#[test]
fn declaration_and_closure_in_one_unit_desynchronise_the_mirror() {
    let mut engine = Engine::new();
    engine
        .eval::<Value>("mut c = 0\nlet inc = () => { c += 1 }")
        .unwrap();

    assert_eq!(engine.get_global("c"), Some(&Value::Int(0)));

    // 闭包写入只落在它捕获的 Cell 上，镜像不动
    engine.eval::<Value>("inc()").unwrap();
    assert_eq!(
        engine.get_global("c"),
        Some(&Value::Int(0)),
        "现状（T8.5 修复后应为 1）：全局镜像是声明时刻的快照"
    );
    // 因此后续求值读到的也是旧值
    assert_eq!(engine.eval::<Value>("c").unwrap(), Value::Int(0));

    // 闭包自己仍在累加（内部的 Cell 是 1、2…），说明确实是两份值
    assert_eq!(engine.eval::<Value>("inc()").unwrap(), Value::Int(2));

    // 宿主写入只落全局表，闭包看不到（继续在自己的 Cell 上走）
    engine.eval::<Value>("c = 100").unwrap();
    assert_eq!(engine.get_global("c"), Some(&Value::Int(100)));
    assert_eq!(
        engine.eval::<Value>("inc()").unwrap(),
        Value::Int(3),
        "现状（T8.5 修复后应为 101）：闭包看不到宿主/全局侧的写入"
    );
    assert_eq!(engine.eval::<Value>("c").unwrap(), Value::Int(100));
}

/// 声明与闭包在不同求值里：闭包体内的引用编译为全局名，读写都在全局表上，因此一致。
/// T8.5 的修复（让被捕获的顶层绑定在全局槽里保存 Cell）必须保持这个形状仍然正确。
#[test]
fn closure_compiled_in_a_later_unit_reads_and_writes_the_global() {
    let mut engine = Engine::new();
    engine.eval::<Value>("mut c = 0").unwrap();
    engine.eval::<Value>("let inc = () => { c += 1 }").unwrap();

    engine.eval::<Value>("inc()").unwrap();
    assert_eq!(engine.get_global("c"), Some(&Value::Int(1)));
    assert_eq!(engine.eval::<Value>("c").unwrap(), Value::Int(1));

    // 全局侧写入对闭包可见
    engine.eval::<Value>("c = 100").unwrap();
    assert_eq!(engine.eval::<Value>("inc()").unwrap(), Value::Int(101));
    assert_eq!(engine.eval::<Value>("c").unwrap(), Value::Int(101));
}
