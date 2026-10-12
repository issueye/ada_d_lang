# Ada-D 开发计划 (Development Plan)

> **版本**：1.0
> **状态**：生效中 (Active)
> **基线版本**：`0.1.0`
> **适用范围**：`ada_d_lang` 全工作区（`crates/`、`docs/`、`examples/`）

---

## 0. 本文档的权威性 (Single Source of Truth)

1. **本文档是全项目唯一的路线图、里程碑、任务优先级与验收标准来源。**
   其它文档（白皮书、各设计规范）中任何"实施路线 / 里程碑 / Roadmap / 工程落地"章节**一律移除**，改为指向本文档，避免同一功能在多处各写一份计划。
2. **文档状态图例**（本计划与所有规范文档统一使用）：

   | 标记 | 含义 |
   | :--- | :--- |
   | ✅ | 已实现，有测试覆盖，行为与规范一致 |
   | ⚠️ | 部分实现，存在明确缺口（缺口须在本文档有对应任务 ID） |
   | 🚧 | 进行中（有所属里程碑，尚未达到退出标准） |
   | 📋 | 计划中，尚未开始 |
   | ❌ | 明确不做 / 已否决（须在 §9 或 §10 有记录） |

3. **规范与实现冲突时**：以规范文档（`SPEC_*.md`）为准，实现按任务修正；若确需改变语义，必须先改规范并在 §10 记录决策。
4. **进度更新规则**：任务状态以任务 ID 前缀标记——`📋 T2.1`（未开始）/ `🚧 T2.1`（进行中）/ `✅ T2.1`（已完成）；已完成的任务必须在本行"验收 / 完成证据"列写明测试名或命令。本文档的修改必须与对应代码改动同一个 commit。

---

## 1. 项目目标与阶段定位

**项目定位**（摘自 [WHITEPAPER](WHITEPAPER.md)）：面向 AI 时代的高性能、超低 Token 消耗、可独立运行且极易嵌入 Rust 的现代脚本语言。

当前实现处于**"语言前端 + 寄存器字节码 VM"原型**阶段，与完整愿景（分层 JIT + 沙箱 + 异步纤程）之间有明确梯度。据此划分三个交付阶段：

| 阶段 | 目标 | 覆盖里程碑 | 用户可感知的结果 |
| :--- | :--- | :--- | :--- |
| **S1 · 可用的嵌入式脚本引擎** | 脚本无法崩溃宿主；文档示例全部可运行；语义可预测 | M0 – M3 | 敢把 `Engine` 接进真实 Rust 宿主，跑 AI 生成的脚本 |
| **S2 · 异步与沙箱生产级** | 真并发、真超时、真权限分级 | M4 – M5 | 能作为 Agent 工具执行器，具备失控防御 |
| **S3 · 高性能与生态** | 有基准、有优化、有工具链 | M6 – M7 | 性能可量化，具备编辑器与包生态 |

---

## 2. 基线快照 (Verified Baseline, 0.1.0)

以下数据均为在本仓库实测所得，可复现（见 §2.3）。

### 2.1 规模与结构

| 项 | 实测值 |
| :--- | :--- |
| Crate 数 | 7（`ada_ast` / `ada_lexer` / `ada_parser` / `ada_bytecode` / `ada_vm` / `ada_engine` / `ada_cli`） |
| Rust 代码 | 6,481 行（`compiler.rs` 1,203 / `parser.rs` 1,025 / `vm.rs` 968 / `lexer.rs` 587 / `core.rs` 500） |
| 设计文档 | 5 份，约 970 行（**整合前基线**；T0.3 完成后为 `docs/` 下 6 份规范/索引 + 本文档，见 §11） |
| 示例脚本 | 1 个（`examples/demo.ada`，98 行，端到端可运行） |
| 编译告警 | `cargo build --workspace` → 0 warning / 0 error |
| 测试 | `cargo test --workspace` → 34 passed / 0 failed（vm 20、parser 6、lexer 3、engine 3、bytecode 2、cli 0、ast 0）；**M3 完成后为 130 passed**（见 §2.4） |
| 版本控制 | 基线时**无任何 commit**（T0.1 已修复，见 §2.4） |
| 许可证 | 基线时声明 `MIT OR Apache-2.0` 但无 `LICENSE` 文件（T0.2 已补齐 `LICENSE-MIT` + `LICENSE-APACHE`） |
| CI / fmt / clippy / bench | 基线时均无；T0.4 已建立 CI（fmt / clippy / test / 文档门禁）；**bench 仍缺**（T6.1） |

### 2.2 能力盘点

**✅ 已兑现**

- 前端全链路：ASI 换行自闭合、`$"..."` 插值、`0x`/`0b`/数字下划线、块注释。
- 语法覆盖：`let`/`mut`、`fn`（单行 `=` 与块体）、`async fn`、`if/else`、`match`（比较模式 + guard）、`while`、`for..in`（含 `[k, v]` 解构）、闭包 `x => expr`、管道 `|>`、`??`、`?.`、`async fn` + 前后缀 `await`。
- 运行时：寄存器字节码 VM、帧调用、`fuel` 指令熔断、512 帧调用深度上限、`OutOfFuel`/`StackOverflow` 干净报错。
- 标准库：全局函数（`print`/`println`/`type_of`/`to_*`/`assert*`/`abs`/`min`/`max`/`clamp`/`round`/`floor`/`ceil`/`len`/`range`/`push`/`sum`/`sleep_ms`/`await_*`）、模块 `json`/`math`/`time`/`sys`/`env`、string/array/map 三套原型方法（子集）。
- 迭代：`for..in` 统一支持 Array / String / Map / Range / Int，`range(start, end, step)` 为惰性区间。
- 嵌入：`Engine::register_fn` / `set_global` / `eval<T: FromValue>`。
- 工具链：`run` / `eval` / `dump`（反汇编）/ `repl` 四个 CLI 入口。

