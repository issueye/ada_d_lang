# Ada-D (Ada Script)

> **面向 AI 时代的高性能、超低 Token 消耗、可独立运行且极易嵌入 Rust 的现代脚本语言。**

当前版本 `0.1.0`：语言前端 + 寄存器字节码 VM 已可用；标准库、异步运行时、沙箱与 JIT 处于部分实现/规划阶段。
**各能力的真实状态一律以文档中的状态标记（✅⚠️🚧📋❌）为准，不要假设白皮书中的目标形态已经实现。**

---

## 📚 文档（入口：[docs/README.md](docs/README.md)）

所有规范文档遵循「一个主题只有一个权威文档」原则，文档索引与权威来源矩阵见 **[docs/README.md](docs/README.md)**。

* 📘 [**WHITEPAPER.md**](docs/WHITEPAPER.md) —— 愿景、定位、Token 密度论据、与既有方案的差异化。
* 🤖 [**AI_GUIDE.md**](docs/AI_GUIDE.md) —— **给模型（或人）的单页规格**：语法速查、反直觉规则、常见错误对照、3 个可运行示例；页内每个示例都由 CLI 真实执行并比对输出。
* 📗 [**SPEC_LANGUAGE.md**](docs/SPEC_LANGUAGE.md) —— 词法、语法、运算符优先级、求值与相等语义、作用域。
* 📙 [**SPEC_RUNTIME.md**](docs/SPEC_RUNTIME.md) —— 字节码指令集、编译器策略、VM 执行模型、值模型、数值语义、错误模型与 CLI 错误格式、宿主嵌入 API、沙箱与权限、性能基线与优化。
* 📕 [**SPEC_STDLIB.md**](docs/SPEC_STDLIB.md) —— 全局内置函数、`json`/`math`/`time`/`sys`/`env` 模块、原型方法、统一迭代器协议与适配器。
* 📔 [**SPEC_ASYNC.md**](docs/SPEC_ASYNC.md) —— 异步语法、并发原语、调度与挂起模型、任务生命周期契约。
* 🗺️ [**DEVELOPMENT_PLAN.md**](docs/DEVELOPMENT_PLAN.md) —— **唯一的路线图**：基线快照、工作流、里程碑 M0–M7、任务与验收标准、质量门禁、风险与待决决策。

文档站（mdBook，与上面同一批 Markdown 单一源）：`mdbook serve docs`，目录见 [docs/SUMMARY.md](docs/SUMMARY.md)。

### 配套工具

| 目录 | 内容 |
| :--- | :--- |
| [editors/vscode](editors/vscode/README.md) | VSCode 扩展：语法高亮 + 实时诊断（零依赖 LSP 客户端） |
| [crates/ada_lsp](crates/ada_lsp) | `ada-lsp` 最小 LSP 服务器（静态诊断：词法/语法/编译） |
| [benches](benches) | criterion 墙钟基准 + 确定性指令数回归门禁（`benches/programs/*.ada`） |
| [examples/ai](examples/ai) | AI 场景示例（数据管道、并发、校验），输出由 golden 测试比对 |

编辑器接入方式（VSCode / Neovim / Emacs / Helix）见 [docs/EDITORS.md](docs/EDITORS.md)。

---

## ⚡ 快速开始

```bash
# 1. 运行示例脚本
cargo run -p ada_cli -- examples/demo.ada

# 2. 命令行直接求值（Eval）
cargo run -p ada_cli -- -e "let arr = [10, 20, 30]; arr |> sum()"

# 3. 静态检查（不执行脚本；CI 与编辑器钩子用）
cargo run -p ada_cli -- check examples/demo.ada

# 4. 反汇编并查看字节码（Dump）
cargo run -p ada_cli -- dump examples/demo.ada

# 5. 交互式 REPL
cargo run -p ada_cli -- repl

# 6. 启动编辑器用的 LSP 服务器（stdio）
cargo run -p ada_lsp -- --stdio

# 7. 运行工作区测试套件
cargo test --workspace
```

### Rust 宿主嵌入（最小示例）

`Engine::new()` 默认使用 **strict 沙箱**：脚本只能做纯计算（+ `json`/`math`），
拿不到 `time`/`env`/`sys`/`sleep_ms`，并受限定的内存、墙钟与任务数。宿主能力必须**显式授予**。

```rust
use ada_engine::{Engine, Sandbox, Value};
use std::sync::Arc;

fn main() -> Result<(), ada_engine::Error> {
    // 1) 默认：最小权限。宿主函数注入不受 profile 限制（这是宿主的显式授权）。
    let mut engine = Engine::new();
    engine.set_fuel(Some(50_000));                  // 指令燃料熔断
    engine.register_fn("double", |args: &[Value]| {
        Ok(Value::Int(args[0].as_int().unwrap_or(0) * 2))
    });
    assert_eq!(engine.eval::<i64>("double(21)")?, 42);

    // 2) 需要宿主能力时按需开档（能力 + 资源限额一起给）
    let sandbox = Sandbox::strict()
        .with_time(true)                            // 允许 time.*
        .with_env_keys(["HOME"])                    // 只放行这一个环境变量
        .with_memory_limit(Some(8 * 1024 * 1024))   // 8 MiB 近似预算
        .with_wall_clock_limit(Some(2_000));        // 单次求值 2 秒
    let mut engine = Engine::with_sandbox(sandbox);
    engine.register_fn("now", |_| Ok(Value::Int(0)));
    let _: i64 = engine.eval("time.now_ms()")?;

    // 3) 注册宿主原生模块：脚本侧表现为一个 map（`geometry.area(3.0, 4.0)`）
    struct Geometry;
    impl ada_engine::NativeModule for Geometry {
        fn name(&self) -> &str { "geometry" }
        fn functions(&self) -> Vec<(String, ada_engine::NativeCallback)> {
            vec![(
                "area".to_string(),
                Arc::new(|args: &[Value]| {
                    let w = args.first().and_then(|a| a.as_float()).unwrap_or(0.0);
                    let h = args.get(1).and_then(|a| a.as_float()).unwrap_or(0.0);
                    Ok(Value::Float(w * h))
                }),
            )]
        }
    }
    let mut engine = Engine::new();
    engine.register_module(Geometry)?;
    assert_eq!(engine.eval::<f64>("geometry.area(3.0, 4.0)")?, 12.0);

    // 4) 受信场景/兼容旧行为
    let _permissive = Engine::permissive();

    Ok(())
}
```

三档 profile 与限额见 [SPEC_RUNTIME §7.1](docs/SPEC_RUNTIME.md)；越权/超限的行为与错误码见
[SPEC_RUNTIME §7.3](docs/SPEC_RUNTIME.md)，安全测试集在 `crates/ada_engine/tests/security.rs`（21 个用例）。

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
