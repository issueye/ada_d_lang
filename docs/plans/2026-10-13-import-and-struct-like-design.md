# 设计：模块导入（import）与结构体惯用法（struct-like）

> **版本**：1.0（草案，待实现）
> **状态**：已与需求方定稿（brainstorming 四节确认）
> **日期**：2026-10-13
> **适用范围**：`ada_d_lang` 全工作区
> **关联计划**：[2026-10-13-m8-development-plan.md](./2026-10-13-m8-development-plan.md)（版本路线 0.3.0、任务编号 T8.6–T8.18）
> **关联**：本设计落地后需更新 [DEVELOPMENT_PLAN](../DEVELOPMENT_PLAN.md)（§9 非目标、§10 决策、新增 M8）与各 `SPEC_*.md`。

---

## 1. 背景与目标

需求方提出两项能力扩展：

1. **一个类似 struct / 对象的角色**——用于表达带字段的结构化数据。
2. **import 系统**——支持多文件代码组织与复用。

这两项目前均落在开发计划 **§9 非目标**（"OOP 类模型"、"模块（import）系统"）。本设计将其**重新定义**为可落地形态，并明确与现有架构的边界：

- **结构体**：**不引入 `struct` 关键字、不引入类模型**，而是用现有 `map` + 一套标准库辅助与惯用法来模拟（决策 **D11**）。因此 §9 的"OOP 类模型"非目标**保留**。
- **import**：新增一条 `import` 语句与编译期声明合并（决策 **D10**）。§9 的"模块（import）系统"非目标**移出**，改建为里程碑 **M8**。

设计原则（延续项目定位）：**可预测、安全默认、Token 高效、易嵌入、零不必要的 VM 风险**。

---

## 2. 功能 A · 结构体惯用法（零语法 / 零 VM 改动）

### 2.1 构造函数约定

复用 map 简写 `{ x, y }`（键取同名变量值）：

```ada
fn Point(x, y) = { x, y }
let p = Point(3, 4)
p.x            // 3
p?.y ?? 0      // 安全导航 + 空值合并
```

### 2.2 方法：自引用闭包（已实测，零 VM 改动）

利用"map 可存任意值（含闭包）+ 闭包可捕获外层变量"两条现有能力，在构造函数内把方法挂到 map 上：

```ada
fn Point(x, y) = {
    let self = { x: x, y: y }
    self["area"] = () => self.x * self.y
    self
}
Point(3, 4).area()      // → 12（已由 CLI 实测验证）
```

- `p.area()` 自然派发：`p.area` 取到 map 自有键 `area`（自有键优先于原型方法），其为捕获 `self` 的闭包。
- **取舍**：方法**不共享**（每个实例一份闭包）；`self` 会含方法键（遍历/深比较时需处理，见 2.3）。
- **明确不做**（方案 2，留作 backlog）：VM 层 `self` 自动注入的方法派发（改 `GetField`/`Call`、方法共享）。其代价是 VM 改动 + "map 内函数首参即 self"的隐式约定 + 改变 `let f = p.area` 语义，本版不采用。

### 2.3 标准库辅助（新增 4 个全局函数）

弥补 `map` 作为"记录类型"的短板（`==` 仅引用相等、无快照、无契约校验）：

| 函数 | 签名 | 语义 |
| :--- | :--- | :--- |
| `deep_eq(a, b)` | `(any, any) -> bool` | 结构化**深比较**：递归比较 array/map/标量；**函数按引用/原型比较，不深入** |
| `clone(v)` | `(any) -> any` | map/array **深拷贝**出独立容器（模拟值语义/快照）；标量原样返回 |
| `has_keys(m, keys)` | `(map, array) -> bool` | `m` 是否含 `keys` 中全部键 |
| `assert_keys(m, keys)` | `(map, array) -> nil` | 缺键即报原生错误（消息含缺失键名） |

```ada
deep_eq(Point(3,4), Point(3,4))   // true（== 为 false）
let q = clone(p); q.x = 99        // 不影响 p
assert_keys(p, ["x", "y"])        // 缺键报错
```

### 2.4 影响面

