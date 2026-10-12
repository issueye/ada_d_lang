# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与[语义化版本](https://semver.org/lang/zh-CN/)。
路线图、任务状态与验收标准见 [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md)；规范以 `docs/SPEC_*.md` 为准。

## [Unreleased]

### Added

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

### Changed

- 文档整合为「单一权威」结构：新增 `docs/README.md` 索引与 `SPEC_LANGUAGE`/`SPEC_RUNTIME`/`SPEC_STDLIB`/`SPEC_ASYNC`，移除 `LANGUAGE_DESIGN`/`STDLIB_DESIGN`/`ITERATOR_SPEC`/`ASYNC_DESIGN`；路线图唯一收敛到 `docs/DEVELOPMENT_PLAN.md`（T0.3）。
- `cargo fmt` 全量格式化（纯格式，无语义变更）。
- 冲突的语义已定案：整数溢出采用「报错」而非回绕（决策 D5）。

### Breaking

- 字符串长度语义由**字节**改为**字符**：`"Ada 语言".len` 与 `len("Ada 语言")` 由 `10` 变为 `6`；字节数请使用 `.len_bytes`。
- 字符串索引改为按字符定位：`"语言"[0]` 由 panic 变为返回 `"语"`。
- 整数溢出、取模零、移位越界由「debug 构建 panic / release 构建静默回绕」改为返回 `VmError::TypeError`（宿主可用 `Error::Vm` 捕获）。
