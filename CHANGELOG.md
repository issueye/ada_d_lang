# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与[语义化版本](https://semver.org/lang/zh-CN/)。
路线图、任务状态与验收标准见 [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md)；规范以 `docs/SPEC_*.md` 为准。

## [Unreleased]

### Added

- **`crypto` 模块（M7 / T7.5）**：`crypto.sha256(text)`、`crypto.hmac_sha256(key, text)`、
  `crypto.hex(text)`。纯计算、不碰宿主能力，因此三档 profile 都可用（与 `json`/`math` 同级）；
  正确性由 Node.js `crypto` 独立实现交叉验证（UTF-8 字节语义、多字节键/消息）。
- **`fs` 模块（M7 / T7.5，受限根目录）**：`read_text` / `write_text` / `exists` / `is_dir` /
  `list`（按名称排序）/ `remove` / `root`。默认**不注册**；只有
  `Sandbox::with_fs_root(dir)`（CLI：`ada --fs-root DIR …`）才暴露，且所有路径被规范化限制在
  根目录内——`..`、绝对路径、符号链接、前缀混淆（`/root` vs `/root-evil`）全部拒绝，
  越界**报错而非静默返回 nil/false**；单次 I/O 与目录列举有上限，不隐式建目录。
  10 个敌意/边界集成用例 + 6 个路径解析单元测试。
- CLI 新增 `--fs-root DIR`（全局开关，配合 `ada`/`ada run`/`ada -e` 使用）。
- 决策 **D9**：`http` 网络模块在 0.1.0 **明确不做**（TLS 依赖、SSRF 面、重定向/超时策略
  自成一体；需要网络时由宿主 `register_fn` 显式注入）。
- **最小 LSP 与编辑器诊断（M7 / T7.2）**：新 crate `ada_lsp` 提供二进制 `ada-lsp`
  （LSP 最小子集：`initialize` 声明全量同步、`didOpen`/`didChange`/`didClose`、
  `shutdown`/`exit`、未知请求回 `-32601`），诊断以 0 基 `range` 推送，与 CLI 同源。
  **只做静态诊断、不执行脚本**（编辑器每次输入都会触发诊断，执行有副作用与卡顿风险），
  覆盖词法/语法/编译错误；运行时错误仍需 `ada run`。
- `Engine::check(source)`：静态检查入口（解析 + 编译，不执行），LSP 与 `ada check` 共用。
- CLI 新增 `ada check <file>`：成功时安静退出 0，失败时输出 `file:line:col: error: <消息>` 并退出 1。
- VSCode 扩展补齐诊断能力：`editors/vscode/lsp-client.js`（零依赖 LSP 客户端核心，
  不引用 `vscode`，因此可在编辑器之外被测试）+ `extension.js`（映射到 `DiagnosticCollection`）
  + `ada.lsp.path` 配置项。
- `docs/EDITORS.md`：编辑器接入说明（VSCode / Neovim / Emacs / Helix）、协议子集表、诊断范围与守护测试清单。
- 测试：`crates/ada_lsp/src/lib.rs` 单元 15（含畸形输入不 panic）、`tests/session.rs` 4
  （真实进程 + 真实分帧会话）、`tests/node_client.rs` 1（**随扩展发布的 JS 客户端**驱动真实二进制做互通验证）。
- **错误定位（T1.8）**：所有类别的错误现在都携带源码位置——
  `CompileError` 变体带 `Span`（`span()` 访问器），VM 用 `FunctionProto.spans` +
  失败指令下标定位（在帧栈被清空前记录），任务失败用任务**自己**的位置。
  CLI 对语法/编译/运行时/任务四类错误全部输出 `file:line:col: error: <消息>`。
  新增 `Engine::eval_diagnostic() -> Result<Value, EvalFailure>`、`EvalFailure { error, position }`、
  `Position { line, col }` 与 `Error::position()`；`eval_value` 签名不变（老代码无需改动）。
  测试：`crates/ada_engine/tests/diagnostics.rs`（12 个）。
- **宿主 panic 兜底（T1.7）**：`Vm::execute` 边界 `catch_unwind`，原生回调 / 适配器回调 /
  自定义模块函数 / **任务体内**的 panic 转为 `VmError::HostPanic { message }`
  （保留 panic 消息与失败位置），不再杀死宿主进程；
  测试见 `crates/ada_engine/tests/host_panic.rs`（5 个）。
  经 criterion 复测无性能代价，确定性指令数基线未变。
