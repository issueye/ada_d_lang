//! 任务（Task）与协作式调度器的记账结构（开发计划 M4，决策 D2）。
//!
//! 并发模型：**单线程协作式事件循环**（无 OS 线程、无 Tokio）。
//! - 每个任务是一段可恢复的字节码执行现场（帧栈 + 寄存器窗口）；
//! - 任务只在**挂起点**让出控制权：`sleep_ms`、`await`、`await_all`/`await_any`、`yield_now`；
//! - 调度顺序为就绪队列 FIFO，定时器按到期时间，因此同一脚本的执行顺序是确定的；
//! - `sleep_ms` 使用**真实时钟**：多个任务并行睡眠只花最长的那个时间（事件循环等待），
//!   而不是各自阻塞相加。
//!
//! 记账结构放在本模块，真正的执行/切换逻辑在 `vm.rs`（`run_scheduler`）。

use crate::value::Value;
use crate::vm::{CallFrame, VmError};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

pub type TaskId = u64;

/// 主任务的伪任务号（脚本入口）。
pub const MAIN_TASK: TaskId = 0;

/// 任务生命周期状态（SPEC_ASYNC §5 契约）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// 已创建但尚未开始执行（`spawn` 之后、首次被调度之前）
    Pending,
    /// 在就绪队列中等待执行
    Queued,
    /// 正在执行
    Running,
    /// 已挂起：执行现场被保存，等待唤醒条件
    Waiting,
    /// 正常结束
    Done,
    /// 执行中出错结束
    Failed,
    /// 被取消
    Cancelled,
}

impl TaskState {
    pub fn name(self) -> &'static str {
        match self {
            TaskState::Pending => "pending",
            TaskState::Queued => "queued",
            TaskState::Running => "running",
            TaskState::Waiting => "waiting",
            TaskState::Done => "done",
            TaskState::Failed => "failed",
            TaskState::Cancelled => "cancelled",
        }
    }

    /// 是否为终态（不会再被调度）。
    pub fn is_settled(self) -> bool {
        matches!(
            self,
            TaskState::Done | TaskState::Failed | TaskState::Cancelled
        )
    }
}

/// 任务的共享句柄（`Value::Task` 持有）。
pub struct TaskCell {
    pub id: TaskId,
    pub label: String,
    pub inner: RwLock<TaskInner>,
}

pub struct TaskInner {
    pub state: TaskState,
    /// `Pending` 时的调用目标与其实参
    pub call: Option<(Value, Vec<Value>)>,
    /// 终态结果：`Done` → `Ok`，`Failed` → 原始错误，`Cancelled` → `TaskCancelled`
    pub result: Option<Result<Value, VmError>>,
    /// 结算序号：`await_any` 用它判定"谁先完成"（0 表示尚未结算）
    pub settled_seq: u64,
}

impl TaskCell {
    pub fn new(id: TaskId, label: impl Into<String>, callee: Value, args: Vec<Value>) -> Self {
        Self {
            id,
            label: label.into(),
            inner: RwLock::new(TaskInner {
                state: TaskState::Pending,
                call: Some((callee, args)),
                result: None,
                settled_seq: 0,
            }),
        }
    }

    /// 立刻完成的任务（`spawn <非调用表达式>`）。
    pub fn done(id: TaskId, label: impl Into<String>, value: Value) -> Self {
        Self {
            id,
            label: label.into(),
            inner: RwLock::new(TaskInner {
                state: TaskState::Done,
                call: None,
                result: Some(Ok(value)),
                settled_seq: 0,
            }),
        }
    }

    pub fn state(&self) -> TaskState {
        self.inner.read().unwrap().state
    }

    pub fn settled_seq(&self) -> u64 {
        self.inner.read().unwrap().settled_seq
    }

    /// 取出终态结果（未结算时返回 `None`）。
    pub fn result(&self) -> Option<Result<Value, VmError>> {
        self.inner.read().unwrap().result.clone()
    }

    pub fn take_call(&self) -> Option<(Value, Vec<Value>)> {
        self.inner.write().unwrap().call.take()
    }

    pub fn set_state(&self, state: TaskState) {
        self.inner.write().unwrap().state = state;
    }

    /// 尝试取消。只有未进入终态的任务会被取消；返回是否生效。
    pub fn cancel(&self) -> bool {
        let mut inner = self.inner.write().unwrap();
        if inner.state.is_settled() {
            return false;
        }
        inner.state = TaskState::Cancelled;
        inner.call = None;
        inner.result = Some(Err(VmError::TaskCancelled(self.label.clone())));
        true
    }

    /// 结算任务；`seq` 由调度器单调递增，用于 `await_any`。
    pub fn settle(&self, result: Result<Value, VmError>, seq: u64) {
        let mut inner = self.inner.write().unwrap();
        inner.state = if result.is_ok() {
            TaskState::Done
        } else {
            TaskState::Failed
        };
        inner.result = Some(result);
        inner.settled_seq = seq;
        inner.call = None;
    }
}