**⚠️ / 📋 关键差距**（详细清单见 §3 映射与各里程碑任务表）

| 差距 | 文档承诺处 | 实测 | 任务 |
| :--- | :--- | :--- | :--- |
| ✅ 5 类脚本可触发宿主进程 panic（`%0`、整数溢出、移位越界、UTF-8 字符串索引、`min([])` 空数组） | [WHITEPAPER](WHITEPAPER.md) §2「Fail-Safe Sandboxing」+ 本文档 §5 门禁第 3 条 | 修复前 exit code 101；现均为干净错误（exit 1） | ✅ T1.1–T1.6 |
| 无 `filter`/`map`/`take` 等迭代器适配器 | [SPEC_STDLIB §6](SPEC_STDLIB.md) | `UndefinedGlobal("filter")` | T2.1 |
| 闭包无 upvalue 捕获 | [SPEC_LANGUAGE §6](SPEC_LANGUAGE.md) | `UndefinedGlobal("a")` | T2.2 |
| REPL 变量不跨行保留 | [../README.md](../README.md) 快速开始 | `Undefined global 'a'` | T2.4 |
| `spawn` 无并发语义、`sleep_ms` 阻塞线程 | [SPEC_ASYNC §1、§3](SPEC_ASYNC.md) | 2×200ms 实测 401ms 串行 | T4.1–T4.4 |
| 无 `SandboxProfile` / 内存预算；`env.get` 无白名单 | [SPEC_RUNTIME §10](SPEC_RUNTIME.md) | 实测可读 `PATH`/`USERPROFILE` | T5.1–T5.4 |
| 值模型非 NaN-boxing | [SPEC_RUNTIME §2](SPEC_RUNTIME.md) | `enum Value` + `Arc<RwLock>` | 决策 D1 |
| 无 `ada_jit` / Cranelift | [SPEC_RUNTIME §11](SPEC_RUNTIME.md) | crate 不存在 | T6.4 |
| `let` 不可变性未强制 | [SPEC_LANGUAGE §5.1](SPEC_LANGUAGE.md) | `let a = 1; a = 2` 静默生效 | T3.1 |
| ✅ `len` 返回字节数而非字符数 | [SPEC_STDLIB §2.6、§4.1](SPEC_STDLIB.md) | 现为字符数，`.len_bytes` 提供字节数 | ✅ T2.5 |
| ✅ `ada_engine` 未 re-export `Value`，文档嵌入示例不可编译 | [WHITEPAPER](WHITEPAPER.md) §2「Dual Execution Modes」 | `use ada_engine::{Engine, Value}` 现可编译 | ✅ T0.7 |
| 整合前 4 处独立"路线图"章节互相重叠 | 原 5 份文档各有一节 | 已收敛到本文档 | ✅ T0.3 |

### 2.3 复现命令

```bash
cargo build --workspace
cargo test --workspace
cargo run -p ada_cli -- examples/demo.ada
cargo run -p ada_cli -- dump examples/demo.ada
cargo run -p ada_cli -- -e "let arr = [10, 20, 30]; arr |> sum()"
```

### 2.4 M0 / M1 完成后的状态增量

| 项 | 基线（0.1.0） | 现在 |
| :--- | :--- | :--- |
| 测试总数 | 34 | **156**（`robustness.rs` 17 + `adapters.rs` 16 + `closures.rs` 15 + `semantics.rs` 21 + `async.rs` 26 + `doc_examples.rs` 14 + `persistent_eval.rs` 8 + `golden_showcase.rs` 3 + `doc_links.rs` 2，另含各 crate 单元测试 34） |
| 脚本可 panic 宿主 | 5 类（`%0`、整数溢出、移位越界、UTF-8 索引、`min([])`） | **0 类已知**（T1.1–T1.6；另修掉 `sum` 累加溢出、`abs(i64::MIN)`、`math.random_int` 区间溢出的潜在 panic） |
| 版本控制 | 无提交 | 4 个提交（基线 / rustfmt / 修复 / 门禁） |
| 许可证 | 无文件 | `LICENSE-MIT` + `LICENSE-APACHE` |
| 代码格式 | 17 个文件未格式化 | `cargo fmt --all -- --check` 通过 |
| Lint | 未接入 | `cargo clippy --workspace --all-targets -- -D warnings` 通过（基线即为 0 lint） |
| CI | 无 | `.github/workflows/ci.yml`（fmt + clippy + test，含文档链接门禁） |
| 文档门禁 | 无 | `cargo test` 内含链接/锚点校验与"规范文档不得写路线图"校验（已做负向验证） |
| 字符串长度/索引 | 字节语义、多字节 panic | 字符语义（`len`/索引按字符，`.len_bytes` 给字节） |
| 宿主嵌入可编译性 | `use ada_engine::{Engine, Value}` 编译失败 | ✅ 已可编译（T0.7） |
| 迭代器能力 | `filter`/`map`/`take` 全族缺失，`iterator` 无任何方法 | ✅ 13 个适配器（方法 + 全局双入口）、`.iter()`/`.next()`、`is_iterator`（T2.1/T2.3） |
| REPL / 多轮求值 | 顶层 `let` 跨轮丢失 | ✅ 顶层绑定写全局表，跨轮可见；函数体内局部不外泄（T2.4/T2.7） |
| Fuel 生命周期 | 一次 `OutOfFuel` 后 `Engine` 永久失效 | ✅ 每次求值重新装填 + `remaining_fuel`/`reset_fuel`（T2.6） |
| 闭包捕获 | 无 upvalue：捕获外层变量报 `UndefinedGlobal` | ✅ 引用共享（可写捕获）、多级传递、逃逸闭包、每轮循环变量独立槽（T2.2） |
| 端到端 golden | 无 | ✅ `examples/showcase.ada` + `showcase.expected.txt` + `golden_showcase.rs` 逐行比对（14 个规范小节，含 `yield_now` 确定性交错） |
| 异步并发 | `spawn` 立即求值、无调度、`sleep_ms` 阻塞线程（2×200ms 实测 401ms） | ✅ 单线程协作式事件循环（决策 D2）：`spawn` 惰性、任务可挂起/恢复、`sleep_ms` 走真实定时器（2×200ms 实测 **200ms**）、`timeout`/`cancel` 可用 |
| 语义确定性 | `let` 可被重新赋值（不可变性不生效）、非穷尽 `match` 返回静默错误值、`5 < "a"` 静默 `false`、Float 与 Int 输出无法区分 | ✅ 全部收紧（T3.1/T3.2/T3.5/T3.6）：编译期不可变、`VmError::NoMatch`、比较报错、`1.0` 可区分；负索引统一（G-L14） |

