use ada_bytecode::FunctionProto;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

pub type NativeCallback = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

#[derive(Clone)]
pub struct Closure {
    pub proto: Arc<FunctionProto>,
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
    NativeFn {
        name: String,
        callback: NativeCallback,
    },
    /// 迭代器适配器（T2.1）：全局形式 `map(xs, f)` 与绑定形式 `xs.map(f)` 共用。
    /// `receiver` 为方法形式的接收者，`args` 为已绑定的额外实参。
    Intrinsic {
        kind: AdapterKind,
        receiver: Option<Box<Value>>,
        args: Vec<Value>,
    },
    Task(Arc<RwLock<Value>>),
    Iterator(Arc<RwLock<IteratorState>>),
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
    pub fn task(val: Value) -> Self {
        Value::Task(Arc::new(RwLock::new(val)))
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
        Value::NativeFn {
            name: name.into(),
            callback: Arc::new(cb),
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// 未绑定的适配器（全局函数形式）。
    pub fn intrinsic(kind: AdapterKind) -> Self {
        Value::Intrinsic {
            kind,
            receiver: None,
            args: Vec::new(),
        }
    }

    /// 绑定接收者（方法形式 `xs.map`）。
    pub fn with_receiver(self, receiver: Value) -> Self {
        match self {
            Value::Intrinsic { kind, args, .. } => Value::Intrinsic {
                kind,
                receiver: Some(Box::new(receiver)),
                args,
            },
            other => other,
        }
    }

    pub fn adapter_kind(&self) -> Option<AdapterKind> {
        match self {
            Value::Intrinsic { kind, .. } => Some(*kind),
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
            Value::NativeFn { name, .. } => {
                serde_json::Value::String(format!("<native fn {name}>"))
            }
            Value::Task(t) => t.read().unwrap().to_json(),
            Value::Iterator(_) => serde_json::Value::String("<iterator>".into()),
            Value::Intrinsic { kind, .. } => {
                serde_json::Value::String(format!("<adapter {}>", kind.name()))
            }
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
            (Value::NativeFn { name: a, .. }, Value::NativeFn { name: b, .. }) => a == b,
            (Value::Task(a), Value::Task(b)) => {
                Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap()
            }
            (Value::Iterator(a), Value::Iterator(b)) => Arc::ptr_eq(a, b),
            (Value::Intrinsic { kind: a, .. }, Value::Intrinsic { kind: b, .. }) => a == b,
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
            Value::NativeFn { name, .. } => write!(f, "<native fn {name}>"),
            Value::Task(t) => write!(f, "Task({:?})", t.read().unwrap()),
            Value::Iterator(it) => write!(f, "Iterator({:?})", it.read().unwrap()),
            Value::Intrinsic { kind, .. } => write!(f, "<adapter {}>", kind.name()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => write!(f, "{fl}"),
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
            Value::NativeFn { name, .. } => write!(f, "<native fn {name}>"),
            Value::Task(t) => write!(f, "<task {}>", t.read().unwrap()),
            Value::Iterator(_) => write!(f, "<iterator>"),
            Value::Intrinsic { kind, .. } => write!(f, "<adapter {}>", kind.name()),
        }
    }
}
