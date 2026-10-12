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
| 测试 | `cargo test --workspace` → 34 passed / 0 failed（vm 20、parser 6、lexer 3、engine 3、bytecode 2、cli 0、ast 0） |
| 版本控制 | **无任何 commit**，全部文件 untracked |
| 许可证 | `Cargo.toml` 声明 `MIT OR Apache-2.0`，但无 `LICENSE` 文件 |
| CI / fmt / clippy / bench | 均无 |

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
| 4 类脚本可触发宿主进程 panic（`%0`、整数溢出、移位越界、UTF-8 字符串索引） | [WHITEPAPER](WHITEPAPER.md) §2「Fail-Safe Sandboxing」+ 本文档 §5 门禁第 3 条 | exit code 101 崩溃 | T1.1–T1.4 |
| 无 `filter`/`map`/`take` 等迭代器适配器 | [SPEC_STDLIB §6](SPEC_STDLIB.md) | `UndefinedGlobal("filter")` | T2.1 |
| 闭包无 upvalue 捕获 | [SPEC_LANGUAGE §6](SPEC_LANGUAGE.md) | `UndefinedGlobal("a")` | T2.2 |
| REPL 变量不跨行保留 | [../README.md](../README.md) 快速开始 | `Undefined global 'a'` | T2.4 |
| `spawn` 无并发语义、`sleep_ms` 阻塞线程 | [SPEC_ASYNC §1、§3](SPEC_ASYNC.md) | 2×200ms 实测 401ms 串行 | T4.1–T4.4 |
| 无 `SandboxProfile` / 内存预算；`env.get` 无白名单 | [SPEC_RUNTIME §10](SPEC_RUNTIME.md) | 实测可读 `PATH`/`USERPROFILE` | T5.1–T5.4 |
| 值模型非 NaN-boxing | [SPEC_RUNTIME §2](SPEC_RUNTIME.md) | `enum Value` + `Arc<RwLock>` | 决策 D1 |
| 无 `ada_jit` / Cranelift | [SPEC_RUNTIME §11](SPEC_RUNTIME.md) | crate 不存在 | T6.4 |
| `let` 不可变性未强制 | [SPEC_LANGUAGE §5.1](SPEC_LANGUAGE.md) | `let a = 1; a = 2` 静默生效 | T3.1 |
| `len` 返回字节数而非字符数 | [SPEC_STDLIB §2.6、§4.1](SPEC_STDLIB.md) | `"Ada 语言".len` → 10（应 6） | T2.5 |
| `ada_engine` 未 re-export `Value`，文档嵌入示例不可编译 | [WHITEPAPER](WHITEPAPER.md) §2「Dual Execution Modes」 | `use ada_engine::{Engine, Value}` 编译失败（E0603） | T0.7 |
| 整合前 4 处独立"路线图"章节互相重叠 | 原 5 份文档各有一节 | 已收敛到本文档 | ✅ T0.3 |

### 2.3 复现命令

```bash
cargo build --workspace
cargo test --workspace
cargo run -p ada_cli -- examples/demo.ada
cargo run -p ada_cli -- dump examples/demo.ada
cargo run -p ada_cli -- -e "let arr = [10, 20, 30]; arr |> sum()"
```

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

### M0 · 治理基线 📋（预计 1–2 天）

