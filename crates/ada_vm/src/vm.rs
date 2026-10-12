use crate::task::{
    AwaitMode, PendingResume, ResumeSource, SavedExecution, Scheduler, Suspend, TaskId, TaskState,
    MAIN_TASK,
};
use crate::value::{AdapterKind, AsyncKind, Closure, IteratorState, Value};
use ada_bytecode::{Constant, FunctionProto, Instruction};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum VmError {
    #[error("Out of fuel (execution step limit exceeded)")]
    OutOfFuel,

    #[error("Call stack overflow")]
    StackOverflow,

    #[error("Undefined global variable '{0}'")]
    UndefinedGlobal(String),

    #[error("Cannot call non-callable value: {0:?}")]
    NotCallable(Value),

    #[error("Argument count mismatch: expected {expected}, got {got}")]
    ArityMismatch { expected: u16, got: u16 },

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Index out of bounds: index {index}, length {len}")]
    IndexOutOfBounds { index: i64, len: usize },

    #[error("Native function error: {0}")]
    NativeError(String),

    #[error("No `match` arm matched the value (non-exhaustive match); add a `_` arm")]
    NoMatch,

    /// 任务体执行失败，在 `await` 处重新抛出（M4 / T4.6）。
    #[error("Task '{label}' failed: {source}")]
    TaskFailed { label: String, source: Box<VmError> },

    #[error("Task '{0}' was cancelled")]
    TaskCancelled(String),

    #[error("Deadlock: all remaining tasks are waiting and no task can make progress")]
    TaskDeadlock,
}

/// 整数算术溢出（含 `i64::MIN` 取负、`i64::MIN % -1` 这类 checked 边界）统一转为
/// 运行时错误，不允许 panic 逃逸到宿主进程（开发计划 §5 门禁第 3 条）。
fn overflow_err(op: &str) -> VmError {
    VmError::TypeError(format!("Integer overflow in `{op}`"))
}

/// 移位量必须落在 `0..64`，否则返回错误而不是 panic。
fn shift_err(op: &str) -> VmError {
    VmError::TypeError(format!(
        "Shift amount out of range for `{op}` (expected 0..64)"
    ))
}

/// 有序比较（`<` `<=` `>` `>=`）只接受数值操作数（T3.5）。
///
/// 返回 `None` 表示任一操作数是 NaN（按 IEEE，所有有序比较均为 false）；
/// 非数值操作数直接报错，而不是静默返回 `false`。
fn compare_numeric(a: &Value, b: &Value, op: &str) -> Result<Option<Ordering>, VmError> {
    let (Some(x), Some(y)) = (a.as_float(), b.as_float()) else {
        return Err(VmError::TypeError(format!(
            "Cannot compare {a:?} and {b:?} with `{op}` (only numbers are orderable)"
        )));
    };
    Ok(x.partial_cmp(&y))
}

/// 数值求和（全局 `sum` / `.sum()` / 迭代器适配器共用）。
/// 全整数保持 Int；出现 Float 则整体 Float；非数值元素忽略（与既有 `sum` 行为一致）。
/// 整数累加使用 checked 运算，溢出返回错误而不是 panic。
fn sum_values(items: &[Value]) -> Result<Value, VmError> {
    let mut is_float = false;
    let mut int_sum: i64 = 0;
    let mut float_sum = 0.0f64;
    for item in items {
        match item {
            Value::Int(n) => {
                if is_float {
                    float_sum += *n as f64;
                } else {
                    int_sum = int_sum.checked_add(*n).ok_or_else(|| overflow_err("sum"))?;
                }
            }
            Value::Float(f) => {
                if is_float {
                    float_sum += *f;
                } else {
                    is_float = true;
                    float_sum = int_sum as f64 + *f;
                }
            }
            _ => {}
        }
    }
    Ok(if is_float {
        Value::Float(float_sum)
    } else {
        Value::Int(int_sum)
    })
}

pub(crate) struct CallFrame {
    proto: Arc<FunctionProto>,
    ip: usize,
    base_reg: usize,
    dst_reg: Option<usize>,
    /// 当前闭包捕获的 upvalue 槽（T2.2）；主帧为空
    captures: Vec<Value>,
}

