# Ada-D 运行时规范 (Runtime Specification)

> **版本**：1.0
> **权威范围**：编译流水线、字节码指令集、编译器策略、VM 执行模型、值模型、数值与算术语义、Fuel 与资源熔断、错误模型、宿主嵌入 API、沙箱权限模型、性能策略。
> **不在此文档**：语言语法与求值语义见 [SPEC_LANGUAGE.md](SPEC_LANGUAGE.md)；标准库 API 见 [SPEC_STDLIB.md](SPEC_STDLIB.md)；异步调度见 [SPEC_ASYNC.md](SPEC_ASYNC.md)；路线图见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。
> **状态图例**：✅ 已实现 · ⚠️ 部分实现（附任务 ID）· 🚧 进行中 · 📋 计划中 · ❌ 不做

---

## 1. 编译与执行流水线

```
.ada 源文件
   │  ada_lexer::Lexer           手写扫描器 → Vec<Token>（含 ASI Newline）
   │  ada_parser::Parser         递归下降 + Pratt → ada_ast::Program
   │  ada_bytecode::Compiler     单遍编译 → FunctionProto（寄存器字节码）
   │  ada_vm::Vm::execute        帧栈解释执行 → Value
   └  ada_engine::Engine         宿主门面（register_fn / set_fuel / eval<T>）
```

| 阶段 | Crate | 状态 |
| :--- | :--- | :--- |
| 词法 | `ada_lexer` | ✅ |
| 语法 | `ada_parser` | ✅ |
| 字节码编译 | `ada_bytecode` | ✅ |
| 解释执行 | `ada_vm` | ✅ |
| 宿主嵌入 | `ada_engine` | ⚠️（无沙箱分级/无资源预算，T5.x） |
| 命令行 | `ada_cli` | ✅（`run` / `-e` / `dump` / `repl`） |
| JIT | — | 📋（T6.4，尚无 crate） |

**设计约束**：依赖方向单向 `ast ← lexer ← parser ← bytecode ← vm ← engine ← cli`，禁止反向依赖（`ada_vm` 仅在 dev-dependencies 依赖 `ada_parser` 供测试）。

---

## 2. 值模型 (Value Model)

### 2.1 当前实现 ✅（enum + Arc）

```rust
enum Value {
    Nil, Bool(bool), Int(i64), Float(f64),
    String(Arc<String>),
    Array(Arc<RwLock<Vec<Value>>>),
    Map(Arc<RwLock<HashMap<String, Value>>>),
    Closure(Arc<Closure>),
    NativeFn { name: String, callback: Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync> },
    Task(Arc<RwLock<Value>>),
    Iterator(Arc<RwLock<IteratorState>>),
}
```

| 特性 | 状态 |
| :--- | :--- |
| 引用语义（array/map 赋值共享同一容器） | ✅ |
| 跨线程可传递（`Send + Sync`，用于宿主 `register_fn`） | ✅ |
| 循环引用回收 | ❌ 依赖 `Arc`，循环引用**泄漏**（决策 D6） |
| NaN-boxing 64 位无装箱 | 📋 未实现（决策 D1；[WHITEPAPER](WHITEPAPER.md) §6 已将其明确标注为规划项，非现状） |

### 2.2 值语义要点

- `array` / `map` / `iterator` / `task` 为**引用类型**：`mut a = [1]; mut b = a; b.push(2)` 后 `a` 也变为 `[1,2]`。
- `string` 也是 `Arc<String>`，但字符串不可变，表现为值语义。
- 相等语义见 [SPEC_LANGUAGE §9.1](SPEC_LANGUAGE.md)。

### 2.3 已知性能特征 ⚠️（任务 T6.2）

| 观察点 | 影响 |
| :--- | :--- |
| 容器访问走 `RwLock` 加解锁 | 每次索引/字段访问都有原子操作开销 |
| `GetField` 每访问一次方法（如 `arr.push`）就**新建一个 `NativeFn` 闭包** | 方法链/循环内高频访问产生持续分配 |
| 指令派发时 `instructions[ip].clone()` | 每条指令一次克隆（多数变体很小，但属可省开销） |
| 每次指令都做一次 `Option<fuel>` 分支 | 可预测分支，成本低但非零 |