**目标**：建立可 review、可回滚、可自动校验的工程基线。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T0.1 | `git init` + 首次提交（含 `Cargo.lock`、`.gitignore`） | W6 | P0 | 仓库根 | `git log --oneline` 至少 1 条；`git status` 干净 |
| T0.2 | 补 `LICENSE`（或把 `Cargo.toml` 的 license 改为与现状一致） | W6 | P0 | `LICENSE`, `Cargo.toml` | 文件存在且与 `Cargo.toml` 一致 |
| ✅ T0.3 | 文档整合：单一权威 + 删除 4 处重复路线图 + 状态标注 | W6 | P0 | `docs/**`, `README.md` | 完成证据：新增 `docs/README.md` 与 `SPEC_LANGUAGE`/`SPEC_RUNTIME`/`SPEC_STDLIB`/`SPEC_ASYNC`；移除 `LANGUAGE_DESIGN`/`STDLIB_DESIGN`/`ITERATOR_SPEC`/`ASYNC_DESIGN`；规范内无路线图章节；全量链接检查通过（§11） |
| T0.4 | CI：`cargo fmt --check` + `cargo clippy -D warnings` + `cargo test --workspace` + Markdown 链接/锚点检查 | W6 | P0 | `.github/workflows/ci.yml`（或等价） | CI 绿灯；故意引入 warning 时红灯；新增/删除文档后死链能被 CI 拦下 |
| T0.5 | 建立测试目录规范：`crates/*/tests/`（集成）、`benches/`（基准）、`examples/`（golden） | W6 | P1 | 各 crate | 空目录 + README 说明落位，后续任务按此落位 |
| T0.6 | 统一错误信息格式为 `file:line:col: error: msg`（CLI 侧渲染） | W1 | P1 | `ada_cli`, `ada_parser` | 语法错误输出含文件路径、行列；已有错误类型保留 span |
| T0.7 | `ada_engine` 补齐公共 re-export（`Value`、`Closure`、`VmError`、`ParseError`、`CompileError`） | W5 | P1 | `ada_engine/src/lib.rs` | `use ada_engine::{Engine, Value}` 可编译；根 README 与 SPEC_RUNTIME §9.3 示例去掉 `ada_vm` 显式依赖后仍通过 |

**退出标准**：仓库有基线提交；CI 生效；文档单一权威结构成型；`cargo fmt --check` 与 clippy 全绿。

---

### M1 · 解释器稳健性 P0 📋（预计 3–5 天）

**目标**：**任何脚本输入都不得让宿主进程 panic**。这是 S1 阶段的第一条红线。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T1.1 | `%` 除零：返回 `VmError`，与 `/` 行为一致 | W2 | P0 | `ada_vm/src/vm.rs` | 用例 `10 % 0` → 干净错误，exit 1 |
| T1.2 | 整数算术溢出：`+ - *` 及 `Neg` 用 `checked_*`，溢出返回 `VmError::TypeError`（或按 D5 决策切换为回绕语义并写规范） | W2 | P0 | `ada_vm/src/vm.rs` | 用例 `9223372036854775807 + 1` → 干净错误或确定性回绕 |
| T1.3 | 移位越界：左右移校验 `0 <= n < 64` | W2 | P0 | `ada_vm/src/vm.rs` | 用例 `1 << 70`、`1 << -1` → 干净错误 |
| T1.4 | 字符串索引 / 切片按字符（`char_indices`/`chars()`）定位，消除 byte-boundary panic | W2 | P0 | `ada_vm/src/vm.rs` | 用例 `"语言"[0]` → `"语"` |
| T1.5 | 4 类 panic 的回归测试（含多字节、极值、边界组合） | W6 | P0 | `crates/ada_vm/tests/robustness.rs` | ≥8 个用例，全绿 |
| T1.6 | 全 crate 审计其余 panic 源（`unwrap`/`expect`/索引越界/`as` 截断），产出清单并逐条处理或标注 | W2 | P0 | 全 crate | 审计清单提交；剩余 `unwrap` 均有注释说明不变量 |
| T1.7 | 宿主兜底：`Engine::eval*` 边界 `catch_unwind`，把意外 panic 转为 `Error::Panic`（默认开启，可关闭） | W5 | P1 | `ada_engine/src/lib.rs` | 故意 panic 的 native fn 不杀死宿主进程 |
| T1.8 | 错误可定位：`VmError` 附带当前指令的 `Span`，CLI 输出行号 | W2/W6 | P1 | `ada_vm`, `ada_bytecode`, `ada_cli` | 运行时错误输出 `file:line:col` |

**退出标准**：§2.2 中 4 个 P0 探针全部返回干净错误（exit 1 而非 101）；新增测试 ≥8；`cargo test --workspace` 全绿；CI 通过。

---

### M2 · 语言核心补齐 P1 📋（预计 1–2 周）