---

## 3. 差距 → 工作流映射 (Workstreams)

所有任务归入 6 条工作流，避免职责与文档归属交叉：

| ID | 工作流 | 主要 crate / 目录 | 权威规范文档 |
| :--- | :--- | :--- | :--- |
| **W1** | 前端与语言核心（词法/语法/AST/语义） | `ada_lexer`, `ada_parser`, `ada_ast` | `SPEC_LANGUAGE.md` |
| **W2** | 编译与执行（字节码/编译/VM/值模型） | `ada_bytecode`, `ada_vm` | `SPEC_RUNTIME.md` |
| **W3** | 标准库与迭代器 | `ada_vm/builtins`, `ada_vm::value` | `SPEC_STDLIB.md` |
| **W4** | 异步与并发运行时 | `ada_vm`, `ada_engine` | `SPEC_ASYNC.md` |
| **W5** | 沙箱、嵌入与宿主安全 | `ada_engine`, `ada_vm` | `SPEC_RUNTIME.md`（§9 嵌入 API / §10 沙箱与权限） |
| **W6** | 工程质量（测试/基准/CI/文档/发布） | 全仓库 | `DEVELOPMENT_PLAN.md`（本文档） |

> 规范文档清单与权威划分见 §11 与 [docs/README.md](README.md)。

---

## 4. 里程碑与任务

优先级定义：**P0** = 阻塞可用性/安全性；**P1** = 阻塞文档承诺的核心能力；**P2** = 质量与体验；**P3** = 长期演进。

### M0 · 治理基线 ✅（预计 1–2 天）

**目标**：建立可 review、可回滚、可自动校验的工程基线。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| ✅ T0.1 | `git init` + 首次提交（含 `Cargo.lock`、`.gitignore`） | W6 | P0 | 仓库根 | 完成证据：基线提交 `338f2a9`（40 个文件入库），`git status` 干净 |
| ✅ T0.2 | 补 `LICENSE`（或把 `Cargo.toml` 的 license 改为与现状一致） | W6 | P0 | `LICENSE-MIT`, `LICENSE-APACHE`, `Cargo.toml` | 完成证据：`LICENSE-MIT`（MIT 全文）+ `LICENSE-APACHE`（apache.org 官方文本，11358 字节），与 `Cargo.toml` 的 `MIT OR Apache-2.0` 一致 |
| ✅ T0.3 | 文档整合：单一权威 + 删除 4 处重复路线图 + 状态标注 | W6 | P0 | `docs/**`, `README.md` | 完成证据：新增 `docs/README.md` 与 `SPEC_LANGUAGE`/`SPEC_RUNTIME`/`SPEC_STDLIB`/`SPEC_ASYNC`；移除 `LANGUAGE_DESIGN`/`STDLIB_DESIGN`/`ITERATOR_SPEC`/`ASYNC_DESIGN`；规范内无路线图章节；全量链接检查通过（§11） |
| ✅ T0.4 | CI：`cargo fmt --check` + `cargo clippy -D warnings` + `cargo test --workspace` + Markdown 链接/锚点检查 | W6 | P0 | `.github/workflows/ci.yml`, `crates/ada_cli/tests/doc_links.rs` | 完成证据：CI 工作流就位；`cargo fmt --all -- --check` exit 0；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；文档门禁测试通过，且经负向验证（临时插入坏链/路线图标题 → 测试 FAILED） |
| ✅ T0.5 | 建立测试目录规范：`crates/*/tests/`（集成）、`benches/`（基准）、`examples/`（golden） | W6 | P1 | 各 crate | 完成证据：`crates/ada_vm/tests/robustness.rs`、`crates/ada_engine/tests/doc_examples.rs`、`crates/ada_cli/tests/doc_links.rs` 已按此落位；`benches/` 待 T6.1 |
| T0.6 | 统一错误信息格式为 `file:line:col: error: msg`（CLI 侧渲染） | W1 | P1 | `ada_cli`, `ada_parser` | 语法错误输出含文件路径、行列；已有错误类型保留 span |
| ✅ T0.7 | `ada_engine` 补齐公共 re-export（`Value`、`Closure`、`VmError`、`ParseError`、`CompileError`） | W5 | P1 | `ada_engine/src/lib.rs` | 完成证据：`crates/ada_engine/tests/doc_examples.rs::spec_runtime_embedding_example_with_exported_value` 用 `use ada_engine::Value` 通过 |

**退出标准**：✅ 达成 —— 仓库有基线提交（4 个）；CI 配置就位；文档单一权威结构成型；`cargo fmt --check` 与 clippy 全绿。
（剩余 T0.6 为 P1，不阻塞 M0 退出）

---

### M1 · 解释器稳健性 P0 ✅（预计 3–5 天）

