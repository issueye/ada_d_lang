# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与[语义化版本](https://semver.org/lang/zh-CN/)。
路线图、任务状态与验收标准见 [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md)；规范以 `docs/SPEC_*.md` 为准。

## [Unreleased]

### Added

- **迭代器适配器（T2.1）**：`map` / `filter` / `take` / `skip` / `collect` / `count` / `find` / `any` / `all` / `enumerate` / `rev` / `join` / `sum`，**方法链与全局函数双入口**（`xs.map(f)` 与 `xs |> map(f)`），可用于数组、字符串、字典、`range` 与任意迭代器。
  - 由 VM 实现（`Value::Intrinsic` + `Vm::invoke_adapter`），因为 `NativeFn` 回调无法回调脚本闭包。
  - 当前为**即时求值**（决策 D3 = A），适配器结果一律是 `Array`。
- **显式迭代协议（T2.3）**：`.iter()` 与 `iterator.next()`（耗尽返回 `nil`）；`string.chars_with_idx`；`is_iterator`。
- **REPL / 多轮求值持久化（T2.4）**：模块顶层的 `let`/`mut` 同时写入全局表，同一 `Engine` 的后续 `eval` 可见；函数体内的局部变量不会外泄。
- **Fuel 生命周期（T2.6）**：每次求值自动重新装填配置预算；新增 `remaining_fuel()` / `reset_fuel()`（`Vm` 与 `Engine`），`get_fuel()` 语义明确为"配置预算"。
- 测试：`crates/ada_vm/tests/adapters.rs`（16）、`crates/ada_engine/tests/persistent_eval.rs`（7），并扩充文档示例测试。
- `ada_engine` 公共 re-export：`Value`、`Closure`、`NativeCallback`、`Vm`、`VmError`、`ParseError`、`CompileError`，宿主不再需要显式依赖 `ada_vm`（T0.7）。
- `LICENSE-MIT` 与 `LICENSE-APACHE`，与 `Cargo.toml` 声明的 `MIT OR Apache-2.0` 对齐（T0.2）。
- CI 质量门禁 `.github/workflows/ci.yml`：`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`（T0.4）。
- 集成测试与文档门禁（T0.5）：
  - `crates/ada_vm/tests/robustness.rs`：panic 回归用例（T1.5）
  - `crates/ada_engine/tests/doc_examples.rs`：README / SPEC 示例可运行性（防文档漂移）
  - `crates/ada_cli/tests/doc_links.rs`：Markdown 链接与锚点校验、规范文档禁用路线图章节
- 字符串 `.len_bytes`：返回 UTF-8 字节数（T2.5）。

### Fixed

- **解释器不再因脚本输入杀死宿主进程**（T1.1–T1.4、T1.6）：
  - `%` 除零、`i64::MIN % -1`、`i64::MIN / -1`
  - 整数 `+ - *` 与一元 `-` 溢出
  - 移位量不在 `0..64`（含负数）
  - 多字节字符串按字节索引导致的 char-boundary panic
  - `min([])` / `max([])` 空数组索引越界
- 迭代器适配器 arity 元数据错误导致的 `map([1,2])` 越界 panic（由 `adapters.rs` 用例捕获）。
- `sum` 整数累加溢出、`abs(i64::MIN)`、`math.random_int` 区间宽度溢出三处潜在 panic（改用 checked 运算 / i128）。

### Changed

- 文档整合为「单一权威」结构：新增 `docs/README.md` 索引与 `SPEC_LANGUAGE`/`SPEC_RUNTIME`/`SPEC_STDLIB`/`SPEC_ASYNC`，移除 `LANGUAGE_DESIGN`/`STDLIB_DESIGN`/`ITERATOR_SPEC`/`ASYNC_DESIGN`；路线图唯一收敛到 `docs/DEVELOPMENT_PLAN.md`（T0.3）。
- `cargo fmt` 全量格式化（纯格式，无语义变更）。
- 冲突的语义已定案：整数溢出采用「报错」而非回绕（决策 D5）；迭代器适配器采用「即时求值」（决策 D3）。
- 成员派发优先级明确化：map 上的适配器同名键优先（`{ map: 7 }.map` → `7`），既有原型方法名（`len` 等）仍优先于同名键（`{ len: 9 }.len` → `1`，缺口 G-S12/S13）。

### Breaking

- 字符串长度语义由**字节**改为**字符**：`"Ada 语言".len` 与 `len("Ada 语言")` 由 `10` 变为 `6`；字节数请使用 `.len_bytes`。
- 字符串索引改为按字符定位：`"语言"[0]` 由 panic 变为返回 `"语"`。
- 整数溢出、取模零、移位越界由「debug 构建 panic / release 构建静默回绕」改为返回 `VmError::TypeError`（宿主可用 `Error::Vm` 捕获）。
- `sum` 语义扩展为接受任意可迭代值（原先仅接受数组，非数组报错）；`sum`/`join`/`enumerate`/`rev` 由原型内联实现改为统一适配器实现。
