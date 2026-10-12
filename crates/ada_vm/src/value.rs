use ada_bytecode::FunctionProto;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

pub type NativeCallback = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

#[derive(Clone)]
pub struct Closure {
    pub proto: Arc<FunctionProto>,
    /// 捕获的 upvalue（T2.2）：每个捕获都是一个 `Value::Cell`，
    /// 因此闭包与外层函数共享同一个可写变量槽。
    /// 捕获表：Arc 共享，闭包调用时只做一次引用计数（T6.2）
    pub captures: Arc<Vec<Value>>,
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.proto, &other.proto)
    }
}

impl fmt::Debug for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<fn {}>", self.proto.name)
    }
}

/// VM 内建的高阶迭代器适配器（T2.1）。
///
/// 之所以放在 VM 而不是 `builtins`：适配器必须回调**脚本函数**（闭包），
/// 而 `NativeFn` 的回调签名只能拿到 `&[Value]`，无法访问虚拟机去压栈执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Map,
    Filter,
    Take,
    Skip,
    Collect,
    Count,
    Find,
    Any,
    All,
    Enumerate,
    Rev,
    Join,
    Sum,
}

impl AdapterKind {
    pub fn name(self) -> &'static str {
        match self {
            AdapterKind::Map => "map",
            AdapterKind::Filter => "filter",
            AdapterKind::Take => "take",
            AdapterKind::Skip => "skip",
            AdapterKind::Collect => "collect",
            AdapterKind::Count => "count",
            AdapterKind::Find => "find",
            AdapterKind::Any => "any",
            AdapterKind::All => "all",
            AdapterKind::Enumerate => "enumerate",
            AdapterKind::Rev => "rev",
            AdapterKind::Join => "join",
            AdapterKind::Sum => "sum",
        }
    }

    /// 方法形式 `.map(...)` 的名字识别（T2.1）。
    pub fn from_method(field: &str) -> Option<Self> {
        AdapterKind::all()
            .iter()
            .copied()
            .find(|k| k.name() == field)
    }

    pub fn all() -> &'static [AdapterKind] {
        &[
            AdapterKind::Map,
            AdapterKind::Filter,
            AdapterKind::Take,
            AdapterKind::Skip,
            AdapterKind::Collect,
            AdapterKind::Count,
            AdapterKind::Find,
            AdapterKind::Any,
            AdapterKind::All,
            AdapterKind::Enumerate,
            AdapterKind::Rev,
            AdapterKind::Join,
            AdapterKind::Sum,
        ]
    }

    /// 该适配器接受的实参个数（**包含**可迭代值本身），`None` 表示无上界。
    pub fn arity(self) -> (usize, Option<usize>) {
        match self {
            AdapterKind::Map
            | AdapterKind::Filter
            | AdapterKind::Find
            | AdapterKind::Take
            | AdapterKind::Skip
            | AdapterKind::Any
            | AdapterKind::All => (2, Some(2)),
            AdapterKind::Join => (1, Some(2)),
            AdapterKind::Collect
            | AdapterKind::Count
            | AdapterKind::Enumerate
            | AdapterKind::Rev
            | AdapterKind::Sum => (1, Some(1)),
        }
    }
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<String>),
    Array(Arc<RwLock<Vec<Value>>>),
    Map(Arc<RwLock<HashMap<String, Value>>>),
    Closure(Arc<Closure>),
    NativeFn(Arc<NativeFunction>),
    /// 迭代器适配器（T2.1）：全局形式 `map(xs, f)` 与绑定形式 `xs.map(f)` 共用。
    /// `receiver` 为方法形式的接收者，`args` 为已绑定的额外实参。
    Intrinsic(Arc<IntrinsicCall>),
    /// 被闭包捕获的变量槽（T2.2）：内外共享同一个 `RwLock`，因此写入互相可见。
    /// 正常路径上不会泄漏到用户代码——只出现在局部寄存器与闭包捕获表里。
    Cell(Arc<RwLock<Value>>),
    /// 协作式任务（M4）：`spawn` 创建，`await` 等待。
    Task(Arc<crate::task::TaskCell>),
    /// 需要 VM 配合的内建操作（M4）：可能挂起当前任务（`sleep_ms` / `await_all` …）。
    /// `receiver` 为方法形式的接收者，调用点实参紧随其后。
    Async {
        kind: AsyncKind,
        receiver: Option<Arc<Value>>,
    },
    Iterator(Arc<RwLock<IteratorState>>),
}

/// 原生函数的载荷（T6.2b）。
///
/// 单独立即分配：`String`(24B) + `Arc<dyn Fn>`(16B) 若内联进 `Value`，会把枚举撑到
/// 48 字节，寄存器文件（2048 槽）随之变成 96 KiB 而溢出 L1。放进 `Arc` 后 `Value`
/// 保持 16 字节（见 `crates/ada_vm/tests/value_layout.rs`）。
#[derive(Clone)]
pub struct NativeFunction {
    pub name: String,
    pub callback: NativeCallback,
}