**目标**：**任何脚本输入都不得让宿主进程 panic**。这是 S1 阶段的第一条红线。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| ✅ T1.1 | `%` 除零：返回 `VmError`，与 `/` 行为一致 | W2 | P0 | `ada_vm/src/vm.rs` | 完成证据：`robustness.rs::modulo_by_zero_is_an_error_not_a_panic`（`10 % 0` → `TypeError("Modulo by zero")`，CLI exit 1） |
| ✅ T1.2 | 整数算术溢出：`+ - *` 及 `Neg` 用 `checked_*`，溢出返回 `VmError::TypeError`（决策 D5 = A） | W2 | P0 | `ada_vm/src/vm.rs` | 完成证据：4 个溢出用例 + `i64::MIN / -1`、`i64::MIN % -1` 用例（`checked_rem`/`checked_div`/`checked_neg`）全绿 |
| ✅ T1.3 | 移位越界：左右移校验 `0 <= n < 64` | W2 | P0 | `ada_vm/src/vm.rs` | 完成证据：`1 << 64`、`1 << -1`、`1 >> 64` 均报 `Shift amount out of range` |
| ✅ T1.4 | 字符串索引按字符（`chars().nth`）定位，消除 byte-boundary panic | W2 | P0 | `ada_vm/src/vm.rs` | 完成证据：`"语言"[0]` → `"语"`、`"Ada 语言"[4]` → `"语"`、越界 → `nil` |
| ✅ T1.5 | panic 路径的回归测试（含多字节、极值、边界组合、ASI 续行陷阱） | W6 | P0 | `crates/ada_vm/tests/robustness.rs` | 完成证据：17 个用例全绿（要求 ≥8） |
| ✅ T1.6 | 全 crate 审计其余 panic 源（`unwrap`/`expect`/索引越界/`as` 截断），逐条处理或标注 | W2 | P0 | 全 crate | 完成证据：发现并修复 `min([])`/`max([])` 空数组越界 panic（2 个用例）；其余 `unwrap` 均为 `RwLock` 中毒语义或编译器不变量（`constants`/`names` 索引由编译器生成，见 SPEC_RUNTIME §4） |
| T1.7 | 宿主兜底：`Engine::eval*` 边界 `catch_unwind`，把意外 panic 转为 `Error::Panic`（默认开启，可关闭） | W5 | P1 | `ada_engine/src/lib.rs` | 故意 panic 的 native fn 不杀死宿主进程 |
| T1.8 | 错误可定位：`VmError` 附带当前指令的 `Span`，CLI 输出行号 | W2/W6 | P1 | `ada_vm`, `ada_bytecode`, `ada_cli` | 运行时错误输出 `file:line:col` |

**退出标准**：✅ 达成 —— 原 5 类 panic 探针全部返回干净错误（exit 1 而非 101）；`robustness.rs` 17 个用例（要求 ≥8）；`cargo test --workspace` 65 passed / 0 failed；fmt + clippy + 文档门禁全绿。
（剩余 T1.7/T1.8 为 P1，归入 M5/M3 的收尾工作）

---

### M2 · 语言核心补齐 P1 ✅（预计 1–2 周）

**目标**：白皮书与规范中所有"已写进语法/标准库"的示例都能真实运行。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| ✅ T2.1 | 迭代器适配器：`map`/`filter`/`take`/`skip`/`enumerate`/`collect`/`count`/`find`/`any`/`all`/`join`/`sum`，同时提供**方法链**与**全局函数**（供 `|>` 使用） | W3 | P1 | `ada_vm/src/{vm,value}.rs`, `builtins/{core,mod}.rs` | 完成证据：`adapters.rs::plan_acceptance_pipeline_returns_165`（计划验收用例 == `165`）+ 15 个适配器用例；双入口经 `Value::Intrinsic` 统一（方法形式绑定接收者） |
| ✅ T2.2 | 闭包 upvalue 捕获：新增 `MakeCell`/`CellGet`/`CellSet`/`UpvalueGet`/`UpvalueSet`/`MakeClosure` 指令与 `FunctionProto.upvalues`；编译器做装箱预扫描并传递式解析捕获 | W2/W1 | P1 | `ada_bytecode/src/{instruction,compiler}.rs`, `ada_vm/src/{vm,value}.rs` | 完成证据：`closures.rs` 15 个用例全绿（计划验收用例 `fn outer(a) { let g = x => x + a; g(5) }` → 15；可写捕获计数器、多级传递、逃逸闭包、每轮循环独立捕获、与适配器组合、Cell 不泄漏） |
| ✅ T2.3 | 原型方法补齐：`string.chars_with_idx/iter/next`、array `iter/next`、map `iter/next`、`is_iterator` | W3 | P1 | `ada_vm/src/vm.rs`, `builtins/core.rs` | 完成证据：`adapters.rs::explicit_iter_and_next_protocol`、`chars_with_idx_provides_index_char_pairs`；`.iter()` 返回可调用的零参方法 |
| ✅ T2.4 | REPL 状态持久化：顶层 `let`/`mut` 落到持久 scope（或编译为 `SetGlobal`），跨行可见 | W2/W6 | P1 | `ada_bytecode/src/compiler.rs`, `ada_engine` | 完成证据：`persistent_eval.rs::top_level_bindings_persist_across_evaluations`（`let a = 41` → `a + 1` == 42）；新增 `is_module_scope` 保证函数体内局部不外泄 |
| ✅ T2.5 | 长度语义统一：`.len` 与 `len()` 返回**字符数**；新增 `.len_bytes` 表达字节数；同步规范 | W3 | P1 | `ada_vm/src/vm.rs`, `builtins/core.rs`, `SPEC_STDLIB.md` | 完成证据：`robustness.rs::string_len_counts_characters_and_len_bytes_counts_bytes`（`len("Ada 语言") == 6`、`.len_bytes == 10`） |
| ✅ T2.6 | `fuel` 生命周期：`execute()` 每次重新装填；新增 `reset_fuel` / `remaining_fuel` | W2/W5 | P1 | `ada_vm/src/vm.rs`, `ada_engine/src/lib.rs` | 完成证据：`persistent_eval.rs::fuel_is_reloaded_on_every_evaluation` + `remaining_fuel_is_observable_and_resettable` |
| ✅ T2.7 | `Engine` 持久化求值测试（多轮 eval 共享状态） | W6 | P1 | `crates/ada_engine/tests/persistent_eval.rs` | 完成证据：7 个用例（变量 / 函数 / 模块与宿主函数 / 局部不外泄 / fuel × 3） |

