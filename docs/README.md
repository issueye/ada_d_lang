# Ada-D 文档中心 (Documentation Hub)

> **本目录是 Ada-D 的唯一规范来源。一个主题只有一个权威文档，其余位置只做引用，不复制内容。**
> 路线图 / 里程碑 / 任务优先级统一收敛到 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。

---

## 1. 文档地图

| 文档 | 权威范围（该主题的唯一出处） | 状态 |
| :--- | :--- | :--- |
| [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) | 路线图、里程碑、任务与验收标准、风险登记、待决决策、文档治理规则 | 生效中 |
| [WHITEPAPER.md](WHITEPAPER.md) | 项目愿景与定位、Token 密度论据、与传统方案的差异化 | 生效中（性能数字引用基准） |
| [AI_GUIDE.md](AI_GUIDE.md) | **面向 AI 生成代码的单页规格**：语法速查、反直觉规则、错误对照、可运行示例 | 生效中（页内示例由 CLI 真实执行） |
| [SPEC_LANGUAGE.md](SPEC_LANGUAGE.md) | 词法、语法、表达式优先级、求值与相等语义、作用域、真值规则 | 生效中（含 ⚠️ 缺口条目） |
| [SPEC_RUNTIME.md](SPEC_RUNTIME.md) | 字节码指令集、编译器策略、VM 执行模型、值模型、错误模型与 CLI 错误格式、宿主嵌入 API、沙箱与 Fuel、性能基线 | 生效中（含 ⚠️/📋 条目） |
| [SPEC_STDLIB.md](SPEC_STDLIB.md) | 全局内置函数、原生模块（json/math/time/sys/env）、原型方法、**统一迭代器协议与适配器** | 生效中（含 ⚠️/📋 条目） |
| [SPEC_ASYNC.md](SPEC_ASYNC.md) | 异步语法、并发原语、调度与挂起模型、任务生命周期契约 | 生效中（语法 ✅ / 运行时 📋） |
| [SUMMARY.md](SUMMARY.md) | 文档站（mdBook）目录：**新增文档必须登记在此**，否则站点看不到 | 生效中 |
| [../README.md](../README.md) | 项目入口、快速开始、工作区结构 | 生效中 |

> 文档站：`mdbook serve docs`（配置 [book.toml](book.toml)），内容与上面同一批 Markdown 单一源；
> `crates/ada_cli/tests/docs_site.rs` 会校验目录完整性并真实执行一次 `mdbook build`。

> 历史文档 `LANGUAGE_DESIGN.md`、`STDLIB_DESIGN.md`、`ITERATOR_SPEC.md`、`ASYNC_DESIGN.md` 的内容已按主题拆分/合并进上述规范文档，原文件已移除。

---

## 2. 状态图例（全项目统一）

| 标记 | 含义 |
| :--- | :--- |
| ✅ | 已实现，有测试覆盖，行为与规范一致 |
| ⚠️ | 部分实现 / 行为与目标语义有偏差，缺口有对应任务 ID |
| 🚧 | 进行中 |
| 📋 | 计划中，尚未开始（有任务 ID） |
| ❌ | 明确不做 / 已否决（见开发计划 §9、§10） |

**规则**：规范文档中的每一条能力描述都必须带状态标记；标 ⚠️ 或 📋 的条目必须能在 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) 中找到对应任务 ID。

---

## 3. 权威来源矩阵（查东西先看这张表）

