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
   │  Engine::resolve_imports    文件 import：加载 + 声明合并（受 root 边界约束，T8.7/T8.8）
   │  ada_bytecode::Compiler     单遍编译 → FunctionProto；`std:` 导入编为 ImportModule/ImportNamed（T8.14）
   │  ada_vm::Vm::execute        帧栈解释执行 → Value；导入指令调 `std_module` 注册内置模块
   └  ada_engine::Engine         宿主门面（register_fn / set_fuel / set_import_root / eval / eval_file）
```

| 阶段 | Crate | 状态 |
| :--- | :--- | :--- |
| 词法 | `ada_lexer` | ✅ |
| 语法 | `ada_parser` | ✅ |
| 字节码编译 | `ada_bytecode` | ✅ |
| 解释执行 | `ada_vm` | ✅ |
| 宿主嵌入 | `ada_engine` | ✅ 沙箱分级 + 资源预算 + 诊断位置（M5/T1.8） |
| 命令行 | `ada_cli` | ✅（`run` / `-e` / `dump` / `repl`） |
| JIT | — | 📋（T6.4，尚无 crate） |

**设计约束**：依赖方向单向 `ast ← lexer ← parser ← bytecode ← vm ← engine ← cli`，禁止反向依赖（`ada_vm` 仅在 dev-dependencies 依赖 `ada_parser` 供测试）。

---

## 2. 值模型 (Value Model)

### 2.1 当前实现 ✅（enum + Arc，`Value` = 16 字节）

```rust
enum Value {
    Nil, Bool(bool), Int(i64), Float(f64),
    String(Arc<String>),
    Array(Arc<RwLock<Vec<Value>>>),
    Map(Arc<RwLock<HashMap<String, Value>>>),
    Closure(Arc<Closure>),
    NativeFn(Arc<NativeFunction>),      // 载荷：{ name: String, callback: Arc<dyn Fn…> }
    Intrinsic(Arc<IntrinsicCall>),      // 载荷：{ kind, receiver: Option<Box<Value>>, args: Vec<Value> }
    Cell(Arc<RwLock<Value>>),
    Task(Arc<TaskCell>),
    Async { kind: AsyncKind, receiver: Option<Arc<Value>> },
    Iterator(Arc<RwLock<IteratorState>>),
}
```

| 特性 | 状态 |
| :--- | :--- |
| `size_of::<Value>()` | ✅ **16 字节**（T6.2b；此前 48 字节，见 §2.3） |
| 引用语义（array/map 赋值共享同一容器） | ✅ |
| 跨线程可传递（`Send + Sync`，用于宿主 `register_fn`） | ✅ |
| 循环引用回收 | ❌ 依赖 `Arc`，循环引用**泄漏**（决策 D6；由 M5 内存预算兜底） |
| NaN-boxing 64 位无装箱 | ❌ 不做（决策 D1：`Value` 已降到 16 字节，收益不足以抵消全量重写） |

### 2.2 值语义要点

- `array` / `map` / `iterator` / `task` 为**引用类型**：`mut a = [1]; mut b = a; b.push(2)` 后 `a` 也变为 `[1,2]`。
- `string` 也是 `Arc<String>`，但字符串不可变，表现为值语义。
- 相等语义见 [SPEC_LANGUAGE §9.1](SPEC_LANGUAGE.md)。

### 2.3 布局与性能特征

**为什么是 16 字节（T6.2b）**：`Value` 曾是 48 字节，由两个"胖变体"决定——
`NativeFn { name: String(24B), callback: Arc<dyn Fn>(16B) }` 与
`Intrinsic { receiver: Option<Box<Value>>, args: Vec<Value>(24B) }`。
VM 的寄存器文件是 `Vec<Value>`（2048 槽），48 字节意味着 **96 KiB**，远超典型 32 KiB 的 L1
数据缓存；每次寄存器读写都在打内存层次。把两者的载荷改为 `Arc<NativeFunction>` /
`Arc<IntrinsicCall>` 后枚举降到 16 字节，寄存器文件缩到 **32 KiB**（刚好放进 L1），
且 `Option<Value>` 仍是 16 字节（Niche 优化）。

- 实测（criterion，见 §11.3）：迭代器管道 **−34.7%**、numeric_loop −9.8%、fib −10.1%、
  冷启动 −11.7%；**指令数基线逐字节不变**（只改表示、不改工作量）。
- 工具：`cargo run -p ada_vm --example size_probe` 打印各尺寸与寄存器文件占用。
- 门禁：`crates/ada_vm/tests/value_layout.rs` 固定"`Value` 必须 ≤16 字节 / 寄存器文件必须放得进 L1"，
  再往 `Value` 里塞大字段会直接失败。

| 其余观察点 | 影响 |
| :--- | :--- |
| 容器访问走 `RwLock` 加解锁 | 每次索引/字段访问都有原子操作开销 |
| `GetField` 每访问一次方法（如 `arr.push`）会新建一个捕获接收者的 `NativeFn` | ✅ 已缓解（T6.8）：方法分派缓存让同一接收者上的重复访问只做一次 `Value` 克隆；受控 A/B 见 §11.3 |
| 指令派发时 `instructions[ip].clone()` | 每条指令一次克隆（`Instruction` 仅 12 字节，成本可忽略，见 §11.3） |

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
| `spans: Vec<Span>` | 与 `instructions` 一一对应的源码位置 ✅（VM 用它定位运行时错误，T1.8） |

指令表（`crates/ada_bytecode/src/instruction.rs`）：

| 分类 | 指令 |
| :--- | :--- |
| 载入/搬运 | `LoadConst{dst,const_idx}` `LoadNil{dst}` `LoadBool{dst,val}` `LoadInt{dst,val:i32}` `Move{dst,src}` |
| 算术 | `Add` `Sub` `Mul` `Div` `Mod` `{dst,lhs,rhs}` |
| 一元 | `Neg` `Not` `BitNot` `{dst,src}` |
| 比较 | `Eq` `Ne` `Lt` `Le` `Gt` `Ge` `{dst,lhs,rhs}` |
| **常量融合（T6.6）** | `BinaryConst{dst,op,src,value:i32}`：`dst = src <op> value`，`op` ∈ 算术与比较。编译器把"一侧是 i32 整数字面量"的表达式编成这一条（可镜像时自动换向：`2 < x` → `Gt`），省掉一条 `LoadInt` 与一次派发；语义与通用指令逐字一致（差分测试固定） |
| 位运算 | `BitAnd` `BitOr` `BitXor` `Shl` `Shr` `{dst,lhs,rhs}` |
| 跳转 | `Jump{offset}` `JumpIfFalse{cond,offset}` `JumpIfTrue{cond,offset}` `JumpIfNil{cond,offset}` `MatchFail`（T3.2：非穷尽 match 的显式失败） |
| **条件融合（T6.7）** | `JumpIfNotCompare{op,lhs,rhs,offset}` 与 `JumpIfNotCompareConst{op,src,value,offset}`：`if`/`while`/`match` guard 的条件是比较表达式时，编译器直接发出"为假则跳转"，省掉比较指令、临时寄存器与一次 truthiness 读取。可镜像时自动换向（`while 0 < i` → `Gt`），不可镜像（`2 - x` 之类）不融合 |
| 调用 | `Call{dst,callee,arg_start,arg_count}` `Return{src:Option<Reg>}` |
| 全局 | `GetGlobal{dst,name_idx}` `SetGlobal{name_idx,src,kind:GlobalWrite}`（T8.3：`Assign`/`DeclareMut`/`DeclareImmutable`） |
| 模块导入（T8.14） | `ImportModule{module_idx,alias_idx}`（命名空间：注册整模块）、`ImportNamed{module_idx,member_idx,alias_idx}`（具名：绑定单个函数）——VM 执行时调 `builtins::std_module`（permissive + 引擎内存预算） |
| 容器 | `NewArray{dst,start,count}` `NewMap{dst}` `MapSet{map,key,val}` `GetIndex{dst,target,index}` `SetIndex{target,index,val}` `GetField{dst,target,field_idx,optional}` `SetField{target,field_idx,val}` |
| 字符串 | `StringConcat{dst,start,count}` |
| 闭包 | `Closure{dst,proto_idx}`（无捕获）、`MakeClosure{dst,proto_idx,upvalue_start,upvalue_count}`（带捕获） |
| 捕获槽（T2.2） | `MakeCell{dst,src}` `CellGet{dst,cell}` `CellSet{cell,src}` `UpvalueGet{dst,idx}` `UpvalueSet{idx,src}` |
| 异步 | `Await{dst,future}` `Spawn{dst,callee,arg_start,arg_count}` `SpawnValue{dst,src}` |
| 任务调度（M4） | 无新指令：调度由 VM 的 `run_scheduler` 与 `SavedExecution`（帧栈 + 寄存器现场）完成 |
| 迭代 | `IterInit{dst,src}` `IterNext{dst,iter,offset}` |

`FunctionProto.upvalues: Vec<UpvalueDesc>` 描述本函数每个捕获的来源
（`ParentLocal(reg)` = 父帧局部槽；`ParentUpvalue(idx)` = 父帧自己的第 idx 个捕获）。

**已落地**（M4 / T4.1）：`Spawn` 携带 `callee + arg_start + arg_count`，在生成点**不执行**调用体；
非调用表达式走 `SpawnValue`（立即完成的任务）。调度模型与任务契约见 [SPEC_ASYNC](SPEC_ASYNC.md)。

---

## 4. 编译器策略

### 4.1 寄存器分配 ✅

- 入口 `Compiler::new(name, arity)`：`next_reg = arity`，即**参数固定占用 `R0..R(arity-1)`**。
- `alloc_reg()` 顺序分配并维护 `max_reg`；上限 10,000（超出报 `TooManyRegisters`）。
- `free_reg()` **仅当释放的是栈顶寄存器时**才回退 `next_reg`，否则不回收（保守策略，寄存器数只会增长到峰值）。
- 一元/二元运算的操作数是临时寄存器，运算后释放；`dst` 由调用方提供。

### 4.2 作用域与局部变量 ✅

- `locals: Vec<Local{name, reg, depth, boxed, is_mut}>` + `scope_depth`；`resolve_local` 逆序查找最近声明，
  返回 `(寄存器, 是否装箱, 是否可写)`。
- **不可变性（T3.1）**：`is_mut == false` 的局部变量在赋值目标位置直接报 `CompileError::AssignToImmutable`；
  被闭包捕获的变量把可变性一并传给子编译器（`outer_names: HashMap<String, bool>`），因此闭包内也不可写。
- 进入块 `enter_scope()`，退出 `exit_scope()` 弹出并释放 `depth > scope_depth` 的局部变量。
- 名字未命中局部变量则编译为 `GetGlobal`（缺失在运行时报 `UndefinedGlobal`）。

### 4.3 函数与闭包 ✅⚠️

```ada
fn f(x) = ...        // 编译为 child_proto + Closure{dst,proto_idx}
                     // 顶层函数额外发一条 SetGlobal（因此脚本内函数是全局的）