仅 `ada_vm/builtins`（4 个新函数）+ 单元/集成测试 + 文档 + CHANGELOG。**不触碰编译器/VM 核心，不回退 §9 非目标。**

---

## 3. 功能 B · import 系统（具名文件导入）

### 3.1 语法

```ada
import { a, b } from "path.ada"
```

- 新增关键字 `import` / `from`（lexer `KEYWORDS`）。
- 新增 `Stmt::Import { names: Vec<String>, path: String, span }`（AST + parser）。
- 具名列表 `{ a, b }`；路径为字符串字面量。

### 3.2 路径解析与安全（决策 D10）

- **相对当前文件**解析；规范化后**禁止 `..` 逃逸 root**。
- **root 来源**：
  - CLI：`--import-root DIR`，默认取主脚本所在目录。
  - Engine：由宿主通过 `ModuleResolver` 提供（见 3.6）。
- 路径规范化、`..`/绝对路径/符号链接拒绝等规则**复用 `fs` 模块同源逻辑**（`ada_vm` 路径安全实现）。

### 3.3 求值语义：纯声明合并

- import **只合并声明**：把被导入文件中**具名的顶层 `fn`/`let`** 引入当前编译单元。
- **不执行**被导入文件的顶层副作用（其 `println` 等不运行）。
- 编译期完成，运行期无额外加载动作。

### 3.4 可见性规则

- 仅 `import { a, b }` 列名的声明对当前文件可见，进入当前全局作用域。
- 被导入文件中**未列名**的顶层声明（如辅助 `helper`）对当前文件**不可见**（引用报未定义）。
- 导入绑定**只读**：`import { x } from "..."` 后 `x = 1` 报编译错误（类 ES module）。
- **命名冲突**：当前文件已定义 `a` 又 `import { a }`，或两个 import 导入同名 `a` → 编译错误（重复定义，带行列）。

### 3.5 循环依赖

- **检测并报错**：`ModuleLoader` 维护"加载中"集合，`A → B → A` 报编译错误 `circular import: A -> B -> A`（带 import 语句位置）。纯声明合并不需容忍循环，报错最可预测。

### 3.6 沙箱与宿主集成

- **Engine（宿主嵌入）**：提供 `ModuleResolver` trait，由宿主完全掌控"路径 → 源码"映射（可从内存/自定义 FS/网络提供，也可拒绝）。
  - **默认无 resolver ⇒ 任何文件 import 直接报错**（与 `fs` 默认关闭同源的安全默认）。
- **CLI**：`--import-root DIR` 启用基于 FS 的默认 resolver，限制在该 root 内。
- **内置模块（`std:`）显式导入**：见 §3.10——所有内置模块必须显式导入，采用"导入即授权、完全授权"模型（决策 D12）。

### 3.7 错误处理（全部编译期，复用 `file:line:col: error:` 模型，位置指向 import 语句）

| 场景 | 错误 |
| :--- | :--- |
| 文件不存在 | `import file not found: <path>` |
| 路径逃逸 root / 绝对路径 / 符号链接越界 | `import path escapes root: <path>` |
| 导入不存在的名字 | `module '<path>' has no export '<name>'` |
| 被导入文件自身语法/编译错误 | 传播该错误（附被导入文件位置） |
| 循环依赖 | `circular import: A -> B -> A` |
| 重复定义 / 只读绑定被写 | 编译错误（带行列） |

均**干净报错、不 panic**（符合 §5 门禁第 3 条）。

### 3.8 实现关键点：声明合并 + 轻量导入作用域

被导入的 `a` 可能依赖同文件的 `helper`。若只抽取 `a` 会缺依赖。

- **做法**：合并被导入文件的**全部顶层声明**以保证内部依赖自洽；用一层轻量"**导入作用域**"标记——**仅具名者对当前文件可见**，其余为私有辅助。
- 这是本次 import **唯一触及编译器核心**的点：需给 `Compiler` 引入"导入层"概念（合并声明 + 可见性过滤 + 只读绑定 + 冲突检测）。
- `ModuleLoader`（新组件）：读文件、规范化路径、root 检查、缓存已加载模块（避免重复加载）、循环依赖检测。