---

## 3. 字节码指令集

`FunctionProto` 结构 ✅：

| 字段 | 说明 |
| :--- | :--- |
| `name` / `arity` / `is_async` | 函数元信息；`arity` 用于调用时**精确**参数个数校验 |
| `register_count` | 本帧需要的寄存器数（`max_reg`） |
| `instructions: Vec<Instruction>` | 指令序列 |
| `constants: Vec<Constant>` | 常量池（Int/Float/String/Bool/Nil），`add_constant` 去重 |
| `names: Vec<String>` | 全局名与字段名池，`add_name` 去重 |
| `child_protos: Vec<FunctionProto>` | 嵌套函数/闭包原型 |
| `spans: Vec<Span>` | 与 `instructions` 一一对应的源码位置 ✅（但 VM 报错尚未使用，T1.8） |

指令表（`crates/ada_bytecode/src/instruction.rs`）：

| 分类 | 指令 |
| :--- | :--- |
| 载入/搬运 | `LoadConst{dst,const_idx}` `LoadNil{dst}` `LoadBool{dst,val}` `LoadInt{dst,val:i32}` `Move{dst,src}` |
| 算术 | `Add` `Sub` `Mul` `Div` `Mod` `{dst,lhs,rhs}` |
| 一元 | `Neg` `Not` `BitNot` `{dst,src}` |
| 比较 | `Eq` `Ne` `Lt` `Le` `Gt` `Ge` `{dst,lhs,rhs}` |
| 位运算 | `BitAnd` `BitOr` `BitXor` `Shl` `Shr` `{dst,lhs,rhs}` |
| 跳转 | `Jump{offset}` `JumpIfFalse{cond,offset}` `JumpIfTrue{cond,offset}` `JumpIfNil{cond,offset}` |
| 调用 | `Call{dst,callee,arg_start,arg_count}` `Return{src:Option<Reg>}` |
| 全局 | `GetGlobal{dst,name_idx}` `SetGlobal{name_idx,src}` |
| 容器 | `NewArray{dst,start,count}` `NewMap{dst}` `MapSet{map,key,val}` `GetIndex{dst,target,index}` `SetIndex{target,index,val}` `GetField{dst,target,field_idx,optional}` `SetField{target,field_idx,val}` |
| 字符串 | `StringConcat{dst,start,count}` |
| 闭包 | `Closure{dst,proto_idx}` |
| 异步 | `Await{dst,future}` `Spawn{dst,expr}` |
| 迭代 | `IterInit{dst,src}` `IterNext{dst,iter,offset}` |

**待扩展**（任务 T2.2）：`GetUpvalue` / `SetUpvalue`（闭包捕获）、带 upvalue 描述的 `Closure` 变体。
**待改造**（任务 T4.1）：`Spawn` 需携带 `proto_idx + arg_start + arg_count`。

---

## 4. 编译器策略

### 4.1 寄存器分配 ✅

- 入口 `Compiler::new(name, arity)`：`next_reg = arity`，即**参数固定占用 `R0..R(arity-1)`**。
- `alloc_reg()` 顺序分配并维护 `max_reg`；上限 10,000（超出报 `TooManyRegisters`）。
- `free_reg()` **仅当释放的是栈顶寄存器时**才回退 `next_reg`，否则不回收（保守策略，寄存器数只会增长到峰值）。
- 一元/二元运算的操作数是临时寄存器，运算后释放；`dst` 由调用方提供。

### 4.2 作用域与局部变量 ✅

- `locals: Vec<Local{name, reg, depth}>` + `scope_depth`；`resolve_local` 逆序查找最近声明。
- 进入块 `enter_scope()`，退出 `exit_scope()` 弹出并释放 `depth > scope_depth` 的局部变量。
- 名字未命中局部变量则编译为 `GetGlobal`（缺失在运行时报 `UndefinedGlobal`）。