/// 挂起原因：决定任务何时被重新放回就绪队列。
#[derive(Debug, Clone)]
pub enum Suspend {
    /// 睡到某个墙钟时刻
    Sleep { deadline_ms: u64 },
    /// 等待一组任务结算
    Await {
        targets: Vec<TaskId>,
        mode: AwaitMode,
    },
    /// 等待某个任务结算，或到 deadline 时超时（T4.5）
    Timeout { target: TaskId, deadline_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwaitMode {
    /// 全部结算才唤醒
    All,
    /// 任一结算即唤醒
    Any,
}

/// 恢复一个挂起任务时需要补做的动作（挂起点之后的收尾）。
#[derive(Debug, Clone)]
pub struct PendingResume {
    /// 结果写回的绝对寄存器下标（无返回值时为 `None`）
    pub dst: Option<usize>,
    pub source: ResumeSource,
}

#[derive(Debug, Clone)]
pub enum ResumeSource {
    /// `sleep_ms`：写回 `nil`
    Sleep,
    /// `await task`：写回该任务的返回值（失败则抛出）
    Await(TaskId),
    /// `await_all([...])`：写回结果数组
    AwaitAll(Vec<TaskId>),
    /// `await_any([...])`：写回最先结算者的值
    AwaitAny(Vec<TaskId>),
    /// `timeout(task, ms)`：任务先结算则写回其值，超时则取消任务并写回 `nil`
    Timeout(TaskId),
}

/// 一个任务的执行现场。
pub struct SavedExecution {
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) registers: Vec<Value>,
    pub(crate) resume: Option<PendingResume>,
}

/// 协作式调度器的记账部分（不执行字节码）。
pub struct Scheduler {
    next_id: TaskId,
    next_seq: u64,
    cells: HashMap<TaskId, Arc<TaskCell>>,
    ready: VecDeque<TaskId>,
    timers: Vec<(u64, TaskId)>,
    waiters: HashMap<TaskId, Vec<TaskId>>,
    saved: HashMap<TaskId, SavedExecution>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            next_seq: 1,
            cells: HashMap::new(),
            ready: VecDeque::new(),
            timers: Vec::new(),
            waiters: HashMap::new(),
            saved: HashMap::new(),
        }
    }

    /// 每次 `Vm::execute` 重置：脚本之间不共享任务。
    pub fn reset(&mut self) {
        self.next_id = 1;
        self.next_seq = 1;
        self.cells.clear();
        self.ready.clear();
        self.timers.clear();
        self.waiters.clear();
        self.saved.clear();
    }

    pub fn next_id(&mut self) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn next_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    pub fn register(&mut self, cell: &Arc<TaskCell>) {
        self.cells.insert(cell.id, cell.clone());
    }

    pub fn cell(&self, id: TaskId) -> Option<&Arc<TaskCell>> {
        self.cells.get(&id)
    }

    /// 把任务放入就绪队列；已在队列中或已结算则返回 false。
    pub fn enqueue(&mut self, id: TaskId) -> bool {
        if id == MAIN_TASK {
            if self.ready.contains(&MAIN_TASK) {
                return false;
            }
            self.ready.push_back(MAIN_TASK);
            return true;
        }
        let Some(cell) = self.cells.get(&id) else {
            return false;
        };
        let mut inner = cell.inner.write().unwrap();
        if inner.state.is_settled() || inner.state == TaskState::Queued {
            return false;
        }
        inner.state = TaskState::Queued;
        drop(inner);
        self.ready.push_back(id);
        true
    }

    /// 取下一个真正可运行的任务（跳过已被取消的排队任务）。
    pub fn pop_ready(&mut self) -> Option<TaskId> {
        while let Some(id) = self.ready.pop_front() {
            if id == MAIN_TASK {
                return Some(id);
            }
            match self.cells.get(&id) {
                Some(cell) if cell.state() == TaskState::Queued => return Some(id),
                Some(_) => continue,
                None => continue,
            }
        }
        None
    }

    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    pub fn add_timer(&mut self, deadline_ms: u64, id: TaskId) {
        self.timers.push((deadline_ms, id));
    }

    pub fn next_deadline(&self) -> Option<u64> {
        self.timers.iter().map(|(d, _)| *d).min()
    }

    pub fn has_timers(&self) -> bool {
        !self.timers.is_empty()
    }

    /// 把到期的定时器任务放回就绪队列，返回唤醒数量。
    pub fn wake_due(&mut self, now_ms: u64) -> usize {
        let mut due = Vec::new();
        self.timers.retain(|(deadline, id)| {
            if *deadline <= now_ms {
                due.push(*id);
                false
            } else {
                true
            }
        });
        let mut woken = 0;
        for id in due {
            // 目标已结算（例如被取消）时不再唤醒
            if id != MAIN_TASK {
                match self.cells.get(&id) {
                    Some(cell) if !cell.state().is_settled() => {}
                    _ => continue,
                }
            }
            if self.enqueue(id) {
                woken += 1;
            }
        }
        woken
    }

    /// 保存挂起任务的执行现场。
    pub fn park(&mut self, id: TaskId, execution: SavedExecution) {
        self.saved.insert(id, execution);
    }

    pub fn take_parked(&mut self, id: TaskId) -> Option<SavedExecution> {
        self.saved.remove(&id)
    }

    pub fn has_parked(&self, id: TaskId) -> bool {
        self.saved.contains_key(&id)
    }

    pub fn add_waiter(&mut self, target: TaskId, waiter: TaskId) {
        let list = self.waiters.entry(target).or_default();
        if !list.contains(&waiter) {
            list.push(waiter);
        }
    }

    pub fn take_waiters(&mut self, target: TaskId) -> Vec<TaskId> {
        self.waiters.remove(&target).unwrap_or_default()
    }

    /// 调度器是否已无任何可推进的工作（既无就绪任务也无定时器）。
    pub fn is_idle(&self) -> bool {
        self.ready.is_empty() && self.timers.is_empty()
    }

    /// 仍处于非终态的任务数（诊断用）。
    pub fn live_tasks(&self) -> usize {
        self.cells
            .values()
            .filter(|c| !c.state().is_settled())
            .count()
    }
}