pub struct Vm {
    registers: Vec<Value>,
    frames: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    /// 配置的燃料预算（每次 `execute` 重新装填）
    fuel: Option<u64>,
    /// 当前这次执行中剩余的燃料
    fuel_remaining: Option<u64>,
    /// 协作式调度器（M4 / T4.2）：就绪队列、定时器、挂起现场
    scheduler: Scheduler,
    /// 当前正在执行的任务号（主任务为 `MAIN_TASK`）
    current_task: TaskId,
    /// Rust 层嵌套深度（适配器回调 / 内在函数内部）：> 0 时不允许挂起
    rust_depth: usize,
    /// 本轮指令循环请求的挂起（由循环顶部检查并返回）
    suspend_request: Option<Suspend>,
    /// 挂起后恢复时要补做的动作（结果写回）
    pending_resume: Option<PendingResume>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        let mut vm = Self {
            registers: vec![Value::Nil; 2048],
            frames: Vec::new(),
            globals: HashMap::new(),
            fuel: None,
            fuel_remaining: None,
            scheduler: Scheduler::new(),
            current_task: MAIN_TASK,
            rust_depth: 0,
            suspend_request: None,
            pending_resume: None,
        };
        vm.register_builtins();
        vm
    }

    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.fuel = fuel;
        self.fuel_remaining = fuel;
    }

    /// 配置的燃料预算（不是剩余量）。
    pub fn get_fuel(&self) -> Option<u64> {
        self.fuel
    }

    /// 当前这次执行剩余的燃料。
    pub fn remaining_fuel(&self) -> Option<u64> {
        self.fuel_remaining
    }

    /// 把剩余燃料重置为配置的预算。
    pub fn reset_fuel(&mut self) {
        self.fuel_remaining = self.fuel;
    }

    pub fn set_global(&mut self, name: impl Into<String>, val: Value) {
        self.globals.insert(name.into(), val);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    fn register_builtins(&mut self) {
        crate::builtins::register_all(&mut self.globals);
    }

    pub fn execute(&mut self, proto: FunctionProto) -> Result<Value, VmError> {
        let proto = Arc::new(proto);
        let needed_regs = proto.register_count as usize;
        if self.registers.len() < needed_regs {
            self.registers.resize(needed_regs * 2, Value::Nil);
        }

        self.frames.clear();
        self.frames.push(CallFrame {
            proto,
            ip: 0,
            base_reg: 0,
            dst_reg: None,
            captures: Vec::new(),
        });

        // T2.6：每次执行都从配置的预算重新装填燃料，
        // 因此一次 OutOfFuel 不会让同一个 Vm 永久失效。
        self.reset_fuel();

        // M4：脚本之间的任务互不影响；主任务从 MAIN_TASK 开始
        self.scheduler.reset();
        self.current_task = MAIN_TASK;
        self.rust_depth = 0;
        self.suspend_request = None;
        self.pending_resume = None;

        let outcome = self.drive_current();
        self.run_scheduler(SchedulerGoal::MainDone, None, Some(outcome))
    }

    /// 以给定实参调用任意可调用值（T2.1）。
    ///
    /// 与 `Instruction::Call` 不同，这里允许**可重入**调用：为闭包压入一个合成帧后
    /// 用 `run_loop_until(stop_depth)` 继续执行，返回后把帧栈与寄存器窗口恢复原状。
    fn call_callable(&mut self, callee: &Value, args: &[Value]) -> Result<Value, VmError> {
        // M4：进入 Rust 嵌套调用后不允许挂起（否则无法保存宿主栈）
        self.rust_depth += 1;
        let result = self.call_callable_inner(callee, args);
        self.rust_depth -= 1;
        result
    }

    fn call_callable_inner(&mut self, callee: &Value, args: &[Value]) -> Result<Value, VmError> {
        match callee {
            Value::Closure(closure) => {
                let expected = closure.proto.arity;
                if expected as usize != args.len() {
                    return Err(VmError::ArityMismatch {
                        expected,
                        got: args.len() as u16,
                    });
                }
                if self.frames.len() >= 512 {
                    return Err(VmError::StackOverflow);
                }
                let base = self.registers.len();
                let needed = base + closure.proto.register_count as usize + 1;
                self.registers.resize(needed, Value::Nil);
                for (i, arg) in args.iter().enumerate() {
                    self.registers[base + i] = arg.clone();
                }
                self.frames.push(CallFrame {
                    proto: closure.proto.clone(),
                    ip: 0,
                    base_reg: base,
                    dst_reg: None,
                    captures: closure.captures.clone(),
                });
                let stop_depth = self.frames.len() - 1;
                let result = self.run_loop_until(stop_depth);
                // 成功或出错都要清理：否则错误会把合成帧留在栈上
                while self.frames.len() > stop_depth {
                    self.frames.pop();
                }
                self.registers.truncate(base);
                result
            }
            Value::NativeFn { callback, .. } => callback(args).map_err(VmError::NativeError),
            Value::Async { kind, receiver } => {
                let mut effective = Vec::with_capacity(args.len() + 1);
                if let Some(receiver) = receiver {
                    effective.push((**receiver).clone());
                }
                effective.extend(args.iter().cloned());
                self.call_async_blocking(*kind, &effective)
            }
            Value::Intrinsic {
                kind,
                receiver,
                args: bound,
            } => {
                let mut effective = Vec::with_capacity(args.len() + bound.len() + 1);
                if let Some(receiver) = receiver {
                    effective.push((**receiver).clone());
                }
                effective.extend(bound.iter().cloned());
                effective.extend(args.iter().cloned());
                self.invoke_adapter(*kind, &effective)
            }
            other => Err(VmError::NotCallable(other.clone())),
        }
    }

    /// 由任意可迭代值构建迭代器状态（`for..in`、`.iter()` 与适配器共用）。
    fn make_iterator(val: &Value) -> IteratorState {
        match val {
            Value::Array(arr) => IteratorState::Array {
                items: arr.read().unwrap().clone(),
                index: 0,
            },
            Value::String(s) => IteratorState::String {
                chars: s.chars().collect(),
                index: 0,
            },
            Value::Map(m) => IteratorState::Map {
                entries: m
                    .read()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                index: 0,
            },
            Value::Int(n) => IteratorState::Range {
                current: 0,
                end: *n,
                step: 1,
            },
            other => IteratorState::Array {
                items: vec![other.clone()],
                index: 0,
            },
        }
    }

    /// 迭代器推进一步；耗尽返回 `None`。
    fn iterator_next(state: &mut IteratorState) -> Option<Value> {
        match state {
            IteratorState::Array { items, index } => {
                let item = items.get(*index).cloned();
                if item.is_some() {
                    *index += 1;
                }
                item
            }
            IteratorState::String { chars, index } => {
                let ch = chars.get(*index).copied();
                match ch {
                    Some(ch) => {
                        *index += 1;
                        Some(Value::string(ch.to_string()))
                    }
                    None => None,
                }
            }
            IteratorState::Map { entries, index } => {
                let entry = entries.get(*index).cloned();
                match entry {
                    Some((key, value)) => {
                        *index += 1;
                        Some(Value::array(vec![Value::string(key), value]))
                    }
                    None => None,
                }
            }
            IteratorState::Range { current, end, step } => {
                if (*step > 0 && *current < *end) || (*step < 0 && *current > *end) {
                    let value = *current;
                    *current += *step;
                    Some(Value::Int(value))
                } else {
                    None
                }
            }
        }
    }

    /// 物化任意可迭代值。
    ///
    /// 适配器当前为**即时求值**（决策 D3 = A：先可用，惰性留待后续）；
    /// 对 `Iterator` 会消耗其状态（与 `.next()` 语义一致）。
    fn collect_iterable(&self, src: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        if let Value::Iterator(it) = src {
            let mut state = it.write().unwrap();
            while let Some(value) = Vm::iterator_next(&mut state) {
                out.push(value);
            }
            return out;
        }
        let mut state = Vm::make_iterator(src);
        while let Some(value) = Vm::iterator_next(&mut state) {
            out.push(value);
        }
        out
    }

    /// 执行迭代器适配器（T2.1）：`args[0]` 恒为可迭代值，其余按适配器语义解释。
    fn invoke_adapter(&mut self, kind: AdapterKind, args: &[Value]) -> Result<Value, VmError> {
        let name = kind.name();
        let (min, max) = kind.arity();
        if args.len() < min || max.is_some_and(|max| args.len() > max) {
            let expected = match max {
                Some(max) if max == min => min.to_string(),
                Some(max) => format!("{min}..{max}"),
                None => format!("{min}+"),
            };
            return Err(VmError::NativeError(format!(
                "{name} expects {expected} argument(s) including the iterable, got {}",
                args.len()
            )));
        }

        let items = self.collect_iterable(&args[0]);
        match kind {
            AdapterKind::Map => {
                let f = &args[1];
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.call_callable(f, &[item])?);
                }
                Ok(Value::array(out))
            }
            AdapterKind::Filter => {
                let f = &args[1];
                let mut out = Vec::new();
                for item in items {
                    if self
                        .call_callable(f, std::slice::from_ref(&item))?
                        .is_truthy()
                    {
                        out.push(item);
                    }
                }
                Ok(Value::array(out))
            }
            AdapterKind::Take => {
                let n = args[1].as_int().unwrap_or(0).max(0) as usize;
                Ok(Value::array(items.into_iter().take(n).collect()))
            }
            AdapterKind::Skip => {
                let n = args[1].as_int().unwrap_or(0).max(0) as usize;
                Ok(Value::array(items.into_iter().skip(n).collect()))
            }
            AdapterKind::Collect => Ok(Value::array(items)),
            AdapterKind::Count => Ok(Value::Int(items.len() as i64)),
            AdapterKind::Find => {
                let f = &args[1];
                for item in items {
                    if self
                        .call_callable(f, std::slice::from_ref(&item))?
                        .is_truthy()
                    {
                        return Ok(item);
                    }
                }
                Ok(Value::Nil)
            }
            AdapterKind::Any => {
                let f = &args[1];
                for item in items {
                    if self.call_callable(f, &[item])?.is_truthy() {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            AdapterKind::All => {
                let f = &args[1];
                for item in items {
                    if !self.call_callable(f, &[item])?.is_truthy() {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            AdapterKind::Enumerate => Ok(Value::array(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| Value::array(vec![Value::Int(i as i64), value]))
                    .collect(),
            )),
            AdapterKind::Rev => {
                let mut items = items;
                items.reverse();
                Ok(Value::array(items))
            }
            AdapterKind::Join => {
                let sep = args
                    .get(1)
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| ",".to_string());
                let parts: Vec<String> = items.iter().map(|item| item.to_string()).collect();
                Ok(Value::string(parts.join(&sep)))
            }
            AdapterKind::Sum => sum_values(&items),
        }
    }
}

// ============================================================
// M4：协作式任务调度（单线程事件循环，决策 D2）
// ============================================================

/// 指令循环的退出方式（M4）。
enum LoopExit {
    Return(Value),
    /// 需要挂起当前任务（现场由调度器保存）
    Suspended(Suspend),
}

/// 异步内建调用的处理结果。
enum AsyncOutcome {
    /// 已把结果写入目标寄存器
    Written,
    /// 已请求挂起（调用方继续循环，由循环顶部退出）
    Suspended,
}

/// 调度器的驱动目标。
#[derive(Clone)]
enum SchedulerGoal {
    /// 跑到主任务结束（`Vm::execute`）
    MainDone,
    /// 跑到指定任务全部结算（Rust 嵌套上下文的阻塞回退）
    Settled(Vec<TaskId>),
}

/// 一次执行现场运行完毕后的归类结果。
enum StepResult {
    MainDone(Result<Value, VmError>),
    TaskDone(TaskId, Result<Value, VmError>),
    Suspended(TaskId, Suspend),
}

/// 任务显示名：优先用闭包/函数名。
fn task_label(callee: &Value) -> String {
    match callee {
        Value::Closure(closure) => closure.proto.name.clone(),
        Value::NativeFn { name, .. } => name.clone(),
        Value::Async { kind, .. } => kind.name().to_string(),
        Value::Intrinsic { kind, .. } => kind.name().to_string(),
        other => format!(
            "<{}>",
            match other {
                Value::Nil => "nil",
                Value::Bool(_) => "bool",
                Value::Int(_) => "int",
                Value::Float(_) => "float",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Map(_) => "map",
                _ => "value",
            }
        ),
    }
}

impl Vm {
    /// 墙钟毫秒（`sleep_ms` 与调度器定时器共用）。
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 请求挂起当前任务。处于 Rust 嵌套调用（适配器回调等）时返回 `false`，
    /// 调用方必须走阻塞式回退——因为宿主栈无法保存。
    fn request_suspend(&mut self, reason: Suspend) -> bool {
        if self.rust_depth > 0 {
            return false;
        }
        self.suspend_request = Some(reason);
        true
    }

    /// 运行当前寄存器/帧栈中的现场，直到结束或挂起。
    fn drive_current(&mut self) -> StepResult {
        let id = self.current_task;
        match self.run_loop_until_exit(0) {
            Ok(LoopExit::Return(value)) => self.finish_current(id, Ok(value)),
            Err(err) => self.finish_current(id, Err(err)),
            Ok(LoopExit::Suspended(reason)) => {
                let execution = SavedExecution {
                    frames: std::mem::take(&mut self.frames),
                    registers: std::mem::take(&mut self.registers),
                    resume: self.pending_resume.take(),
                };
                self.scheduler.park(id, execution);
                StepResult::Suspended(id, reason)
            }
        }
    }

    fn finish_current(&mut self, id: TaskId, result: Result<Value, VmError>) -> StepResult {
        self.frames.clear();
        self.registers.clear();
        if id == MAIN_TASK {
            StepResult::MainDone(result)
        } else {
            StepResult::TaskDone(id, result)
        }
    }

    fn is_settled(&self, id: TaskId) -> bool {
        self.scheduler
            .cell(id)
            .map(|c| c.state().is_settled())
            .unwrap_or(true)
    }

    /// 主调度循环（M4 / T4.2）：单线程协作式事件循环。
    ///
    /// - 就绪队列 FIFO → 同一脚本的执行顺序确定；
    /// - 没有就绪任务时把墙钟推进到最早的定时器（因此并发 `sleep_ms` 只花最长时间）；
    /// - 既无就绪任务也无定时器 → 死锁错误。
    fn run_scheduler(
        &mut self,
        goal: SchedulerGoal,
        blocked: Option<TaskId>,
        first: Option<StepResult>,
    ) -> Result<Value, VmError> {
        let mut pending = first;
        loop {
            if let Some(outcome) = pending.take() {
                match outcome {
                    // 主任务结束即脚本结束；未完成的任务被丢弃（SPEC_ASYNC §5）
                    StepResult::MainDone(result) => return result,
                    StepResult::TaskDone(id, result) => self.settle_task(id, result),
                    StepResult::Suspended(id, reason) => self.register_suspension(id, reason),
                }
            }

            if let SchedulerGoal::Settled(targets) = &goal {
                if targets.iter().all(|id| self.is_settled(*id)) {
                    return Ok(Value::Nil);
                }
            }

            let mut next = None;
            while let Some(id) = self.scheduler.pop_ready() {
                if Some(id) == blocked {
                    continue;
                }
                next = Some(id);
                break;
            }

            match next {
                Some(id) => pending = Some(self.resume_task(id)),
                None => {
                    let Some(deadline) = self.scheduler.next_deadline() else {
                        return Err(VmError::TaskDeadlock);
                    };
                    let now = Self::now_ms();
                    if deadline > now {
                        std::thread::sleep(std::time::Duration::from_millis(deadline - now));
                    }
                    self.scheduler.wake_due(Self::now_ms());
                }
            }
        }
    }

    /// 结算任务并把等待它的现场放回就绪队列。
    fn settle_task(&mut self, id: TaskId, result: Result<Value, VmError>) {
        let seq = self.scheduler.next_seq();
        if let Some(cell) = self.scheduler.cell(id) {
            if !cell.state().is_settled() {
                cell.settle(result, seq);
            }
        }
        for waiter in self.scheduler.take_waiters(id) {
            self.wake_waiter(waiter);
        }
    }

    /// 唤醒条件已满足则把等待者放回就绪队列（否则继续挂起）。
    fn wake_waiter(&mut self, waiter: TaskId) {
        let Some(execution) = self.scheduler.take_parked(waiter) else {
            return;
        };
        let satisfied = match execution.resume.as_ref().map(|r| &r.source) {
            Some(ResumeSource::Sleep) => true,
            Some(ResumeSource::Await(id)) => self.is_settled(*id),
            Some(ResumeSource::AwaitAll(ids)) => ids.iter().all(|id| self.is_settled(*id)),
            Some(ResumeSource::AwaitAny(ids)) => ids.iter().any(|id| self.is_settled(*id)),
            // 超时：无论被任务结算还是被定时器唤醒都要恢复（结果由 apply_resume 判定）
            Some(ResumeSource::Timeout(_)) => true,
            None => true,
        };
        self.scheduler.park(waiter, execution);
        if satisfied {
            self.scheduler.enqueue(waiter);
        }
    }

    /// 注册挂起条件：定时器或等待者关系（同时处理"目标已结算"的竞态）。
    fn register_suspension(&mut self, id: TaskId, reason: Suspend) {
        match reason {
            Suspend::Sleep { deadline_ms } => {
                self.scheduler.add_timer(deadline_ms, id);
            }
            Suspend::Await { targets, mode } => {
                let done = match mode {
                    AwaitMode::All => targets.iter().all(|t| self.is_settled(*t)),
                    AwaitMode::Any => targets.iter().any(|t| self.is_settled(*t)),
                };
                if done {
                    self.scheduler.enqueue(id);
                } else {
                    for target in &targets {
                        if !self.is_settled(*target) {
                            self.scheduler.add_waiter(*target, id);
                        }
                    }
                }
            }
            Suspend::Timeout {
                target,
                deadline_ms,
            } => {
                // 两条唤醒路径：目标任务结算，或定时器到期
                if self.is_settled(target) {
                    self.scheduler.enqueue(id);
                } else {
                    self.scheduler.add_waiter(target, id);
                    self.scheduler.add_timer(deadline_ms, id);
                }
            }
        }
        if id != MAIN_TASK {
            if let Some(cell) = self.scheduler.cell(id) {
                if !cell.state().is_settled() {
                    cell.set_state(TaskState::Waiting);
                }
            }
        }
    }

    /// 恢复一个就绪任务/主任务：恢复现场、补做挂起点的收尾、继续执行。
    fn resume_task(&mut self, id: TaskId) -> StepResult {
        self.current_task = id;
        if id != MAIN_TASK {
            if let Some(cell) = self.scheduler.cell(id) {
                if cell.state().is_settled() {
                    // 排队期间被取消
                    return StepResult::TaskDone(id, cell.result().unwrap_or(Ok(Value::Nil)));
                }
                cell.set_state(TaskState::Running);
            }
        }

        let Some(execution) = self.scheduler.take_parked(id) else {
            return self.start_task(id);
        };
        self.frames = execution.frames;
        self.registers = execution.registers;
        let value = match execution.resume {
            Some(resume) => match self.apply_resume(&resume) {
                Ok(value) => value,
                Err(err) => return self.finish_current(id, Err(err)),
            },
            None => Value::Nil,
        };
        // `spawn sleep_ms(n)` 这类"空任务体"：唤醒后没有可继续执行的帧，直接结算
        if self.frames.is_empty() {
            return self.finish_current(id, Ok(value));
        }
        self.drive_current()
    }

    /// 首次执行一个任务（`spawn` 之后第一次被调度）。
    fn start_task(&mut self, id: TaskId) -> StepResult {
        let Some(cell) = self.scheduler.cell(id).cloned() else {
            return StepResult::TaskDone(id, Err(VmError::TaskDeadlock));
        };
        let Some((callee, args)) = cell.take_call() else {
            return StepResult::TaskDone(id, cell.result().unwrap_or(Ok(Value::Nil)));
        };

        match &callee {
            Value::Closure(closure) => match self.begin_closure_task(closure, &args) {
                Ok(()) => self.drive_current(),
                Err(err) => StepResult::TaskDone(id, Err(err)),
            },
            // `spawn sleep_ms(n)`：任务本身只睡一觉，直接挂起而不是占用宿主线程
            Value::Async {
                kind: AsyncKind::Sleep,
                receiver,
            } => {
                let mut effective = Vec::new();
                if let Some(receiver) = receiver {
                    effective.push((**receiver).clone());
                }
                effective.extend(args.iter().cloned());
                let ms = effective
                    .first()
                    .and_then(|a| a.as_int())
                    .unwrap_or(0)
                    .max(0);
                let deadline = Self::now_ms() + ms as u64;
                self.scheduler.park(
                    id,
                    SavedExecution {
                        frames: Vec::new(),
                        registers: Vec::new(),
                        resume: Some(PendingResume {
                            dst: None,
                            source: ResumeSource::Sleep,
                        }),
                    },
                );
                StepResult::Suspended(
                    id,
                    Suspend::Sleep {
                        deadline_ms: deadline,
                    },
                )
            }
            other => {
                // 原生函数/内在函数/其它异步操作：无法挂起，直接求值
                let value = self.call_callable(other, &args);
                StepResult::TaskDone(id, value)
            }
        }
    }

    /// 为任务体准备独立的帧栈与寄存器窗口。
    fn begin_closure_task(
        &mut self,
        closure: &Arc<Closure>,
        args: &[Value],
    ) -> Result<(), VmError> {
        if closure.proto.arity as usize != args.len() {
            return Err(VmError::ArityMismatch {
                expected: closure.proto.arity,
                got: args.len() as u16,
            });
        }
        self.frames.clear();
        self.registers.clear();
        let needed = closure.proto.register_count as usize + args.len() + 1;
        self.registers = vec![Value::Nil; needed];
        for (i, arg) in args.iter().enumerate() {
            self.registers[i] = arg.clone();
        }
        self.frames.push(CallFrame {
            proto: closure.proto.clone(),
            ip: 0,
            base_reg: 0,
            dst_reg: None,
            captures: closure.captures.clone(),
        });
        Ok(())
    }

    /// 补做挂起点的收尾：算出等待结果并写回目标寄存器（失败则抛错）。
    fn apply_resume(&mut self, resume: &PendingResume) -> Result<Value, VmError> {
        let value = match &resume.source {
            ResumeSource::Sleep => Value::Nil,
            ResumeSource::Await(id) => self.task_value(*id)?,
            ResumeSource::AwaitAll(ids) => {
                let mut out = Vec::with_capacity(ids.len());
                for id in ids {
                    out.push(self.task_value(*id)?);
                }
                Value::array(out)
            }
            ResumeSource::AwaitAny(ids) => {
                let winner = self.first_settled(ids).ok_or(VmError::TaskDeadlock)?;
                self.task_value(winner)?
            }
            ResumeSource::Timeout(target) => {
                // 任务已结算 → 返回它的值；否则说明是超时唤醒 → 取消任务并返回 nil
                match self.scheduler.cell(*target).cloned() {
                    Some(cell) if cell.state().is_settled() => self.cell_value(&cell)?,
                    Some(cell) => {
                        self.cancel_task(&cell);
                        Value::Nil
                    }
                    None => Value::Nil,
                }
            }
        };
        if let Some(dst) = resume.dst {
            if dst < self.registers.len() {
                self.registers[dst] = value.clone();
            }
        }
        Ok(value)
    }

    /// 任务错误传播（SPEC_ASYNC §5）：加一层 `TaskFailed` 保留任务名与因果链；
    /// 取消保持 `TaskCancelled`（不再包一层）。
    fn propagate_task_error(label: &str, err: VmError) -> VmError {
        match err {
            VmError::TaskCancelled(name) => VmError::TaskCancelled(name),
            other => VmError::TaskFailed {
                label: label.to_string(),
                source: Box::new(other),
            },
        }
    }

    /// 读取一个任务的返回值（失败时按 §5 规则包装）。
    fn cell_value(&self, cell: &Arc<crate::task::TaskCell>) -> Result<Value, VmError> {
        match cell.result() {
            Some(Ok(value)) => Ok(value),
            Some(Err(err)) => Err(Self::propagate_task_error(&cell.label, err)),
            None => Err(VmError::TaskDeadlock),
        }
    }

    /// 取任务结果（未结算视为死锁——调用方应当只在已结算时使用）。
    fn task_value(&self, id: TaskId) -> Result<Value, VmError> {
        match self.scheduler.cell(id) {
            Some(cell) => self.cell_value(cell),
            None => Err(VmError::TaskDeadlock),
        }
    }

    /// 最先结算的任务（`await_any` 的判定依据）。
    fn first_settled(&self, ids: &[TaskId]) -> Option<TaskId> {
        ids.iter()
            .filter_map(|id| self.scheduler.cell(*id).map(|c| (c.settled_seq(), *id)))
            .filter(|(seq, _)| *seq > 0)
            .min_by_key(|(seq, _)| *seq)
            .map(|(_, id)| id)
    }

    /// Rust 嵌套上下文里的阻塞式等待：把当前现场交给 Rust 栈持有，
    /// 驱动调度器直到目标结算，再恢复现场。
    fn blocking_drain(&mut self, targets: &[TaskId]) -> Result<(), VmError> {
        if targets.iter().all(|id| self.is_settled(*id)) {
            return Ok(());
        }
        let frames = std::mem::take(&mut self.frames);
        let registers = std::mem::take(&mut self.registers);
        let blocked = self.current_task;
        let result = self.run_scheduler(
            SchedulerGoal::Settled(targets.to_vec()),
            Some(blocked),
            None,
        );
        self.frames = frames;
        self.registers = registers;
        self.current_task = blocked;
        result.map(|_| ())
    }

    /// 从 `await_all`/`await_any` 的实参里取出任务号列表。
    fn task_ids_from_args(&self, args: &[Value]) -> Result<Vec<TaskId>, VmError> {
        let mut ids = Vec::new();
        match args.first() {
            Some(Value::Array(items)) => {
                for item in items.read().unwrap().iter() {
                    match item {
                        Value::Task(cell) => ids.push(cell.id),
                        other => {
                            return Err(VmError::TypeError(format!(
                                "await_all/await_any expects tasks, got {other:?}"
                            )))
                        }
                    }
                }
            }
            Some(Value::Task(cell)) => ids.push(cell.id),
            Some(other) => {
                return Err(VmError::TypeError(format!(
                    "await_all/await_any expects an array of tasks, got {other:?}"
                )))
            }
            None => {
                return Err(VmError::TypeError(
                    "await_all/await_any expects an array of tasks".to_string(),
                ))
            }
        }
        Ok(ids)
    }

    /// 不可挂起上下文里的异步操作实现（嵌套回调 / 任务首次执行）。
    fn call_async_blocking(&mut self, kind: AsyncKind, args: &[Value]) -> Result<Value, VmError> {
        match kind {
            AsyncKind::Sleep => {
                let ms = args.first().and_then(|a| a.as_int()).unwrap_or(0);
                if ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                }
                Ok(Value::Nil)
            }
            AsyncKind::YieldNow => Ok(Value::Nil),
            AsyncKind::TaskCancel => {
                let cell = match args.first() {
                    Some(Value::Task(cell)) => cell.clone(),
                    other => {
                        return Err(VmError::TypeError(format!(
                            "cancel expects a task, got {other:?}"
                        )))
                    }
                };
                Ok(Value::Bool(self.cancel_task(&cell)))
            }
            AsyncKind::AwaitAll | AsyncKind::AwaitAny => {
                let ids = self.task_ids_from_args(args)?;
                self.blocking_drain(&ids)?;
                if kind == AsyncKind::AwaitAll {
                    let mut out = Vec::with_capacity(ids.len());
                    for id in ids {
                        out.push(self.task_value(id)?);
                    }
                    Ok(Value::array(out))
                } else {
                    let winner = self.first_settled(&ids).ok_or(VmError::TaskDeadlock)?;
                    self.task_value(winner)
                }
            }
            // 回调上下文里无法挂起，`timeout` 退化为"等到任务结束"（不实现超时）
            AsyncKind::Timeout => {
                let target = match args.first() {
                    Some(Value::Task(cell)) => cell.clone(),
                    other => {
                        return Err(VmError::TypeError(format!(
                            "timeout expects (task, ms), got {other:?}"
                        )))
                    }
                };
                self.blocking_drain(&[target.id])?;
                self.cell_value(&target)
            }
        }
    }

    /// 取消任务；成功时立刻唤醒等待它的现场。
    fn cancel_task(&mut self, cell: &Arc<crate::task::TaskCell>) -> bool {
        let id = cell.id;
        if !cell.cancel() {
            return false;
        }
        for waiter in self.scheduler.take_waiters(id) {
            self.wake_waiter(waiter);
        }
        true
    }

    /// 指令循环里的异步内建调用（可以挂起）。
    fn handle_async_call(
        &mut self,
        kind: AsyncKind,
        args: Vec<Value>,
        dst_reg: usize,
    ) -> Result<AsyncOutcome, VmError> {
        match kind {
            AsyncKind::Sleep | AsyncKind::YieldNow => {
                let ms = if kind == AsyncKind::YieldNow {
                    0
                } else {
                    args.first().and_then(|a| a.as_int()).unwrap_or(0).max(0)
                };
                let deadline = Self::now_ms() + ms as u64;
                if self.request_suspend(Suspend::Sleep {
                    deadline_ms: deadline,
                }) {
                    self.pending_resume = Some(PendingResume {
                        dst: Some(dst_reg),
                        source: ResumeSource::Sleep,
                    });
                    Ok(AsyncOutcome::Suspended)
                } else {
                    // 回调上下文：退化为真实阻塞（无法保存宿主栈）
                    let value = self.call_async_blocking(kind, &args)?;
                    self.registers[dst_reg] = value;
                    Ok(AsyncOutcome::Written)
                }
            }
            AsyncKind::TaskCancel => {
                let value = self.call_async_blocking(kind, &args)?;
                self.registers[dst_reg] = value;
                Ok(AsyncOutcome::Written)
            }
            AsyncKind::AwaitAll | AsyncKind::AwaitAny => {
                let ids = self.task_ids_from_args(&args)?;
                // 确保未结算的目标已被调度
                for id in &ids {
                    if !self.is_settled(*id) {
                        self.scheduler.enqueue(*id);
                    }
                }
                let mode = if kind == AsyncKind::AwaitAll {
                    AwaitMode::All
                } else {
                    AwaitMode::Any
                };
                let ready = match mode {
                    AwaitMode::All => ids.iter().all(|id| self.is_settled(*id)),
                    AwaitMode::Any => ids.iter().any(|id| self.is_settled(*id)),
                };
                if ready {
                    let value = self.collect_await_result(kind, &ids)?;
                    self.registers[dst_reg] = value;
                    return Ok(AsyncOutcome::Written);
                }
                let source = if kind == AsyncKind::AwaitAll {
                    ResumeSource::AwaitAll(ids.clone())
                } else {
                    ResumeSource::AwaitAny(ids.clone())
                };
                if self.request_suspend(Suspend::Await { targets: ids, mode }) {
                    self.pending_resume = Some(PendingResume {
                        dst: Some(dst_reg),
                        source,
                    });
                    Ok(AsyncOutcome::Suspended)
                } else {
                    let value = self.call_async_blocking(kind, &args)?;
                    self.registers[dst_reg] = value;
                    Ok(AsyncOutcome::Written)
                }
            }
            AsyncKind::Timeout => {
                // 形式：`timeout(task, ms)` 或 `task.timeout(ms)`（receiver 已前置）
                let (target, ms) = match (args.first(), args.get(1)) {
                    (Some(Value::Task(cell)), Some(ms)) => {
                        (cell.clone(), ms.as_int().unwrap_or(0).max(0))
                    }
                    (Some(Value::Task(cell)), None) => (cell.clone(), 0),
                    (other, _) => {
                        return Err(VmError::TypeError(format!(
                            "timeout expects (task, ms), got {other:?}"
                        )))
                    }
                };
                if target.state().is_settled() {
                    self.registers[dst_reg] = self.cell_value(&target)?;
                    return Ok(AsyncOutcome::Written);
                }
                let deadline = Self::now_ms() + ms as u64;
                if self.request_suspend(Suspend::Timeout {
                    target: target.id,
                    deadline_ms: deadline,
                }) {
                    self.scheduler.enqueue(target.id);
                    self.pending_resume = Some(PendingResume {
                        dst: Some(dst_reg),
                        source: ResumeSource::Timeout(target.id),
                    });
                    Ok(AsyncOutcome::Suspended)
                } else {
                    let value = self.call_async_blocking(AsyncKind::Timeout, &args)?;
                    self.registers[dst_reg] = value;
                    Ok(AsyncOutcome::Written)
                }
            }
        }
    }

    /// 汇总 `await_all` / `await_any` 的结果。
    fn collect_await_result(&self, kind: AsyncKind, ids: &[TaskId]) -> Result<Value, VmError> {
        if kind == AsyncKind::AwaitAll {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                out.push(self.task_value(*id)?);
            }
            Ok(Value::array(out))
        } else {
            let winner = self.first_settled(ids).ok_or(VmError::TaskDeadlock)?;
            self.task_value(winner)
        }
    }
}

