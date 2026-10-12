# Ada-D 单页规格（AI 生成代码专用）

> **用途**：把这一页交给模型（或人）即可写出正确的 Ada-D 代码。它是**唯一**需要随身携带的语法/语义速查。
> **权威顺序**：本页与 `SPEC_LANGUAGE` / `SPEC_RUNTIME` / `SPEC_STDLIB` / `SPEC_ASYNC` 冲突时，以规范为准，
> 并请提 issue——本页所有 ```ada run``` 示例都由 `crates/ada_cli/tests/ai_guide.rs` 真实执行并比对输出，
> 不会与实现漂移。
> **运行**：`ada file.ada` / `ada -e "表达式"` / `ada repl`；沙箱档位见 [§9](#9-沙箱与宿主能力)。

---

## 1. 最小程序

```ada run
println("hello, ada")
```

```output
hello, ada
```

- 脚本的**最后一个表达式**是脚本返回值（会被 CLI 打印，除非是 `nil`）。
- `println(...)` 输出一行；`print(...)` 不换行。
- 注释：`// 行注释`、`/* 块注释 */`。

```ada run
let x = 41
x + 1
```

```output
42
```

---

## 2. 变量：`let` 不可变，`mut` 可变

```ada run
let fixed = 1
mut counter = 0
counter += 2
println($"{fixed}/{counter}")
```

```output
1/2
```

- **`let` 绑定不可重新赋值**（编译期报错）；要改就用 `mut`。参数、`for` 变量、`match` 绑定可重新赋值。
- 绑定不可重指向 ≠ 内容不可变：`let arr = [1]` 之后 `arr.push(2)` **合法**（与 JS `const` 一致）。

```ada run
let a = 1
a = 2
```

```error
Cannot assign to immutable variable
```

---

## 3. 函数

```ada run
fn add(a, b) = a + b              // 单行：表达式即返回值
fn greet(name) {                   // 块体：末尾表达式是返回值
    let upper = name.to_upper()
    $"Hello, {upper}!"
}
fn early(n) {
    if n < 0 { return "negative" }
    "non-negative"
}
println($"{add(2, 3)}|{greet("ada")}|{early(-1)}")
```

```output
5|Hello, ADA!|negative
```

- 无默认参数、无可变参数；实参个数必须精确匹配。
- 类型标注语法存在但**不生效**：`fn f(x: int) -> int = x` 可解析、被忽略。

---

## 4. 控制流与循环

```ada run
mut sum = 0
for i in range(1, 5) { sum += i }      // 1..4
mut n = 3
while n > 0 { n -= 1 }
mut flags = ""
for i in 3 { flags += to_str(i) }      // `for i in N` = 0..N-1
println($"{sum}|{n}|{flags}")
```

```output
10|0|012
```

`for` 可迭代：数组、字符串（按字符）、字典（`for [k, v] in m`）、`range(a, b[, step])`、整数 `N`、迭代器。
`break` / `continue` 可用。

---

## 5. `match`：支持比较模式与 guard

```ada run
fn grade(score) {
    match score {
        >= 90 => "A",
        >= 60 => "C",
        _     => "F"
    }
}
let hit = match 7 { n if n > 5 => { hit: n }, _ => { hit: 0 } }
println($"{grade(95)}{grade(70)}{grade(10)}|{hit.hit}")
```

```output
ACF|7
```

- 模式：字面量、`>=`/`>`/`<=`/`<` 比较、`n if 条件`（guard）、`_`。
- **分支体与表达式同规则**：`{ k: v }` 是 map 字面量，`{ let a = 1; a }` 才是块。
- **非穷尽 `match` 会运行时报错**（不会静默返回 nil）：

```ada run
match 99 { 1 => "one" }
```

```error
No `match` arm matched
```

---

## 6. 闭包：捕获是「引用共享」

```ada run
fn make_adder(n) { x => x + n }        // 返回闭包（逃逸后仍持有 n）
let add10 = make_adder(10)

mut hits = 0
let bump = () => { hits += 1 }          // 闭包内写外层变量
bump()
bump()
println($"{add10(5)}|{hits}")
```

```output
15|2
```

- 箭头闭包：`x => expr`、`(a, b) => expr`、`() => expr`。
- 被捕获的变量与外层**共享同一个槽**（写入互相可见）；`for` 变量每轮独立，因此 `for i in range(0,3) { fs.push(() => i) }` 得到 `[0,1,2]`。
- 闭包是值：可以传参、放进数组、作为 `map`/`filter` 的参数。

---

## 7. 字符串、数组、字典

```ada run
let s = "  Ada-D  "
let xs = [3, 1, 2]
let cfg = { retries: 3, verbose: true }
println($"{s.trim().to_lower()}|{xs.rev()}|{cfg.retries}|{xs.contains(2)}")
```

```output
ada-d|[2, 1, 3]|3|true
```

- **字符串插值**：`$"Hello, {name}!"`，`{...}` 内可放任意表达式。
- 字符串索引按**字符**：`"语言"[0] == "语"`；负索引从末尾计数（`"abc"[-1] == "c"`）；`.len` 是字符数，`.len_bytes` 是字节数。
- 数组越界读 → `nil`；字典取不存在的键 → `nil`。
- 常用方法：字符串 `.trim/.to_upper/.to_lower/.split/.join/.replace/.contains/.starts_with/.ends_with/.slice`；
  数组 `.len/.push/.pop/.insert/.remove/.contains/.sort/.rev/.join/.sum`；
  字典 `.len/.keys/.values/.entries/.contains_key/.remove/.merge`。

---

## 8. 迭代器适配器（管道是首选写法）

```ada run
println(range(1, 10) |> filter(x => x % 2 == 1) |> map(x => x * x) |> sum())
println([1, 2, 3, 4].map(x => x * 10).filter(x => x > 20).collect())
```

```output
165
[30, 40]
```

- 适配器：`map` `filter` `take` `skip` `collect` `count` `find` `any` `all` `enumerate` `rev` `join` `sum`。
- **方法链与全局函数等价**：`xs.map(f)` ≡ `xs |> map(f)` ≡ `map(xs, f)`；适配器结果是**数组**（非惰性链）。
- 可用范围：数组、字符串、字典、`range`、任意迭代器。

---

## 9. 沙箱与宿主能力

| 环境 | 默认允许 |
| :--- | :--- |
| CLI（`ada`，本地受信） | `time`、`sys`、`env`（**空白名单**）、`sleep_ms`、`crypto`；`fs` 需 `--fs-root DIR` |
| Rust 宿主 `Engine::new()` | **只有纯计算 + `json` + `math` + `crypto`**（strict） |

```ada run
println($"{to_int(1 < 2.5)}|{json.valid("{\"ok\": true}")}|{math.sqrt(144.0)}")
```

```output
1|true|12.0
```

- 在 strict 下访问 `time`/`env`/`sys`/`sleep_ms`/`fs` 会报 `CapabilityDenied`（而不是"未定义"）。
- **哈希**（纯计算，任何环境都可用）：`crypto.sha256(text)`、`crypto.hmac_sha256(key, text)`、`crypto.hex(text)`。

  ```ada run
  println(crypto.sha256("abc"))
  ```

  ```output
  ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
  ```
- **文件读写**：只有宿主显式给出根目录才存在（`Sandbox::with_fs_root` / `ada --fs-root DIR`），
  且路径被限制在该目录内（`..`、绝对路径、符号链接都逃不出去，越界会**报错**而不是返回 nil）。
  可用 `fs.read_text` / `fs.write_text` / `fs.exists` / `fs.is_dir` / `fs.list` / `fs.remove` / `fs.root`。
- 宿主还有内存/墙钟/任务数预算，超限报 `MemoryLimitExceeded` / `WallClockLimitExceeded` / `TaskLimitExceeded`。
- 需要宿主能力时由宿主显式开档，脚本侧无法自行提权。

---

## 10. 并发（协作式调度，无抢占）

```ada run
fn work(x, ms) {
    sleep_ms(ms)
    x * 2
}
let a = spawn work(21, 10)
let b = spawn work(1, 5)
println($"first: {await a}")
println($"second: {b.await}")
let all = [a, b].await_all()
println($"{all}|{a.state}")
```

```output
first: 42
second: 2
[42, 2]|done
```

- `spawn f(...)` **不在生成点执行**任务体，任务在调度器首次运行它时才开始。
- 等待单个任务：前缀 `await t` 或后缀 `t.await`（**不要写 `t.await()`**——那会先等待、再调用结果）。
- `await_all`（按实参顺序）/ `await_any`（最先结算）/ `timeout(task, ms)`（超时取消并返回 `nil`）/ `task.cancel()` / `task.state`。
- 并发只对**挂起点**（`sleep_ms`、`await`）有效：两个各 `sleep_ms(200)` 的任务总耗时 ≈200ms。
- 任务里的错误在 `await` 处抛出为 `TaskFailed{label, source}`，并**指向任务内部出错的那一行**；主任务结束即脚本结束（未完成任务被丢弃）。

---

## 11. 必须记住的 10 条反直觉规则

| # | 规则 | 例子 |
| :--- | :--- | :--- |
| 1 | `let` 不可重新赋值，容器**内容**可以变 | `let a=1; a=2` ❌；`let a=[1]; a.push(2)` ✅ |
| 2 | 换行后跟 `+ - * / % \|> ?? && \|\| .` 会**续行**（ASI） | `a⏎-b` 是 `a - b`，不是两条语句 |
| 3 | `{ x }` 是 **map**，不是块 | `type_of({ x }) == "map"` |
| 4 | `match` 分支体同规则，且**必须穷尽** | `{ k: v }` 是 map；无 `_` 且未命中 → 运行时报错 |
| 5 | `spawn f(1).await` = `spawn (f(1).await)` | 想"先生成再等待"请分两行写 |
| 5b | 等待任务写 `await t` / `t.await`，**不是** `t.await()` | `.await()` 先等待、再调用结果（结果不是函数就报 "Cannot call non-callable value"） |
| 6 | 有序比较**只接受数值** | `5 < "a"` 报错（不再静默 `false`）；`NaN` 比较恒 `false` |
| 7 | 浮点保留小数位，整数不保留 | `to_str(1.0) == "1.0"`，`to_str(1) == "1"` |
| 8 | 整数溢出/除零/取模零/移位越界都**报错** | `i64::MAX + 1` → `TypeError`；浮点除零按 IEEE 得 `inf` |
| 9 | 负索引从末尾计数，越界读得 `nil` | `[10,20][-1] == 20`；`"abc"[-9] == nil` |
| 10 | 字符串 `+=` 是 O(n²) | 大批量拼接请考虑收集成数组再 `join` |

---

## 12. 常见错误信息 → 修法

> 所有错误都会给出 `file:line:col`（位置指向真正出问题的那一行；任务内失败指向任务内部）。
> 下面括号里是消息片段。

| 错误（片段） | 含义 | 修法 |
| :--- | :--- | :--- |
| `Cannot assign to immutable variable 'a'` | 给 `let` 绑定赋值 | 声明改成 `mut a` |
| `UndefinedGlobal("x")` | 名字不存在/拼错/作用域外 | 检查拼写与作用域 |
| `CapabilityDenied { name: "time", profile: "strict" }` | 沙箱未开放该能力 | 由宿主开档（脚本无法自行提权） |
| `No \`match\` arm matched the value` | `match` 未穷尽 | 加 `_` 分支或补齐模式 |
| `TypeError("Cannot compare ... only numbers are orderable")` | 非数值比较 | 先判断类型再比较 |
| `TypeError("Division by zero")` / `Integer overflow` | 整数运算边界 | 加零值/范围检查 |
| `TypeError("Cannot add ... and ...")` | 类型不支持该运算 | 用 `to_str`/`to_int` 显式转换 |
| `TaskFailed { label, source }` | 任务体内部报错 | 看 `source`；必要时在任务内先兜底 |
| `MemoryLimitExceeded` / `WallClockLimitExceeded` | 触发沙箱预算 | 减少工作量或由宿主放宽预算 |
| `Parse(UnexpectedToken { found: "-", ...})` | 换行续行（规则 2） | 把表达式用括号包起来 |
| `Argument count mismatch` | 实参个数不对 | 对齐函数签名 |

---

## 13. 三个完整可运行示例

| 示例 | 展示 | 运行 |
| :--- | :--- | :--- |
| [examples/ai/transform.ada](../examples/ai/transform.ada) | JSON → 适配器管道 → 表格输出 | `ada examples/ai/transform.ada` |
| [examples/ai/async.ada](../examples/ai/async.ada) | `spawn` / `await_any` / `await_all` / `timeout` | `ada examples/ai/async.ada` |
| [examples/ai/validate.ada](../examples/ai/validate.ada) | `match` + guard + 闭包 + 布尔逻辑 | `ada examples/ai/validate.ada` |

它们的期望输出与示例同目录存放（`*.expected.txt`），由 golden 测试逐行比对。

---

## 14. 给模型的提示模板（可直接粘贴）

```text
你在写 Ada-D（Ada Script）代码。规则：
- 变量：let 不可重新赋值，mut 可变；容器内容可变。
- 函数：fn f(a, b) = 表达式 或 fn f(a, b) { ... }（末尾表达式即返回值）。
- 控制流：if/else、while、for x in 序列、match（支持 >= 比较模式与 n if 条件，必须穷尽）。
- 集合：优先用管道 xs |> filter(f) |> map(g) |> collect()；字符串插值 $"a {b}"。
- 并发：spawn 创建任务（不在生成点执行），await / await_all / await_any / timeout。
- 不要用：`a⏎-b`（会续行）、`{ x }` 当块、`5 < "a"`、依赖 nil 静默转换、依赖整数溢出回绕。
- 输出：println(...)；脚本最后一个表达式即为返回值。
- 完整单页规格：docs/AI_GUIDE.md（示例都可运行）。
```

需要更细的语义时按需查阅：[语言规范](SPEC_LANGUAGE.md) · [运行时与错误](SPEC_RUNTIME.md) ·
[标准库](SPEC_STDLIB.md) · [异步并发](SPEC_ASYNC.md) · [文档索引](README.md)
