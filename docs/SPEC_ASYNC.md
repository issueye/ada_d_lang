# Ada-D 异步与并发规范 (Async & Concurrency Specification)

> **版本**：1.1（M4 落地协作式调度器）
> **权威范围**：异步语法、并发原语、调度与挂起模型、任务生命周期契约、宿主异步 API、异步熔断。
> **不在此文档**：语言通用语法见 [SPEC_LANGUAGE.md](SPEC_LANGUAGE.md)；指令集与 VM 结构见 [SPEC_RUNTIME.md](SPEC_RUNTIME.md)；`await_all`/`await_any` 的参数细节见 [SPEC_STDLIB §2.7](SPEC_STDLIB.md)；路线图见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。
> **状态图例**：✅ 已实现 · ⚠️ 部分实现 / 行为与目标有偏差（附任务 ID）· 🚧 进行中 · 📋 计划中 · ❌ 不做

---

## 1. 状态总览（先看这张表）

| 层面 | 状态 | 说明 |
| :--- | :--- | :--- |
| 异步**语法**（`async fn` / `spawn` / 前缀 `await` / 后缀 `.await`） | ✅ | 词法、语法、编译全部落地 |
| 异步**运行时** | ✅ | 单线程协作式事件循环：`spawn` 惰性创建任务，`await` 挂起/恢复（决策 D2，T4.1/T4.2） |
| 任务调度器 / 挂起 | ✅ | 每个任务保存自己的帧栈 + 寄存器窗口；就绪队列 FIFO + 真实定时器 |
| 非阻塞 `sleep_ms` | ✅ | 挂起到真实墙钟时刻，由事件循环统一等待（T4.3）；⚠️ 适配器回调内退化为阻塞（§4.3） |
| 并发汇聚 `await_all` / `await_any` | ✅ | 真实挂起；`await_all` 按实参顺序、`await_any` 返回最先结算者（T4.5） |
| 取消与超时 `cancel` / `timeout` | ✅ | 协作式取消（挂起点生效）+ 定时器超时（T4.5/T4.6） |
| 任务契约（错误传播 / 取消 / 泄漏） | ✅ | 见本文 §5（T4.6） |
| 宿主异步 API（`eval_async` / `register_async_fn`） | ❌ | 决策 D2 不引入 Tokio/Future：宿主异步工作需自行同步完成后用 `register_fn` 注入（T4.4 重定义） |
| CPU 并行（多线程/多核） | ❌ | 单线程模型，CPU 密集任务不会加速（§4.4） |

> **一句话**：`spawn`/`await` 现在是**真实的协作式并发**——I/O 型等待（`sleep_ms`、等待其它任务）会重叠，
> 但同一时刻只有一个任务在执行，且执行顺序是**确定的**。

---

## 2. 异步语法 ✅

### 2.1 异步函数

```ada
async fn fetch_service(name, latency_ms) {
    sleep_ms(latency_ms)
    $"Service [{name}] ready"
}

async fn fast(x) = x * 2          // 单行异步函数
```

- `async` 只能修饰 `fn`；`async` 与 `fn` 之间允许换行。
- `is_async` 标记被写入 `FunctionProto`，但运行时不区分：`async fn` 与 `fn` 的调用方式、返回值一致，
  唯一区别是**函数体内允许使用挂起点**（`sleep_ms`/`await`）——两者在普通 `fn` 里同样可用。

### 2.2 任务创建 `spawn`

```ada
let t1 = spawn fetch_service("auth", 5)
let t2 = spawn 100
let t3 = spawn { sleep_ms(20) 7 }
```

- `spawn` 是**前缀一元算子**，操作数按 Unary(14) 优先级解析。
  派生结论：`spawn f(1) + 2` 解析为 `(spawn f(1)) + 2`；而 `spawn f(1).await` 解析为
  `spawn (f(1).await)`（`.await` 是 Call(15) 级，会进入操作数）。
- 操作数可以是任意表达式：若是**调用**则创建"待执行调用"任务（T4.1）；若是其它表达式，
  则在 spawn 点求值并直接成为**已完成**任务。
- `spawn` 的结果是 `task` 类型的值（`type_of` 返回 `"task"`）。
- **关键语义**：`spawn` **不会**在生成点执行调用体；任务体在调度器首次运行它时才开始。

### 2.3 等待

```ada
let r1 = t1.await          // 后缀（推荐：链式平坦，无括号嵌套）
let r2 = await t2          // 前缀（兼容主流直觉）
let r3 = (await t3).field  // 链式属性访问
```