impl Vm {
    /// 从当前帧继续执行，直到帧栈回落到 `stop_depth`（`execute` 传 0）。
    ///
    /// 这让 VM 内部（迭代器适配器 T2.1）能够**可重入地**调用脚本闭包：
    /// 压入一个合成帧后，以 `stop_depth = frames.len() - 1` 再次进入执行循环。
    ///
    /// 该路径**不允许挂起**（调用方是 Rust 栈帧，无法保存），需要挂起的操作
    /// 会走阻塞式回退（见 `call_async_blocking`）。
    fn run_loop_until(&mut self, stop_depth: usize) -> Result<Value, VmError> {
        match self.run_loop_until_exit(stop_depth)? {
            LoopExit::Return(value) => Ok(value),
            LoopExit::Suspended(_) => Err(VmError::TypeError(
                "internal error: suspension requested inside a native callback".to_string(),
            )),
        }
    }

    fn run_loop_until_exit(&mut self, stop_depth: usize) -> Result<LoopExit, VmError> {
        loop {
            // M4：挂起点检查（指令循环唯一的安全退出点）
            if let Some(reason) = self.suspend_request.take() {
                return Ok(LoopExit::Suspended(reason));
            }

            // Check Fuel
            if let Some(ref mut fuel) = self.fuel_remaining {
                if *fuel == 0 {
                    return Err(VmError::OutOfFuel);
                }
                *fuel -= 1;
            }

            let frame_idx = self.frames.len() - 1;
            let ip = self.frames[frame_idx].ip;

            if ip >= self.frames[frame_idx].proto.instructions.len() {
                // Implicit return nil if reached end of function
                let frame = self.frames.pop().unwrap();
                if let Some(dst) = frame.dst_reg {
                    self.registers[dst] = Value::Nil;
                }
                if self.frames.len() == stop_depth {
                    return Ok(LoopExit::Return(Value::Nil));
                }
                continue;
            }

            let inst = self.frames[frame_idx].proto.instructions[ip].clone();
            self.frames[frame_idx].ip += 1;
            let base = self.frames[frame_idx].base_reg;

            match inst {
                Instruction::LoadConst { dst, const_idx } => {
                    let c = &self.frames[frame_idx].proto.constants[const_idx as usize];
                    let val = match c {
                        Constant::Int(n) => Value::Int(*n),
                        Constant::Float(f) => Value::Float(*f),
                        Constant::String(s) => Value::string(s.clone()),
                        Constant::Bool(b) => Value::Bool(*b),
                        Constant::Nil => Value::Nil,
                    };
                    self.registers[base + dst.0 as usize] = val;
                }

                Instruction::LoadNil { dst } => {
                    self.registers[base + dst.0 as usize] = Value::Nil;
                }

                Instruction::LoadBool { dst, val } => {
                    self.registers[base + dst.0 as usize] = Value::Bool(val);
                }

                Instruction::LoadInt { dst, val } => {
                    self.registers[base + dst.0 as usize] = Value::Int(val as i64);
                }

                Instruction::Move { dst, src } => {
                    self.registers[base + dst.0 as usize] =
                        self.registers[base + src.0 as usize].clone();
                }

                Instruction::Add { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            Value::Int(x.checked_add(*y).ok_or_else(|| overflow_err("+"))?)
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x + y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 + y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x + *y as f64),
                        (Value::String(x), y) => Value::string(format!("{x}{y}")),
                        (x, Value::String(y)) => Value::string(format!("{x}{y}")),
                        _ => return Err(VmError::TypeError(format!("Cannot add {a:?} and {b:?}"))),
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Sub { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            Value::Int(x.checked_sub(*y).ok_or_else(|| overflow_err("-"))?)
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x - y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 - y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x - *y as f64),
                        _ => {
                            return Err(VmError::TypeError(format!(
                                "Cannot subtract {b:?} from {a:?}"
                            )))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Mul { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            Value::Int(x.checked_mul(*y).ok_or_else(|| overflow_err("*"))?)
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x * y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 * y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x * *y as f64),
                        _ => {
                            return Err(VmError::TypeError(format!(
                                "Cannot multiply {a:?} and {b:?}"
                            )))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Div { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            if *y == 0 {
                                return Err(VmError::TypeError("Division by zero".to_string()));
                            }
                            match x.checked_rem(*y) {
                                // 整除保持 Int；i64::MIN / -1 会被 checked_rem 拦下
                                Some(0) => {
                                    Value::Int(x.checked_div(*y).ok_or_else(|| overflow_err("/"))?)
                                }
                                Some(_) => Value::Float(*x as f64 / *y as f64),
                                None => return Err(overflow_err("/")),
                            }
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x / y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 / y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x / *y as f64),
                        _ => {
                            return Err(VmError::TypeError(format!("Cannot divide {a:?} by {b:?}")))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Mod { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            if *y == 0 {
                                return Err(VmError::TypeError("Modulo by zero".to_string()));
                            }
                            Value::Int(x.checked_rem(*y).ok_or_else(|| overflow_err("%"))?)
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x % y),
                        // T3.5：混合数值类型统一提升为 float 取模（与 `/` 提升规则一致）
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 % y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x % *y as f64),
                        _ => {
                            return Err(VmError::TypeError(format!("Cannot modulo {a:?} by {b:?}")))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Neg { dst, src } => {
                    let val = &self.registers[base + src.0 as usize];
                    let res = match val {
                        Value::Int(n) => {
                            Value::Int(n.checked_neg().ok_or_else(|| overflow_err("-"))?)
                        }
                        Value::Float(f) => Value::Float(-f),
                        _ => return Err(VmError::TypeError(format!("Cannot negate {val:?}"))),
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Not { dst, src } => {
                    let val = &self.registers[base + src.0 as usize];
                    self.registers[base + dst.0 as usize] = Value::Bool(!val.is_truthy());
                }

                Instruction::BitNot { dst, src } => {
                    let val = &self.registers[base + src.0 as usize];
                    if let Value::Int(n) = val {
                        self.registers[base + dst.0 as usize] = Value::Int(!n);
                    } else {
                        return Err(VmError::TypeError(format!("Cannot bitwise NOT {val:?}")));
                    }
                }

                Instruction::Eq { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    self.registers[base + dst.0 as usize] = Value::Bool(a == b);
                }

                Instruction::Ne { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    self.registers[base + dst.0 as usize] = Value::Bool(a != b);
                }

                Instruction::Lt { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = matches!(compare_numeric(a, b, "<")?, Some(Ordering::Less));
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Le { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = matches!(
                        compare_numeric(a, b, "<=")?,
                        Some(Ordering::Less | Ordering::Equal)
                    );
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Gt { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = matches!(compare_numeric(a, b, ">")?, Some(Ordering::Greater));
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Ge { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = matches!(
                        compare_numeric(a, b, ">=")?,
                        Some(Ordering::Greater | Ordering::Equal)
                    );
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::BitAnd { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        self.registers[base + dst.0 as usize] = Value::Int(x & y);
                    } else {
                        return Err(VmError::TypeError("Bitwise AND requires integers".into()));
                    }
                }

                Instruction::BitOr { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        self.registers[base + dst.0 as usize] = Value::Int(x | y);
                    } else {
                        return Err(VmError::TypeError("Bitwise OR requires integers".into()));
                    }
                }

                Instruction::BitXor { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        self.registers[base + dst.0 as usize] = Value::Int(x ^ y);
                    } else {
                        return Err(VmError::TypeError("Bitwise XOR requires integers".into()));
                    }
                }

                Instruction::Shl { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        let shifted = u32::try_from(*y)
                            .ok()
                            .and_then(|amount| x.checked_shl(amount))
                            .ok_or_else(|| shift_err("<<"))?;
                        self.registers[base + dst.0 as usize] = Value::Int(shifted);
                    } else {
                        return Err(VmError::TypeError("Shift requires integers".into()));
                    }
                }

                Instruction::Shr { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        let shifted = u32::try_from(*y)
                            .ok()
                            .and_then(|amount| x.checked_shr(amount))
                            .ok_or_else(|| shift_err(">>"))?;
                        self.registers[base + dst.0 as usize] = Value::Int(shifted);
                    } else {
                        return Err(VmError::TypeError("Shift requires integers".into()));
                    }
                }

                Instruction::Jump { offset } => {
                    self.frames[frame_idx].ip =
                        (self.frames[frame_idx].ip as i32 + offset) as usize;
                }

                Instruction::MatchFail => {
                    // T3.2：非穷尽 match 的显式失败路径
                    return Err(VmError::NoMatch);
                }

                Instruction::JumpIfFalse { cond, offset } => {
                    let val = &self.registers[base + cond.0 as usize];
                    if !val.is_truthy() {
                        self.frames[frame_idx].ip =
                            (self.frames[frame_idx].ip as i32 + offset) as usize;
                    }
                }

                Instruction::JumpIfTrue { cond, offset } => {
                    let val = &self.registers[base + cond.0 as usize];
                    if val.is_truthy() {
                        self.frames[frame_idx].ip =
                            (self.frames[frame_idx].ip as i32 + offset) as usize;
                    }
                }

                Instruction::JumpIfNil { cond, offset } => {
                    let val = &self.registers[base + cond.0 as usize];
                    if val.is_nil() {
                        self.frames[frame_idx].ip =
                            (self.frames[frame_idx].ip as i32 + offset) as usize;
                    }
                }

                Instruction::Call {
                    dst,
                    callee,
                    arg_start,
                    arg_count,
                } => {
                    let callee_val = self.registers[base + callee.0 as usize].clone();
                    match callee_val {
                        Value::Closure(closure) => {
                            if closure.proto.arity != arg_count {
                                return Err(VmError::ArityMismatch {
                                    expected: closure.proto.arity,
                                    got: arg_count,
                                });
                            }
                            if self.frames.len() >= 512 {
                                return Err(VmError::StackOverflow);
                            }

                            let new_base =
                                base + self.frames[frame_idx].proto.register_count as usize;
                            let needed = new_base + closure.proto.register_count as usize;
                            if self.registers.len() < needed {
                                self.registers.resize(needed * 2, Value::Nil);
                            }

                            // Copy args to new frame base
                            for i in 0..arg_count as usize {
                                self.registers[new_base + i] =
                                    self.registers[base + arg_start.0 as usize + i].clone();
                            }

                            self.frames.push(CallFrame {
                                proto: closure.proto.clone(),
                                ip: 0,
                                base_reg: new_base,
                                dst_reg: Some(base + dst.0 as usize),
                                captures: closure.captures.clone(),
                            });
                        }
                        Value::NativeFn { callback, .. } => {
                            let start = base + arg_start.0 as usize;
                            let end = start + arg_count as usize;
                            let args = &self.registers[start..end];
                            let res = callback(args).map_err(VmError::NativeError)?;
                            self.registers[base + dst.0 as usize] = res;
                        }
                        Value::Intrinsic { .. } => {
                            let start = base + arg_start.0 as usize;
                            let end = start + arg_count as usize;
                            let args = self.registers[start..end].to_vec();
                            let res = self.call_callable(&callee_val, &args)?;
                            self.registers[base + dst.0 as usize] = res;
                        }
                        Value::Async { kind, receiver } => {
                            // M4：可挂起的异步内建（sleep_ms / await_all / cancel …）
                            let start = base + arg_start.0 as usize;
                            let end = start + arg_count as usize;
                            let mut args = Vec::with_capacity(arg_count as usize + 1);
                            if let Some(receiver) = receiver {
                                args.push((*receiver).clone());
                            }
                            args.extend(self.registers[start..end].iter().cloned());
                            if let AsyncOutcome::Suspended =
                                self.handle_async_call(kind, args, base + dst.0 as usize)?
                            {
                                continue;
                            }
                        }
                        other => return Err(VmError::NotCallable(other)),
                    }
                }

                Instruction::Return { src } => {
                    let ret_val = if let Some(r) = src {
                        self.registers[base + r.0 as usize].clone()
                    } else {
                        Value::Nil
                    };

                    let frame = self.frames.pop().unwrap();
                    if let Some(dest) = frame.dst_reg {
                        self.registers[dest] = ret_val.clone();
                    }

                    if self.frames.len() == stop_depth {
                        return Ok(LoopExit::Return(ret_val));
                    }
                }

                Instruction::GetGlobal { dst, name_idx } => {
                    let name = &self.frames[frame_idx].proto.names[name_idx as usize];
                    if let Some(val) = self.globals.get(name) {
                        self.registers[base + dst.0 as usize] = val.clone();
                    } else {
                        return Err(VmError::UndefinedGlobal(name.clone()));
                    }
                }

                Instruction::SetGlobal { name_idx, src } => {
                    let name = self.frames[frame_idx].proto.names[name_idx as usize].clone();
                    let val = self.registers[base + src.0 as usize].clone();
                    self.globals.insert(name, val);
                }

                Instruction::NewArray { dst, start, count } => {
                    let mut items = Vec::with_capacity(count as usize);
                    for i in 0..count as usize {
                        items.push(self.registers[base + start.0 as usize + i].clone());
                    }
                    self.registers[base + dst.0 as usize] = Value::array(items);
                }

                Instruction::NewMap { dst } => {
                    self.registers[base + dst.0 as usize] = Value::map(HashMap::new());
                }

                Instruction::MapSet { map, key, val } => {
                    let map_val = &self.registers[base + map.0 as usize];
                    let key_val = &self.registers[base + key.0 as usize];
                    let v = self.registers[base + val.0 as usize].clone();

                    if let Value::Map(m) = map_val {
                        let k = key_val.to_string();
                        m.write().unwrap().insert(k, v);
                    } else {
                        return Err(VmError::TypeError("MapSet on non-map".into()));
                    }
                }

                Instruction::GetIndex { dst, target, index } => {
                    let t = &self.registers[base + target.0 as usize];
                    let idx_val = &self.registers[base + index.0 as usize];

                    let res = match (t, idx_val) {
                        (Value::Array(arr), Value::Int(i)) => {
                            let items = arr.read().unwrap();
                            // T3.5：负索引从末尾计数（与 `.slice` 一致）
                            let len = items.len() as i64;
                            let idx = if *i < 0 { len + *i } else { *i };
                            if idx < 0 || idx >= len {
                                Value::Nil
                            } else {
                                items[idx as usize].clone()
                            }
                        }
                        (Value::Map(m), key) => {
                            let map = m.read().unwrap();
                            map.get(&key.to_string()).cloned().unwrap_or(Value::Nil)
                        }
                        (Value::String(s), Value::Int(i)) => {
                            // T3.5：按 Unicode 字符索引，负索引从末尾计数
                            let char_count = s.chars().count() as i64;
                            let idx = if *i < 0 { char_count + *i } else { *i };
                            if idx < 0 || idx >= char_count {
                                Value::Nil
                            } else {
                                s.chars()
                                    .nth(idx as usize)
                                    .map(|c| Value::string(c.to_string()))
                                    .unwrap_or(Value::Nil)
                            }
                        }
                        _ => Value::Nil,
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::SetIndex { target, index, val } => {
                    let t = &self.registers[base + target.0 as usize];
                    let idx_val = &self.registers[base + index.0 as usize];
                    let v = self.registers[base + val.0 as usize].clone();

                    match (t, idx_val) {
                        (Value::Array(arr), Value::Int(i)) => {
                            let mut items = arr.write().unwrap();
                            // T3.5：负索引从末尾计数
                            let len = items.len() as i64;
                            let idx = if *i < 0 { len + *i } else { *i };
                            if idx >= 0 && idx < len {
                                items[idx as usize] = v;
                            } else {
                                return Err(VmError::IndexOutOfBounds {
                                    index: *i,
                                    len: items.len(),
                                });
                            }
                        }
                        (Value::Map(m), key) => {
                            let mut map = m.write().unwrap();
                            map.insert(key.to_string(), v);
                        }
                        _ => return Err(VmError::TypeError("Cannot set index on target".into())),
                    }
                }

                Instruction::GetField {
                    dst,
                    target,
                    field_idx,
                    optional,
                } => {
                    let t = &self.registers[base + target.0 as usize];
                    let field = &self.frames[frame_idx].proto.names[field_idx as usize];

                    // T2.1：迭代器适配器的方法形式（`xs.map(f)`）。
                    // 先于原型方法派发；map 类型上的同名键优先（这些名字属纯新增，不构成回归）。
                    if let Some(kind) = AdapterKind::from_method(field) {
                        if t.is_iterable() {
                            if let Value::Map(m) = t {
                                let existing = m.read().unwrap().get(field).cloned();
                                if let Some(value) = existing {
                                    self.registers[base + dst.0 as usize] = value;
                                    continue;
                                }
                            }
                            self.registers[base + dst.0 as usize] =
                                Value::intrinsic(kind).with_receiver(t.clone());
                            continue;
                        }
                    }

                    // T2.3：显式迭代协议 `.iter()`（与其它原型方法一致，返回可调用的零参方法）
                    if field == "iter" && t.is_iterable() {
                        let iter = match t {
                            Value::Iterator(it) => Value::Iterator(it.clone()),
                            other => Value::iterator(Vm::make_iterator(other)),
                        };
                        self.registers[base + dst.0 as usize] =
                            Value::native_fn("iter", move |_| Ok(iter.clone()));
                        continue;
                    }

                    // M4：任务方法（`.cancel()` / `.timeout(ms)` / `.state` / `.id`）
                    if let Value::Task(cell) = t {
                        let bound = match field.as_str() {
                            "cancel" => Some(Value::bound_async(
                                AsyncKind::TaskCancel,
                                Value::Task(cell.clone()),
                            )),
                            "timeout" => Some(Value::bound_async(
                                AsyncKind::Timeout,
                                Value::Task(cell.clone()),
                            )),
                            "state" => Some(Value::string(cell.state().name())),
                            "id" => Some(Value::Int(cell.id as i64)),
                            "label" => Some(Value::string(cell.label.clone())),
                            _ => None,
                        };
                        if let Some(value) = bound {
                            self.registers[base + dst.0 as usize] = value;
                            continue;
                        }
                    }

                    let res = match t {
                        Value::Map(m) => {
                            if field == "len" {
                                Value::Int(m.read().unwrap().len() as i64)
                            } else if field == "keys" {
                                let m_clone = m.clone();
                                Value::native_fn("keys", move |_| {
                                    let map = m_clone.read().unwrap();
                                    let keys: Vec<Value> =
                                        map.keys().map(|k| Value::string(k.clone())).collect();
                                    Ok(Value::array(keys))
                                })
                            } else if field == "values" {
                                let m_clone = m.clone();
                                Value::native_fn("values", move |_| {
                                    let map = m_clone.read().unwrap();
                                    let vals: Vec<Value> = map.values().cloned().collect();
                                    Ok(Value::array(vals))
                                })
                            } else if field == "entries" {
                                let m_clone = m.clone();
                                Value::native_fn("entries", move |_| {
                                    let map = m_clone.read().unwrap();
                                    let entries: Vec<Value> = map
                                        .iter()
                                        .map(|(k, v)| {
                                            Value::array(vec![Value::string(k.clone()), v.clone()])
                                        })
                                        .collect();
                                    Ok(Value::array(entries))
                                })
                            } else if field == "contains_key" {
                                let m_clone = m.clone();
                                Value::native_fn("contains_key", move |args| {
                                    let key =
                                        args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(m_clone.read().unwrap().contains_key(&key)))
                                })
                            } else if field == "remove" {
                                let m_clone = m.clone();
                                Value::native_fn("remove", move |args| {
                                    let key =
                                        args.first().map(|a| a.to_string()).unwrap_or_default();
                                    let removed =
                                        m_clone.write().unwrap().remove(&key).unwrap_or(Value::Nil);
                                    Ok(removed)
                                })
                            } else if field == "merge" {
                                let m_clone = m.clone();
                                Value::native_fn("merge", move |args| {
                                    if let Some(Value::Map(other)) = args.first() {
                                        let other_map = other.read().unwrap();
                                        let mut map = m_clone.write().unwrap();
                                        for (k, v) in other_map.iter() {
                                            map.insert(k.clone(), v.clone());
                                        }
                                    }
                                    Ok(Value::Nil)
                                })
                            } else {
                                let map = m.read().unwrap();
                                map.get(field).cloned().unwrap_or(Value::Nil)
                            }
                        }
                        Value::Array(arr) => {
                            if field == "len" {
                                Value::Int(arr.read().unwrap().len() as i64)
                            } else if field == "push" {
                                let arr_clone = arr.clone();
                                Value::native_fn("push", move |args| {
                                    if let Some(item) = args.first() {
                                        arr_clone.write().unwrap().push(item.clone());
                                    }
                                    Ok(Value::Nil)
                                })
                            } else if field == "pop" {
                                let arr_clone = arr.clone();
                                Value::native_fn("pop", move |_| {
                                    let mut items = arr_clone.write().unwrap();
                                    Ok(items.pop().unwrap_or(Value::Nil))
                                })
                            } else if field == "insert" {
                                let arr_clone = arr.clone();
                                Value::native_fn("insert", move |args| {
                                    if args.len() < 2 {
                                        return Err("insert expects (index, value)".into());
                                    }
                                    let idx = args[0].as_int().ok_or("insert: index must be int")?
                                        as usize;
                                    let val = args[1].clone();
                                    let mut items = arr_clone.write().unwrap();
                                    if idx <= items.len() {
                                        items.insert(idx, val);
                                        Ok(Value::Nil)
                                    } else {
                                        Err("insert index out of bounds".into())
                                    }
                                })
                            } else if field == "remove" {
                                let arr_clone = arr.clone();
                                Value::native_fn("remove", move |args| {
                                    let idx = args
                                        .first()
                                        .and_then(|a| a.as_int())
                                        .ok_or("remove: index must be int")?
                                        as usize;
                                    let mut items = arr_clone.write().unwrap();
                                    if idx < items.len() {
                                        Ok(items.remove(idx))
                                    } else {
                                        Err("remove index out of bounds".into())
                                    }
                                })
                            } else if field == "contains" {
                                let arr_clone = arr.clone();
                                Value::native_fn("contains", move |args| {
                                    let item = args.first().cloned().unwrap_or(Value::Nil);
                                    let items = arr_clone.read().unwrap();
                                    Ok(Value::Bool(items.contains(&item)))
                                })
                            } else if field == "reverse" {
                                let arr_clone = arr.clone();
                                Value::native_fn("reverse", move |_| {
                                    arr_clone.write().unwrap().reverse();
                                    Ok(Value::Nil)
                                })
                            } else if field == "sort" {
                                let arr_clone = arr.clone();
                                Value::native_fn("sort", move |_| {
                                    let mut items = arr_clone.write().unwrap();
                                    items.sort_by(|a, b| match (a, b) {
                                        (Value::Int(x), Value::Int(y)) => x.cmp(y),
                                        (Value::Float(x), Value::Float(y)) => {
                                            x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                                        }
                                        (Value::Int(x), Value::Float(y)) => (*x as f64)
                                            .partial_cmp(y)
                                            .unwrap_or(std::cmp::Ordering::Equal),
                                        (Value::Float(x), Value::Int(y)) => x
                                            .partial_cmp(&(*y as f64))
                                            .unwrap_or(std::cmp::Ordering::Equal),
                                        (Value::String(x), Value::String(y)) => x.cmp(y),
                                        _ => std::cmp::Ordering::Equal,
                                    });
                                    Ok(Value::Nil)
                                })
                            } else if field == "await_all" {
                                // M4：绑定式异步操作，由 VM 调度器执行（可挂起）
                                Value::bound_async(AsyncKind::AwaitAll, Value::Array(arr.clone()))
                            } else if field == "await_any" || field == "await_race" {
                                Value::bound_async(AsyncKind::AwaitAny, Value::Array(arr.clone()))
                            } else {
                                Value::Nil
                            }
                        }
                        Value::String(s) => {
                            if field == "len" {
                                Value::Int(s.chars().count() as i64)
                            } else if field == "len_bytes" {
                                Value::Int(s.len() as i64)
                            } else if field == "chars" {
                                let chars: Vec<Value> =
                                    s.chars().map(|c| Value::string(c.to_string())).collect();
                                Value::native_fn("chars", move |_| Ok(Value::array(chars.clone())))
                            } else if field == "chars_with_idx" {
                                let chars: Vec<Value> = s
                                    .chars()
                                    .enumerate()
                                    .map(|(i, c)| {
                                        Value::array(vec![
                                            Value::Int(i as i64),
                                            Value::string(c.to_string()),
                                        ])
                                    })
                                    .collect();
                                Value::native_fn("chars_with_idx", move |_| {
                                    Ok(Value::array(chars.clone()))
                                })
                            } else if field == "lines" {
                                let lines: Vec<Value> = s.lines().map(Value::string).collect();
                                Value::native_fn("lines", move |_| Ok(Value::array(lines.clone())))
                            } else if field == "trim" {
                                let trimmed = s.trim().to_string();
                                Value::native_fn("trim", move |_| {
                                    Ok(Value::string(trimmed.clone()))
                                })
                            } else if field == "to_upper" {
                                let upper = s.to_uppercase();
                                Value::native_fn("to_upper", move |_| {
                                    Ok(Value::string(upper.clone()))
                                })
                            } else if field == "to_lower" {
                                let lower = s.to_lowercase();
                                Value::native_fn("to_lower", move |_| {
                                    Ok(Value::string(lower.clone()))
                                })
                            } else if field == "split" {
                                let s_clone = s.clone();
                                Value::native_fn("split", move |args| {
                                    let sep = args
                                        .first()
                                        .map(|a| a.to_string())
                                        .unwrap_or_else(|| " ".to_string());
                                    let parts: Vec<Value> =
                                        s_clone.split(&sep).map(Value::string).collect();
                                    Ok(Value::array(parts))
                                })
                            } else if field == "contains" {
                                let s_clone = s.clone();
                                Value::native_fn("contains", move |args| {
                                    let sub =
                                        args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(s_clone.contains(&sub)))
                                })
                            } else if field == "starts_with" {
                                let s_clone = s.clone();
                                Value::native_fn("starts_with", move |args| {
                                    let prefix =
                                        args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(s_clone.starts_with(&prefix)))
                                })
                            } else if field == "ends_with" {
                                let s_clone = s.clone();
                                Value::native_fn("ends_with", move |args| {
                                    let suffix =
                                        args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(s_clone.ends_with(&suffix)))
                                })
                            } else if field == "replace" {
                                let s_clone = s.clone();
                                Value::native_fn("replace", move |args| {
                                    if args.len() < 2 {
                                        return Err("replace requires (from, to)".into());
                                    }
                                    let from = args[0].to_string();
                                    let to = args[1].to_string();
                                    Ok(Value::string(s_clone.replace(&from, &to)))
                                })
                            } else if field == "slice" {
                                let s_clone = s.clone();
                                Value::native_fn("slice", move |args| {
                                    let char_count = s_clone.chars().count();
                                    let start = args.first().and_then(|a| a.as_int()).unwrap_or(0);
                                    let end = args
                                        .get(1)
                                        .and_then(|a| a.as_int())
                                        .unwrap_or(char_count as i64);

                                    let start_idx = (if start < 0 {
                                        (char_count as i64 + start).max(0)
                                    } else {
                                        start.min(char_count as i64)
                                    }) as usize;
                                    let end_idx = (if end < 0 {
                                        (char_count as i64 + end).max(0)
                                    } else {
                                        end.min(char_count as i64)
                                    }) as usize;

                                    if start_idx >= end_idx {
                                        Ok(Value::string(""))
                                    } else {
                                        let sliced: String = s_clone
                                            .chars()
                                            .skip(start_idx)
                                            .take(end_idx - start_idx)
                                            .collect();
                                        Ok(Value::string(sliced))
                                    }
                                })
                            } else {
                                Value::Nil
                            }
                        }
                        Value::Iterator(it) => {
                            if field == "next" {
                                let it_clone = it.clone();
                                Value::native_fn("next", move |_| {
                                    let mut state = it_clone.write().unwrap();
                                    Ok(Vm::iterator_next(&mut state).unwrap_or(Value::Nil))
                                })
                            } else {
                                Value::Nil
                            }
                        }
                        Value::Nil => {
                            if optional {
                                Value::Nil
                            } else {
                                return Err(VmError::TypeError(format!(
                                    "Cannot access property '{field}' on nil"
                                )));
                            }
                        }
                        _ => Value::Nil,
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::SetField {
                    target,
                    field_idx,
                    val,
                } => {
                    let t = &self.registers[base + target.0 as usize];
                    let field = self.frames[frame_idx].proto.names[field_idx as usize].clone();
                    let v = self.registers[base + val.0 as usize].clone();

                    if let Value::Map(m) = t {
                        m.write().unwrap().insert(field, v);
                    } else {
                        return Err(VmError::TypeError(format!(
                            "Cannot set property '{field}' on non-map"
                        )));
                    }
                }

                Instruction::StringConcat { dst, start, count } => {
                    let mut s = String::new();
                    for i in 0..count as usize {
                        s.push_str(&self.registers[base + start.0 as usize + i].to_string());
                    }
                    self.registers[base + dst.0 as usize] = Value::string(s);
                }

                Instruction::Closure { dst, proto_idx } => {
                    let child_proto = Arc::new(
                        self.frames[frame_idx].proto.child_protos[proto_idx as usize].clone(),
                    );
                    self.registers[base + dst.0 as usize] = Value::Closure(Arc::new(Closure {
                        proto: child_proto,
                        captures: Vec::new(),
                    }));
                }

                Instruction::MakeClosure {
                    dst,
                    proto_idx,
                    upvalue_start,
                    upvalue_count,
                } => {
                    let child_proto = Arc::new(
                        self.frames[frame_idx].proto.child_protos[proto_idx as usize].clone(),
                    );
                    // T2.2：从连续寄存器窗口取出捕获（都是 Value::Cell）
                    let start = base + upvalue_start.0 as usize;
                    let captures = self.registers[start..start + upvalue_count as usize].to_vec();
                    self.registers[base + dst.0 as usize] = Value::Closure(Arc::new(Closure {
                        proto: child_proto,
                        captures,
                    }));
                }

                Instruction::MakeCell { dst, src } => {
                    let value = self.registers[base + src.0 as usize].clone();
                    self.registers[base + dst.0 as usize] = Value::cell(value);
                }

                Instruction::CellGet { dst, cell } => {
                    let value = self.registers[base + cell.0 as usize].deref_cell();
                    self.registers[base + dst.0 as usize] = value;
                }

                Instruction::CellSet { cell, src } => {
                    let value = self.registers[base + src.0 as usize].clone();
                    let handle = self.registers[base + cell.0 as usize].clone();
                    if !handle.write_cell(value) {
                        return Err(VmError::TypeError(
                            "internal error: CellSet on a non-cell slot".to_string(),
                        ));
                    }
                }

                Instruction::UpvalueGet { dst, idx } => {
                    let captures = &self.frames[frame_idx].captures;
                    let value = captures
                        .get(idx as usize)
                        .map(|slot| slot.deref_cell())
                        .unwrap_or(Value::Nil);
                    self.registers[base + dst.0 as usize] = value;
                }

                Instruction::UpvalueSet { idx, src } => {
                    let value = self.registers[base + src.0 as usize].clone();
                    let slot = self.frames[frame_idx]
                        .captures
                        .get(idx as usize)
                        .cloned()
                        .ok_or_else(|| {
                            VmError::TypeError(format!("internal error: missing upvalue #{idx}"))
                        })?;
                    if !slot.write_cell(value) {
                        return Err(VmError::TypeError(
                            "internal error: upvalue slot is not a cell".to_string(),
                        ));
                    }
                }

                Instruction::Spawn {
                    dst,
                    callee,
                    arg_start,
                    arg_count,
                } => {
                    // T4.1：这里**不执行**被 spawn 的调用，只登记任务
                    let callee_val = self.registers[base + callee.0 as usize].clone();
                    let mut args = Vec::with_capacity(arg_count as usize);
                    for i in 0..arg_count as usize {
                        args.push(self.registers[base + arg_start.0 as usize + i].clone());
                    }
                    let label = task_label(&callee_val);
                    let id = self.scheduler.next_id();
                    let task = Value::new_task(id, label, callee_val, args);
                    if let Value::Task(cell) = &task {
                        self.scheduler.register(cell);
                        self.scheduler.enqueue(cell.id);
                    }
                    self.registers[base + dst.0 as usize] = task;
                }

                Instruction::SpawnValue { dst, src } => {
                    let val = self.registers[base + src.0 as usize].clone();
                    let id = self.scheduler.next_id();
                    let task = Value::ready_task(id, "value", val);
                    // 就绪任务也要登记：`await_all` 等按任务号取结果
                    if let Value::Task(cell) = &task {
                        self.scheduler.register(cell);
                    }
                    self.registers[base + dst.0 as usize] = task;
                }

                Instruction::Await { dst, future } => {
                    let fut = self.registers[base + future.0 as usize].clone();
                    let dst_reg = base + dst.0 as usize;
                    match fut {
                        Value::Task(cell) => {
                            // 已结算：直接取结果（快路径，与旧行为一致）
                            if cell.state().is_settled() {
                                self.registers[dst_reg] = self.cell_value(&cell)?;
                            } else if self.request_suspend(Suspend::Await {
                                targets: vec![cell.id],
                                mode: AwaitMode::All,
                            }) {
                                self.pending_resume = Some(PendingResume {
                                    dst: Some(dst_reg),
                                    source: ResumeSource::Await(cell.id),
                                });
                                self.scheduler.enqueue(cell.id);
                                continue;
                            } else {
                                // 回调上下文无法挂起：驱动调度器直到该任务结算
                                self.blocking_drain(&[cell.id])?;
                                self.registers[dst_reg] = self.cell_value(&cell)?;
                            }
                        }
                        other => self.registers[dst_reg] = other,
                    }
                }

                Instruction::IterInit { dst, src } => {
                    let val = self.registers[base + src.0 as usize].clone();
                    let iter = match val {
                        // 已是迭代器：原样传递（消费语义与 `.next()` 一致）
                        Value::Iterator(it) => Value::Iterator(it),
                        other => Value::iterator(Vm::make_iterator(&other)),
                    };
                    self.registers[base + dst.0 as usize] = iter;
                }

                Instruction::IterNext { dst, iter, offset } => {
                    let it_val = self.registers[base + iter.0 as usize].clone();
                    let mut advanced = false;
                    if let Value::Iterator(it) = it_val {
                        let mut state = it.write().unwrap();
                        if let Some(value) = Vm::iterator_next(&mut state) {
                            self.registers[base + dst.0 as usize] = value;
                            advanced = true;
                        }
                    }
                    if !advanced {
                        self.frames[frame_idx].ip =
                            (self.frames[frame_idx].ip as i32 + offset) as usize;
                    }
                }
            }
        }
    }
}
