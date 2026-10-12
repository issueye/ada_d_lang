//! 值模型尺寸探针（决策 D1 / 任务 T6.2b 的证据工具）。
//!
//! 运行：`cargo run -p ada_vm --example size_probe`
//!
//! 关注点：`Value` 是 VM 寄存器文件的元素类型（2048 槽）。它每多 8 字节，
//! 寄存器文件就多 16 KiB —— 48 字节时是 96 KiB（超出 L1），16 字节时是 32 KiB（刚好放得下）。
//! `crates/ada_vm/tests/value_layout.rs` 把这条性质作为门禁固定住。

fn main() {
    use std::mem::size_of;

    let value = size_of::<ada_vm::Value>();
    let slots = 2048usize;

    println!("值模型尺寸（T6.2b）");
    println!("  Value                = {value} bytes   (M6 之前为 48)");
    println!(
        "  Option<Value>        = {} bytes",
        size_of::<Option<ada_vm::Value>>()
    );
    println!(
        "  Result<Value,String> = {} bytes",
        size_of::<Result<ada_vm::Value, String>>()
    );
    println!(
        "  Instruction          = {} bytes",
        size_of::<ada_bytecode::Instruction>()
    );
    println!("  胖载荷（放在 Arc 后面）");
    println!(
        "    NativeFunction     = {} bytes",
        size_of::<ada_vm::NativeFunction>()
    );
    println!(
        "    IntrinsicCall      = {} bytes",
        size_of::<ada_vm::IntrinsicCall>()
    );
    println!("  寄存器文件（{slots} 槽）");
    println!("    当前               = {} KiB", value * slots / 1024);
    println!("    若 Value 仍为 48B  = {} KiB", 48 * slots / 1024);
    println!("  L1 参考线            = 32 KiB（寄存器文件应能放下）");
}
