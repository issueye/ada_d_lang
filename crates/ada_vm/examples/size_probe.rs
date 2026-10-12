fn main() {
    use std::mem::size_of;
    println!("Value            = {} bytes", size_of::<ada_vm::Value>());
    println!(
        "Instruction      = {} bytes",
        size_of::<ada_bytecode::Instruction>()
    );
    println!(
        "Option<Value>    = {} bytes",
        size_of::<Option<ada_vm::Value>>()
    );
    println!(
        "Result<Value,String> = {} bytes",
        size_of::<Result<ada_vm::Value, String>>()
    );
}