| 我想了解…… | 看这里 |
| :--- | :--- |
| 关键字、字面量、换行自闭合（ASI）规则 | [SPEC_LANGUAGE §2](SPEC_LANGUAGE.md#2-词法-lexical) |
| 运算符优先级与结合性、管道 `\|>` 如何脱糖 | [SPEC_LANGUAGE §4](SPEC_LANGUAGE.md#4-表达式与优先级) |
| `??` / `?.` / 字符串插值 / 闭包 / `match` 语义 | [SPEC_LANGUAGE §5–§8](SPEC_LANGUAGE.md#5-语句与声明) |
| 相等与真值规则、类型标注是否生效 | [SPEC_LANGUAGE §9–§10](SPEC_LANGUAGE.md#9-相等与真值语义) |
| 有哪些指令、编译器怎么分配寄存器、调用帧怎么工作 | [SPEC_RUNTIME §3–§5](SPEC_RUNTIME.md#3-字节码指令集) |
| 值的内部表示（当前 enum，还是 NaN-boxing） | [SPEC_RUNTIME §2](SPEC_RUNTIME.md#2-值模型-value-model) |
| 溢出 / 除零 / 字符串索引的算术语义 | [SPEC_RUNTIME §6](SPEC_RUNTIME.md#6-数值与算术语义) |
| Fuel 熔断、超时、内存预算、错误类型 | [SPEC_RUNTIME §7–§8](SPEC_RUNTIME.md#7-fuel-与资源熔断) |
| 如何把脚本嵌进 Rust（Engine / register_fn / FromValue） | [SPEC_RUNTIME §9](SPEC_RUNTIME.md#9-宿主嵌入-api) |
| 沙箱权限分级、`env` 白名单 | [SPEC_RUNTIME §10](SPEC_RUNTIME.md#10-沙箱与权限模型) |
| `println` / `len` / `to_int` 等全局函数清单 | [SPEC_STDLIB §2](SPEC_STDLIB.md#2-全局内置函数) |
| `json` / `math` / `time` / `sys` / `env` 模块 API | [SPEC_STDLIB §3](SPEC_STDLIB.md#3-原生模块-native-modules) |
| `str.trim()` / `arr.push()` / `map.keys()` 等原型方法 | [SPEC_STDLIB §4](SPEC_STDLIB.md#4-原型方法-prototype-methods) |
| `for..in` 支持哪些类型、`range` 是否惰性 | [SPEC_STDLIB §5](SPEC_STDLIB.md#5-统一迭代器协议) |
| `map` / `filter` / `take` 等适配器 | [SPEC_STDLIB §6](SPEC_STDLIB.md#6-迭代器适配器) |
| `async fn` / `spawn` / `.await` 现在到底是怎么执行的 | [SPEC_ASYNC §3](SPEC_ASYNC.md) |
| 某个功能到底做没做、什么时候做 | [DEVELOPMENT_PLAN §4](DEVELOPMENT_PLAN.md#4-里程碑与任务) |

---

## 4. 推荐阅读路径

- **第一次接触项目**：根 [README.md](../README.md) → [WHITEPAPER.md](WHITEPAPER.md) → [DEVELOPMENT_PLAN §2 基线快照](DEVELOPMENT_PLAN.md#2-基线快照-verified-baseline-010)
- **要写 Ada-D 脚本**：[SPEC_LANGUAGE.md](SPEC_LANGUAGE.md) → [SPEC_STDLIB.md](SPEC_STDLIB.md) → `examples/demo.ada`
- **要把脚本嵌进 Rust 宿主**：[SPEC_RUNTIME §9–§10](SPEC_RUNTIME.md#9-宿主嵌入-api) → [SPEC_ASYNC §6](SPEC_ASYNC.md)
- **要改编译器 / VM**：[SPEC_RUNTIME §3–§8](SPEC_RUNTIME.md#3-字节码指令集) → [DEVELOPMENT_PLAN §4](DEVELOPMENT_PLAN.md#4-里程碑与任务) 领任务
- **AI Agent 生成脚本**：[SPEC_LANGUAGE.md](SPEC_LANGUAGE.md) 全文（优先级与语义一节必读，避免生成不支持的形式）→ [SPEC_STDLIB.md](SPEC_STDLIB.md)

---

## 5. 文档治理规则（摘要，权威版本见开发计划 §11）

1. 一个主题只允许一个权威文档；其它文档用链接引用，禁止复制内容。
2. 禁止在规范文档中写"实施路线 / 里程碑 / Roadmap"——统一指向 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。
3. 所有规范条目带 §2 的状态标记。
4. 规范与实现冲突时以规范为准；需要改变语义时先在开发计划 §10 记录决策，再改规范，最后改代码。
5. 每个里程碑结束时执行文档巡检：状态标记是否过期、链接是否可达、示例是否仍可运行。
6. 示例代码块必须可运行（golden 测试覆盖后自动校验，见开发计划 T0.5 / §6）。

---

## 6. 贡献者快速自检

在提交任何改动前自查：

- [ ] 我改的行为属于哪个主题？我是否更新了该主题的**唯一权威文档**？
- [ ] 我新增的能力是否带了状态标记与任务 ID？
- [ ] 我是否在规范文档里误加了路线图内容？
- [ ] 我是否引入了新的 panic 路径？（见开发计划 §5 门禁第 3 条）
- [ ] `cargo fmt --check` / `cargo clippy -- -D warnings` / `cargo test --workspace` 是否全绿？