### 4.3 函数与闭包 ✅⚠️

```ada
fn f(x) = ...        // 编译为 child_proto + Closure{dst,proto_idx}
                     // 顶层函数额外发一条 SetGlobal（因此脚本内函数是全局的）
fn outer(a) { let g = x => x + a }   // ⚠️ a 无法被 g 捕获（无 upvalue，T2.2）
```

| 规则 | 状态 |
| :--- | :--- |
| 顶层 `fn` 同时写全局 | ✅ |
| 块内 `fn` 仅注册为局部变量 | ✅ |
| 闭包捕获外层局部变量 | ⚠️ 未实现（T2.2） |
| 调用参数数量精确匹配（无默认值/可变参数） | ✅（不匹配报 `ArityMismatch`） |

### 4.4 控制流编译 ✅

- `while`：条件 → `JumpIfFalse` 出口 → 体 → `Jump` 回边；`break` 收集为待回填跳转；`continue` 直接跳到条件起点。
- `for`：`IterInit` 一次 → 循环头 `IterNext{dst,offset}`（耗尽时按 offset 跳出口）→ 体 → `Jump` 回边；`[k,v]` 解构编译为 `LoadInt` + `GetIndex`。
- `if` / `??` / `&&` / `||`：条件跳转 + 回填偏移（`&&`/`||` 真短路，返回操作数值而非布尔）。
- `match`：逐 arm 生成 比较/`Eq` + `JumpIfFalse` 跳过，命中体后 `Jump` 到统一出口；无 arm 命中时 `dst` 保持未赋值（T3.2）。

### 4.5 已知编译器缺口

| ID | 缺口 | 任务 |
| :--- | :--- | :--- |
| G-R1 | 无 upvalue 表 | T2.2 |
| G-R2 | 顶层 `let` 不写全局 → REPL 跨行不持久 | T2.4 |
| G-R3 | `is_mut` 被丢弃，不可变性不校验 | T3.1 |
| G-R4 | 插值片段独立重新解析，span 相对片段 | T3.4 |
| G-R5 | `match` 大整数字面量模式截断为 `i32` | T3.7 |
| G-R6 | 游离 `break`/`continue` 静默忽略；闭包内 `break` 不作用于外层循环 | T3.7 |
| G-R7 | `main` 的返回寄存器恒为 `R0` | 已知设计，不改 |

---

## 5. VM 执行模型

### 5.1 状态 ✅

```rust
struct Vm {
    registers: Vec<Value>,        // 初始 2048 槽，按需 resize(needed * 2)
    frames: Vec<CallFrame>,       // { proto, ip, base_reg, dst_reg }
    globals: HashMap<String, Value>,
    fuel: Option<u64>,
}
```

| 行为 | 状态 | 说明 |
| :--- | :--- | :--- |
| 调用帧 | ✅ | 扁平寄存器窗口：`new_base = caller.base_reg + caller.register_count` |
| 帧数上限 | ✅ | 512 帧，超出报 `StackOverflow`（`fib(600)` 触发，干净报错） |
| 叶帧结束隐式返回 `nil` | ✅ | |
| 全局变量跨 `execute()` 保留 | ✅ | 同一 `Vm`/`Engine` 可多轮求值 |
| 顶层 `let` 跨轮次保留 | ⚠️ | 不保留（G-R2 / T2.4） |
| 寄存器窗口回收 | ⚠️ | **从不回收**；`registers` 只增不减（T6.2） |
| `execute()` 重置帧 | ✅ | 每次清空 `frames` |
| `execute()` 重置 fuel | ⚠️ | **不重置**（T2.6） |

### 5.2 调用约定 ✅

- `Call{dst,callee,arg_start,arg_count}`：实参必须**连续排布**在 `arg_start..arg_start+arg_count`（编译器在实参求值后回退 `next_reg` 来保证）。
- `Closure` 调用：校验 `arity == arg_count` → 扩容寄存器 → 拷贝实参到新帧 `R0..` → 压帧。
- `NativeFn` 调用：直接把寄存器切片交给 Rust 回调，返回值写入 `dst`。
- `Return{src}`：弹出帧，把返回值写回**调用者**的 `dst_reg`；帧栈空则整体返回。