**退出标准**：✅ 达成 —— golden 脚本 `examples/showcase.ada`（覆盖 SPEC_LANGUAGE §5–§8、SPEC_STDLIB §2–§6、SPEC_ASYNC §2 共 13 个小节）+ 期望输出 `examples/showcase.expected.txt`，由 `crates/ada_cli/tests/golden_showcase.rs` 逐行比对；
`examples/demo.ada` 也扩展了闭包 + 适配器小节。两个文件均已连续两次运行验证输出一致（确定性）。

---

### M3 · 语义规范收紧 ✅（预计 1 周）

**目标**：把"能跑"提升为"语义可预测、报错可定位"。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| ✅ T3.1 | `let` 不可变性：编译期拒绝向非 `mut` 绑定赋值 | W1 | P1 | `compiler.rs`, `parser.rs`（`is_mut` 落地） | 完成证据：`semantics.rs` 6 个用例（普通/复合赋值、`mut` 可写、参数与循环变量可写、捕获的不可变变量在闭包内报错、容器内容仍可变） |
| ✅ T3.2 | `match` 非穷尽：无 `_` 且无匹配时抛运行时错误（决策 D7 = A） | W1/W2 | P1 | `compiler.rs`, `vm.rs`, `instruction.rs` | 完成证据：新增 `MatchFail` 指令 + `VmError::NoMatch`；4 个用例（非穷尽报错、`_`/裸绑定为兜底、带 guard 的绑定不算兜底、正常 arm 不受影响） |
| ✅ T3.3 | `{ x }` map / block 歧义规则定型并写入规范（决策 D4 = B） | W1 | P1 | `SPEC_LANGUAGE.md` | 完成证据：SPEC_LANGUAGE §5.6 规则表 + `semantics.rs::brace_with_a_bare_identifier_is_a_map_not_a_block` |
| ✅ T3.4 | 插值子表达式 span 映射修复（片段内行列 → 源文件绝对行列） | W1 | P2 | `parser.rs`, `parser/error.rs` | 完成证据：`Parser::from_fragment` 把片段 token 的字节/行/列映射回源文件；`errors_inside_string_interpolation_point_at_the_real_source_line` 断言错误落在第 2 行插值内部 |
| ✅ T3.5 | 数值语义统一：Int/Float 混合 `%`、`x/0.0`（保留 IEEE）、比较类型不匹配（报错）、索引负号语义（G-L14） | W2 | P2 | `vm.rs`, `SPEC_LANGUAGE.md`, `SPEC_RUNTIME.md` | 完成证据：`5.0 % 2` → `1.0`；`5 < "a"`/`nil > 0` → `TypeError`；`NaN` 比较恒 false；字符串与数组负索引（含写入）统一；7 个用例 |
| ✅ T3.6 | Float 打印可区分：`1.0` 输出 `1.0` 而非 `1` | W3 | P2 | `value.rs` | 完成证据：`to_str(1.0) == "1.0"`、`to_str(1e300) == "1e300"`；golden 脚本仅 3 行输出变化（`150.0`/`3.0`/`16.0`、`256.0`） |
| ✅ T3.7 | 语义测试集：≥15 个边界用例 | W6 | P1 | `crates/ada_vm/tests/semantics.rs` | 完成证据：21 个用例覆盖 M3 全部规则 + 交叉回归；另有 `persistent_eval.rs` 的跨轮求值不可变性用例 |

**退出标准**：✅ 达成 —— 规范文档中每条 M3 语义规则都有对应测试（`semantics.rs` 21 个）；
`VmError::NoMatch` / `CompileError::AssignToImmutable` 已进入错误模型表。
⚠️ 遗留：错误信息统一带 `file:line:col` 属 T0.6/T1.8（P1，未在本轮范围）。

---

### M4 · 异步与并发运行时 🚧（T4.1/T4.2/T4.3/T4.5/T4.6 ✅，T4.4 按 D2 转为非目标）

**前置决策**：D2（真 Fiber + Tokio，或先落地"明确标注的同步占位"）。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| ✅ T4.1 | `Instruction::Spawn` 语义改造：携带 `callee + args`，真正创建待执行任务而非包装已求值结果 | W4/W2 | P1 | `instruction.rs`, `compiler.rs`, `vm.rs` | 完成证据：`async.rs::spawn_does_not_execute_the_call_at_the_spawn_point`（spawn 后任务体未执行）；非调用表达式走 `SpawnValue` |
| ✅ T4.2 | 任务调度器：任务队列 + 挂起模型（按 D2 采用单线程协作式事件循环 + `SavedExecution`） | W4 | P1 | `ada_vm/src/{task,vm}.rs` | 完成证据：2 个各 `sleep_ms(200)` 的任务实测总耗时 **200ms**（< 250ms），串行对照 401ms；4×150ms 也只需约 150ms |
| ✅ T4.3 | `sleep_ms` 非阻塞化（协作式调度器 + 真实定时器） | W4 | P1 | `builtins/core.rs`, `vm.rs` | 完成证据：`concurrent_sleeps_overlap_instead_of_adding_up`（含下界断言证明真的等待）；适配器回调内的阻塞降级已记录（SPEC_ASYNC §4.3） |
| ❌ T4.4 | 宿主异步接口：`Engine::eval_async` / `register_async_fn` | W5 | P1 | — | **按 D2 重定义为非目标**：不引入 Tokio/Future 桥接；宿主异步工作用 `register_fn` 同步注入（SPEC_ASYNC §6）。需要真 I/O 并发时按 D2b 重新评估 |
| ✅ T4.5 | `await_any`（原 `await_race`）语义正确化 + `timeout(task, ms)` | W3/W4 | P2 | `builtins/core.rs`, `vm.rs` | 完成证据：`await_any` 返回最先结算者（慢/快两任务用例）；`timeout` 到期取消任务并返回 `nil`（实测 51ms 触发、状态 `cancelled`） |
| ✅ T4.6 | 并发契约文档化：任务生命周期、取消、错误传播规则 | W6 | P1 | `SPEC_ASYNC.md` | 完成证据：SPEC_ASYNC §5 状态机 + 错误传播/取消/泄漏/确定性四张条文表；`async.rs` **26 个用例**覆盖每条条文 |