**目标**：白皮书与规范中所有"已写进语法/标准库"的示例都能真实运行。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 / 完成证据 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T2.1 | 迭代器适配器：`map`/`filter`/`take`/`skip`/`enumerate`/`collect`/`count`/`find`/`any`/`all`/`join`/`sum`，同时提供**方法链**与**全局函数**（供 `|>` 使用） | W3 | P1 | `ada_vm/src/vm.rs`（原型派发）, `builtins/core.rs` | `range(1,1000) \|> filter(x => x % 2 != 0) \|> map(x => x * x) \|> take(5) \|> sum()` == `165` |
| T2.2 | 闭包 upvalue 捕获：新增 `GetUpvalue`/`SetUpvalue` 与带 upvalue 描述的 `Closure` 指令；编译器维护 upvalue 表 | W2/W1 | P1 | `ada_bytecode/src/{instruction,compiler}.rs`, `ada_vm/src/vm.rs` | `fn outer(a) { let g = x => x + a; g(5) }` → `15`；多层嵌套捕获用例通过 |
| T2.3 | 原型方法补齐：`string.chars_with_idx/iter/next`、array `iter/next`、map `iter/next` | W3 | P1 | `ada_vm/src/vm.rs` | SPEC_STDLIB §4/§5 示例逐条可运行 |
| T2.4 | REPL 状态持久化：顶层 `let`/`mut` 落到持久 scope（或编译为 `SetGlobal`），跨行可见 | W2/W6 | P1 | `ada_bytecode/src/compiler.rs`, `ada_engine` | 连续输入 `let a = 41` → `a + 1` → 输出 `42` |
| T2.5 | 长度语义统一：`.len` 与 `len()` 返回**字符数**；新增 `.len_bytes` 表达字节数；同步规范 | W3 | P1 | `ada_vm/src/vm.rs`, `builtins/core.rs`, `SPEC_STDLIB.md` | `len("Ada 语言") == 6`；`"Ada 语言".len_bytes == 10` |
| T2.6 | `fuel` 生命周期：`execute()` 每次重新装填；新增 `reset_fuel` / `remaining_fuel` | W2/W5 | P1 | `ada_vm/src/vm.rs`, `ada_engine/src/lib.rs` | 一次 `OutOfFuel` 后同一 `Engine` 仍可正常执行下一段脚本 |
| T2.7 | `Engine` 持久化求值测试（多轮 eval 共享状态） | W6 | P1 | `crates/ada_engine/tests/` | ≥3 个用例覆盖函数/变量/模块 |

**退出标准**：`examples/demo.ada` 扩展为覆盖 SPEC_LANGUAGE §5–§8、SPEC_STDLIB §4–§6、SPEC_ASYNC §2 中全部脚本示例的 golden 脚本；golden 输出比对通过。

---

### M3 · 语义规范收紧 📋（预计 1 周）

**目标**：把"能跑"提升为"语义可预测、报错可定位"。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T3.1 | `let` 不可变性：编译期拒绝向非 `mut` 绑定赋值 | W1 | P1 | `compiler.rs`, `parser.rs`（`is_mut` 落地） | `let a = 1; a = 2` → 编译错误含行列 |
| T3.2 | `match` 非穷尽：无 `_` 且无匹配时抛运行时错误（或编译期警告，按 D4/D7 决策） | W1/W2 | P1 | `compiler.rs`, `vm.rs` | `match 99 { 1 => "one" }` → 明确错误而非静默 `nil` |
| T3.3 | `{ x }` map / block 歧义规则定型并写入规范 | W1 | P1 | `parser.rs`, `SPEC_LANGUAGE.md` | 规则有规范条文 + 正反用例测试 |
| T3.4 | 插值子表达式 span 映射修复（片段内行列 → 源文件绝对行列） | W1 | P2 | `parser.rs`, `lexer.rs` | `$"{1 +}"` 报错行号指向源文件真实位置 |
| T3.5 | 数值语义统一：Int/Float 混合 `%` 与位运算规则定义并实现 | W2 | P2 | `vm.rs`, `SPEC_LANGUAGE.md` | `5.0 % 2` 行为与规范一致 |
| T3.6 | Float 打印可区分：`1.0` 输出 `1.0` 而非 `1` | W3 | P2 | `value.rs` | `println(1.0)` → `1.0`；demo 输出同步更新 |
| T3.7 | 语义测试集：≥15 个边界用例（作用域、遮蔽、短路、赋值目标、深度递归） | W6 | P1 | `crates/ada_vm/tests/` | 全绿；覆盖 M3 全部规则 |

**退出标准**：规范文档中每一条语义规则都有对应测试；错误信息统一含 `file:line:col`。

---

### M4 · 异步与并发运行时 📋（预计 2–3 周）