---

## 6. 数值与算术语义

### 6.1 类型组合矩阵（当前实现）

| 运算 | Int ⊕ Int | Float ⊕ Float | Int ⊕ Float | String 参与 |
| :--- | :--- | :--- | :--- | :--- |
| `+` | Int，溢出报 `TypeError`（`checked_add`） | Float | Float | ✅ 任一侧为 String 则字符串拼接（`1 + "a"` → `"1a"`） |
| `-` `*` | Int，溢出报 `TypeError`（`checked_sub` / `checked_mul`） | Float | Float | ❌ `TypeError` |
| `/` | 整除则 Int，否则 Float；`/0` → `TypeError`；`i64::MIN / -1` → `TypeError`(overflow) | Float；**`x/0.0` 得 `inf`/`NaN`（不报错）⚠️** | Float | ❌ `TypeError` |
| `%` | `%0` → `TypeError`；`i64::MIN % -1` → `TypeError`(overflow)；混合类型不支持 ⚠️ | Float | ❌ `TypeError`（`5.0 % 2` 报错）⚠️ | ❌ `TypeError` |
| 比较 `< <= > >=` | bool | bool | bool | 类型不匹配**静默返回 `false`** ⚠️ |
| 位运算 `& \| ^` | Int | ❌ `TypeError` | ❌ `TypeError` | ❌ `TypeError` |
| 移位 `<< >>` | Int；移位量不在 `0..64`（含负数）→ `TypeError` | ❌ | ❌ | ❌ |
| 一元 `-` | Int；`-(i64::MIN)` → `TypeError`(overflow) | Float | — | ❌ |
| 一元 `!` | 真值取反 → Bool | 同 | 同 | 同 |
| 一元 `~` | Int | ❌ | ❌ | ❌ |

### 6.2 目标语义与任务

| ID | 问题 | 目标 | 任务 |
| :--- | :--- | :--- | :--- |
| G-R8 | 整数溢出导致宿主 panic（debug）/ 静默回绕（release） | ✅ 已修复：`+ - * / %` 与一元 `-` 全部走 `checked_*`，溢出返回 `TypeError`（决策 D5 = A，T1.2） | ✅ T1.2 |
| G-R9 | `% 0` panic | ✅ 已修复：返回 `TypeError("Modulo by zero")`，与 `/` 一致（T1.1） | ✅ T1.1 |
| G-R10 | 移位越界 panic | ✅ 已修复：校验 `0 <= n < 64`，否则 `TypeError`（T1.3） | ✅ T1.3 |
| G-R11 | `x % y` 混合类型不支持 | 提升为 Float 后取模 | T3.5 |
| G-R12 | `x / 0.0` 静默得 `inf` | 明确定义（保留 IEEE 语义或报错） | T3.5 |
| G-R13 | 比较类型不匹配静默 `false` | 明确定义（建议报错） | T3.5 |

> **规范红线**：任何脚本输入都不得让宿主进程 panic（开发计划 §5 门禁第 3 条）。当前 4 类 panic 使这条红线处于**未达成**状态。

---

## 7. Fuel 与资源熔断

### 7.1 Fuel ✅⚠️

| 能力 | 状态 |
| :--- | :--- |
| `Vm::set_fuel(Some(n))` / `Engine::set_fuel` | ✅ |
| 每执行一条指令扣减 1，归零返回 `VmError::OutOfFuel` | ✅ |
| 死循环可被中断（`while true {}` 实测可中断） | ✅ |
| **每次 `execute()` 重新装填** | ⚠️ 未实现：fuel 原地递减且不重置，一次耗尽后同一 `Engine` 永久失效（T2.6） |
| `remaining_fuel` / `reset_fuel` 查询接口 | 📋（T2.6） |
| 对阻塞型原生函数（`sleep_ms`）计费 | ❌ 无法中断（T4.3 / T5.4） |