**退出标准**：✅ 基本达成 —— 并发性由测试断言（真实耗时 + 下界断言），ASYNC 规范与实现一一对应。
⚠️ 遗留：T4.4 按 D2 转为非目标；适配器回调内无法挂起（G-A9）与任务上限/整体墙钟预算（T5.3/T5.4）留待后续。

---

### M5 · 沙箱与宿主安全 📋（预计 1–2 周）

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T5.1 | `SandboxProfile`：`Strict`（默认，仅纯计算 + json/math）/ `Standard`（+ time/env 白名单）/ `Custom` | W5 | P1 | `ada_engine/src/lib.rs`, `ada_vm/src/builtins/mod.rs` | `Strict` 下 `env.get`/`time.*` 抛权限错误 |
| T5.2 | `env` 白名单：仅允许宿主显式声明的键 | W5/W3 | P1 | `builtins/sys.rs` | 未授权键返回 `nil` 且不读 host 环境 |
| T5.3 | 内存预算 `MaxMemoryLimit`：值/容器规模追踪，超限返回专用错误 | W5/W2 | P2 | `ada_vm/src/{value,vm}.rs` | 超大数组分配被拒绝；错误类型可断言 |
| T5.4 | 墙钟超时（wall-clock）：与 fuel 互补，防"单条指令长时间阻塞" | W5 | P2 | `ada_engine/src/lib.rs` | 死循环在 N ms 内被中断 |
| T5.5 | `NativeModule` trait + `Engine::register_module` | W5 | P2 | `ada_engine`, `ada_vm` | 自定义模块可注册并被脚本调用（含测试） |
| T5.6 | 安全测试集：越权访问、资源耗尽、panic 注入 | W6 | P1 | `crates/ada_engine/tests/security.rs` | ≥10 用例全绿 |

**退出标准**：默认 `Strict` 配置下，脚本无法读取宿主环境、无法消耗无界内存、无法 panic 宿主。

---

### M6 · 性能与 JIT 📋（预计 3–6 周，需先有基准）

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T6.1 | 基准基线：`criterion`，覆盖 fib 递归、数值循环、字符串拼接、迭代器管道、JSON 解析 | W6 | P1 | `benches/`（新） | 5 组基准出数；结果归档到 CI artifact |
| T6.2 | VM 微优化：寄存器窗口回收、常量折叠（`LoadInt+Add`）、常用指令融合 | W2 | P2 | `compiler.rs`, `vm.rs` | 基准相较 T6.1 基线提速 ≥30%，无回归 |
| T6.3 | 决策 D1 落地：维持 `enum Value` 或实施 NaN-boxing | W2 | P2 | `value.rs` | 决策记录 + （若实施）基准提速数据 |
| T6.4 | `ada_jit` crate：字节码 → Cranelift IR 下沉，热点计数与 guard/deopt | W2 | P3 | `crates/ada_jit/`（新） | 热点函数原生执行；`fib(30)` 较 VM 提速 ≥10x |
| T6.5 | 性能回归门禁：基准劣化 >5% 时 CI 报警 | W6 | P3 | CI 配置 | 门禁生效 |

**退出标准**：性能陈述有基准数据支撑；白皮书中所有性能数字改为引用基准结果。

---

### M7 · 生态与工具链 📋（长期）

| ID | 任务 | 优先级 | 验收 |
| :--- | :--- | :--- | :--- |
| T7.1 | VSCode 语法高亮（TextMate grammar） | P3 | demo 脚本在 VSCode 中正确着色 |
| T7.2 | LSP 最小实现：诊断（语法/运行时错误行列） | P3 | 编辑器内实时报错 |
| T7.3 | 文档站：mdBook 从 `docs/` 单源构建 | P2 | `mdbook build` 成功且无死链 |
| T7.4 | AI Prompt 模板与语法规格单页（供 LLM 生成代码） | P2 | 单页规格 + 3 个可运行示例 |
| T7.5 | 可选扩展模块：`fs` / `http` / `crypto`（受 `SandboxProfile` 管控） | P3 | 模块遵循 T5.1 权限模型 |

---

## 5. 质量门禁 (Definition of Done)

任何任务/PR 合并前必须全部满足：

1. `cargo fmt --check` 通过；`cargo clippy --workspace -- -D warnings` 通过。
2. `cargo test --workspace` 全绿，且新功能/修复**必须**附带测试（无测试的 P0/P1 任务不算完成）。
3. **禁止新增 panic 路径**：库代码中不得出现可被脚本输入触发的 `unwrap`/`expect`/切片索引/未检查算术；确需 `unwrap` 时必须有注释说明不可失败的不变量。
4. 行为变更同步更新对应 `SPEC_*.md`，并在本计划任务行标注完成证据。
5. 公开 API 变更需在 `CHANGELOG.md` 记录（T0.1 之后建立）。
6. 提交信息格式：`<area>: <what>`（area ∈ `lexer|parser|bytecode|vm|stdlib|async|sandbox|docs|ci`）。