- **后缀**：`.await` 由「`.` 之后紧跟 `await` 记号」识别，是 `Call` 级优先级（15），可与 `.field` 混链。
- **前缀**：`await expr` 操作数按 Unary(14) 解析；`await a.b` 解析为 `await (a.b)`。
- ⚠️ 由于 `await` 是保留字，不能作为字段名/变量名（`.await` 是语法糖而非属性访问）。
- `await` 作用于**非 task 值**时原样返回（`await 5` → `5`）。
- `await` 一个已结算的任务是"快路径"：直接取结果，不产生挂起。

### 2.4 并发汇聚与任务方法

```ada
let scores = [spawn 100, spawn 200, spawn 300].await_all()
let first = [spawn slow(), spawn fast()].await_any()   // 最先结算者
let all = await_all([t1, t2])                          // 全局函数形式
let guarded = t1.timeout(1000)                         // 超时返回 nil 并取消任务
let ok = t1.cancel()                                   // 协作式取消，返回是否生效
```

| 形式 | 等价全局形式 | 语义 |
| :--- | :--- | :--- |
| `arr.await_all()` | `await_all(arr)` | 等全部结算，按**实参顺序**返回结果数组 |
| `arr.await_any()` / `arr.await_race()` | `await_any(arr)` | 返回**最先结算**者的值（失败即抛出） |
| `task.cancel()` | — | 取消任务，返回 `bool` |
| `task.timeout(ms)` | `timeout(task, ms)` | 期限内结算则返回其值，超时则取消任务并返回 `nil` |
| `task.state` | — | `pending`/`queued`/`running`/`waiting`/`done`/`failed`/`cancelled` |
| `task.id` / `task.label` | — | 任务号与显示名（诊断用） |

---

## 3. 实现语义（务必先读）✅

### 3.1 实际执行路径

| 环节 | 实际行为 |
| :--- | :--- |
| `spawn f(a, b)` 编译 | 生成 `Spawn{dst, callee, arg_start, arg_count}`：只求值**调用目标与实参**，不求值调用体 |
| `Spawn` 指令执行 | 创建 `TaskCell`（`Pending`）并放入就绪队列；立即返回 `task` 值 |
| `spawn <非调用表达式>` | `SpawnValue`：值在生成点求值，任务直接是 `Done` 状态 |
| 任务首次被调度 | 取 `Pending` 任务的调用与实参，压入该任务**独立的帧栈 + 寄存器窗口** |
| `await task` | 已结算 → 直接取结果；未结算 → 保存现场并挂起，任务结算时唤醒 |
| `await` 非 task | 原样返回 |
| `sleep_ms(n)` | 挂起到 `now + n`（真实墙钟），事件循环统一等待 |
| `await_all(arr)` | 未全部结算则挂起；全部结算后按实参顺序收集 `Array` |
| `await_any(arr)` | 等待**任一**结算，返回最先结算者的值 |
| `timeout(task, ms)` | 同时注册"任务结算"与"定时器到期"两条唤醒路径 |
| 调度顺序 | 就绪队列 FIFO；无就绪任务时把墙钟推进到最早定时器 |

### 3.2 可观测行为（实测）

```ada
import time from "std:time"
let t0 = time.now_ms()
let a = spawn { sleep_ms(200)
7 }
let b = spawn { sleep_ms(200)
8 }
println(a.await + b.await)     // 15
println(time.elapsed_ms(t0))   // ≈200  → 并发重叠（串行会是 ≈400）
```

| 行为 | 结果 |
| :--- | :--- |
| 并发加速（I/O 型） | N 个各 `sleep_ms(M)` 的任务总耗时 ≈ M（不是 N×M） |
| 顺序睡眠对照 | `sleep_ms(200); sleep_ms(200)` 仍是 ≈400ms |
| 错误传播 | 任务体错误在 `await` 处抛出 `TaskFailed{label, source}`；不在 `spawn` 点中断 |
| 取消 | `cancel()`/`timeout()` 使任务进入 `cancelled`；`await` 抛 `TaskCancelled` |
| 确定性与否 | 同一脚本每次运行的调度顺序一致（无随机、无抢断） |
| 宿主线程 | 主任务挂起期间**不占用** OS 线程做忙等（事件循环 `sleep` 到最早定时器） |

---

## 4. 调度与挂起模型 ✅（决策 D2）