### 3.9 编译架构改动概览

```
.ada 源文件
   │  ModuleLoader（新）  递归加载 import → 规范化/root/缓存/循环检测
   │  ada_lexer / ada_parser  解析（含 import 语句）
   │  声明合并 + 导入作用域（compiler 扩展）
   │  ada_bytecode::Compiler  单遍编译（现有逻辑复用）
   │  ada_vm::Vm::execute
   └  ada_engine::Engine（ModuleResolver / import root）
```

### 3.10 内置模块显式导入（std: 方案，决策 D12）

**所有内置模块必须显式导入**，不再默认注入全局名表。支持命名空间与具名两种形式：

```ada
import json from "std:json"                    // 命名空间：json.parse(...)
import { parse, stringify } from "std:json"   // 具名：parse(...)
import { sqrt, PI } from "std:math"
import time from "std:time"
```

适用模块：`json` / `math` / `time` / `sys` / `env` / `crypto` / `fs`。

**授权模型：导入即授权、完全授权（D12）**
- `import` 某 `std:` 模块即授予其**完整能力**，**取代**原沙箱 profile 对内置模块的逐项开关。
- **完全授权**：`import fs from "std:fs"` 后 `fs` 可访问任意文件；`import env from "std:env"` 后可读全部环境变量——**不再受 `fs_root`/`env_allowlist` 限制**。
- ⚠️ **安全影响（重大）**：`strict` 沙箱对内置模块的保护将被 import 绕过；脚本可借 import 任意读文件/环境变量。这与项目"安全默认"定位冲突，**需在文档与 CHANGELOG 醒目标记**。
- **建议的最后防线（可选，待定）**：保留一个**宿主全局否决开关**——宿主可配置"即使 import 也禁用某模块"（如宿主禁用 `fs`，则任何 `import ... from "std:fs"` 报错）。默认不启用（默认 import 即可用）。

**全局函数保持隐式**：`print`/`println`/`len`/`range`/`type_of`/`sum`/`assert` 等 core globals **不需导入**，直接可用（D12 不含 core globals）。

**与文件导入的关系**：`std:` 前缀由 `ModuleLoader` 识别为内置模块（不走文件系统/root 检查）；`.ada` 等相对路径走文件导入（仍受 §3.2 root 边界约束）。二者共用 `import` 语句与声明合并机制。

**架构影响**：`register_all` 不再默认插入内置模块；改为在编译期处理 `import ... from "std:xxx"` 时，把对应模块/具名函数注册进 globals。

---

## 4. 测试策略

- **stdlib 辅助**：`deep_eq`/`clone`/`has_keys`/`assert_keys` 单元测试（`ada_vm` builtins）；新增 `crates/ada_vm/tests/struct_like.rs`（构造/访问/自引用闭包方法/深比较/快照/校验）。
- **import**：
  - parser：`import {a,b} from "..."` 解析与错误形态。
  - engine 集成：可见性、只读绑定、重复定义冲突、循环依赖、四类错误。
  - CLI：`--import-root`、运行含 import 的脚本、错误行列格式。
  - 安全：`..` 逃逸拒绝、默认无 resolver 拒绝、被导入文件错误传播。
- **文档门禁**：`AI_GUIDE` 新增示例由 CLI 真实执行比对；所有新能力带状态标记并登记任务 ID。

---

## 5. 文档与决策登记

- **DEVELOPMENT_PLAN.md**：
  - §9 非目标移出"模块（import）系统"；保留"OOP 类模型"。
  - §10 记 **D10**（import：具名文件导入 + 纯声明合并 + root 边界 + Engine 默认拒绝）、**D11**（struct = map 约定 + 自引用闭包，零语法）、**D12**（内置模块显式导入：`std:` 前缀 + 命名空间/具名双形式 + 导入即完全授权 + 全局函数保持隐式）。
  - 新增 **M8 · 模块导入与结构体惯用法**（任务与验收标准见 §6）。
