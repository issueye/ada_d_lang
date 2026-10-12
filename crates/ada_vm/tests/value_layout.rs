//! 值模型布局门禁（开发计划 T6.2b）。
//!
//! `Value` 在 M6 之前是 **48 字节**：`NativeFn { name: String(24B), callback: Arc<dyn Fn>(16B) }`
//! 与 `Intrinsic { receiver: Option<Box<Value>>, args: Vec<Value>(24B) }` 两个"胖变体"决定
//! 了枚举大小。而 VM 的寄存器文件是 `Vec<Value>`（2048 槽），48 字节意味着 **96 KiB** ——
//! 远超典型 32 KiB 的 L1 数据缓存，每次寄存器读写都在打内存层次。
//!
//! 把两个胖变体改为 `Arc<载荷>` 后 `Value` 降到 **16 字节**，寄存器文件缩到 **32 KiB**。
//! 本测试把这两条性质固定住：任何人再往 `Value` 里塞大字段都会在这里失败，
//! 并被迫先读一遍这段说明。

use ada_vm::Value;
use std::sync::Arc;

/// 寄存器文件大小（`Vm::with_sandbox` 初始 `vec![Value::Nil; 2048]`）。
const REGISTER_SLOTS: usize = 2048;
/// 典型 L1 数据缓存大小。
const L1_BYTES: usize = 32 * 1024;

#[test]
fn value_stays_sixteen_bytes() {
    assert_eq!(
        std::mem::size_of::<Value>(),
        16,
        "Value grew past 16 bytes; box any new large variant payload \
         (see crates/ada_vm/src/value.rs::NativeFunction/IntrinsicCall)"
    );
    // Niche 优化：`Option<Value>` 不应额外占空间（否则 Option<Box<Value>> 之类的
    // 组合会悄悄变贵）
    assert_eq!(std::mem::size_of::<Option<Value>>(), 16);
}

#[test]
fn register_file_fits_in_l1() {
    let register_bytes = std::mem::size_of::<Value>() * REGISTER_SLOTS;
    assert!(
        register_bytes <= L1_BYTES,
        "register file is {register_bytes} bytes; it must stay within a {L1_BYTES}-byte L1"
    );
}

#[test]
fn native_functions_are_shared_not_copied() {
    let value = Value::native_fn("demo", |_args| Ok(Value::Nil));
    let clone = value.clone();
    match (&value, &clone) {
        (Value::NativeFn(a), Value::NativeFn(b)) => {
            assert!(
                Arc::ptr_eq(a, b),
                "cloning a native fn must share its payload, not deep-copy it"
            );
            assert_eq!(a.name, "demo");
        }
        other => panic!("expected NativeFn values, got {other:?}"),
    }
}

#[test]
fn adapter_binding_still_works_after_the_shrink() {
    use ada_vm::AdapterKind;

    let adapter = Value::intrinsic(AdapterKind::Map);
    assert_eq!(adapter.adapter_kind(), Some(AdapterKind::Map));

    // 方法形式：`xs.map` 会绑定接收者，且绑定后仍可识别是哪个适配器
    let bound = adapter.with_receiver(Value::array(vec![Value::Int(1)]));
    assert_eq!(bound.adapter_kind(), Some(AdapterKind::Map));
    match &bound {
        Value::Intrinsic(call) => {
            assert!(call.receiver.is_some(), "receiver should be bound");
            assert!(call.args.is_empty());
        }
        other => panic!("expected an Intrinsic value, got {other:?}"),
    }
}

#[test]
fn payload_structs_are_what_makes_value_small() {
    // 说明性断言：两个载荷本身都远大于 16 字节，正因如此才必须放在 `Arc` 后面
    assert!(std::mem::size_of::<ada_vm::NativeFunction>() > 16);
    assert!(std::mem::size_of::<ada_vm::IntrinsicCall>() > 16);
}