fn outer(a) { let g = x => x + a; g(5) }   // a 被装箱为 Cell，g 通过捕获共享读写
```

| 规则 | 状态 |
| :--- | :--- |
| 顶层 `fn` 同时写全局 | ✅ |
| 块内 `fn` 仅注册为局部变量 | ✅ |
| 闭包捕获外层局部变量 | ✅（T2.2）：捕获槽为 `Value::Cell`，内外部共享读写 |
| 多级捕获传递 | ✅：子函数报告捕获**名字**，父层解析来源并可继续向上物化（支持"中间层不直接引用"） |
| 调用参数数量精确匹配（无默认值/可变参数） | ✅（不匹配报 `ArityMismatch`） |

**装箱判定（编译器侧）**：编译期为每个函数体（含模块单元）预扫描"嵌套函数中出现过的标识符"，
命中同名局部变量即装箱；这是上近似，宁可多装箱也不会漏（漏装箱会让捕获退化为值拷贝）。

**每轮循环变量**：`for` 的变量被捕获时，编译器在循环体内每次迭代重新发一条 `MakeCell`，
因此每个迭代闭包持有独立槽。

### 4.4 控制流编译 ✅

- `while`：条件 → `JumpIfFalse` 出口 → 体 → `Jump` 回边；`break` 收集为待回填跳转；`continue` 直接跳到条件起点。
- `for`：`IterInit` 一次 → 循环头 `IterNext{dst,offset}`（耗尽时按 offset 跳出口）→ 体 → `Jump` 回边；`[k,v]` 解构编译为 `LoadInt` + `GetIndex`。
- `if` / `??` / `&&` / `||`：条件跳转 + 回填偏移（`&&`/`||` 真短路，返回操作数值而非布尔）。
- `match`：逐 arm 生成 比较/`Eq` + `JumpIfFalse` 跳过，命中体后 `Jump` 到统一出口；无 arm 命中时 `dst` 保持未赋值（T3.2）。

### 4.5 已知编译器缺口

| ID | 缺口 | 任务 |
| :--- | :--- | :--- |
| G-R1 | 无 upvalue 表 | ✅ 已修复（T2.2）：`FunctionProto.upvalues` + `MakeCell`/`CellGet`/`CellSet`/`UpvalueGet`/`UpvalueSet`/`MakeClosure` | ✅ T2.2 |
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
| 帧携带捕获 | ✅ | 每个 `CallFrame` 保存该闭包的捕获表（`captures: Vec<Value>`）；主帧为空，`UpvalueGet`/`UpvalueSet` 从中读取 |
| 帧数上限 | ✅ | 512 帧，超出报 `StackOverflow`（`fib(600)` 触发，干净报错） |
| 叶帧结束隐式返回 `nil` | ✅ | |
| 全局变量跨 `execute()` 保留 | ✅ | 同一 `Vm`/`Engine` 可多轮求值 |
| 顶层 `let` 跨轮次保留 | ✅ | 保留（T2.4）：顶层声明同时写入全局表，同一个 `Engine`/REPL 中可见 |
| 寄存器窗口回收 | ⚠️ | **从不回收**；`registers` 只增不减（性能特征，非缺陷：2048 × 16 B = 32 KiB，见 §11.3） |
| `execute()` 重置帧 | ✅ | 每次清空 `frames` |
| `execute()` 重置 fuel | ✅ | **每次执行都从配置预算重新装填**（T2.6）：一次 `OutOfFuel` 不会让同一个 `Vm`/`Engine` 永久失效 |
| 燃料可观测 | ✅ | `remaining_fuel()` 查询剩余；`reset_fuel()` 手动装填；`get_fuel()` 返回配置预算 |

### 5.2 调用约定 ✅

- `Call{dst,callee,arg_start,arg_count}`：实参必须**连续排布**在 `arg_start..arg_start+arg_count`（编译器在实参求值后回退 `next_reg` 来保证）。
- `Closure` 调用：校验 `arity == arg_count` → 扩容寄存器 → 拷贝实参到新帧 `R0..` → 压帧。
- `NativeFn` 调用：直接把寄存器切片交给 Rust 回调，返回值写入 `dst`。
  ⚠️ 回调签名只有 `&[Value]`，**无法回调脚本闭包**——这是迭代器适配器必须由 VM 而非 builtins 实现的原因。
- `Intrinsic` 调用（T2.1）：把接收者与已绑定实参拼到调用实参之前，交给 `Vm::invoke_adapter`。
- **可重入调用**：`Vm::call_callable(callee, args)` 支持从 VM 内部（适配器回调）调用任意可调用值。
  对闭包会压入一个合成帧（寄存器窗口从 `registers.len()` 起分配），用 `run_loop_until(stop_depth)`
  执行到该帧返回为止，随后无条件恢复帧栈与寄存器长度（即使中途报错）。
- `Return{src}`：弹出帧，把返回值写回**调用者**的 `dst_reg`；帧栈空则整体返回。

---

## 6. 数值与算术语义

### 6.1 类型组合矩阵（当前实现）

| 运算 | Int ⊕ Int | Float ⊕ Float | Int ⊕ Float | String 参与 |
| :--- | :--- | :--- | :--- | :--- |
| `+` | Int，溢出报 `TypeError`（`checked_add`） | Float | Float | ✅ 任一侧为 String 则字符串拼接（`1 + "a"` → `"1a"`） |
| `-` `*` | Int，溢出报 `TypeError`（`checked_sub` / `checked_mul`） | Float | Float | ❌ `TypeError` |
| `/` | 整除则 Int，否则 Float；`/0` → `TypeError`；`i64::MIN / -1` → `TypeError`(overflow) | Float；`x/0.0` 得 `inf`/`NaN`（IEEE-754，决策见 §6.3） | Float | ❌ `TypeError` |
| `%` | `%0` → `TypeError`；`i64::MIN % -1` → `TypeError`(overflow) | Float | ✅ 提升为 Float 取模（`5.0 % 2` → `1.0`，T3.5） | ❌ `TypeError` |
| 比较 `< <= > >=` | bool | bool | bool | ✅ **只接受数值**：非数值操作数报 `TypeError`（T3.5）；`NaN` 比较恒为 `false` |
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
| G-R11 | `x % y` 混合类型不支持 | ✅ 已修复（T3.5）：混合数值类型提升为 Float 取模 | ✅ T3.5 |
| G-R12 | `x / 0.0` 静默得 `inf` | ✅ 已定案（T3.5）：**整数除零报错、浮点遵循 IEEE**（`inf`/`NaN`）——见 §6.3 | ✅ T3.5 |
| G-R13 | 比较类型不匹配静默 `false` | ✅ 已修复（T3.5）：非数值操作数报 `TypeError`，`NaN` 比较恒为 false | ✅ T3.5 |

### 6.3 浮点除零为何不报错（决策记录）

`fuel`/沙箱的目标是"明确且可预期"，而不是"所有异常都中断"。整数是精确类型，
`1 / 0` 无定义，因此报错；`f64` 遵循 IEEE-754，`1.0 / 0.0 = inf` 是标准且可检测
（`is_finite()`）的结果，`math.INF` 也把它作为一等值暴露。二者在规范里分别写清，
避免"看起来像错误却被静默吞掉"的情况（这也正是 §6.1 比较运算改为报错的原因）。

> **规范红线**：任何脚本输入都不得让宿主进程 panic（开发计划 §5 门禁第 3 条）。当前 4 类 panic 使这条红线处于**未达成**状态。

---

## 7. Fuel 与资源熔断

> 能力开关（哪些宿主模块可见、`env` 白名单、三档 profile）见 [§10 沙箱与权限模型](#10-沙箱与权限模型)；
> 本节只讲**资源限额**：燃料、内存、墙钟、任务数。

### 7.1 限额总览（M5 / T5.3 / T5.4）✅⚠️

| 限额 | 配置项 | 默认值 | 强制点 |
| :--- | :--- | :--- | :--- |
| 指令燃料 | `fuel` / `set_fuel` | 不限 | 每条指令扣 1，归零 `OutOfFuel` |
| 内存（近似） | `max_memory_bytes` | strict 16 MiB / standard 64 MiB | `MemoryBudget`：VM 指令（容器/字符串/`+`/适配器物化）+ 原生原型（`push`/`insert`/`split`）+ 迭代器物化**前**的长度预检查 |
| 墙钟 | `max_wall_clock_ms` | strict 5 s / standard 不限 | 每 4096 条指令检查一次；**调度器等待处**也检查并把睡眠裁剪到剩余预算 |
| 并发任务数 | `max_tasks` | strict 128 / standard 512 | `spawn` 时统计存活任务；每次 spawn 另计入内存 |
| 调用栈深度 | 固定 | 512 帧 | 既有 |
| 每帧寄存器数 | 固定 | 10,000 | 既有 |

| 能力 | 状态 | 说明 |
| :--- | :--- | :--- |
| 超限错误可断言 | ✅ | `MemoryLimitExceeded { limit, requested }`、`WallClockLimitExceeded { limit_ms }`、`TaskLimitExceeded { limit }`；原生原型路径为 `NativeError("memory limit exceeded: …")` |
| 大对象预检查 | ✅ | `range(0, 1e8) \|> collect()` 在分配前被拒（实测拒绝耗时 < 200ms，无内存峰值） |
| `json.parse`/`stringify` 字节上限 | ✅ | 与内存预算同值，避免单条原生指令吃掉全部预算 |
| 计量精度 | ❌ | 不做分配器级精确计量：宿主源码文本按上限处理，预算是**上界约束** |

> **计量语义（重要）**：预算是**上界约束**，不是精确账本。它的保证是：脚本无法在不被拒绝的
> 前提下，用容器增长、字符串拼接、迭代器物化或任务创建把宿主内存/时间耗尽。

### 7.2 Fuel ✅

| 能力 | 状态 |
| :--- | :--- |
| `Vm::set_fuel(Some(n))` / `Engine::set_fuel` / `Sandbox::with_fuel` | ✅ |
| 每执行一条指令扣减 1，归零返回 `VmError::OutOfFuel` | ✅ |
| 死循环可被中断（`while true {}` 实测可中断） | ✅ |
| 每次 `execute()` 重新装填（一次 `OutOfFuel` 不会永久失效） | ✅（T2.6） |
| `remaining_fuel` / `reset_fuel` 查询接口 | ✅（T2.6） |
| 与墙钟互补（fuel 管 CPU、墙钟管 I/O 与挂起等待） | ✅（M4/M5） |

### 7.3 内存与墙钟限额（M5 / T5.3 / T5.4）✅⚠️

| 能力 | 状态 | 说明 |
| :--- | :--- | :--- |
| 内存预算 `max_memory_bytes` | ✅⚠️ | **近似计量**：VM 在 `NewArray`/`MapSet`/`SetIndex`/字符串拼接（`StringConcat` 与 `+`）/适配器物化处计费，原生原型（`push`/`insert`/`split`）通过共享的原子计数器计费 |
| 大对象预检查 | ✅ | 迭代器物化**前**按长度估算拒绝（`range(0, 1e8) \|> collect()` 不会先分配），因此拒绝是瞬时且无内存峰值的 |
| `json.parse`/`stringify` 字节上限 | ✅ | 与内存预算同值，避免"一条原生指令吃掉全部预算" |
| 超限错误 | ✅ | VM 路径 `VmError::MemoryLimitExceeded { limit, requested }`；原生原型路径 `NativeError("memory limit exceeded: …")` |
| 墙钟预算 `max_wall_clock_ms` | ✅ | 每 4096 条指令检查一次；**调度器等待处也检查**，并把睡眠裁剪到剩余预算 |
| 超限错误 | ✅ | `VmError::WallClockLimitExceeded { limit_ms }` |
| 任务数上限 `max_tasks` | ✅ | `spawn` 时检查存活任务数；每次 spawn 也计入内存 |
| 调用栈深度上限（512 帧）/ 寄存器数上限（10,000/帧） | ✅ | 既有 |
| 精确分配器级计量 | ❌ | 不做：计量是"放大路径覆盖"而非逐字节精确（宿主源码文本、`json` 输入等按上限处理） |
| 宿主 panic 兜底（`catch_unwind`） | ✅ | T1.7：转为 `VmError::HostPanic`，保留 panic 消息与失败位置 |

> **计量语义（重要）**：预算是**上界约束**，不是精确账本。它的保证是：脚本无法在不被拒绝的
> 前提下，用容器增长、字符串拼接、迭代器物化或任务创建把宿主内存/时间耗尽。

---

## 8. 错误模型

| 错误类型 | 变体 | 触发 |
| :--- | :--- | :--- |
| `LexError` | `UnexpectedChar` `UnterminatedString` `UnterminatedBlockComment` `InvalidNumber` `UnterminatedInterpExpr` | 词法阶段 |
| `ParseError` | `UnexpectedToken` `UnexpectedEof` `InvalidAssignmentTarget` `Lex(..)` | 语法阶段 |
| `CompileError` | `VariableNotFound` `TooManyRegisters` `InvalidAssignmentTarget` **`AssignToImmutable`** | 编译阶段（T3.1：向 `let` 绑定赋值） |
| `VmError` | `OutOfFuel` `StackOverflow` `UndefinedGlobal` `NotCallable` `ArityMismatch` `TypeError` `IndexOutOfBounds` `NativeError` `NoMatch` `TaskFailed` `TaskCancelled` `TaskDeadlock` **`CapabilityDenied` `MemoryLimitExceeded` `WallClockLimitExceeded` `TaskLimitExceeded` `HostPanic`** | 运行阶段（T3.2 非穷尽 `match`；M4 任务错误；M5 沙箱与资源限额；T1.7 宿主 panic 兜底） |
| `ada_engine::Error` | `Parse` `Compile` `Vm` `Conversion` **`ModuleConflict`** | 宿主门面统一封装 |
| `EvalFailure` | `{ error: Error, position: Option<Position> }` | 求值失败 + 位置（T1.8，见 §8.2/§9.2） |

| 规范要求 | 状态 |
| :--- | :--- |
| 所有错误携带源码位置（`line:col`） | ✅（T1.8）词法/语法用词法位置；编译错误用 `CompileError::span`；运行时错误用"失败指令"的 span；任务失败用**任务自己**的位置。仅少数调度器层面错误（死锁/墙钟/任务数上限）与 `TooManyRegisters` 无位置可归因 |
| CLI 输出统一格式（T0.6 + T1.8） | ✅ 单行 `[file:]line:col: error: <消息>`；确实无位置时退化为 `file: error: <消息>`（不再输出 Debug 形式）；`crates/ada_cli/tests/cli_errors.rs` 固定该契约 |
| 库错误不得 panic | ✅ 脚本输入不再触发宿主 panic（M1 清零 + M5 敌意输入回归）；宿主回调 panic 由 `catch_unwind` 兜底转为 `VmError::HostPanic`（T1.7） |

> **T1.7 适用前提**：兜底依赖 Rust 的 `panic = "unwind"`（默认）。若宿主在 profile 里设了
> `panic = "abort"`，`catch_unwind` 无法拦截，panic 仍会终止进程。
> 另外，宿主代码在持有内部 `RwLock` 写锁时 panic 会**中毒**该锁，因此收到 `HostPanic` 后
> 应丢弃并重建 `Engine`/`Vm`（规范如此建议，测试只断言"不崩溃 + 错误种类"）。

### 8.1 CLI 错误格式 ✅（T0.6 / T1.8）

```
file:line:col: error: <消息>      # 位置已知（绝大多数错误）
file: error: <消息>              # 位置无法归因（少数调度器层面错误）
```

- 一律**单行**（编辑器/工具可直接切分），`file` 为脚本路径，`-e` 求值时是 `<eval>`。
- 消息用 `Display`（人话）而非 `Debug`：例如
  `Cannot assign to immutable variable 'a' (declare it with `mut`)`、
  `No `match` arm matched the value (non-exhaustive match); add a `_` arm`、
  `Capability 'time' is not available in the 'strict' sandbox profile`。
- 实际输出示例（每类错误都指向真正出问题的位置）：

```
script.ada:1:13: error: unexpected token <EOF>, expected expression   # 语法
script.ada:2:1: error: Cannot assign to immutable variable 'a' …      # 编译
script.ada:2:9: error: Type error: Division by zero                   # 运行时
script.ada:2:5: error: Task 'boom' failed: Type error: Division by zero  # 任务内部
```

### 8.2 位置从哪来 ✅（T1.8）

| 错误类别 | 位置来源 |
| :--- | :--- |
| 词法 / 语法 | `LexError::position()` / `ParseError::position()`（词法阶段记录） |
| 编译 | `CompileError::span()`：编译器在报错点取当前表达式的 `Span`（除 `TooManyRegisters`） |
| 运行时 | `FunctionProto.spans`（与 `instructions` 同步维护）+ 失败指令下标。循环在派发前已把 `ip` 自增，所以失败指令是 `ip - 1`；VM 在帧栈被清空前（`drive_current` 的错误分支）记录 |
| 任务失败 | 任务**自己**的失败位置（`TaskInner.error_span`）：`await` 那一行不是真正的失败点，报错指向任务内部 |
| 适配器回调内失败 | 归到调用该适配器的那一行（回调是 Rust 栈帧里的可重入调用，无法归属到闭包内部） |
| 宿主 panic（T1.7） | 展开不会清空帧栈，因此仍能补记失败指令位置 |

位置经 `Engine::eval_diagnostic` 以 `EvalFailure::position` 交付（见 §9.2）；
`Error::position()` 只覆盖自带 span 的类别（词法/语法/编译），运行时位置需要 VM 状态。

---

## 9. 宿主嵌入 API

### 9.1 `Engine` 能力清单

| API | 状态 | 说明 |
| :--- | :--- | :--- |
| `Engine::new()` | ✅ | **最小权限**（strict）：只有纯计算 + `json`/`math`（M5） |
| `Engine::with_sandbox(Sandbox)` / `permissive()` | ✅ | 指定沙箱档位 / 旧的全内置行为（M5） |
| `sandbox()` / `set_sandbox(Sandbox)` | ✅ | 读取 / 运行期替换沙箱（替换会重建全局表） |
| `memory_used()` | ✅ | 本次求值已计费的近似字节数（M5 / T5.3） |
| `set_global(name, Value)` / `get_global(name)` | ✅ | 注入/读取全局 |
| `register_fn(name, cb)` | ✅ | 注册原生函数；签名 `Fn(&[Value]) -> Result<Value, String> + Send + Sync` |
| `register_module(NativeModule)` | ✅ | 注册宿主自定义模块；与内置同名时返回 `Error::ModuleConflict`（M5 / T5.5） |
| `set_fuel(Option<u64>)` / `get_fuel()` | ✅ | 设置/读取燃料预算（每次 `eval` 自动重新装填，T2.6） |
| `remaining_fuel()` / `reset_fuel()` | ✅ | 查询剩余燃料 / 手动装填（T2.6） |
| `eval_value(source) -> Result<Value, Error>` | ✅ | 求值并返回原生值（不含位置） |
| `eval_diagnostic(source) -> Result<Value, EvalFailure>` | ✅ | 求值并在失败时附带**位置**（T1.8，见 §9.2） |
| `Engine::check(source) -> Result<(), EvalFailure>` | ✅ | **静态检查**：只做词法/语法/编译检查，**不执行**脚本（T7.2；LSP 与 `ada check` 的入口） |
| `eval::<T: FromValue>(source)` | ✅ | 泛型转换返回 |
| 公共类型 re-export | ✅ | `use ada_engine::{Engine, Value, VmError, Sandbox, ...}` 即可，无需直接依赖 `ada_vm`（T0.7） |
| `eval_async` / `register_async_fn` | ❌ | 不做：D2 选择单线程协作式调度，宿主侧用 `Vm`/脚本内 `spawn` 即可（T4.4） |
| 执行超时 | ✅ | 经 `Sandbox::with_wall_clock_limit`（M5 / T5.4）而非独立 API |

### 9.2 诊断信息 ✅（T1.8）

```rust
pub struct Position { pub line: usize, pub col: usize }   // 均从 1 开始
pub struct EvalFailure { pub error: Error, pub position: Option<Position> }