```
                    ┌────────────────────────────────────────┐
   脚本 ──► compile ─►│  Register VM（单线程协作式调度器）      │
                    └───────────────┬────────────────────────┘
                                    │ 执行到挂起点
                 ┌──────────────────┴───────────────────┐
                 ▼                                      ▼
        [快路径：无需挂起]                     [慢路径：保存现场并让出]
   await 已结算 / sleep_ms(0) 等价        SavedExecution{frames, registers, resume}
   await_all 全部就绪                     换入下一个就绪任务
                                                     │
                                    无就绪任务 → sleep 到最早定时器
                                                     │
                                        定时器到期 / 被等待任务结算 → 唤醒
                                                     │
                                   恢复现场 → 补做挂起点收尾（结果写回）→ 继续
```

### 4.1 设计要点

1. **决定论优先**：单线程 + FIFO 就绪队列 + 真实定时器，执行顺序完全可复现
   （对比 Promise 微任务/多线程抢占的不可复现性）。这对 AI 生成代码的可测试性至关重要。
2. **零依赖**：不引入 Tokio/OS 线程，"任务"是同一 VM 上的一段可保存执行现场
   （`frames` + `registers`），切换成本只是一次 `Vec` 移动。
3. **挂起点白名单**：`sleep_ms`、`yield_now`、`await`、`await_all`、`await_any`、`timeout`。
   任意指令处**不会**被抢占（无信号、无时间片），因此共享可变状态不需要锁。
4. **定时器即墙钟**：`sleep_ms` 用真实 `u64` 毫秒时间戳，事件循环在没有就绪任务时
   `sleep` 到最早截止时间；并发睡眠因此只花最长时间。
5. **燃料与挂起的关系**：`fuel` 只在执行指令时计费；挂起等待不计费，
   恢复后继续计费——因此 `fuel` 仍能约束总计算量（见 §7）。

### 4.2 JIT 协同（Tier 2，📋 T6.4）

以 `.await` 为界把函数切分为同步基本块，块内生成原生机器码，`.await` 处生成 Trampoline；
未就绪则保存寄存器并回退到本调度器。当前先交付解释器路径，JIT 复用同一套 `SavedExecution`。

### 4.3 已知限制（务必阅读）

| 限制 | 原因 | 影响与规避 |
| :--- | :--- | :--- |
| **适配器回调内不能挂起** | 回调是 Rust 栈帧（`map`/`filter` 的闭包调用），无法保存宿主栈 | 回调内的 `sleep_ms` 退化为**真实阻塞**（结果正确，但不与其它任务重叠）；需要并发请在任务体内使用循环而非适配器回调 |
| **适配器回调内的网络调用退化为阻塞**（M9 / T9.2） | 同上：`rust_depth > 0` 时无法挂起 | 网络调用在**任务体内**是真正的挂起点（可与其它任务交错）；在适配器回调内则同步跑完当次操作（结果正确、不与其它任务重叠），并仍受网络策略的硬超时约束。这是 §5.5「阻塞禁令」要求显式标注的第二处阻塞原语 |
| **无 CPU 并行** | 单线程模型 | CPU 密集任务不会加速；需要并行应拆分为宿主 native fn |
| **无抢占** | 协作式 | 不含挂起点的死循环仍靠 `fuel` 熔断（见 §7） |
| **每任务一段寄存器窗口** | 现场保存需要 | 任务内存 ≈ 该任务的寄存器数 × `Value` 大小；由 `max_tasks` 限制（✅ M5/T5.3） |

### 4.4 为什么不用真 Fiber + Tokio（决策 D2 记录）

| 维度 | 单线程协作式（已选） | Fiber + Tokio |
| :--- | :--- | :--- |
| 依赖 | 无 | 强制宿主接受 async 运行时 |
| 确定性 | 完全确定 | 就绪顺序取决于调度器实现与计时 |
| 沙箱熔断 | 挂起点天然是墙钟检查点 | 需与 Tokio 计时器协作 |
| CPU 并行 | ❌ | ✅ |
| 嵌入成本 | 低（`Engine::eval` 同步可用） | 宿主必须 `block_on` 或 `#[tokio::main]` |

结论：0.1.0 的定位是**可嵌入、可预测**，故选择前者；真并行留待有明确宿主需求时按 D2b 重新评估。

> **D14 的修订（M9）**：上表的**任务模型**结论不变——字节码仍单线程执行、任务仍是
> `SavedExecution`、调度顺序仍由 FIFO 就绪队列决定。变化只在**网络 I/O 的承载**：
> socket/TLS/HTTP 交给一个跑在独立驱动线程上的 Tokio 运行时，VM 只在自己空闲等待时
> 收取完成事件。因此"依赖：无"这一行不再成立（`ada_vm` 现在依赖 tokio/hyper/rustls），
> 而"确定性"与"沙箱熔断"两行仍然成立。详见 SPEC_RUNTIME §10 与计划 §10 D14。