### 7.2 其他资源限制

| 能力 | 状态 | 任务 |
| :--- | :--- | :--- |
| 调用栈深度上限（512 帧） | ✅ | — |
| 寄存器数上限（10,000/帧） | ✅ | — |
| 内存预算 `MaxMemoryLimit` | 📋 | T5.3 |
| 墙钟超时（wall-clock timeout） | 📋 | T5.4 |
| 宿主 panic 兜底（`catch_unwind`） | 📋 | T1.7 |

---

## 8. 错误模型

| 错误类型 | 变体 | 触发 |
| :--- | :--- | :--- |
| `LexError` | `UnexpectedChar` `UnterminatedString` `UnterminatedBlockComment` `InvalidNumber` `UnterminatedInterpExpr` | 词法阶段 |
| `ParseError` | `UnexpectedToken` `UnexpectedEof` `InvalidAssignmentTarget` `Lex(..)` | 语法阶段 |
| `CompileError` | `VariableNotFound` `TooManyRegisters` `InvalidAssignmentTarget` | 编译阶段 |
| `VmError` | `OutOfFuel` `StackOverflow` `UndefinedGlobal` `NotCallable` `ArityMismatch` `TypeError` `IndexOutOfBounds` `NativeError` | 运行阶段 |
| `ada_engine::Error` | `Parse` `Compile` `Vm` `Conversion` | 宿主门面统一封装 |

| 规范要求 | 状态 |
| :--- | :--- |
| 所有错误携带源码位置（`file:line:col`） | ⚠️ `Span` 已存在但 VM 错误未附带（T1.8） |
| CLI 输出统一格式 | ⚠️ 当前仅 `Error: {Debug 形式}`（T0.6） |
| 库错误不得 panic | ⚠️ 未达成（§6 的 G-R8~G-R10、字符串索引 panic） |

---

## 9. 宿主嵌入 API

### 9.1 `Engine` 能力清单

| API | 状态 | 说明 |
| :--- | :--- | :--- |
| `Engine::new()` | ✅ | 创建 VM 并注册全部内置函数与模块 |
| `set_global(name, Value)` / `get_global(name)` | ✅ | 注入/读取全局 |
| `register_fn(name, cb)` | ✅ | 注册原生函数；签名 `Fn(&[Value]) -> Result<Value, String> + Send + Sync` |
| `set_fuel(Option<u64>)` / `get_fuel()` | ✅ | 指令熔断 |
| `eval_value(source) -> Value` | ✅ | 求值并返回原生值 |
| `eval::<T: FromValue>(source)` | ✅ | 泛型转换返回 |
| 公共类型 re-export（`Value` / `Closure` / `VmError` / `ParseError`） | ⚠️ | **未导出**：`use ada_engine::{Engine, Value}` 编译失败（E0603），宿主必须自行依赖 `ada_vm`（任务 T0.7） |
| `set_memory_limit` | 📋 | T5.3 |
| `set_sandbox_profile` | 📋 | T5.1 |
| `register_module`（`NativeModule` trait） | 📋 | T5.5 |
| `eval_async` / `register_async_fn` | 📋 | T4.4 |
| 执行超时 | 📋 | T5.4 |

### 9.2 `FromValue` 实现 ✅

| 目标类型 | 转换规则 |
| :--- | :--- |
| `Value` | 原样 |
| `i64` | `as_int()`（Float 会**静默截断**） |
| `f64` | `as_float()` |
| `bool` | 仅接受 `Bool`，否则 `Error::Conversion` |
| `String` | `String` 取值；其它类型走 `Display`（不失败） |
| `serde_json::Value` | `Value::to_json()`；闭包/原生函数/迭代器会被降级为字符串占位 |

### 9.3 嵌入示例（可直接编译）