/// 迭代器适配器的载荷（T6.2b）：`Vec<Value>`(24B) 同样会撑大每个 `Value`。
#[derive(Clone)]
pub struct IntrinsicCall {
    pub kind: AdapterKind,
    pub receiver: Option<Box<Value>>,
    pub args: Vec<Value>,
}

/// 需要 VM 配合（可能挂起）的内建操作（M4）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsyncKind {
    /// `sleep_ms(ms)`：挂起到真实墙钟时刻
    Sleep,
    /// `yield_now()`：让出一次调度（不推进墙钟）
    YieldNow,
    /// `await_all([tasks])` / `arr.await_all()`：等全部结算
    AwaitAll,
    /// `await_any([tasks])` / `arr.await_any()`：等任一结算
    AwaitAny,
    /// `task.cancel()`：取消任务
    TaskCancel,
    /// `timeout(task, ms)` / `task.timeout(ms)`：超时取消并返回 `nil`（T4.5）
    Timeout,
}

impl AsyncKind {
    pub fn name(self) -> &'static str {
        match self {
            AsyncKind::Sleep => "sleep_ms",
            AsyncKind::YieldNow => "yield_now",
            AsyncKind::AwaitAll => "await_all",
            AsyncKind::AwaitAny => "await_any",
            AsyncKind::TaskCancel => "cancel",
            AsyncKind::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Debug)]
pub enum IteratorState {
    Array {
        items: Vec<Value>,
        index: usize,
    },
    String {
        chars: Vec<char>,
        index: usize,
    },
    Map {
        entries: Vec<(String, Value)>,
        index: usize,
    },
    Range {
        current: i64,
        end: i64,
        step: i64,
    },
}

impl Value {
    /// 已完成的任务值（`spawn <非调用表达式>`）。
    pub fn ready_task(id: crate::task::TaskId, label: impl Into<String>, val: Value) -> Self {
        Value::Task(Arc::new(crate::task::TaskCell::done(id, label, val)))
    }

    /// 新建（尚未执行）的任务值。
    pub fn new_task(
        id: crate::task::TaskId,
        label: impl Into<String>,
        callee: Value,
        args: Vec<Value>,
    ) -> Self {
        Value::Task(Arc::new(crate::task::TaskCell::new(
            id, label, callee, args,
        )))
    }

    /// 绑定式异步操作（方法形式）：接收者会被拼到调用实参之前。
    pub fn bound_async(kind: AsyncKind, receiver: Value) -> Self {
        Value::Async {
            kind,
            receiver: Some(Arc::new(receiver)),
        }
    }

    pub fn iterator(state: IteratorState) -> Self {
        Value::Iterator(Arc::new(RwLock::new(state)))
    }

    pub fn string(s: impl Into<String>) -> Self {
        Value::String(Arc::new(s.into()))
    }

    pub fn array(items: Vec<Value>) -> Self {
        Value::Array(Arc::new(RwLock::new(items)))
    }

    pub fn map(entries: HashMap<String, Value>) -> Self {
        Value::Map(Arc::new(RwLock::new(entries)))
    }

    pub fn native_fn(
        name: impl Into<String>,
        cb: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        Self::native_fn_shared(name, Arc::new(cb))
    }