- **SPEC_LANGUAGE.md**：新增 `import` 语句语法（带状态标记）。
- **SPEC_STDLIB.md**：新增 4 个全局函数 + "用 map 模拟结构体"小节。
- **SPEC_RUNTIME.md**：import 编译期合并语义、路径安全、`ModuleResolver` 宿主 API。
- **AI_GUIDE.md**：import 用法 + 结构体惯用法 + 反直觉规则（如"记录深比较用 `deep_eq` 非 `==`"、"map 是引用类型"）。
- **CHANGELOG.md**：记录新功能。

---

## 6. 里程碑 M8 · 任务分解（优先级）

> ⚠️ **编号以《开发计划》为准**：项目 §13 已占用 `T8.1–T8.5`（缺口收尾，属 0.2.0），故 M8 新功能任务在 [2026-10-13-m8-development-plan.md](./2026-10-13-m8-development-plan.md) 中编号为 **T8.6–T8.18**。下表已同步该编号。

### import（主工作量）

| ID | 任务 | 优先级 |
| :--- | :--- | :--- |
| T8.6 | lexer：`import`/`from` 关键字 + parser：`Stmt::Import` | P1 |
| T8.7 | `ModuleLoader`：路径解析/规范化/root 检查/缓存/循环检测 | P0 |
| T8.8 | compiler：声明合并 + 轻量导入作用域（可见性/只读/冲突） | P0 |
| T8.9 | Engine：`ModuleResolver` trait + import root；默认拒绝 | P0 |
| T8.10 | CLI：`--import-root` + 运行含 import 脚本 + 错误格式 | P1 |
| T8.11 | 测试：parser/engine/CLI/安全 全链路 | P0 |
| T8.12 | 文档：SPEC_LANGUAGE/SPEC_RUNTIME/AI_GUIDE + 决策登记 | P1 |

### 内置模块显式导入（D12，破坏性变更）

| ID | 任务 | 优先级 |
| :--- | :--- | :--- |
| T8.13 | `register_all` 不再默认插入内置模块；`ModuleLoader` 识别 `std:` 前缀并在 import 时注册（命名空间 + 具名双形式） | P0 |
| T8.14 | 迁移现有约 **213 处** `模块.函数` 直接用法（测试 111 / 示例 18 / 文档 71 / 源码 13）为显式导入 | P0 |
| T8.15 | 安全影响醒目标记（SPEC/CHANGELOG）；评估"宿主全局否决开关"是否纳入 | P1 |
| T8.16 | 更新 `security.rs` 等沙箱测试以反映"导入即授权"新语义 | P0 |

### stdlib 辅助（次要）

| ID | 任务 | 优先级 |
| :--- | :--- | :--- |
| T8.17 | `deep_eq`/`clone`/`has_keys`/`assert_keys` 实现 | P1 |
| T8.18 | `struct_like.rs` 集成测试 + 文档惯用法 | P2 |

---

## 7. 发布影响

- **破坏性变更（版本 0.3.0）**：内置模块改为必须显式导入（D12），现有约 **213 处** `模块.函数` 直接用法全部失效，需迁移（加 `import` 或改具名）。
- 文件 import 默认拒绝不影响现有脚本；stdlib 辅助为纯新增。
- 因 0.x 约定，破坏性变更由 `0.3.0`（minor）承载；完整版本路线与发布条件见《开发计划》§1、§2。

---

## 8. 开放问题 / 后续 backlog

- **D12 安全缓解（已落地 T8.15）**："导入即完全授权"使 `strict` 沙箱对内置模块失效。**已提供宿主全局否决开关**：`Sandbox::with_denied_module(name)` / `with_denied_modules([...])`——即使脚本显式 `import` 也报 `CapabilityDenied`，默认空（维持 D12 现状）。仍待定：是否提供**迁移过渡期**（旧式 `json.parse` 告警但可用）以降低 213 处迁移的冲击。
- **方案 2（VM 方法派发 / self 自动注入）**：方法共享版"对象方法"，留作后续评估（需改 `GetField`/`Call`，见 2.2）。
- **惰性迭代器（G-S12）/ 被捕获顶层变量全局镜像（G-L16）**：与本设计无关的既有 backlog，不在 M8 范围。
