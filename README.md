# Ada-D (Ada Script)

> **面向 AI 时代的高性能、超低 Token 消耗、可独立运行且极易嵌入 Rust 的现代脚本语言。**

当前版本 `0.1.0`：语言前端 + 寄存器字节码 VM 已可用；标准库、异步运行时、沙箱与 JIT 处于部分实现/规划阶段。
**各能力的真实状态一律以文档中的状态标记（✅⚠️🚧📋❌）为准，不要假设白皮书中的目标形态已经实现。**

---

## 📚 文档（入口：[docs/README.md](docs/README.md)）

所有规范文档遵循「一个主题只有一个权威文档」原则，文档索引与权威来源矩阵见 **[docs/README.md](docs/README.md)**。

* 📘 [**WHITEPAPER.md**](docs/WHITEPAPER.md) —— 愿景、定位、Token 密度论据、与既有方案的差异化。
* 📗 [**SPEC_LANGUAGE.md**](docs/SPEC_LANGUAGE.md) —— 词法、语法、运算符优先级、求值与相等语义、作用域。
* 📙 [**SPEC_RUNTIME.md**](docs/SPEC_RUNTIME.md) —— 字节码指令集、编译器策略、VM 执行模型、值模型、数值语义、错误模型、宿主嵌入 API、沙箱与权限。
* 📕 [**SPEC_STDLIB.md**](docs/SPEC_STDLIB.md) —— 全局内置函数、`json`/`math`/`time`/`sys`/`env` 模块、原型方法、统一迭代器协议与适配器。
* 📔 [**SPEC_ASYNC.md**](docs/SPEC_ASYNC.md) —— 异步语法、并发原语、调度与挂起模型、任务生命周期契约。
* 🗺️ [**DEVELOPMENT_PLAN.md**](docs/DEVELOPMENT_PLAN.md) —— **唯一的路线图**：基线快照、工作流、里程碑 M0–M7、任务与验收标准、质量门禁、风险与待决决策。

---

## ⚡ 快速开始

```bash
# 1. 运行示例脚本
cargo run -p ada_cli -- examples/demo.ada

# 2. 命令行直接求值（Eval）
cargo run -p ada_cli -- -e "let arr = [10, 20, 30]; arr |> sum()"

# 3. 反汇编并查看字节码（Dump）
cargo run -p ada_cli -- dump examples/demo.ada

# 4. 交互式 REPL
cargo run -p ada_cli -- repl

# 5. 运行工作区测试套件
cargo test --workspace
```

### Rust 宿主嵌入（最小示例）

```rust
use ada_engine::Engine;
use ada_vm::Value;   // ⚠️ 见下方说明：ada_engine 目前未 re-export Value（任务 T0.7）

fn main() -> Result<(), ada_engine::Error> {
    let mut engine = Engine::new();
    engine.set_fuel(Some(50_000));                  // 指令燃料熔断
    engine.register_fn("double", |args: &[Value]| {
        Ok(Value::Int(args[0].as_int().unwrap_or(0) * 2))
    });

    let n: i64 = engine.eval("double(21)")?;
    assert_eq!(n, 42);
    Ok(())
}
```

> ⚠️ `ada_engine` 当前未 re-export `Value`/`VmError` 等公共类型（`Cargo.toml` 需显式依赖 `ada_vm`）。这是已知的嵌入体验缺口，修复任务见 [开发计划 T0.7](docs/DEVELOPMENT_PLAN.md)；修复后即可写回 `use ada_engine::{Engine, Value};`。

---

## 📦 工作区结构 (Workspace Crates)

| Crate | 职责 |
| :--- | :--- |
| `crates/ada_ast` | 抽象语法树与 `Span` 位置定义 |
| `crates/ada_lexer` | 零拷贝分词器（含换行自闭合 ASI 与 `$"..."` 插值） |
| `crates/ada_parser` | 递归下降 + Pratt 表达式分析器 |
| `crates/ada_bytecode` | 寄存器式指令集与字节码编译器 |
| `crates/ada_vm` | 值模型、寄存器解释器、内置函数与 Fuel 熔断 |
| `crates/ada_engine` | 面向 Rust 宿主的高层嵌入 API (`Engine`) |
| `crates/ada_cli` | 命令行工具（run / eval / dump / repl） |

---

## ✅ 贡献前必读

1. 改动行为前，先确认归属主题并阅读对应 `SPEC_*.md`（权威来源矩阵见 [docs/README.md](docs/README.md)）。
2. 规范文档中禁止写路线图内容——统一指向 [DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md)。
3. 提交前跑齐质量门禁（见开发计划 §5）：

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

4. **不得引入可被脚本触发的 panic 路径**（当前存在 4 处待修复，见开发计划 M1）。
