# Ada-D 标准库与迭代器规范 (Standard Library & Iterator Specification)

> **版本**：1.0
> **权威范围**：全局内置函数、原生模块（`json`/`math`/`time`/`sys`/`env`）、string/array/map 原型方法、**统一迭代器协议与适配器**、宿主模块扩展接口。
> **不在此文档**：语言语法与相等/真值语义见 [SPEC_LANGUAGE.md](SPEC_LANGUAGE.md)；值的内部表示与错误类型见 [SPEC_RUNTIME.md](SPEC_RUNTIME.md)；`await_all`/`await_race` 的运行时语义见 [SPEC_ASYNC.md](SPEC_ASYNC.md)；路线图见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。
> **状态图例**：✅ 已实现 · ⚠️ 部分实现 / 行为与目标有偏差（附任务 ID）· 🚧 进行中 · 📋 计划中 · ❌ 不做

---

## 1. 设计准则

1. **Token 优先**：函数名短、语义自明（`len`、`sum`、`abs`），优先链式与属性式调用（`arr.len`、`s.trim()`）。
2. **双入口一致**：同一能力尽量同时提供**全局函数**（可被 `|>` 管道调用）与**原型方法**（可链式），例如 `sum(arr)` 与 `arr.sum()`。
3. **权限分级**：纯计算（`json`/`math`/原型方法）默认开启；时间、环境变量等宿主能力须经 `SandboxProfile` 显式授权（当前尚未分级，见 [SPEC_RUNTIME §10](SPEC_RUNTIME.md#10-沙箱与权限模型)）。
4. **零成本对齐 Rust**：底层为原生实现，不做多余序列化（`json` 除外）。

---

## 2. 全局内置函数

所有内置函数在 VM 初始化时注入全局作用域，无需 import，任何作用域可直接调用。

### 2.1 I/O 与调试 ✅

| 签名 | 状态 | 行为 |
| :--- | :--- | :--- |
| `print(...args)` | ✅ | 各参数 `Display` 后以**空格**连接，写 stdout，不换行 |
| `println(...args)` | ✅ | 同上，末尾换行 |
| `eprintln(...args)` | ✅ | 同上，写 stderr |

### 2.2 类型内省 ✅

| 签名 | 返回 | 说明 |
| :--- | :--- | :--- |
| `type_of(v)` | `string` | `"nil"` `"bool"` `"int"` `"float"` `"string"` `"array"` `"map"` `"function"` `"task"` `"iterator"` |
| `is_nil(v)` | `bool` | 无参时返回 `true` |
| `is_num(v)` | `bool` | `int` 或 `float` |
| `is_str(v)` / `is_arr(v)` / `is_map(v)` / `is_fn(v)` / `is_task(v)` / `is_iterator(v)` | `bool` | ✅ 全部可用（`is_iterator` 于 T2.1 补齐） |

### 2.3 类型转换

| 签名 | 状态 | 规则 |
| :--- | :--- | :--- |
| `to_int(v)` | ✅ | Int 原样；Float 截断（±inf/NaN 走 Rust 饱和转换）；Bool→1/0；String 解析十进制（失败 `nil`）；其它 → `nil` |
| `to_float(v)` | ✅ | Float 原样；Int 提升；Bool→1.0/0.0；String 解析（失败 `nil`） |
| `to_str(v)` | ✅ | 任意值 `Display`；整数值的浮点保留小数位（`to_str(1.0) == "1.0"`，T3.6），极大/极小值用科学计数法 |
| `to_bool(v)` | ✅ | 按真值规则（见 [SPEC_LANGUAGE §9.2](SPEC_LANGUAGE.md)） |

### 2.4 断言 ✅

| 签名 | 行为 |
| :--- | :--- |
| `assert(cond, [msg])` | `cond` 为假 → 原生错误（消息默认 `"Assertion failed"`）；成功返回 `true` |
| `assert_eq(a, b, [msg])` | 不相等 → 原生错误（默认消息含左右值 Debug）；⚠️ 使用 §9.1 的相等语义，数组/map 为引用相等 |

### 2.5 数学 ✅

| 签名 | 行为 | 备注 |
| :--- | :--- | :--- |
| `abs(x)` | Int → Int；Float → Float | ⚠️ 非数字返回 `Int(0)`（静默） |
| `min(a, b, ...)` / `max(a, b, ...)` | 返回最小/最大值；单参数且为数组时作用于数组；空数组返回 `nil` | ✅ 不可比较的元素（非数字）被跳过 |
| `clamp(x, min, max)` | 夹取；首参为 Int 则返回 Int，否则 Float | ✅ |
| `round(x)` / `floor(x)` / `ceil(x)` | 返回 **Int** | ⚠️ 非数字返回 `Int(0)` |
| `math.*` 高级函数 | 见 §3.2 | — |

### 2.6 集合与区间

| 签名 | 状态 | 行为 |
| :--- | :--- | :--- |
| `len(v)` | ✅ | Array→元素数、Map→键数、**String→字符数**、其它→0 |
| `range(start, end, [step])` | ✅ | 返回**惰性** `iterator`；左闭右开；`step` 缺省 1；`range(n)` = `range(0, n)`；`step` 为负时倒序；`step == 0` 产生空迭代 |
| `push(arr, v)` | ✅ | 就地向数组追加，返回 `nil`；等价 `arr.push(v)` |
| `sum(arr)` | ✅ | 数值累加（Int 保持 Int，出现 Float 则整体 Float）；⚠️ 非数值元素**静默忽略** |

### 2.7 异步原语 ✅（M4：由 VM 调度器实现，语义见 [SPEC_ASYNC](SPEC_ASYNC.md)）

| 签名 | 状态 | 行为 |
| :--- | :--- | :--- |
| `sleep_ms(ms)` | ✅ | 挂起到真实墙钟时刻（不阻塞 OS 线程）；适配器回调内退化为阻塞（SPEC_ASYNC §4.3） |
| `yield_now()` | ✅ | 让出一次调度（不推进墙钟），用于观察确定性交错 |
| `await_all(arr)` | ✅ | 未全部结算则挂起；按**实参顺序**返回结果数组；任一失败即抛出 |
| `await_any(arr)` | ✅ | 等待**任一**结算，返回最先结算者的值 |
| `await_race(arr)` | ✅ | `await_any` 的旧名（等价） |
| `timeout(task, ms)` / `task.timeout(ms)` | ✅ | 期限内结算返回其值，超时则取消任务并返回 `nil` |
| `task.cancel()` | ✅ | 协作式取消，返回 `bool`（已终态返回 `false`） |
| `task.state` / `task.id` / `task.label` | ✅ | 任务诊断信息（状态名见 SPEC_ASYNC §5.1） |
| `is_task(v)` | ✅ | 是否为任务值 |

---

## 3. 原生模块 (Native Modules)

模块是 `map` 值，属性访问即取成员，支持 `json.parse(...)` 形式调用。

### 3.1 `json` ✅

| API | 说明 |
| :--- | :--- |
| `json.parse(str)` | 反序列化为原生 Map/Array/标量；解析失败 → 原生错误 |
| `json.stringify(v, [pretty])` | 序列化；`pretty` 为真值则缩进输出；函数/迭代器降级为字符串占位 |
| `json.valid(str)` | 是否为合法 JSON |

### 3.2 `math` ✅

| API | 说明 |
| :--- | :--- |
| `math.PI` / `math.E` / `math.INF` | 常量（Float） |
| `math.sqrt(x)` | 平方根；非数字按 0 处理 ⚠️ |
| `math.pow(x, y)` | 幂；参数不足报错 |
| `math.log(x)` / `math.log10(x)` | 自然对数 / 常用对数 |
| `math.sin(x)` / `math.cos(x)` / `math.tan(x)` | 三角函数（弧度） |
| `math.random()` | `[0.0, 1.0)`；线程局部 xorshift 状态，进程启动时以时间播种 |
| `math.random_int(min, max)` | `[min, max]` 闭区间；`min >= max` 时返回 `min` |
| 舍入函数（`round`/`floor`/`ceil`） | 以**全局函数**形式提供，不在 `math` 命名空间 ⚠️（命名不一致） |

### 3.3 `time` ✅（受沙箱管控目标见 T5.1）

| API | 说明 |
| :--- | :--- |
| `time.now_ms()` | Unix 毫秒时间戳（Int） |
| `time.now_secs()` | Unix 秒时间戳（Int） |
| `time.elapsed_ms(start_ms)` | 距 `start_ms` 的毫秒差；时钟回拨时返回 0 |

### 3.4 `sys` / `env` ✅（M5：`env` 受键白名单管控）

| API | 状态 | 说明 |
| :--- | :--- | :--- |
| `sys.version` | ✅ | crate 版本字符串 |
| `sys.os` / `sys.arch` | ✅ | `std::env::consts::{OS, ARCH}` |
| `env.get(key, [default])` | ✅ | 只读**白名单内**的键；未授权或未设置时返回 `default` 或 `nil`（未授权时**不读**宿主环境，T5.2） |
| `env.has(key)` | ✅ | 同上，仅白名单内键返回 `true` |
| `env.allowed()` | ✅ | 返回当前白名单（便于自省与测试） |

### 3.5 `crypto` ✅（T7.5：纯计算，三档 profile 都可用）

| API | 状态 | 说明 |
| :--- | :--- | :--- |
| `crypto.sha256(text)` | ✅ | SHA-256，返回 64 位小写十六进制（按 **UTF-8 字节**哈希） |
| `crypto.hmac_sha256(key, text)` | ✅ | HMAC-SHA256（键与消息都按 UTF-8 字节），返回十六进制 |
| `crypto.hex(text)` | ✅ | 把字符串的 UTF-8 字节编码成十六进制 |

- 正确性由**独立实现交叉验证**：期望值来自 Node.js `crypto`（见 `crates/ada_engine/tests/builtins_crypto.rs`）。
- 不涉及宿主能力（不读环境/不落盘/不联网），因此与 `json`/`math` 同级：**无需宿主授权**。
- 实现使用 `sha2`/`hmac` crate，不自造密码学原语。

### 3.6 `fs` ✅（T7.5：**受限根目录**，默认不暴露）

| API | 状态 | 说明 |
| :--- | :--- | :--- |
| `fs.root` | ✅ | 规范化后的根目录（便于自省/测试） |
| `fs.read_text(path)` | ✅ | 读 UTF-8 文本；文件不存在 → `nil`；目录/超大/非 UTF-8 → 报错 |
| `fs.write_text(path, text)` | ✅ | 写文本（**不隐式建目录**），成功返回 `true` |
| `fs.exists(path)` / `fs.is_dir(path)` | ✅ | 存在性/类型判断 |
| `fs.list(path)` | ✅ | 目录项名称数组，**按名称排序**（保证脚本输出确定） |
| `fs.remove(path)` | ✅ | 仅删除文件：存在 → `true`，不存在 → `false`，目录 → 报错 |

**只有宿主显式给出根目录时模块才存在**（`Sandbox::with_fs_root(dir)`；CLI 为 `ada --fs-root DIR …`）。
默认三档 profile 都不暴露 `fs`，脚本访问会得到 `CapabilityDenied { name: "fs" }`。

安全约束（实现见 `crates/ada_vm/src/builtins/fs.rs`，敌意输入测试见
`crates/ada_engine/tests/fs_sandbox.rs`）：

1. 空路径、含 NUL 的路径、任何含 `..` 的路径直接拒绝。
2. 目标路径（写文件时取其**父目录**）经 `canonicalize` 解析后，必须仍位于规范化后的根目录之下——
   因此符号链接无法逃逸。
3. 前缀按**路径组件**比较，`/root` 与 `/root-evil` 不会被混淆。
4. 单次读写受字节上限约束（取 1 MiB 与内存预算的较小者），目录列举条目数有上限。
5. 越界路径**报错**而不是静默返回 `nil`/`false`（静默失败会把攻击伪装成"没事"）。
6. 根目录不可用时，模块仍注册但每次调用都报 `fs root '…' is not an accessible directory`。

---

## 4. 原型方法 (Prototype Methods)

通过 `.` 访问内建类型的方法；**方法调用返回 `NativeFn`，需要 `()` 触发**。

### 4.1 `string` ✅⚠️

| 成员 | 状态 | 行为 |
| :--- | :--- | :--- |
| `.len` | ✅ | **字符数**（Unicode 标量个数，T2.5） |
| `.len_bytes` | ✅ | 字节数（UTF-8 编码长度） |
| `.chars` | ✅ | 返回单字符数组（按 Unicode 字符切分） |
| `.chars_with_idx` | ✅ | 返回 `[[0, ch0], [1, ch1], ...]`（T2.3） |
| `.lines` | ✅ | 按行切分为数组 |
| `.trim()` | ✅ | 去首尾空白 |
| `.to_upper()` / `.to_lower()` | ✅ | 大小写转换 |
| `.split(sep)` | ✅ | 切分为数组，缺省分隔符为空格 |
| `.contains(s)` / `.starts_with(s)` / `.ends_with(s)` | ✅ | 子串判断 |
| `.replace(from, to)` | ✅ | 全部替换（参数不足报错） |
| `.slice(start, [end])` | ✅ | **按字符**切片，支持负索引，越界自动夹取 |
| `.iter()` | ✅ | 返回显式迭代器（零参方法，T2.3）；`iterator.next()` 逐项推进，耗尽返回 `nil` |

### 4.2 `array` ✅⚠️

| 成员 | 状态 | 行为 |
| :--- | :--- | :--- |
| `.len` | ✅ | 元素个数 |
| `.push(v)` | ✅ | 就地追加，返回 `nil` |
| `.pop()` | ✅ | 弹出末尾，空数组返回 `nil` |
| `.insert(idx, v)` | ✅ | 指定位置插入，越界报错 |
| `.remove(idx)` | ✅ | 按下标移除并返回移除值，越界报错 |
| `.contains(v)` | ✅ | 使用 §9.1 相等语义（⚠️ 嵌套容器为引用比较） |
| `.reverse()` | ✅ | **就地**反转，返回 `nil` |
| `.rev()` | ✅ | 返回**新数组**（逆序拷贝，非惰性） |
| `.sort()` | ✅ | 就地排序；数字类型混合按数值，字符串按字典序，其它类型视为相等（不稳定） ⚠️ |
| `.sum()` | ✅ | 数值累加；非数值元素静默忽略 ⚠️ |
| `.enumerate()` | ✅ | 返回 `[[0, v0], [1, v1], ...]` 新数组 |
| `.join(sep)` | ✅ | 拼接字符串，缺省分隔符 `","` |
| `.await_all()` / `.await_any()` / `.await_race()` | ✅ | 见 §2.7（真实挂起与并发） |
| `.map(f)` / `.filter(f)` / `.take(n)` / `.skip(n)` / `.count()` / `.collect()` / `.find(f)` / `.any(f)` / `.all(f)` | ✅ | **迭代器适配器**（T2.1），语义与惰性程度统一见 §6 |
| `.iter()` | ✅ | 返回显式迭代器（T2.3）；`iterator.next()` 逐项推进，耗尽返回 `nil` |

### 4.3 `map` ✅⚠️

| 成员 | 状态 | 行为 |
| :--- | :--- | :--- |
| `.len` | ✅ | 键值对数量 |
| `.keys()` / `.values()` | ✅ | 返回数组；⚠️ **顺序不保证**（底层 `HashMap`） |
| `.entries()` | ✅ | 返回 `[[k, v], ...]`；⚠️ 顺序不保证 |
| `.contains_key(k)` | ✅ | 键存在性 |
| `.remove(k)` | ✅ | 删除并返回旧值，缺失返回 `nil` |
| `.merge(other)` | ✅ | 就地覆盖合并，返回 `nil` |
| `.get(k)` / `.has(k)` | 📋 | 可用属性访问/`contains_key` 替代，暂不新增 |
| `.iter()` | ✅ | 返回显式迭代器（T2.3） |
| `.map(f)` / `.filter(f)` / `.take(n)` / `.skip(n)` / `.count()` / `.collect()` / `.find(f)` / `.any(f)` / `.all(f)` | ✅ | 迭代器适配器（T2.1）；map 按 `[key, value]` 二元数组迭代 |

### 4.4 成员派发优先级 ✅（T7.5 统一，关闭原缺口 G-S12）

字段访问（`x.field`）的解析顺序：

1. **自有键优先**：map 上的同名键一律优先，**无论该名字是否与原型方法/适配器方法重名**——
   `{ len: 9 }.len` → `9`（不是"键的数量"），`{ remove: 3 }.remove` → `3`。
   - 与索引访问语义一致：`m.key` 等价于 `m["key"]`（此前二者在重名时会给出不同结果）。
   - **必要性**：宿主模块（`json`/`math`/`fs`/自定义 `NativeModule`）在脚本侧就是普通 map，
     若方法名能遮蔽键，模块里与原型同名的函数会被静默替换。T7.5 正是撞上这个真实缺陷
     （`fs.remove` 拿到的是 map 原型方法而不是模块函数）才做的这个决定。
   - 若确实需要方法而键恰好重名：给键换个名字。
2. **原型方法名**：`len` / `keys` / `values` / `entries` / `contains_key` / `remove` / `merge` /
   `push` / `pop` / `sort` / `trim` / `split` / `slice` … —— **仅在该键不存在时**命中。
3. **适配器方法名**（`map`/`filter`/`take`/`skip`/`collect`/`count`/`find`/`any`/`all`/`enumerate`/`rev`/`join`/`sum`）：仅对可迭代目标生效，且同样让位于自有键。
4. 其余情况返回 `nil`；对 `nil` 目标做非可选访问则报 `TypeError`。对不可迭代的目标调用适配器名字（如 `true.filter`）会得到 `nil`，随后调用报 `NotCallable`。

> **变更记录**：0.1.0 早期版本中，原型方法名（`len`/`keys`/…）会**遮蔽**同名键
> （`{ len: 9 }.len` → `1`），而适配器名则让键优先——两套规则并存，且与 `m["len"]` 不一致。
> 现统一为"键优先"，回归测试见 `crates/ada_vm/tests/adapters.rs::own_map_keys_win_over_method_names`。

---

## 5. 统一迭代器协议

### 5.1 目标 ✅（`for..in` 层面已统一）

任何可迭代对象都能直接用于 `for`，编译为 `IterInit` → 循环头 `IterNext`（字节码细节见 [SPEC_RUNTIME §3](SPEC_RUNTIME.md#3-字节码指令集)）。

| 迭代源 | 元素 | 状态 |
| :--- | :--- | :--- |
| `array` | 元素本身 | ✅ |
| `string` | 单个 Unicode 字符（1 字符串） | ✅ |
| `map` | `[key, value]` 二元数组（配合 `for [k, v] in m` 解构） | ✅（⚠️ 顺序不保证） |
| `iterator`（如 `range(...)`） | 迭代器产出值 | ✅（原样透传） |
| `int n` | `0, 1, ..., n-1`（等价 `range(n)`） | ✅ |
| 其它任何值 | **单元素序列**（只迭代一次） | ⚠️ 静默，不报错 |

```ada
for fruit in ["apple", "banana"] { ... }         // 元素
for ch in "Ada 语言" { ... }                     // 字符
for [k, v] in { a: 1, b: 2 } { ... }             // 解构
for i in range(0, 10, 2) { ... }                 // 惰性区间
for i in 5 { ... }                               // 0..4
for [idx, item] in [10, 20].enumerate() { ... }  // 带索引
```

### 5.2 惰性区间 `range` ✅

- **不预分配数组**，内存 `O(1)`，状态机为 `{current, end, step}`。
- 左闭右开：`range(0, 5)` → `0,1,2,3,4`；`range(5, 0, -1)` → `5,4,3,2,1`（实测）。
- `range` 返回值的 `type_of` 为 `"iterator"`。
- ✅ `iterator` 拥有一整套适配器方法与显式迭代协议（见 §6）；`println` 仍显示为 `<iterator>`（不物化）。
- ⚠️ 迭代器是**有状态、可消费**的：`.next()`、`for..in` 与适配器都会推进同一份状态；被消费后即耗尽。需要重复遍历时请先 `.collect()`。

### 5.3 迭代器状态机（类型）

| 变体 | 字段 | 来源 |
| :--- | :--- | :--- |
| `Array` | `items, index` | 数组（快照拷贝，迭代中修改原数组不影响本轮） |
| `String` | `chars, index` | 字符串 |
| `Map` | `entries, index` | 字典（快照） |
| `Range` | `current, end, step` | `range(...)` 或 `for i in n` |

---

## 6. 迭代器适配器

**状态**：✅ 已实现（T2.1，2026-10）。**当前为即时求值**（决策 D3 = A：先把能力打通，惰性链留待后续）；因此适配器结果一律是 `Array`，`range(1_000_000).map(...)` 会先物化整段区间。

**双入口**：同名能力同时提供方法形式与全局函数形式，后者供 `|>` 管道使用。

| 适配器 | 参数（含可迭代值） | 返回 | 功能 | 状态 |
| :--- | :--- | :--- | :--- | :--- |
| `.map(f)` / `map(xs, f)` | `f: (T) -> R` | `Array<R>` | 元素映射 | ✅ |
| `.filter(f)` / `filter(xs, f)` | `f: (T) -> bool` | `Array<T>` | 谓词过滤 | ✅ |
| `.take(n)` / `take(xs, n)` | `n: int` | `Array<T>` | 截取前 n 个（`n < 0` 视为 0） | ✅ |
| `.skip(n)` / `skip(xs, n)` | `n: int` | `Array<T>` | 跳过前 n 个 | ✅ |
| `.collect()` / `collect(xs)` | — | `Array` | 物化 | ✅ |
| `.count()` / `count(xs)` | — | `int` | 元素计数 | ✅ |
| `.sum()` / `sum(xs)` | — | `num` | 数值求和（整数溢出报错） | ✅ |
| `.find(f)` / `find(xs, f)` | `f: (T) -> bool` | `T \| nil` | 首个满足项 | ✅ |
| `.any(f)` / `any(xs, f)` | `f: (T) -> bool` | `bool` | 存在性（空集合为 `false`） | ✅ |
| `.all(f)` / `all(xs, f)` | `f: (T) -> bool` | `bool` | 全称判断（空集合为 `true`） | ✅ |
| `.enumerate()` / `enumerate(xs)` | — | `Array<[int, T]>` | 附加递增索引 | ✅ |
| `.rev()` / `rev(xs)` | — | `Array<T>` | 逆序拷贝 | ✅ |
| `.join(sep)` / `join(xs, sep)` | `sep: string`（缺省 `","`） | `string` | 拼接 | ✅ |
| `.iter()` | — | `Iterator` | 显式迭代器（零参方法） | ✅ |
| `iterator.next()` | — | `T \| nil` | 推进一步；耗尽返回 `nil` | ✅ |

### 6.1 参数与错误契约

- 参数个数（**含可迭代值**）：`map`/`filter`/`find`/`take`/`skip`/`any`/`all` 恰为 2；`join` 为 1–2；其余恰为 1。不符时报 `NativeError`（不是 panic）。
- 回调参数个数不匹配报 `ArityMismatch`。
- 回调抛错会原样向上传播（适配器不做吞错）。
- 可迭代源：`array` / `string`（按字符）/ `map`（按 `[key, value]`）/ `iterator` / `int`（`0..n`）。其它类型作为**单元素序列**处理（与 `for..in` 一致）。
- 对不可迭代目标调用适配器名字得到 `nil`，随后调用报 `NotCallable`（派发规则见 §4.4）。

### 6.2 已通过的计划验收用例

```ada
let result = range(1, 1000)
    |> filter(x => x % 2 != 0)
    |> map(x => x * x)
    |> take(5)
    |> sum()
assert_eq(result, 165)   // 1 + 9 + 25 + 49 + 81
```

对应测试：`crates/ada_vm/tests/adapters.rs::plan_acceptance_pipeline_returns_165`。

> 注：该示例只用到回调自身参数。**自 T2.2 起闭包也能捕获外层变量**，因此
> `let t = 3; xs |> filter(x => x > t)` 这类写法同样可用（回调可以累加外层计数器）。

### 6.3 双入口实现方式 ✅

为了让 `|>` 与链式调用都自然，适配器同时存在于两个位置：

- **全局函数**：`filter(xs, f)`、`map(xs, f)` …（`xs |> filter(f)` 会脱糖为 `filter(xs, f)`）。
- **原型方法**：`xs.filter(f)` …（VM 在字段派发时绑定接收者）。

实现上二者共用同一个 `Value::Intrinsic` 值：全局形式无接收者，方法形式把接收者绑定进 `receiver`，调用时拼成 `[接收者, ...已绑定实参, ...调用实参]` 交给 `Vm::invoke_adapter`。因此 `let m = xs.map` 之后再 `m(f)` 同样可用。

---

## 7. 宿主模块扩展接口

**目标形态** 📋（任务 T5.5）：

```rust
pub trait NativeModule: Send + Sync {
    /// 模块名（如 "crypto"、"fs"）
    fn name(&self) -> &str;
    /// 导出函数表
    fn exports(&self) -> HashMap<String, Value>;
}

// 宿主侧
engine.register_module(MyToolModule::new());
```

要求：

1. 注册的模块受 `SandboxProfile` 管控（`Strict` 默认不加载宿主模块）。
2. 模块函数不得 panic；返回 `Err(String)` 由 VM 包装为 `NativeError`。
3. 模块必须可跨线程（`Send + Sync`），因为 VM 允许在不同线程的独立实例中运行。

---

## 8. 缺口汇总（与任务对应）

| ID | 缺口 | 状态 | 任务 |
| :--- | :--- | :--- | :--- |
| G-S1 | 迭代器适配器全族缺失（`map`/`filter`/`take`/`skip`/`collect`/`count`/`find`/`any`/`all`） | ✅ 已修复（T2.1）：13 个适配器已实现，方法/全局双入口 | ✅ T2.1 |
| G-S2 | `string.len` 与 `len()` 返回字节数 | ✅ 已修复（T2.5）：二者均为字符数，字节数走 `.len_bytes` | ✅ T2.5 |
| G-S3 | `iterator` 无任何方法，无法链式消费 | ✅ 已修复（T2.3）：适配器 + `.iter()`/`.next()` 可用 | ✅ T2.3 |
| G-S4 | `sleep_ms` 阻塞线程；`await_race` 语义非"首个就绪" | ✅ 已修复（T4.3/T4.5）：`sleep_ms` 挂起而非阻塞；`await_any`/`await_race` 返回最先结算者，新增 `timeout`/`cancel` | ✅ M4 |
| G-S5 | `env` 无白名单，`time`/`sys` 无权限分级 | ✅ 已修复（M5）：`Sandbox` 三档 + builder，`env` 键白名单（未授权不读宿主环境） | ✅ T5.1 / T5.2 |
| G-S6 | `sum`/`min`/`max`/`abs`/`round` 对非数字**静默**处理 | ⚠️ 口径统一 | T3.5 |
| G-S7 | 舍入函数不在 `math` 命名空间，命名不一致 | ⚠️ | T3.5 |
| G-S8 | map 迭代/`keys()`/`entries()` 顺序不保证（`HashMap`） | ⚠️ | 规范明示；如需稳定顺序见 D6 |
| G-S9 | Float 字符串化丢失小数位（`1.0` → `"1"`） | ✅ 已修复（T3.6）：`1.0` → `"1.0"`，`1e300` → `"1e300"` | ✅ T3.6 |
| G-S10 | 缺失 `is_iterator` | ✅ 已修复（T2.1）：已注册 `is_iterator` | ✅ T2.1 |
| G-S11 | `NativeModule` 与模块注册接口不存在 | ✅ 已修复（M5）：`NativeModule` trait + `Engine::register_module`（冲突报 `ModuleConflict`） | ✅ T5.5 |
| G-S12 | 适配器结果为即时求值（非惰性），大区间会先物化 | ⚠️ 已按决策 D3 = A 记录 | 后续评估惰性链 |
| G-S13 | map 上 `len`/`keys` 等原型方法名遮蔽同名键（`{len: 9}.len` → `1`） | ✅ 已修复（T7.5）：统一为**自有键优先**，与 `m["len"]` 一致；`fs.remove` 被遮蔽的真实缺陷促成了该决定（见 §4.4） | ✅ T7.5 |