---

## 5. 任务生命周期契约 ✅（T4.6）

### 5.1 状态机

```
        spawn
          │
          ▼
      ┌────────┐  被调度器取出   ┌─────────┐  正常结束   ┌──────┐
      │Pending ├───────────────►│(Running)├───────────►│ Done │
      └───┬────┘  排队中         └────┬────┘            └──────┘
          │        ┌────────┐        │ 出错            ┌────────┐
          └───────►│ Queued │        └────────────────►│ Failed │
                   └───┬────┘                         └────────┘
                       │ 执行到挂起点
                       ▼
                  ┌─────────┐  被等待者结算 / 定时器到期
                  │ Waiting ├──────────────────────────► 回到 Running
                  └────┬────┘
                       │ cancel() / timeout()
                       ▼
                  ┌───────────┐
                  │ Cancelled │
                  └───────────┘
```

`task.state` 可观察：`pending`（创建后、入队前的瞬时态）、`queued`、`running`、`waiting`、
`done`、`failed`、`cancelled`。

### 5.2 错误传播

| 规则 | 条文 |
| :--- | :--- |
| 延迟到 await | 任务体内的错误**不会**在 `spawn` 点中断当前任务，而是记为该任务的终态 `Failed` |
| 抛出形态 | `await` / `await_all` / `await_any` 抛出 `VmError::TaskFailed { label, source }`，`source` 保留原始错误（如 `TypeError`），可用错误链读取 |
| 取消形态 | 被取消的任务抛 `VmError::TaskCancelled(label)`（不再包一层 `TaskFailed`） |
| 汇聚语义 | `await_all` 在**任一**任务失败时立即抛出该错误；`await_any` 由最先结算者决定（失败也算结算） |
| 未观察 | 任务失败但从未被 `await`：错误被丢弃，脚本正常结束（不静默改变结果） |

### 5.3 取消

| 规则 | 条文 |
| :--- | :--- |
| 协作式 | `cancel()` 在**挂起点**生效：排队中/等待中的任务立即进入 `cancelled`；正在运行的任务（只能是自身）在本轮执行结束后进入 `cancelled` |
| 返回值 | `cancel()` 返回 `bool`：已进入终态的任务返回 `false`（幂等，不报错） |
| 唤醒等待者 | 任务被取消会立即唤醒所有等待它的现场（`Await` → `TaskCancelled`；`timeout` → `nil`） |
| 超时 | `timeout(task, ms)` 到期时取消该任务并返回 `nil`；任务先结算则返回其值 |

### 5.4 泄漏与结构化并发

| 规则 | 条文 |
| :--- | :--- |
| 脚本结束 | **主任务结束即脚本结束**，尚未完成的任务被丢弃，其错误与结果都不可观察（与 Go 的 main 退出语义一致） |
| 不隐式等待 | 脚本不会为了后台任务而延长运行时间（`spawn sleep_ms(200)` 后立即返回，实测 < 100ms） |
| 跨脚本隔离 | 每次 `Vm::execute` 重置调度器：任务、定时器、等待关系都不跨脚本泄漏 |
| 死锁 | 所有任务都在等待且无定时器时抛 `VmError::TaskDeadlock`（例如任务等待自身），而不是挂起宿主 |

### 5.5 确定性

| 规则 | 条文 |
| :--- | :--- |
| 调度顺序 | 就绪队列 FIFO；`spawn` 按语句顺序入队 |
| 结果顺序 | `await_all` 结果顺序 = 实参顺序（不是完成顺序）；`await_any` = 最先结算者（结算序号裁定） |
| 无抢断 | 任务只在挂起点让出，因此共享状态无需加锁，读写顺序可推导 |
| 阻塞禁令 | 标准库不得新增阻塞原语；若必须阻塞（如适配器回调内的 `sleep_ms`、网络调用），必须在 §4.3 显式标注 |

---

## 6. 宿主异步 API ❌（T4.4 重定义）

按决策 D2，0.1.0 **不向宿主暴露** Tokio/Future 桥接，因此不提供 `Engine::eval_async` /
`register_async_fn`——宿主侧 API 仍是全同步的。