    /// 用**已构造好的**回调创建原生函数（T6.2b）。
    ///
    /// 宿主模块表 / 引擎内部手里已经握着 `Arc<dyn Fn>`，这里直接复用而不是再包一层。
    pub fn native_fn_shared(name: impl Into<String>, callback: NativeCallback) -> Self {
        Value::NativeFn(Arc::new(NativeFunction {
            name: name.into(),
            callback,
        }))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// 未绑定的适配器（全局函数形式）。
    pub fn intrinsic(kind: AdapterKind) -> Self {
        Value::Intrinsic(Arc::new(IntrinsicCall {
            kind,
            receiver: None,
            args: Vec::new(),
        }))
    }

    /// 把值包成可共享的变量槽（T2.2）。
    pub fn cell(value: Value) -> Self {
        Value::Cell(Arc::new(RwLock::new(value)))
    }

    /// 若为 Cell 则读出内部值（并返回 `true`）；否则克隆自身。
    pub fn deref_cell(&self) -> Value {
        match self {
            Value::Cell(cell) => cell.read().unwrap().clone(),
            other => other.clone(),
        }
    }

    /// 若为 Cell 则写入内部值；返回是否写成功。
    pub fn write_cell(&self, value: Value) -> bool {
        match self {
            Value::Cell(cell) => {
                *cell.write().unwrap() = value;
                true
            }
            _ => false,
        }
    }

    /// 绑定接收者（方法形式 `xs.map`）。
    pub fn with_receiver(self, receiver: Value) -> Self {
        match self {
            Value::Intrinsic(mut call) => {
                // 唯一持有时原地改，避免复制 args
                Arc::make_mut(&mut call).receiver = Some(Box::new(receiver));
                Value::Intrinsic(call)
            }
            other => other,
        }
    }

    pub fn adapter_kind(&self) -> Option<AdapterKind> {
        match self {
            Value::Intrinsic(call) => Some(call.kind),
            _ => None,
        }
    }

    /// 是否为可迭代值（`for..in` 与适配器都接受这些类型）。
    pub fn is_iterable(&self) -> bool {
        matches!(
            self,
            Value::Array(_) | Value::String(_) | Value::Map(_) | Value::Iterator(_) | Value::Int(_)
        )
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0 && !f.is_nan(),
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Nil => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(n) => serde_json::Value::Number((*n).into()),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::String(s) => serde_json::Value::String((**s).clone()),
            Value::Array(arr) => {
                let items = arr.read().unwrap();
                serde_json::Value::Array(items.iter().map(|v| v.to_json()).collect())
            }
            Value::Map(m) => {
                let map = m.read().unwrap();
                let mut obj = serde_json::Map::new();
                for (k, v) in map.iter() {
                    obj.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(obj)
            }
            Value::Closure(c) => serde_json::Value::String(format!("<fn {}>", c.proto.name)),
            Value::NativeFn(f) => serde_json::Value::String(format!("<native fn {}>", f.name)),
            Value::Task(t) => serde_json::Value::String(format!("<task {}>", t.state().name())),
            Value::Iterator(_) => serde_json::Value::String("<iterator>".into()),
            Value::Intrinsic(call) => {
                serde_json::Value::String(format!("<adapter {}>", call.kind.name()))
            }
            Value::Async { kind, .. } => {
                serde_json::Value::String(format!("<async {}>", kind.name()))
            }
            Value::Cell(cell) => cell.read().unwrap().to_json(),
        }
    }

    pub fn from_json(json: &serde_json::Value) -> Self {
        match json {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(num) => {
                if let Some(i) = num.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = num.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Nil
                }
            }
            serde_json::Value::String(s) => Value::string(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::array(arr.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (k, v) in obj {
                    map.insert(k.clone(), Value::from_json(v));
                }
                Value::map(map)
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => Arc::ptr_eq(a, b),
            (Value::Map(a), Value::Map(b)) => Arc::ptr_eq(a, b),
            (Value::Closure(a), Value::Closure(b)) => a == b,
            (Value::NativeFn(a), Value::NativeFn(b)) => a.name == b.name,
            (Value::Task(a), Value::Task(b)) => Arc::ptr_eq(a, b),
            (Value::Iterator(a), Value::Iterator(b)) => Arc::ptr_eq(a, b),
            (Value::Intrinsic(a), Value::Intrinsic(b)) => a.kind == b.kind,
            (Value::Async { kind: a, .. }, Value::Async { kind: b, .. }) => a == b,
            (Value::Cell(a), Value::Cell(b)) => {
                Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap()
            }
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => write!(f, "{fl}"),
            Value::String(s) => write!(f, "\"{s}\""),
            Value::Array(arr) => {
                let items = arr.read().unwrap();
                write!(f, "{items:?}")
            }
            Value::Map(m) => {
                let map = m.read().unwrap();
                write!(f, "{map:?}")
            }
            Value::Closure(c) => write!(f, "{c:?}"),
            Value::NativeFn(native) => write!(f, "<native fn {}>", native.name),
            Value::Task(t) => write!(f, "<task #{} {} {}>", t.id, t.label, t.state().name()),
            Value::Iterator(it) => write!(f, "Iterator({:?})", it.read().unwrap()),
            Value::Intrinsic(call) => write!(f, "<adapter {}>", call.kind.name()),
            Value::Async { kind, .. } => write!(f, "<async {}>", kind.name()),
            Value::Cell(cell) => write!(f, "<cell {:?}>", cell.read().unwrap()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                // T3.6：整数值的浮点保留小数位，避免与 int 在输出上混淆（1.0 而非 1）；
                // 极大/极小值改用科学计数法，避免打印出几百位数字
                let magnitude = fl.abs();
                if fl.is_finite() && fl.fract() == 0.0 && magnitude < 1e15 {
                    write!(f, "{fl:.1}")
                } else if fl.is_finite() && (magnitude >= 1e15 || (*fl != 0.0 && magnitude < 1e-10))
                {
                    write!(f, "{fl:e}")
                } else {
                    write!(f, "{fl}")
                }
            }
            Value::String(s) => write!(f, "{s}"),
            Value::Array(arr) => {
                let items = arr.read().unwrap();
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                let map = m.read().unwrap();
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Value::Closure(c) => write!(f, "<fn {}>", c.proto.name),
            Value::NativeFn(native) => write!(f, "<native fn {}>", native.name),
            Value::Task(t) => write!(f, "<task #{} {}>", t.id, t.state().name()),
            Value::Iterator(_) => write!(f, "<iterator>"),
            Value::Intrinsic(call) => write!(f, "<adapter {}>", call.kind.name()),
            Value::Async { kind, .. } => write!(f, "<async {}>", kind.name()),
            Value::Cell(cell) => write!(f, "{}", cell.read().unwrap()),
        }
    }
}
