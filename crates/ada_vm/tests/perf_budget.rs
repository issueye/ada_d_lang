//! 性能回归门禁（开发计划 T6.5，配合 T6.1 的基准）。
//!
//! 门禁度量的是**指令数**而不是墙钟：指令数完全确定，不受机器、负载、CI 抖动影响，
//! 因此可以精确断言"工作量变多了没有"。墙钟数字由 `benches/`（criterion）负责，
//! 只做纵向对比。
//!
//! 基线记录在 `benches/budgets.txt`（`程序名<TAB>指令数`）。超过基线 **5%** 即失败——
//! 与计划里的"劣化 >5% 报警"一致；降低指令数是好事，测试允许通过但会提示更新基线。
//!
//! 重新生成基线：`UPDATE_BUDGETS=1 cargo test -p ada_vm --test perf_budget -- --nocapture`

use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::Vm;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 允许的指令数劣化上限（T6.5）。
const MAX_REGRESSION: f64 = 1.05;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn programs_dir() -> PathBuf {
    workspace_root().join("benches/programs")
}

fn budgets_path() -> PathBuf {
    workspace_root().join("benches/budgets.txt")
}

/// 执行一个基准程序并返回 (指令数, 结果的可打印形式)。
fn run_program(path: &Path) -> (u64, String) {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut parser = Parser::from_source(&source).expect("parse");
    let program = parser.parse_program().expect("program");
    let proto = Compiler::compile_program(&program).expect("compile");
    let mut vm = Vm::new();
    vm.enable_instruction_counting(true);
    let value = vm.execute(proto).expect("execute");
    (vm.instructions_executed(), value.to_string())
}

fn load_budgets() -> BTreeMap<String, u64> {
    let Ok(text) = std::fs::read_to_string(budgets_path()) else {
        // 首次生成时文件可能不存在；非更新模式下后续的缺失检查会给出明确提示
        return BTreeMap::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|line| {
            let (name, count) = line.split_once('\t').expect("name<TAB>count");
            (
                name.trim().to_string(),
                count.trim().parse().expect("count"),
            )
        })
        .collect()
}

fn program_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(programs_dir())
        .expect("benches/programs readable")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ada"))
        .collect();
    files.sort();
    files
}

#[test]
fn instruction_counts_stay_within_budget() {
    let budgets = load_budgets();
    let files = program_files();
    assert!(!files.is_empty(), "no benchmark programs found");

    let mut measured: BTreeMap<String, (u64, String)> = BTreeMap::new();
    for file in &files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let (count, result) = run_program(file);
        measured.insert(name, (count, result));
    }

    // 更新模式：把当前数字写回基线文件后即返回
    if std::env::var("UPDATE_BUDGETS").is_ok() {
        let mut out = String::from(
            "# 基准程序的指令数基线（开发计划 T6.1 / T6.5）\n\
             # 格式：<程序名>\\t<指令数>；超过基线 5% 时 perf_budget 测试失败。\n\
             # 重新生成：UPDATE_BUDGETS=1 cargo test -p ada_vm --test perf_budget -- --nocapture\n",
        );
        for (name, (count, _)) in &measured {
            out.push_str(&format!("{name}\t{count}\n"));
        }
        std::fs::write(budgets_path(), out).expect("write budgets");
        println!("updated {}", budgets_path().display());
        return;
    }

    for (name, (count, result)) in &measured {
        let budget = budgets.get(name).copied().unwrap_or_else(|| {
            panic!("no recorded budget for program '{name}'; regenerate benches/budgets.txt")
        });
        let limit = (budget as f64 * MAX_REGRESSION).ceil() as u64;
        assert!(
            *count <= limit,
            "instruction-count regression in '{name}': {count} instructions \
             (budget {budget}, limit {limit}). Work grew by more than 5%.\n\
             If the growth is intentional, regenerate benches/budgets.txt."
        );
        if *count < budget {
            println!(
                "note: '{name}' improved: {count} instructions (baseline {budget}); \
                 consider refreshing benches/budgets.txt"
            );
        }
        // 结果必须仍然正确（防止"为了跑得快把活少干了"）
        assert!(
            !result.is_empty(),
            "program '{name}' produced no observable result"
        );
    }

    // 每个基准程序都必须有基线，避免新增程序被静默跳过
    for file in &files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        assert!(
            budgets.contains_key(&name),
            "benchmark program '{name}' is missing from benches/budgets.txt"
        );
    }
}

#[test]
fn benchmark_programs_produce_the_expected_values() {
    // 这些断言保证基准程序真的在算东西（而不是被优化掉/空转），
    // 否则性能数字毫无意义。
    let expectations = [
        ("fib", "46368"),
        ("numeric_loop", "40000000000"),
        ("string_concat", "200000"),
        // range(1, 200000) 的偶数 → ×3 → 前 5000 个：3 × 2 × (1+…+5000) = 75,015,000
        ("iterator_pipeline", "75015000"),
        // 3000 × (meta.retries=3 + tags.len=3) = 18,000
        ("json_roundtrip", "18000"),
    ];
    for (name, expected) in expectations {
        let path = programs_dir().join(format!("{name}.ada"));
        let (_, result) = run_program(&path);
        assert_eq!(result, expected, "unexpected result for {name}.ada");
    }
}