- **AI 单页规格（M7 / T7.4）**：`docs/AI_GUIDE.md`——给模型（或人）的语法/语义速查，含 15 个
  ```` ```ada run ```` 代码块（由真实 CLI 执行比对输出）、反直觉规则表、错误信息对照表与提示模板；
  `examples/ai/{transform,async,validate}.ada` 三个完整示例 + golden 输出，
  由 `crates/ada_cli/tests/ai_guide.rs` 守门。
- **文档站（M7 / T7.3）**：`docs/book.toml` + `docs/SUMMARY.md`，`mdbook build docs` 可真实构建
  （产物在 `target/mdbook`）；`crates/ada_cli/tests/docs_site.rs` 校验"每个 `docs/*.md` 必须登记在
  SUMMARY"并执行一次构建；CI 新增 `docs-site` job。
- **VSCode 语法高亮（M7 / T7.1）**：`editors/vscode/`（TextMate 语法 + 语言配置 + 扩展清单）；
  `crates/ada_cli/tests/editor_grammar.rs` 把语法里的关键字/运算符与 `ada_lexer::KEYWORDS`/`OPERATORS`
  做集合比对，杜绝漂移。
- `ada_lexer::KEYWORDS` / `ada_lexer::OPERATORS`：编辑器与 AI 规格共用的权威记号表（由词法测试保证）。
- `ParseError::position()` / `LexError::position()`：错误位置公开 API（T0.6/T7.2 前置）。
- **基准与性能门禁（M6 / T6.1 / T6.5）**：`benches/`（独立包，criterion）覆盖 5 组基准 + 冷启动 + 仅编译，
  程序源 `benches/programs/*.ada`；`crates/ada_vm/tests/perf_budget.rs` 以**指令数**做确定性门禁
  （超基线 5% 失败，并校验基准程序的计算结果），基线存于 `benches/budgets.txt`。
  `Vm::enable_instruction_counting` / `instructions_executed` 公开计量接口；CI 新增 `benchmarks` job。
- `crates/ada_vm/examples/size_probe.rs`：打印 `size_of::<Value>()` 等值模型尺寸（决策 D1 的证据工具）。
- **沙箱与宿主安全（M5 / T5.1–T5.6）**：`Engine::new()` 现在是**最小权限**（strict）。
  - `Sandbox` 三档 profile + builder：`strict()`（仅纯计算 + `json`/`math`；内存 16 MiB / 墙钟 5s / 任务 128）、`standard()`（+ `time`/`sys`/`env` 白名单/`sleep_ms`）、`permissive()`（等效 M5 之前）。
  - 被关闭的能力不注册到脚本名字空间，访问报 `VmError::CapabilityDenied { name, profile }`（与 `UndefinedGlobal` 可区分）。
  - `env` 改为**键白名单**：未授权键返回 `nil`/`false` 且**不读**宿主环境；`env.allowed()` 可自省。
  - 资源限额：内存 `MemoryBudget`（VM 指令与原生原型共享计数器；大迭代器物化**前**预检查）、墙钟预算（每 4096 条指令 + 调度器等待处检查，并裁剪睡眠）、任务数上限；新增 `VmError::MemoryLimitExceeded` / `WallClockLimitExceeded` / `TaskLimitExceeded`。
  - `Vm::with_sandbox` / `set_sandbox`、`Engine::with_sandbox` / `set_sandbox` / `sandbox` / `permissive` / `memory_used`。
- **宿主原生模块（M5 / T5.5）**：`NativeModule` trait + `Engine::register_module`；模块在脚本侧是普通 `map`（`geometry.area(3.0, 4.0)`），同名冲突报 `Error::ModuleConflict`。
- CLI 新增 `--strict`（默认 `standard`）。
- 测试：`crates/ada_engine/tests/security.rs`（21 个用例，覆盖越权、`env` 白名单、内存/墙钟/任务上限、模块注册、11 条敌意输入不 panic）。

- **协作式并发运行时（M4 / T4.1–T4.3）**：`spawn`/`await` 从"语法占位"变为真实调度（决策 D2）。
  - `spawn f(a, b)` 只求值调用目标与实参并登记任务，**不在生成点执行任务体**；任务首次被调度时才运行。
  - 单线程协作式事件循环：每个任务持有独立的帧栈 + 寄存器窗口（`SavedExecution`），挂起点让出、按需恢复；就绪队列 FIFO 保证执行顺序确定。
  - `sleep_ms` 改为挂起到真实墙钟时刻（不再阻塞 OS 线程），事件循环统一等待：2 个各 200ms 的任务实测总耗时 **200ms**（旧实现 401ms），4×150ms 也只需约 150ms。
  - 新指令 `Spawn{dst,callee,arg_start,arg_count}` 与 `SpawnValue{dst,src}`。
- **任务 API（M4 / T4.5–T4.6）**：`await_all`/`await_any`（`await_race` 为旧名）、`timeout(task, ms)`、`task.cancel()`、`task.state`/`task.id`/`task.label`、`yield_now()`。
  - `await_any` 返回**最先结算**者的值（旧实现错误地返回数组首元素）。
  - `timeout(task, ms)` 到期取消任务并返回 `nil`；`task.cancel()` 为协作式取消，返回 `bool`。
  - 错误在 `await` 处抛 `TaskFailed{label, source}`（保留原始错误），取消抛 `TaskCancelled`，全部等待且无定时器时抛 `TaskDeadlock`。
- 测试：`crates/ada_vm/tests/async.rs`（26 个用例，含真实并发耗时断言与确定性交错断言）；`examples/showcase.ada` 增加确定性调度演示，`examples/demo.ada` 增加并发耗时对照。

- **闭包 upvalue 捕获（T2.2）**：闭包可以读写外层局部变量，语义为**引用共享**。
  - 新增指令 `MakeCell` / `CellGet` / `CellSet` / `UpvalueGet` / `UpvalueSet` / `MakeClosure` 与 `FunctionProto.upvalues`（`UpvalueDesc`）。
  - 编译器做"嵌套函数引用名"预扫描来决定装箱；捕获链逐层传递，中间层不直接引用也能把槽传下去。
  - 循环变量被捕获时**每轮独立槽**（`for i in range(0,3) { grabs.push((() => i)) }` → `[0, 1, 2]`）。
- **Golden 端到端测试（M2 退出标准）**：`examples/showcase.ada` + `examples/showcase.expected.txt`，由 `crates/ada_cli/tests/golden_showcase.rs` 逐行比对，覆盖 SPEC_LANGUAGE §5–§8 / SPEC_STDLIB §2–§6 / SPEC_ASYNC §2 共 13 个规范小节；`examples/demo.ada` 也扩展了闭包 + 适配器小节。
- **迭代器适配器（T2.1）**：`map` / `filter` / `take` / `skip` / `collect` / `count` / `find` / `any` / `all` / `enumerate` / `rev` / `join` / `sum`，**方法链与全局函数双入口**（`xs.map(f)` 与 `xs |> map(f)`），可用于数组、字符串、字典、`range` 与任意迭代器。
  - 由 VM 实现（`Value::Intrinsic` + `Vm::invoke_adapter`），因为 `NativeFn` 回调无法回调脚本闭包。
  - 当前为**即时求值**（决策 D3 = A），适配器结果一律是 `Array`。
- **显式迭代协议（T2.3）**：`.iter()` 与 `iterator.next()`（耗尽返回 `nil`）；`string.chars_with_idx`；`is_iterator`。
- **REPL / 多轮求值持久化（T2.4）**：模块顶层的 `let`/`mut` 同时写入全局表，同一 `Engine` 的后续 `eval` 可见；函数体内的局部变量不会外泄。
- **Fuel 生命周期（T2.6）**：每次求值自动重新装填配置预算；新增 `remaining_fuel()` / `reset_fuel()`（`Vm` 与 `Engine`），`get_fuel()` 语义明确为"配置预算"。
- 测试：`crates/ada_vm/tests/adapters.rs`（16）、`crates/ada_vm/tests/closures.rs`（15）、`crates/ada_engine/tests/persistent_eval.rs`（7）、`crates/ada_cli/tests/golden_showcase.rs`（3），并扩充文档示例测试。
- **文档门禁（T0.4/T0.5）**：`crates/ada_cli/tests/doc_links.rs` 校验 Markdown 链接/锚点并按 GitHub slug 规则解析，同时禁止规范文档中出现路线图章节；`crates/ada_engine/tests/doc_examples.rs` 固化 README/SPEC 中标注可运行的示例。
- `ada_engine` 公共 re-export：`Value`、`Closure`、`NativeCallback`、`Vm`、`VmError`、`ParseError`、`CompileError`，宿主不再需要显式依赖 `ada_vm`（T0.7）。
- `LICENSE-MIT` 与 `LICENSE-APACHE`，与 `Cargo.toml` 声明的 `MIT OR Apache-2.0` 对齐（T0.2）。
- CI 质量门禁 `.github/workflows/ci.yml`：`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`（T0.4）。
- 字符串 `.len_bytes`：返回 UTF-8 字节数（T2.5）。
- **`let` 不可变性（T3.1）**：向非 `mut` 绑定赋值现在是**编译期错误**（`CompileError::AssignToImmutable`）。`mut`、函数参数、`for` 变量、`match` 绑定不受限；容器内容仍可变（`let a = [1]; a.push(2)` 合法，与 JS `const` 一致）。
- **`match` 非穷尽检查（T3.2）**：无 `_`（或无 guard 的标识符绑定）作为兜底时，未匹配即抛 `VmError::NoMatch`，不再留下未赋值的匹配目标。新增 `MatchFail` 指令。
- **负索引（T3.5）**：字符串与数组的索引支持从末尾计数，与 `.slice` 一致；数组写入同样支持（`a[-1] = v`）。
- **语义测试集（T3.7）**：`crates/ada_vm/tests/semantics.rs` 21 个用例，逐条覆盖 M3 规则。

### Fixed

- `ada check <file>` / `ada run <file>` 的错误前缀此前会写成 `<eval>`（子命令里的文件路径没被
  当成来源名），编辑器与 CI 会因此丢掉文件名；现在统一取子命令里的路径（T0.6）。

- **解释器不再因脚本输入杀死宿主进程**（T1.1–T1.4、T1.6）：
  - `%` 除零、`i64::MIN % -1`、`i64::MIN / -1`
  - 整数 `+ - *` 与一元 `-` 溢出
  - 移位量不在 `0..64`（含负数）
  - 多字节字符串按字节索引导致的 char-boundary panic
  - `min([])` / `max([])` 空数组索引越界
- 迭代器适配器 arity 元数据错误导致的 `map([1,2])` 越界 panic（由 `adapters.rs` 用例捕获）。
- `sum` 整数累加溢出、`abs(i64::MIN)`、`math.random_int` 区间宽度溢出三处潜在 panic（改用 checked 运算 / i128）。
- 插值子表达式内的语法错误定位错误（T3.4）：片段被独立解析导致行号从 1 开始；现在通过 `Parser::from_fragment` 把 token 的字节/行/列映射回源文件，错误指向真实的插值位置。
- 混合数值类型的取模（`5.0 % 2`、`5 % 2.0`）由报错改为提升为 Float 计算（T3.5）。
- 比较运算对非数值操作数静默返回 `false` 改为报 `TypeError`（T3.5）；`NaN` 与任何值的有序比较仍为 `false`（IEEE-754）。
- 整数值的浮点不再与整数同形（T3.6）：`to_str(1.0)` 由 `"1"` 改为 `"1.0"`，极大/极小值用科学计数法（`1e300`）。
- `spawn` 由「立即执行并包装结果」改为「惰性创建任务」（M4）：`spawn f()` 之后 `f` 不再立刻执行；
  需要结果请 `await`。此外 `spawn f(1).await` 按优先级解析为 `spawn (f(1).await)`（`.await` 是 Call 级），
  要"生成后再等待"请拆成两行。
- `await_race` 的语义修正为「最先结算」（原为「数组首元素」）；`await_any` 为推荐名，`await_race` 保留为别名。
- `sleep_ms` 不再阻塞宿主线程（M4）：在任务内挂起等待；仅在迭代器适配器回调（Rust 栈帧）内退化为真实阻塞。
- `await_all`/`await_any` 的实参必须是任务数组：非任务元素不再被原样拷贝，而是报 `TypeError`。
- **`Engine::new()` 的默认权限收紧为 strict 沙箱（M5，破坏性变更）**：脚本不再能直接使用
  `time`/`sys`/`env`/`sleep_ms`，并受到内存/墙钟/任务数限额。迁移方式：
  - 需要旧行为：`Engine::permissive()`；
  - 需要部分宿主能力：`Engine::with_sandbox(Sandbox::standard())` 或
    `Sandbox::strict().with_env_keys(["HOME"]).with_wall_clock_limit(Some(10_000))`。
  直接使用 `Vm::new()` 的场合不受影响（仍是全量内置）。
- `env.get`/`env.has` 只对白名单键读取宿主环境；白名单为空时读取任何键都得到 `nil`/`false`。
- CLI 默认使用 `standard` 档（本地工具定位），`--strict` 可切换到最小权限。
- **CLI 错误格式统一（T0.6 / T1.8）**：错误从 `Error: {Debug 形式}` 改为**单行**
  `[file:]line:col: error: <消息>`，消息用 `Display` 人话；语法错误保留 `expected` 细节。
  编译错误与运行时错误现在也带行列（此前只会显示 `file: error: …`）。
  `crates/ada_cli/tests/cli_errors.rs`（9 个用例）固定该契约。
- **VM 性能优化（M6 / T6.2）**：闭包调用的捕获表改为 `Arc` 共享（不再每次克隆 `Vec`）、
  指令循环头部合并（每条指令只借出帧一次）、挂起检查免去每指令 32 字节写回、
  墙钟/指令计数计量合并为单布尔判断（修掉 M5 引入的每指令开销）。
  实测（criterion，释放构建）：`fib(24)` **−13.7%**、20 万次数值循环 **−12.1%**、
  迭代器管道 **−10.7%**、冷启动 **−16.6%**；未达计划 ≥30% 的目标，剩余路径见 SPEC_RUNTIME §11.3/§11.7。
### Fixed

- **`match` 分支体的括号判定与决策 D4 不一致**：分支体此前无条件把 `{` 当块，导致
  `_ => { ok: false }` 这类 map 字面量体无法解析（报 `expected expression`）。现在分支体与表达式
  共用同一消歧函数（`{ k: v }` 是 map、`{ let a = 1; a }` 是块），回归测试见
  `ada_vm/tests/semantics.rs::match_arm_bodies_follow_the_same_brace_rule`，规范见 SPEC_LANGUAGE §5.6。
- 文档门禁此前只能校验 Markdown 目标，链接到示例脚本等非 md 文件会误报"file missing"；
  现在对所有相对链接做存在性检查、只对 Markdown 目标做锚点检查。
- 字符串拼接的内存计费由"每次记整串长度"改为**按增量**计费（`s += x` 不再二次方消耗预算）；
  安全测试仍能拒绝真正的字符串放大攻击。

### Changed

- **数值函数口径统一（G-S6/G-S7）**：`abs` / `round` / `floor` / `ceil` / `min` / `max` / `sum`
  以及 `math.*` 数值函数对**非数值参数一律报错**（`abs: expected a number, got string`），
  不再静默返回 `0` 或忽略该元素——静默 0 恰好会掩盖类型错误（`sum([1, "2"])` 此前得到 `1`）。
  这与 M3 对比较运算的收紧一致（`5 < "a"` 报错而非静默 `false`）。
  同时补齐 `math.round` / `math.floor` / `math.ceil`（全局名保留，无破坏性变更）。
  新增 `Value::type_name()` 使错误消息与 `type_of()` 口径一致。
- **模式字面量不再截断为 `i32`（修复静默失配）**：`match 5000000000 { 5000000000 => "hit" }`
  此前静默落到 `_` 分支；现在超过 `i32` 的字面量走常量池，与表达式路径一致。
- **`match` 支持负数模式字面量**：`-5 => …` / `-2.5 => …`（此前报语法错，而 `< -5` 却可用，
  口径不一致）。
- **文档状态收敛**：清掉十余处指向**已完成任务**的 ⚠️/📋 标记（`HostPanic`、`max_tasks`、
  顶层 `let` 跨轮次、`env` 白名单、闭包 upvalue、`abs`/`sum` 静默处理……），
  并把真实剩余的缺口登记为任务（见计划 §13：T8.1 插值定位、T8.2 游离 `break`/`continue`、
  T8.3 REPL 全局镜像一致性）。新增门禁 `crates/ada_cli/tests/doc_task_status.rs`：
  ⚠️/📋 条目若引用计划中已完成（或已否决）的任务即失败。
- **map 成员派发规则统一为「自有键优先」（T7.5）**：此前"适配器名让键优先、原型方法名遮蔽键"
  两套规则并存，导致 `m.remove` 与 `m["remove"]` 结果不一致，并让宿主模块里与原型同名的函数
  被静默替换（`fs.remove` 拿到的其实是 map 原型方法）。现在 `{ len: 9 }.len` → `9`、
  `{ remove: 3 }.remove` → `3`，与该键不存在时才命中方法名的规则一致。
  **迁移**：若依赖旧行为，请给键改用别的名字；规范见 SPEC_STDLIB §4.4（缺口 G-S13 已关闭）。
- **`Value` 从 48 字节降到 16 字节（T6.2e）**：`NativeFn` 与 `Intrinsic` 的载荷改放
  `Arc<NativeFunction>` / `Arc<IntrinsicCall>` 之后。寄存器文件（2048 槽）随之从 96 KiB
  降到 **32 KiB**，可以放进 L1。实测（criterion，对 M6 原始基线）：
  **iterator_pipeline −46.6%**、冷启动 −27.9%、fib −25.3%、numeric_loop −23.7%、json −21.4%；
  该项单独贡献 pipeline −34.7%、fib −10.1%。
  **指令数基线逐字节不变**（只改表示与派发成本，未减工作量）。
  新增 `Value::native_fn_shared`（复用宿主已有的 `Arc<dyn Fn>`）、
  `crates/ada_vm/tests/value_layout.rs`（把"≤16 字节 / 寄存器文件放得进 L1"变成门禁）、
  `size_probe` 示例扩展为打印载荷与寄存器文件占用。
- `Engine::register_module` 改用 `Value::native_fn_shared`（不再二次包装回调）。

- 文档整合为「单一权威」结构：新增 `docs/README.md` 索引与 `SPEC_LANGUAGE`/`SPEC_RUNTIME`/`SPEC_STDLIB`/`SPEC_ASYNC`，移除 `LANGUAGE_DESIGN`/`STDLIB_DESIGN`/`ITERATOR_SPEC`/`ASYNC_DESIGN`；路线图唯一收敛到 `docs/DEVELOPMENT_PLAN.md`（T0.3）。
- `cargo fmt` 全量格式化（纯格式，无语义变更）。
- 冲突的语义已定案：整数溢出采用「报错」而非回绕（决策 D5）；迭代器适配器采用「即时求值」（决策 D3）；闭包捕获采用「引用共享」（决策 D8）；`{ x }` 保持为 map 简写（决策 D4）；`match` 非穷尽采用「运行时错误」而非警告（决策 D7）；异步采用「单线程协作式事件循环」而非 Tokio/Fiber（决策 D2）。
- 成员派发优先级明确化：map 上的适配器同名键优先（`{ map: 7 }.map` → `7`），既有原型方法名（`len` 等）仍优先于同名键（`{ len: 9 }.len` → `1`，缺口 G-S12/S13）。

### Breaking

- 字符串长度语义由**字节**改为**字符**：`"Ada 语言".len` 与 `len("Ada 语言")` 由 `10` 变为 `6`；字节数请使用 `.len_bytes`。
- 字符串索引改为按字符定位：`"语言"[0]` 由 panic 变为返回 `"语"`。
- 整数溢出、取模零、移位越界由「debug 构建 panic / release 构建静默回绕」改为返回 `VmError::TypeError`（宿主可用 `Error::Vm` 捕获）。
- `sum` 语义扩展为接受任意可迭代值（原先仅接受数组，非数组报错）；`sum`/`join`/`enumerate`/`rev` 由原型内联实现改为统一适配器实现。
- `let` 绑定的重新赋值由「静默生效」改为「编译错误」：既有脚本若依赖 `let x = 0` 后再 `x += 1`，需改为 `mut x = 0`（T3.1）。
- 非穷尽 `match` 由「返回未赋值的旧寄存器值」改为抛 `VmError::NoMatch`：此前 `let r = match 99 { 1 => "one" }` 会得到一个不确定的值（T3.2）。
- 有序比较对非数值操作数由静默 `false` 改为 `TypeError`：`if x > 0` 这类写法若 `x` 可能不是数值，需先做类型判断（T3.5）。
- 浮点输出格式变化：`1.0` 由 `1` 变为 `1.0`，依赖旧输出的快照/断言需更新（T3.6，`examples/showcase.expected.txt` 已同步）。
