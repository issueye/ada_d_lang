//! Ada-D 基准（开发计划 T6.1）。
//!
//! 两组互补的度量：
//! 1. **墙钟**（本文件，criterion）：回答"有多快"；机器相关，只做纵向对比。
//! 2. **指令数**（`crates/ada_vm/tests/perf_budget.rs`）：回答"做了多少工作"；
//!    完全确定，作为 CI 的性能回归门禁（见 T6.5）。
//!
//! 两条路径共用 `benches/programs/*.ada`，避免基准程序漂移。
//!
//! 运行：`cargo bench --manifest-path benches/Cargo.toml`
//! （该包独立于主工作区，`cargo test --workspace` 不会编译 criterion）

use criterion::{criterion_group, criterion_main, Criterion};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn programs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("programs")
}

/// 编译一次，重复执行同一份字节码（隔离编译开销）。
fn compiled(name: &str) -> ada_bytecode::FunctionProto {
    let source = std::fs::read_to_string(programs_dir().join(name))
        .unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
    let mut parser = ada_parser::Parser::from_source(&source).expect("parse");
    let program = parser.parse_program().expect("program");
    ada_bytecode::Compiler::compile_program(&program).expect("compile")
}

fn exec(proto: &ada_bytecode::FunctionProto) -> ada_vm::Value {
    let mut vm = ada_vm::Vm::new();
    vm.execute(proto.clone()).expect("execute")
}

fn bench_program(c: &mut Criterion, group: &str, file: &str) {
    let proto = compiled(file);
    c.bench_function(&format!("{group}/{}", file.trim_end_matches(".ada")), |b| {
        b.iter(|| exec(&proto))
    });
}

fn ada_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("ada");
    group
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3))
        .sample_size(30);

    // 五组基准（T6.1）——各自侧重不同热点：
    //   调用与捕获、整数算术、字符串分配、适配器/闭包、原生模块与容器
    group.bench_function("fib_recursion", |b| {
        let proto = compiled("fib.ada");
        b.iter(|| exec(&proto))
    });
    group.bench_function("numeric_loop", |b| {
        let proto = compiled("numeric_loop.ada");
        b.iter(|| exec(&proto))
    });
    group.bench_function("string_concat", |b| {
        let proto = compiled("string_concat.ada");
        b.iter(|| exec(&proto))
    });
    group.bench_function("iterator_pipeline", |b| {
        let proto = compiled("iterator_pipeline.ada");
        b.iter(|| exec(&proto))
    });
    group.bench_function("json_roundtrip", |b| {
        let proto = compiled("json_roundtrip.ada");
        b.iter(|| exec(&proto))
    });
    // 冷启动（编译 + 执行）单独一组：嵌入场景的真实成本
    group.bench_function("cold_start_numeric_loop", |b| {
        let source = std::fs::read_to_string(programs_dir().join("numeric_loop.ada")).unwrap();
        b.iter(|| {
            let mut parser = ada_parser::Parser::from_source(&source).unwrap();
            let program = parser.parse_program().unwrap();
            let proto = ada_bytecode::Compiler::compile_program(&program).unwrap();
            let mut vm = ada_vm::Vm::new();
            vm.execute(proto).unwrap()
        })
    });

    group.finish();

    // 仅作对照：编译开销（不含执行）
    let mut compile_group = c.benchmark_group("compile");
    compile_group.bench_function("numeric_loop_compile_only", |b| {
        let source = std::fs::read_to_string(programs_dir().join("numeric_loop.ada")).unwrap();
        b.iter(|| {
            let mut parser = ada_parser::Parser::from_source(&source).unwrap();
            let program = parser.parse_program().unwrap();
            ada_bytecode::Compiler::compile_program(&program).unwrap()
        })
    });
    compile_group.finish();
    let _ = bench_program; // 保留辅助函数供后续基准使用
}

criterion_group!(benches, ada_benches);
criterion_main!(benches);
