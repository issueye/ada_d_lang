# Ada-D 异步与并发规范 (Async & Concurrency Specification)

> **版本**：1.0
> **权威范围**：异步语法、并发原语、调度与挂起模型、任务生命周期契约、宿主异步 API、异步熔断。
> **不在此文档**：语言通用语法见 [SPEC_LANGUAGE.md](SPEC_LANGUAGE.md)；指令集与 VM 结构见 [SPEC_RUNTIME.md](SPEC_RUNTIME.md)；`await_all`/`await_race` 的参数细节见 [SPEC_STDLIB §2.7](SPEC_STDLIB.md)；路线图见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。
> **状态图例**：✅ 已实现 · ⚠️ 部分实现 / 行为与目标有偏差（附任务 ID）· 🚧 进行中 · 📋 计划中 · ❌ 不做

---

## 1. 状态总览（先看这张表）

| 层面 | 状态 | 说明 |
| :--- | :--- | :--- |
| 异步**语法**（`async fn` / `spawn` / 前缀 `await` / 后缀 `.await`） | ✅ | 词法、语法、编译全部落地 |
| 异步**运行时** | ⚠️ | `spawn` 立即求值后包装为 `Task` 值；`await` 解包。**不存在并发、调度、挂起** |
| 任务调度器 / Fiber 挂起 | 📋 | 任务 T4.2 / 决策 D2 |
| 非阻塞 `sleep_ms` | ⚠️ | 当前阻塞宿主线程（T4.3） |
| 宿主异步 API（`eval_async` / `register_async_fn`） | 📋 | T4.4 |
| 超时熔断（`timeout`） | 📋 | T4.5 |
| 任务契约（错误传播 / 取消 / 泄漏） | 📋 | T4.6 |

> **重要**：当前 `spawn`/`await` 是**语法占位**，语义等价于同步求值。任何依赖"并发"的文档示例目前都得不到并发效果。

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
- `is_async` 标记被写入 `FunctionProto`，**但运行时不区分**：`async fn` 与 `fn` 的调用方式、返回值完全一致（T4.1 起才会有真实差异）。

### 2.2 任务创建 `spawn`

```ada
let t1 = spawn fetch_service("auth", 5)
let t2 = spawn 100
let t3 = spawn { sleep_ms(20) 7 }
```

- `spawn` 是**前缀一元算子**，操作数按 Unary(14) 优先级解析。
  派生结论：`spawn f(1) + 2` 解析为 `(spawn f(1)) + 2`（`+` 优先级 11 < 14，不进入操作数）。
- 操作数可以是任意表达式，包括块 `{ ... }`（块值即任务结果）。
- `spawn` 的结果是 `task` 类型的值（`type_of` 返回 `"task"`）。

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

### 2.4 并发汇聚

```ada
let scores = [spawn 100, spawn 200, spawn 300].await_all()
let all = await_all([t1, t2])       // 全局函数形式，可被 |> 管道调用
```

语义细节见 [SPEC_STDLIB §2.7](SPEC_STDLIB.md)。

---

## 3. 当前实现语义（务必先读）

### 3.1 实际执行路径 ⚠️

| 环节 | 实际行为 |
| :--- | :--- |
| `spawn expr` 编译 | 先编译 `expr` 到临时寄存器，再发 `Spawn{dst, expr}`；**`expr` 在 spawn 处就已完整求值** |
| `Spawn` 指令执行 | 把已求值的值包成 `Value::Task(Arc<RwLock<Value>>)` |
| `await` / `.await` 执行 | 读取 `Task` 内部值并返回；非 `Task` 原样返回 |
| `async fn` 调用 | 与普通函数完全相同（`is_async` 不参与执行） |
| 调度 | 无调度器、无队列、无 Future/Waker、无挂起点 |
| `sleep_ms(n)` | `std::thread::sleep`，**阻塞当前 OS 线程** |
| `await_all(arr)` | 遍历解包（因任务早已完成，等价拷贝） |
| `await_race(arr)` | 返回数组**首元素**的解包结果，与"就绪顺序"无关 |

### 3.2 可观测后果（实测）

```ada
let t0 = time.now_ms()
let a = spawn { sleep_ms(200)
7 }
let b = spawn { sleep_ms(200)
8 }
println(a.await + b.await)     // 15
println(time.elapsed_ms(t0))   // 401  → 串行，且阻塞宿主线程
```

| 后果 | 影响 |
| :--- | :--- |
| 无并发加速 | 任务总耗时 = 各任务之和 |
| 错误在 `spawn` 点同步抛出 | 无法在 `await` 处集中处理错误 |
| 无取消/超时 | 长任务无法中断，`fuel` 也管不到 `sleep_ms` |
| 阻塞宿主线程 | 在异步宿主（Tokio）中会阻塞 worker，与定位冲突 |
| `examples/demo.ada` 的 "Async result" 输出 | 顺序执行，不是并发结果 |

---

## 4. 目标语义：Fiber 挂起模型 📋

```
                            ┌──────────────────────────┐
   脚本 / 字节码  ──────────►│   Register VM (Tier 0)   │
                            └───────────┬──────────────┘
                                        │ 执行到 Await
                        ┌───────────────┴───────────────┐
                        ▼                               ▼
                  [Ready 快路径]                    [Pending 慢路径]
           直接写 dst 寄存器并继续执行      保存 frames + ip + 寄存器快照
             （无调度、无上下文切换）               打包为 AdaSuspendedFiber
                                                        │
                                           VmYield::Awaiting(host future)
                                                        │
                                             交还宿主（Tokio 事件循环）
                                                        │
                                                  Waker 唤醒后 resume
```