let mut engine = Engine::new();
match engine.eval_diagnostic(source) {
    Ok(value) => { /* ... */ }
    Err(failure) => {
        // failure.error 保留错误种类，宿主可继续 match
        // failure.to_string() 给出 "line:col: <消息>"
        eprintln!("{failure}");
    }
}
```

- `eval_value` 保持原签名（只返回 `Error`），老代码不受影响；两者共享同一条实现路径。
- `Error::position()` 覆盖自带 span 的类别（词法/语法/编译）；运行时位置由 `EvalFailure` 提供。
- 位置归属规则见 §8.2；`crates/ada_engine/tests/diagnostics.rs` 固定这些行为。

### 9.3 `FromValue` 实现 ✅

| 目标类型 | 转换规则 |
| :--- | :--- |
| `Value` | 原样 |
| `i64` | `as_int()`（Float 会**静默截断**） |
| `f64` | `as_float()` |
| `bool` | 仅接受 `Bool`，否则 `Error::Conversion` |
| `String` | `String` 取值；其它类型走 `Display`（不失败） |
| `serde_json::Value` | `Value::to_json()`；闭包/原生函数/迭代器会被降级为字符串占位 |

### 9.4 嵌入示例（可直接编译）

```rust
use ada_engine::{Engine, Value};