---

## 6. 测试与基准策略

| 层级 | 位置 | 覆盖目标 | 里程碑 |
| :--- | :--- | :--- | :--- |
| 单元测试 | 各 crate `src/lib.rs` 内 | 词法/语法/编译/VM 指令 | 已有，持续补 |
| 集成测试 | `crates/*/tests/` | 跨 crate 行为、脚本级语义、稳健性、安全性 | M1–M5 |
| Golden 测试 | `examples/showcase.ada` + `examples/showcase.expected.txt` | 端到端行为与规范示例一致性（防文档漂移） | ✅ M2 已启用（`golden_showcase.rs`） |
| Fuzz | `fuzz/`（cargo-fuzz） | lexer/parser 在随机输入下不 panic、不 OOM | M3 起 |
| 基准 | `benches/`（criterion） | 性能回归门禁 | M6 |
| 文档测试 | `crates/ada_engine/tests/doc_examples.rs` | README / SPEC 中标注可运行的示例（防文档漂移，R1 缓解） | ✅ M1 已启用 |
| 文档门禁 | `crates/ada_cli/tests/doc_links.rs` | 链接/锚点可达 + 规范文档不得写路线图 | ✅ M0 已启用 |

**当前缺口**：无 golden（`examples/` 期望输出比对）、无 fuzz、无基准；`ada_ast` 测试数为 0（crate 仅含数据定义）。
**当前状态**：`cargo test --workspace` → 156 passed / 0 failed（单元 34 + 稳健性 17 + 适配器 16 + 闭包 15 + 语义 21 + 异步 26 + 文档示例 14 + 多轮求值 8 + golden 3 + 文档门禁 2）。

---

## 7. 版本与发布节奏

| 版本 | 内容 | 对应里程碑 |
| :--- | :--- | :--- |
| `0.1.0` | 当前原型（前端 + VM + Fuel + 嵌入 API） | 基线 |
| `0.2.0` | 不崩溃、文档示例可运行、语义收紧 | M0–M3 |
| `0.3.0` | 真异步并发 + 沙箱权限与资源预算 | M4–M5 |
| `0.4.0` | 基准与 VM 优化（JIT 可选） | M6 |
| `1.0.0` | 工具链与生态成型 | M7 |

发布条件：该里程碑全部 P0/P1 任务 ✅；golden 测试通过；`CHANGELOG.md` 更新；文档状态标注同步。

---

## 8. 风险登记表

| ID | 风险 | 影响 | 概率 | 缓解措施 | 触发条件 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| R1 | **文档-实现漂移**（承诺 > 实现） | 高：使用者按文档写代码即失败，损害可信度 | 已发生 | T0.3 状态标注 + 单一权威 + golden 测试锁死示例 | 示例与文档不一致 |
| R2 | 解释器 panic 被 AI 脚本触发 | 高：宿主进程崩溃 | 已发生 | M1 全量修复 + catch_unwind 兜底 + 禁 panic 门禁 | 脚本返回 exit 101 |
| R3 | 无版本控制导致的不可回滚 | 高 | 已发生 | T0.1 立即建立基线 | 无 commit |
| R4 | 范围蔓延（JIT/NaN-boxing 过早启动） | 中：基础稳健性被拖延 | 中 | M6 入口绑定 T6.1 基准；D1/D2 决策前置 | M1–M3 未完成即开 M6 |
| R5 | 性能叙事无数据支撑 | 中 | 高 | M6 基准先行；白皮书数字改为引用基准 | 文档出现未测性能数字 |
| R6 | 异步改造破坏现有同步语义 | 中 | 中 | D2 决策 + 并发契约规范 + 兼容测试 | 现有 async 测试失败 |
| R7 | 单人/小团队节奏下里程碑过长 | 中 | 中 | 每里程碑拆到 ≤1 周可验收的子任务；优先 P0 | 里程碑超期 >50% |
| R8 | `Arc<RwLock>` 值模型在写并发下引入死锁/性能问题 | 中 | 低 | T5.3 资源追踪 + M6 基准；必要时重构为 GC（D6） | 基准或并发测试暴露问题 |

---

## 9. 非目标 (Non-Goals，本阶段明确不做)

- 泛型、宏系统、trait/接口、模块（`import`）系统、OOP 类模型。
- 完整 GC（分代/压缩）；当前采用 `Arc` 引用计数，仅在基准证伪时才重新评估（D6）。
- 完整 LSP（补全/重构）；M7 只做诊断。
- 包管理器与中央仓库。
- 非 Windows/Linux/macOS 的目标平台适配（保持源码跨平台即可）。

---

## 10. 待决决策 (Open Decisions)