> ⚠️ **M9 / D14 的区分**：引擎**内部**从 M9 起用 Tokio 承载网络 I/O（`ada_net`，跑在
> 独立驱动线程上），但这不改变宿主 API 的形状：`Engine::eval` 依然同步、依然自带
> 调度器。也就是说"不引入 Tokio"这条已被 D14 修订为"**不把 Tokio 暴露给宿主**"，
> 而 `register_async_fn` 仍然不存在。

宿主的异步工作有两种接入方式：

```rust
// 1) 同步 native fn：宿主自行完成 I/O，然后注入结果（会阻塞调用线程）
engine.register_fn("http_get", |args| {
    let body = blocking_http(&args[0].to_string()).map_err(|e| e.to_string())?;
    Ok(Value::string(body))
});
```

```ada
// 2) 脚本侧并发：把"等待"表达为 sleep_ms / 任务依赖，由调度器重叠
let a = spawn http_get("https://a.example")
let b = spawn http_get("https://b.example")
let both = [a, b].await_all()
```

| API | 状态 |
| :--- | :--- |
| `Engine::register_async_fn` | ❌ 非目标（D2）：用 `register_fn` 同步注入 |
| `Engine::eval_async` | ❌ 非目标（D2） |
| `VmYield` / `AdaSuspendedFiber` | 📋 若将来需要"宿主事件循环驱动任务"，按 D2b 评估 |

---

## 7. 异步安全熔断

| 机制 | 现状 | 目标 |
| :--- | :--- | :--- |
| Fuel（指令步数） | ✅ 执行指令计费，挂起等待不计费，恢复后继续计费 | 与任务数配合（T5.3） |
| 墙钟超时（整体） | ✅ 已实现（M5/T5.4） | `Sandbox::with_wall_clock_limit`；指令循环每 4096 条检查一次，调度等待处也检查并裁剪睡眠 |
| 任务级超时 | ✅ `timeout(task, ms)`（T4.5） | — |
| 任务上限 | ✅ 已实现（M5/T5.3） | `max_tasks` 限制并发任务数，超限报 `TaskLimitExceeded` |
| 未处理任务 | ✅ 主任务结束即丢弃（§5.4），不泄漏到下一次执行 | — |

**双轨熔断原则**：`fuel` 防 CPU 死循环，墙钟超时防 I/O 永久挂起；两者缺一不可。
本模型下前者对纯计算有效，后者应在 `run_scheduler` 的等待分支实现（T5.4）。

---

## 8. 缺口汇总

| ID | 缺口 | 状态 | 任务 |
| :--- | :--- | :--- | :--- |
| G-A1 | `spawn` 立即求值，无并发 | ✅ 已修复（T4.1）：`Spawn` 携带 callee+args，惰性执行 | ✅ T4.1 |
| G-A2 | 无调度器 / 挂起模型 | ✅ 已修复（T4.2）：`SavedExecution` + 就绪队列 + 定时器 | ✅ T4.2 |
| G-A3 | `sleep_ms` 阻塞宿主线程 | ✅ 已修复（T4.3）：挂起到真实定时器，事件循环统一等待 | ✅ T4.3 |
| G-A4 | 无 `eval_async` / `register_async_fn` | ❌ 非目标（D2 不向宿主暴露 Tokio；M9/D14 起引擎内部用 Tokio 承载网络 I/O，但宿主 API 仍全同步） | T4.4 重定义 |
| G-A5 | `await_race` 语义错误；无 `timeout` | ✅ 已修复（T4.5）：`await_any` 返回最先结算者；`timeout(task, ms)` 可取消 | ✅ T4.5 |
| G-A6 | 任务契约（错误/取消/泄漏）未定义 | ✅ 已定稿（T4.6，§5） | ✅ T4.6 |
| G-A7 | 错误在 `spawn` 点同步抛出 | ✅ 已修复（T4.6）：延迟到 `await` 抛 `TaskFailed` | ✅ T4.6 |
| G-A8 | 示例输出易被误读为"并发" | ✅ 已修复：`showcase.ada` 增加确定性交错演示，`demo.ada` 增加并发耗时对照 | ✅ M4 |
| G-A9 | 适配器回调内无法挂起 | ⚠️ 真实限制（§4.3）：回调是 Rust 栈帧，无法保存现场 → `sleep_ms` 退化为阻塞。消除它需要重构回调模型（宿主事件循环驱动任务），暂无计划 | 📋 后续评估 |
| G-A10 | 无任务数量上限 / 整体墙钟预算 | ✅ 已修复（M5）：`max_tasks` 与 `max_wall_clock_ms` | ✅ T5.3 / T5.4 |