```rust
use ada_engine::Engine;
use ada_vm::Value;   // ⚠️ 当前必须显式依赖 ada_vm：ada_engine 尚未 re-export Value（任务 T0.7）

fn main() -> Result<(), ada_engine::Error> {
    let mut engine = Engine::new();
    engine.set_fuel(Some(50_000));
    engine.register_fn("calculate_tax", |args: &[Value]| {
        let income = args.first().and_then(|v| v.as_float()).unwrap_or(0.0);
        let rate = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.1);
        Ok(Value::Float(income * rate))
    });

    let tax: f64 = engine.eval(r#"
        let t = calculate_tax(10000.0, 0.2)
        t
    "#)?;
    println!("{tax}");   // 2000
    Ok(())
}
```

> 该示例已在集成测试中验证可编译可运行（`cargo test -p ada_engine`）。T0.7 完成后可简化为 `use ada_engine::{Engine, Value};`。

> ⚠️ 当前 `set_fuel` 是一次性配额（§7.1），宿主若在同一 `Engine` 上连续求值，需自行重建 `Engine` 或等待 T2.6。

---

## 10. 沙箱与权限模型

### 10.1 现状 ⚠️（等同于"无沙箱"）

| 面 | 现状 | 风险 |
| :--- | :--- | :--- |
| 模块可见性 | `json` / `math` / `time` / `sys` / `env` **全部默认可用** | 无法做纯计算隔离 |
| `env.get` | 直接读宿主环境变量，**无白名单**（实测可读 `PATH`、`USERPROFILE`） | 敏感信息泄漏 |
| 内存 | 无预算 | 可被超大容器耗尽 |
| 阻塞 | `sleep_ms` 阻塞宿主线程 | 可拖死调用方 |
| 崩溃 | 脚本可触发 panic（§6） | 宿主进程崩溃 |
| 文件/网络 | 未提供 | 暂无越权面 |

### 10.2 目标模型 📋

三档 profile（任务 T5.1）：

| Profile | 允许 | 用途 |
| :--- | :--- | :--- |
| `Strict`（默认） | 纯计算 + `json` + `math` + 字符串/集合原型 | AI 生成代码的默认执行环境 |
| `Standard` | `Strict` + `time` + `env` 白名单 | 需要时间/配置的宿主 |
| `Custom` | 宿主显式注册的模块集合 | 业务扩展 |

配套：`MaxMemoryLimit`（T5.3）、跳墙钟超时（T5.4）、`NativeModule` trait（T5.5）、权限测试集（T5.6）。

---

## 11. 性能策略

### 11.1 当前实测基线

| 指标 | 数值（debug 构建，Windows） |
| :--- | :--- |
| `fib(27)`（≈63.5 万次调用） | 667 ms |
| 进程启动 + 求值 `1` | 21 ms |
| `examples/demo.ada` 全流程 | < 1 s（含两次 `sleep_ms`） |

> ⚠️ **当前无基准套件**，以上为一次性手测数据，不构成性能承诺。任务 T6.1 建立 `criterion` 基准后，所有性能表述改为引用基准结果。

### 11.2 分层执行策略

| 层 | 形态 | 状态 |
| :--- | :--- | :--- |
| Tier 0 · 解释器 | 寄存器字节码 VM，毫秒级启动 | ✅ |
| Tier 1 · VM 微优化 | 寄存器窗口回收、常量折叠、指令融合 | 📋 T6.2 |
| Tier 2 · JIT | Cranelift IR 下沉 + 热点计数 + guard/deopt | 📋 T6.4（`ada_jit` crate 尚不存在） |

### 11.3 已知可优化点

1. `GetField` 每次访问方法都新建闭包 → 方法表缓存（T6.2）。
2. 容器 `RwLock` 访问开销 → 单线程路径用 `RefCell`/无锁句柄（需与 D6 内存模型一并决策）。
3. 指令派发克隆 → 改为按引用匹配或预解码（T6.2）。
4. 寄存器窗口不回收 → 添加窗口栈回收（T6.2）。
5. 值模型仍是 enum → NaN-boxing 可减少拷贝与内存（决策 D1，须以基准数据支撑）。