| ID | 决策点 | 选项 | 影响 | 截止 |
| :--- | :--- | :--- | :--- | :--- |
| D1 | 值模型 | A 保持 `enum Value`；B 实施 NaN-boxing | B 影响 `value.rs` 全量重写与所有 builtins；收益需基准证明 | M6 开始前 |
| D2 | 异步实现 | ~~A 真 Fiber + Tokio~~；~~B 同步 Future 占位~~ → **已决：C（单线程协作式事件循环，无 Tokio/无线程）** | **决策（2026-10，M4 执行期）**：任务 = 同一 VM 上可保存的执行现场（帧栈 + 寄存器窗口），挂起点白名单（`sleep_ms`/`yield_now`/`await*`/`timeout`），就绪队列 FIFO + 真实定时器。理由：零依赖、执行顺序完全确定（对 AI 生成代码可测性关键）、嵌入成本低、挂起点天然是沙箱墙钟检查点。代价：无 CPU 并行；适配器回调内无法保存宿主栈，`sleep_ms` 退化为阻塞（已记录为 G-A9）。T4.4 因此重定义为 ❌ 非目标 | ✅ 已决（M4 落地） |
| D3 | 迭代器求值 | ~~A 即时~~；~~B 惰性~~ → **已决：A（即时）** | **决策（2026-10，M2 执行期）**：适配器一律物化为 `Array`，先把能力与错误契约打通；惰性链需引入迭代器对象与去虚拟化，收益要用基准证明（缺口 G-S12）。若未来启用惰性，`collect()` 语义与 `IterNext` 指令可复用 | ✅ 已决（T2.1 落地） |
| D4 | `{ x }` 歧义 | ~~A 判为 block~~；~~B 保持现状判 map~~ → **已决：B（保持 map）** | **决策（2026-10，M3 执行期）**：`{}` 是 map 字面量，`{ name }` 是 Token 高效的简写（AI-first 目标），块表达式改为"必须以语句开头"即可无歧义地区分（SPEC_LANGUAGE §5.6）。选 A 会删掉 map 简写并静默改变既有脚本含义 | ✅ 已决（T3.3 落地） |
| D5 | 整数溢出语义 | ~~A 报错~~；~~B 回绕~~ → **已决：A（报错）** | **决策（2026-10，M1 执行期）**：采用 `checked_*` + `VmError::TypeError`。理由：AI 沙箱场景下"静默回绕"会产生难排查的错误结果，而硬错误可被宿主捕获；若未来基准显示热路径成本显著，再评估回绕或 f64 提升 | ✅ 已决（T1.2 落地） |
| D6 | 内存模型 | A 维持 `Arc<RwLock>`；B 引入 GC | B 是大重构 | M5 结束前评估 |
| D7 | `match` 非穷尽 | ~~A 运行时错误~~；~~B 编译期警告~~ → **已决：A（运行时错误）** | **决策（2026-10，M3 执行期）**：无 arm 匹配时旧实现会留下**未赋值的目标寄存器**（读到上一个值而非 `nil`），这是静默错误值；改为 `MatchFail` 指令抛 `VmError::NoMatch`。选 B（仅警告）无法阻止错误值继续传播 | ✅ 已决（T3.2 落地） |
| D8 | 闭包捕获语义 | ~~A 值拷贝~~；~~B 引用共享（Cell）~~ → **已决：B（引用共享）** | **决策（2026-10，M2 执行期）**：被捕获变量装箱为共享槽，闭包内写入对外层可见；循环变量每轮独立槽（类 JS `let`）；代价是被捕获变量的读写多一层 Cell 间接（仅对被捕获的变量生效，未捕获的局部变量仍是纯寄存器） | ✅ 已决（T2.2 落地） |

> 决策流程：在本文档补记结论 → 更新对应 `SPEC_*.md` → 再动手实现。

---

## 11. 文档整合与治理

### 11.1 最终结构（每主题唯一权威）

```
docs/
├── README.md             # 文档索引：状态图例、权威来源矩阵、阅读路径、治理摘要
├── DEVELOPMENT_PLAN.md   # 唯一路线图/里程碑/任务/验收（本文档）
├── WHITEPAPER.md         # 愿景、定位、Token 论据（不含规范细节与路线图）
├── SPEC_LANGUAGE.md      # 语法、类型、语义规则（唯一权威）
├── SPEC_RUNTIME.md       # 字节码、指令集、VM、值模型、错误模型、嵌入 API、沙箱与权限
├── SPEC_STDLIB.md        # 全局函数、原生模块、原型方法、统一迭代器协议与适配器（合并原 ITERATOR_SPEC）
└── SPEC_ASYNC.md         # 异步语法、并发原语、调度与挂起模型、任务契约
```

原 `LANGUAGE_DESIGN.md`、`STDLIB_DESIGN.md`、`ITERATOR_SPEC.md`、`ASYNC_DESIGN.md` 已按主题拆分/合并进上述规范文档并移除；新文档状态与阅读路径见 [docs/README.md](README.md)。

### 11.2 治理规则

1. **一个主题只允许一个权威文档**；其它文档引用链接，不复制内容。
2. 所有规范条目必须带状态标记（✅/⚠️/🚧/📋/❌）。
3. 禁止在规范文档中写"实施路线/里程碑"——统一指向本文档。
4. 规范与实现冲突时以规范为准；改规范需在 §10 留决策记录。
5. 每个里程碑结束时执行一次文档巡检：状态标记、失效链接、示例可运行性。

---

## 12. 进度追踪

| 里程碑 | 预计工时 | 状态 | 退出标准达成 |
| :--- | :--- | :--- | :--- |
| M0 治理基线 | 1–2 天 | ✅ | ☑（T0.6 为 P1 遗留） |
| M1 解释器稳健性 | 3–5 天 | ✅ | ☑（T1.7/T1.8 为 P1 遗留） |
| M2 语言核心补齐 | 1–2 周 | ✅ | ☑ |
| M3 语义收紧 | 1 周 | ✅ | ☑（T0.6/T1.8 错误定位遗留） |
| M4 异步并发 | 2–3 周 | 🚧 | ☑（T4.1/2/3/5/6 完成；T4.4 按 D2 转非目标） |
| M5 沙箱安全 | 1–2 周 | 📋 | ☐ |
| M6 性能与 JIT | 3–6 周 | 📋 | ☐ |
| M7 生态工具链 | 长期 | 📋 | ☐ |

**更新方式**：任务状态变更直接编辑 §4 对应行；里程碑状态与"退出标准达成"勾选在 §12 汇总；每次更新必须与代码改动同 commit。

---

*本文档取代项目内所有其它位置的路线图与里程碑描述。*