### 4.1 设计要点

1. **快路径优先**：Future 已就绪时（内存计算、缓存命中）直接写寄存器，不产生任何调度开销 ✅ 目标。
2. **挂起慢路径**：`Pending` 时把调用帧、`ip`、寄存器快照封存为轻量 Fiber，`Vm::execute` 返回 `VmYield::Awaiting(...)`，宿主在 Tokio 上注册 Waker。
3. **零线程阻塞**：挂起期间不占用 OS 线程（当前 `sleep_ms` 违反此原则）。
4. **指令改造**：`Instruction::Spawn` 需携带 `proto_idx + arg_start + arg_count`（T4.1），而非当前的"包装已求值寄存器"。
5. **错误传播**：任务内的错误需延迟到 `await` 点抛出（需在 T4.6 定义具体形态）。

### 4.2 JIT 协同（Tier 2，📋 T6.4）

以 `.await` 为界把函数切分为同步基本块，块内生成原生机器码，`.await` 处生成 Trampoline 检查 Future 是否就绪，未就绪则保存寄存器并回退到 VM 调度器。

---

## 5. 任务生命周期契约 📋（T4.6 定稿）

| 议题 | 待定规则（草案） |
| :--- | :--- |
| **错误传播** | 任务内的错误应捕获并挂起，在最近一次 `await` 或 `await_all` 处抛出；避免错误在 `spawn` 点同步中断其余任务 |
| **取消** | `timeout(ms)` 超时、`await_race` 落败的任务应被取消并释放资源，避免任务泄漏 |
| **结构化并发** | 脚本结束前未 `await` 的任务需被明确处理（隐式等待或显式取消），不得静默遗留 |
| **阻塞禁令** | 标准库中任何阻塞原语必须改为挂起形式；若必须阻塞，需在规范中显式标注为"阻塞 API" |
| **确定性** | `await_all` 结果顺序必须与输入顺序一致；`await_race` 返回"首个就绪"而非"首个元素" |

> 这些规则在 T4.6 完成后从"草案"转为正式条文，并同步更新 §1 状态表。

---

## 6. 宿主异步 API 📋（T4.4）

```rust
use ada_engine::{Engine, Value};

#[tokio::main]
async fn main() -> Result<(), ada_engine::Error> {
    let mut engine = Engine::new();

    // 注册原生异步函数（内部可 await reqwest/tokio::fs 等）
    engine.register_async_fn("http_get", |args: Vec<Value>| async move {
        let url = args[0].to_string();
        let body = reqwest::get(&url).await.map_err(|e| e.to_string())?
            .text().await.map_err(|e| e.to_string())?;
        Ok(Value::string(body))
    });

    let len: i64 = engine.eval_async(r#"
        let html = http_get("https://example.com").await
        html.len
    "#).await?;

    println!("{len}");
    Ok(())
}
```

| API | 状态 |
| :--- | :--- |
| `Engine::register_async_fn` | 📋 |
| `Engine::eval_async` | 📋 |
| `VmYield` / `AdaSuspendedFiber` | 📋 |

当前替代方案：宿主的异步工作必须**先同步完成**，再用 `register_fn` 注入结果（会阻塞调用线程）。

---

## 7. 异步安全熔断

| 机制 | 现状 | 目标 |
| :--- | :--- | :--- |
| Fuel（指令步数） | ✅ 可用，但**无法中断 `sleep_ms`/原生阻塞调用** | 与挂起模型配合，挂起不计费、恢复继续计费 |
| 墙钟超时 | 📋 不存在（T5.4） | `tokio::time::timeout(engine.eval_async(..))` 或内置执行超时 |
| 任务上限 | 📋 不存在 | 限制并发任务数，防任务风暴 |
| 未处理任务 | 📋 无策略 | 按 §5 契约处理 |

**双轨熔断原则**：`fuel` 防 CPU 死循环，墙钟超时防 I/O 永久挂起；两者缺一不可（当前只有前者，且对阻塞调用无效）。

---

## 8. 缺口汇总

| ID | 缺口 | 状态 | 任务 |
| :--- | :--- | :--- | :--- |
| G-A1 | `spawn` 立即求值，无并发 | ⚠️ | T4.1 / D2 |
| G-A2 | 无调度器 / Fiber 挂起 / `VmYield` | 📋 | T4.2 |
| G-A3 | `sleep_ms` 阻塞宿主线程 | ⚠️ | T4.3 |
| G-A4 | 无 `eval_async` / `register_async_fn` | 📋 | T4.4 |
| G-A5 | `await_race` 语义错误；无 `timeout` | ⚠️ | T4.5 |
| G-A6 | 任务契约（错误/取消/泄漏）未定义 | 📋 | T4.6 |
| G-A7 | 错误在 `spawn` 点同步抛出，无法集中处理 | ⚠️ | T4.6 |
| G-A8 | `examples/demo.ada` 的异步输出易被误读为并发 | ⚠️ | T2.2/M4 完成后更新示例注释 |