fn main() -> Result<(), ada_engine::Error> {
    let mut engine = Engine::new();          // strict：只放行纯计算 + json/math
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

> `Engine` 只依赖 `ada_engine` 即可（`Value`/`VmError`/`Sandbox` 等已 re-export，T0.7）；
> 该示例由文档示例门禁编译验证（`crates/ada_engine/tests/doc_examples.rs`）。

> 需要宿主能力（`time`/`env`/`sys`/`sleep_ms`）时用 `Engine::permissive()` 或
> `Engine::with_sandbox(Sandbox::standard())`；`set_fuel` 是**每次求值**的配额，
> 每次 `eval*` 自动重新装填（T2.6）。

---

## 10. 沙箱与权限模型

### 10.1 现状 ✅（M5 落地；T7.5 追加 `fs`/`crypto`）

| 面 | 实现 | 强制点 |
| :--- | :--- | :--- |
| 模块可见性（T8.14，D12） | **所有内置模块默认不注册**；脚本必须 `import X from "std:X"`。`fs` 例外：`with_fs_root` 时按根目录注册 | `builtins::register_all`（仅核心函数）+ VM 执行 `ImportModule`/`ImportNamed` 时调 `builtins::std_module` |
| 未导入 / 越权 | 未导入时模块名 → `UndefinedGlobal` / `CapabilityDenied`；`sleep_ms` 等仍由沙箱拒绝（`CapabilityDenied { name, profile }`，`ImportModule` 由 VM 执行 | `Instruction::GetGlobal` 拒绝名单 + import 注册 |
| `env` 读取（D12） | `import env` 即**完全授权**：可读全部键（等价 `create_env_module(None)`）；旧白名单路径保留于 `Sandbox`（供 `register_all` 的历史路径），但默认注册已移除 | `builtins::std_module` |
| 文件系统（T7.5） | **默认无 `fs`**；只有 `Sandbox::with_fs_root(dir)`（CLI：`--fs-root DIR`）才注册，且所有路径被规范化到根目录内（`..`、绝对路径、符号链接、前缀混淆均拒绝），单次 I/O 有字节上限 | `builtins::fs::resolve_within` + 模块注册开关 |
| 网络 | 未提供任何 API（`http` 明确不做，见决策 D9） | — |
| 内存 | `MemoryBudget`（近似、共享原子计数器）+ 大对象分配前预检查 | VM 指令 + 原生原型 + 迭代器物化 |
| 时间/挂起 | 墙钟预算在指令循环与调度等待处检查；睡眠裁剪到剩余预算 | `Vm::check_wall_clock` |
| 崩溃 | 脚本输入不再触发宿主 panic（M1 清零），敌意输入返回 `Err`；宿主回调 panic 由 `catch_unwind` 兜底（T1.7） | 11 条敌意用例 + `host_panic.rs` |

### 10.2 三档 profile 与用途 ✅

| Profile | 允许 | 用途 |
| :--- | :--- | :--- |
| `strict()`（`Engine::new()` 默认） | 全局函数 + 迭代器 + 语言特性 + 任务；**无 `sleep_ms`**；模块均需 `import`（D12） | AI 生成代码的默认执行环境 |
| `standard()` | strict + `sleep_ms`；模块仍均需 `import`（D12，沙箱不再决定模块可见性） | 需要时间/配置的宿主、CLI 默认档 |
| `permissive()` | 全部 + `sleep_ms`；模块仍均需 `import` | 受信场景、兼容性开关、测试 |
| 自定义 builder | `with_fs_root` 是唯一开启 `fs`（按根目录注册）的途径；其余模块靠 `import` | 业务按需授权 |

配套：`max_memory_bytes`（T5.3）、`max_wall_clock_ms`（T5.4）、`max_tasks`、`with_fs_root`（T7.5）、
`NativeModule`/`register_module`（T5.5）、
安全测试集 `crates/ada_engine/tests/security.rs`（T5.6，21 用例）与
`crates/ada_engine/tests/fs_sandbox.rs`（T7.5，10 用例）。

### 10.3 权限设计原则

1. **默认拒绝**：宿主能力必须显式授予；`Engine::new()` 不给出任何宿主面。
2. **可区分错误**：能力被关闭 ≠ 名字不存在（`CapabilityDenied` vs `UndefinedGlobal`），便于宿主与 AI 修正。
3. **不触碰即不泄漏**：`env` 未授权键在读取前就被拦截，避免"读了再判断"的侧信道与日志泄漏。
4. **限额是上界**：内存/墙钟是近似约束，保证的是"无法在不被拒绝的前提下耗尽宿主"，而不是精确计量。
5. **宿主注入优先**：`register_fn`/`register_module` 表示宿主显式授权，不受 profile 限制，但不得覆盖已有全局。
6. **越权要报错，不要静默**：`fs` 的越界路径返回错误而不是 `nil`/`false`，否则攻击会伪装成"什么都没发生"；
   能力被关闭时同样是显式错误（`CapabilityDenied`）。

---

## 11. 性能策略

### 11.1 实测基线（M6 / T6.1；数字为 T6.2 优化后的当前值）✅

基准套件位于 `benches/`（独立包，`criterion`；`cargo bench --manifest-path benches/Cargo.toml`），
程序源在 `benches/programs/*.ada`，被两组度量共用：

| 基准程序 | 侧重 | release 墙钟（中位，Windows） | 指令数（确定性） |
| :--- | :--- | :--- | :--- |
| `fib.ada`（fib(24)，46,368 次调用） | 调用/闭包捕获 | 20.3 ms | **1,275,420** |
| `numeric_loop.ada`（20 万次迭代） | 整数算术 | 15.4 ms | **1,400,009** |
| `string_concat.ada`（2 万次 `+=`） | 字符串分配 | ~116 ms（O(n²) memcpy 主导，噪声大） | 100,010 |
| `iterator_pipeline.ada`（20 万元素管道） | 适配器 + 闭包回调 | 46.9 ms | **1,100,009** |
| `json_roundtrip.ada`（3000 次解析） | 原生模块 + 容器 | 11.0 ms | 45,012 |
| `while_loop.ada`（30 万次 while 迭代） | while 条件 + 计数器（T6.7） | 15.1 ms | 2,400,011 |
| `method_calls.ada`（6 万次方法调用） | 方法分派缓存（T6.8） | 26.6 ms | 1,140,199 |
| 冷启动（编译 + 执行 `numeric_loop`） | 嵌入成本 | 13.4 ms | — |
| 仅编译 `numeric_loop` | 前端成本 | 4.4 µs | — |

> ⚠️ **测量纪律**：共享/开发机上的墙钟抖动实测可达 ±10%（`string_concat` 尤其明显，且它是
> O(n²) 的 memcpy 主导，不能代表 VM 开销）。因此**CI 门禁用指令数**（完全确定），墙钟只做纵向对比。
> 上表数字来自同一台机器连续运行中的稳定值；绝对数字不构成跨机器承诺。
> 优化历史与逐项收益见 §11.3。

### 11.2 确定性回归门禁（M6 / T6.5）✅

`crates/ada_vm/tests/perf_budget.rs` 记录每个基准程序的**指令数基线**（`benches/budgets.txt`），
超过基线 **5%** 即失败（与计划里的"劣化 >5% 报警"一致）；同时校验基准程序的**计算结果**
（避免"为了跑得快把活少干了"）。`Vm::enable_instruction_counting(true)` 计量，
关闭时热路径只多一条可预测分支。

重新生成基线：`UPDATE_BUDGETS=1 cargo test -p ada_vm --test perf_budget -- --nocapture`（**仅在确认指令数变化是刻意的、且语义由测试守住时才刷新**；T6.6 就是这种情况：融合后 fib/iterator_pipeline/numeric_loop 的指令数分别下降 17.4%/31.3%/22.2%）

### 11.3 M6 已落地的优化（T6.2，实测）✅

| 优化 | 说明 | 实测收益 |
| :--- | :--- | :--- |
| 捕获表 `Vec` → `Arc`（T6.2a） | 闭包调用不再每次克隆捕获表（`fib` 每次递归少一次堆分配） | fib **−8.6%**、iterator_pipeline **−7.4%**（p<0.05） |
| 指令循环头部合并（T6.2b） | 每条指令只借出帧一次，`frames` 索引从 5 次降到 2 次 | numeric_loop **−11.9%**、fib −5.3%、pipeline −5.5% |
| 挂起检查免写（T6.2c） | `Option<Suspend>`（32B）从"每条指令写回 None"改为先判 `is_some()` | 与上项同批测量 |
| 计量合并为单布尔（T6.2d） | 墙钟检查 + 指令计数在关闭时只判一次布尔（修掉 M5 引入的每指令开销） | 同批测量 |
| **`Value` 48 → 16 字节（T6.2e）** | `NativeFn`/`Intrinsic` 载荷改放 `Arc` 后面，寄存器文件 96 KiB → **32 KiB**（进 L1） | **iterator_pipeline −34.7%**、fib −10.1%、numeric_loop −9.8%、冷启动 −11.7%、json −6.5%（p<0.05） |
| **常量融合 `BinaryConst`（T6.6）** | "一侧是 i32 整数字面量"的算术/比较编成单条指令（`x * 2 + 1` 由 4 条变 2 条；`2 < x` 自动换向为 `Gt`） | **指令数**（确定性）：fib −17.4%、iterator_pipeline **−31.3%**、numeric_loop −22.2%；**墙钟**：iterator_pipeline −15.8%、冷启动 −14.6%、fib −11.1%、numeric_loop −11.1%（json 无字面量运算，+2.0% 不显著 = 符合预期） |
| **条件融合 `JumpIfNotCompare*`（T6.7）** | `if`/`while`/`match` guard 的条件是比较表达式时直接发出条件跳转；`x += 1`（复合赋值 + 字面量、目标为普通局部）也用 `BinaryConst` 完成 | **指令数**（确定性）：fib 1,425,469 → **1,275,420（−10.5%）**；同语义循环写法的对照：融合 **17 条** vs 未融合 **27 条**；新增 `while_loop.ada` 基准（30 万次迭代 = 2,400,011 条指令）。⚠️ 本次墙钟整体下降约 10%，但**无相关模式的基准（json/string_concat）也同样"变快"**，属机器漂移，故不据此声明墙钟收益 |
| **方法分派缓存（T6.8）** | `GetField` 记住「字段名 → (接收者, 已绑定闭包)」，命中时只做一次 `Value` 克隆；限定在同一接收者（`Arc` 指针相等）、仅缓存原型方法名、每次求值清空 | **受控 A/B**（criterion，同一基准、release）：`method_calls` **54.8 ms → 26.6 ms（−51.4%，p<0.05）**；debug 对照：`xs.push(i)` 与等价的全局 `push(xs, i)` 之比从 **1.46× 降到 1.08×**。⚠️ 该优化**不改变指令数**（确定性门禁看不到），因此墙钟证据配了对照组 |
| **累计（对 M6 原始基线）** | 六次改动 | fib 29.73→**21.41 ms（−28.0%）**、numeric_loop 20.63→**15.38 ms（−25.4%）**、iterator_pipeline 89.27→**46.95 ms（−47.4%）**、冷启动 21.77→**13.89 ms（−36.2%）**、json 13.95→12.30 ms（−11.8%） |

**后续改动的性能核对（M7 轮次）**：T1.7 的 `catch_unwind` 兜底与 T1.8 的位置记录（后者只在
**失败时**写入，成功路径零成本）经 criterion 复测**无可测代价**；T6.2e 的收获见上表。
两者的确定性**指令数基线都逐字节未变**（`crates/ada_vm/tests/perf_budget.rs` 全绿），
说明"工作量"没有增加，收益全部来自表示与派发成本。

**T6.2 完成度**：计划验收线是"≥30%"。目前**迭代器管道 −46.6%**、冷启动 −27.9%、
fib −25.3%、numeric_loop −23.7%，平均约 **−29%**——即"≥30%"已在适配器/闭包密集路径上达标，
纯算术循环仍差几个百分点。剩余空间：
1. 指令级特化 + 按景点内联缓存（`AddInt`、`GetField` 方法表缓存）。
2. **Tier 2 JIT**（T6.4）——数量级提升的唯一途径。

（`string_concat` 不计入：它是 O(n²) 的 memcpy 主导，抖动 ±10%。）

### 11.4 值模型决策 D1：维持 `enum Value` ✅

| 证据 | 数值 |
| :--- | :--- |
| `size_of::<Value>()` | **16 字节**（T6.2b 前为 48；`cargo run -p ada_vm --example size_probe`） |
| 胖载荷（放 `Arc` 后） | `NativeFunction` 40 B、`IntrinsicCall` 40 B |
| `size_of::<Instruction>()` | 12 字节（每条指令克隆的代价可忽略，无需"按引用匹配"） |
| `size_of::<Option<Value>>()` | 16 字节（Niche 优化，无额外开销） |
| 寄存器文件 | 2048 × 16 B = **32 KiB**（放进 L1；此前 96 KiB 放不进） |

**结论**：0.1.0 **不做 NaN-boxing**（D1 = A）。原本的理由是"收益需证据、成本是 `value.rs`
全量重写 + builtins 改造"；T6.2b 给出了更强的证据：**先把两个胖变体 box 化就拿到了
−34.7%（适配器路径）/ −25%（整体热路径）**，而 `Value` 已到 16 字节（该枚举设计下
tag + 8 字节载荷的实际上限）。NaN-boxing 能再省的那 8 字节，不足以抵消全量重写与
与 `Arc` 所有权/GC（D6）耦合的风险。

### 11.5 内存模型决策 D6：维持 `Arc<RwLock<Value>>` ✅

| 方案 | 结论 |
| :--- | :--- |
| A 维持 `Arc<RwLock<Value>>`（现状） | ✅ **采用**：无 GC 停顿、析构确定、与单线程协作式调度（D2）天然匹配；代价是引用计数开销与**循环引用泄漏** |
| B 引入 tracing GC | ❌ 0.1.0 不做：大重构 + 引入停顿与不确定性，与"可预测"定位冲突 |

循环引用的风险由 M5 的内存预算兜底（`max_memory_bytes`）——这是把"不可回收"从"无界"降为"有界"，
规范在此明确记录该组合语义。

### 11.6 分层执行策略

| 层 | 形态 | 状态 |
| :--- | :--- | :--- |
| Tier 0 · 解释器 | 寄存器字节码 VM，毫秒级启动 | ✅（上表为实测基线） |
| Tier 1 · VM 微优化 | 循环头合并、捕获表共享、计量合并、**`Value` 瘦身 48→16 B** | ✅ 已完成 5 项（≈29% 平均，适配器路径 −46.6%） |
| Tier 2 · JIT | Cranelift IR 下沉 + 热点计数 + guard/deopt | 📋 T6.4（唯一未启动的计划项；`ada_jit` crate 尚不存在；验收线：`fib(30)` 较 VM 提速 ≥10×） |

### 11.7 今后的可优化点（按证据排序）

1. ~~指令级特化/常量融合~~ ✅ 已完成（T6.6 常量融合、T6.7 条件融合、T6.8 方法分派缓存）。**剩余**：按景类型反馈内联缓存（当前缓存按"字段名 + 接收者"而不是按调用点）、把容器原型方法做成"方法调用直通"指令（可再省掉一次 `GetField` 派发与接收者克隆）、以及更多超指令。
2. 容器 `RwLock` 访问开销 → 单线程路径用 `RefCell`（需与 D6 一并复核）。
3. 字符串 `+=` 是 O(n²)（`string_concat` 基准把这一点量化了）→ 若要优化需要语言级 builder 或 rope。
4. JIT 前先做第 1 项，把解释器剩余开销压到 JIT 值得做为止。