**前置决策**：D2（真 Fiber + Tokio，或先落地"明确标注的同步占位"）。

| ID | 任务 | 工作流 | 优先级 | 涉及文件 | 验收 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| T4.1 | `Instruction::Spawn` 语义改造：携带 `proto_idx + args`，真正创建待执行任务而非包装已求值结果 | W4/W2 | P1 | `instruction.rs`, `compiler.rs`, `vm.rs` | `spawn f()` 不在 spawn 处执行 `f` |
| T4.2 | 任务调度器：任务队列 + `VmYield::Awaiting` 或等价挂起模型 | W4 | P1 | `ada_vm/src/vm.rs`（新 `task.rs`） | 2 个各 `sleep_ms(200)` 的任务并发总耗时 < 250ms |
| T4.3 | `sleep_ms` 非阻塞化（按 D2 走 tokio timer 或协作式调度器） | W4 | P1 | `builtins/core.rs` | `sleep_ms` 不再阻塞 OS 线程（集成测试断言并发耗时） |
| T4.4 | 宿主异步接口：`Engine::eval_async` / `register_async_fn` | W5 | P1 | `ada_engine/src/lib.rs` | 在 `#[tokio::main]` 下调用异步 native fn 成功 |
| T4.5 | `await_race` / `timeout(ms)` 语义正确化 | W3/W4 | P2 | `builtins/core.rs` | 首个就绪返回；超时返回 `nil` 或约定值 |
| T4.6 | 并发契约文档化：任务生命周期、取消、错误传播规则 | W6 | P1 | `SPEC_ASYNC.md` | 规范含"错误传播/取消"小节 + 测试 |

**退出标准**：并发性可由测试断言（而非人工观察）；ASYNC 规范与实现一一对应。

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
| Golden 测试 | `examples/*.ada` + 期望输出 | 端到端行为与文档示例一致性（防文档漂移） | M2 起 |
| Fuzz | `fuzz/`（cargo-fuzz） | lexer/parser 在随机输入下不 panic、不 OOM | M3 起 |
| 基准 | `benches/`（criterion） | 性能回归门禁 | M6 |
| 文档测试 | `README`/`docs` 代码块 | 文档示例可运行（T7.3 后启用） | M7 |

**当前缺口**：无集成测试（`tests/` 不存在）、无 golden、无 fuzz、无基准；`ada_cli` 与 `ada_ast` 测试数为 0。

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
| D2 | 异步实现 | A 真 Fiber + Tokio；B 先落地"同步 Future 占位"并明确标注 | 决定 M4 工作量与文档措辞 | M4 开始前 |
| D3 | 迭代器求值 | A 即时（先可用）；B 惰性（符合 SPEC_STDLIB §5.2「惰性优先」原则） | 惰性需引入迭代器对象与适配器链，工作量更大 | T2.1 开始时 |
| D4 | `{ x }` 歧义 | A 判为 block（需显式 `{ "x": x }` 表 map）；B 保持现状判 map | 影响既有脚本兼容性与规范 | T3.3 前 |
| D5 | 整数溢出语义 | A 报错（更安全，适合 AI 沙箱）；B 回绕（更快，类 C） | 影响 M1 实现与规范 | T1.2 前 |
| D6 | 内存模型 | A 维持 `Arc<RwLock>`；B 引入 GC | B 是大重构 | M5 结束前评估 |
| D7 | `match` 非穷尽 | A 运行时错误；B 编译期警告 | 影响语言严格度 | T3.2 前 |

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
| M0 治理基线 | 1–2 天 | 📋 | ☐ |
| M1 解释器稳健性 | 3–5 天 | 📋 | ☐ |
| M2 语言核心补齐 | 1–2 周 | 📋 | ☐ |
| M3 语义收紧 | 1 周 | 📋 | ☐ |
| M4 异步并发 | 2–3 周 | 📋 | ☐ |
| M5 沙箱安全 | 1–2 周 | 📋 | ☐ |
| M6 性能与 JIT | 3–6 周 | 📋 | ☐ |
| M7 生态工具链 | 长期 | 📋 | ☐ |

**更新方式**：任务状态变更直接编辑 §4 对应行；里程碑状态与"退出标准达成"勾选在 §12 汇总；每次更新必须与代码改动同 commit。

---

*本文档取代项目内所有其它位置的路线图与里程碑描述。*
