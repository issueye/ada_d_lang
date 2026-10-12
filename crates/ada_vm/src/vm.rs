use crate::value::{Closure, IteratorState, Value};
use ada_bytecode::{Constant, FunctionProto, Instruction};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
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
}

struct CallFrame {
    proto: Arc<FunctionProto>,
    ip: usize,
    base_reg: usize,
    dst_reg: Option<usize>,
}

pub struct Vm {
    registers: Vec<Value>,
    frames: Vec<CallFrame>,
    globals: HashMap<String, Value>,
    fuel: Option<u64>,
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
        };
        vm.register_builtins();
        vm
    }

    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.fuel = fuel;
    }

    pub fn get_fuel(&self) -> Option<u64> {
        self.fuel
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
        });

        self.run_loop()
    }

    fn run_loop(&mut self) -> Result<Value, VmError> {
        loop {
            // Check Fuel
            if let Some(ref mut fuel) = self.fuel {
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
                if self.frames.is_empty() {
                    return Ok(Value::Nil);
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
                        (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
                        (Value::Float(x), Value::Float(y)) => Value::Float(x + y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 + y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x + *y as f64),
                        (Value::String(x), y) => Value::string(format!("{x}{y}")),
                        (x, Value::String(y)) => Value::string(format!("{x}{y}")),
                        _ => {
                            return Err(VmError::TypeError(format!(
                                "Cannot add {a:?} and {b:?}"
                            )))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Sub { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => Value::Int(x - y),
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
                        (Value::Int(x), Value::Int(y)) => Value::Int(x * y),
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
                            if x % y == 0 {
                                Value::Int(x / y)
                            } else {
                                Value::Float(*x as f64 / *y as f64)
                            }
                        }
                        (Value::Float(x), Value::Float(y)) => Value::Float(x / y),
                        (Value::Int(x), Value::Float(y)) => Value::Float(*x as f64 / y),
                        (Value::Float(x), Value::Int(y)) => Value::Float(x / *y as f64),
                        _ => {
                            return Err(VmError::TypeError(format!(
                                "Cannot divide {a:?} by {b:?}"
                            )))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Mod { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => Value::Int(x % y),
                        (Value::Float(x), Value::Float(y)) => Value::Float(x % y),
                        _ => {
                            return Err(VmError::TypeError(format!(
                                "Cannot modulo {a:?} by {b:?}"
                            )))
                        }
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::Neg { dst, src } => {
                    let val = &self.registers[base + src.0 as usize];
                    let res = match val {
                        Value::Int(n) => Value::Int(-n),
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
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x < y,
                        (Value::Float(x), Value::Float(y)) => x < y,
                        (Value::Int(x), Value::Float(y)) => (*x as f64) < *y,
                        (Value::Float(x), Value::Int(y)) => *x < (*y as f64),
                        _ => false,
                    };
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Le { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x <= y,
                        (Value::Float(x), Value::Float(y)) => x <= y,
                        (Value::Int(x), Value::Float(y)) => (*x as f64) <= *y,
                        (Value::Float(x), Value::Int(y)) => *x <= (*y as f64),
                        _ => false,
                    };
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Gt { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x > y,
                        (Value::Float(x), Value::Float(y)) => x > y,
                        (Value::Int(x), Value::Float(y)) => (*x as f64) > *y,
                        (Value::Float(x), Value::Int(y)) => *x > (*y as f64),
                        _ => false,
                    };
                    self.registers[base + dst.0 as usize] = Value::Bool(res);
                }

                Instruction::Ge { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x >= y,
                        (Value::Float(x), Value::Float(y)) => x >= y,
                        (Value::Int(x), Value::Float(y)) => (*x as f64) >= *y,
                        (Value::Float(x), Value::Int(y)) => *x >= (*y as f64),
                        _ => false,
                    };
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
                        self.registers[base + dst.0 as usize] = Value::Int(x << y);
                    } else {
                        return Err(VmError::TypeError("Shift requires integers".into()));
                    }
                }

                Instruction::Shr { dst, lhs, rhs } => {
                    let a = &self.registers[base + lhs.0 as usize];
                    let b = &self.registers[base + rhs.0 as usize];
                    if let (Value::Int(x), Value::Int(y)) = (a, b) {
                        self.registers[base + dst.0 as usize] = Value::Int(x >> y);
                    } else {
                        return Err(VmError::TypeError("Shift requires integers".into()));
                    }
                }

                Instruction::Jump { offset } => {
                    self.frames[frame_idx].ip =
                        (self.frames[frame_idx].ip as i32 + offset) as usize;
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

                            let new_base = base + self.frames[frame_idx].proto.register_count as usize;
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
                            });
                        }
                        Value::NativeFn { callback, .. } => {
                            let start = base + arg_start.0 as usize;
                            let end = start + arg_count as usize;
                            let args = &self.registers[start..end];
                            let res = callback(args).map_err(VmError::NativeError)?;
                            self.registers[base + dst.0 as usize] = res;
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

                    if self.frames.is_empty() {
                        return Ok(ret_val);
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
                            if *i < 0 || *i as usize >= items.len() {
                                Value::Nil
                            } else {
                                items[*i as usize].clone()
                            }
                        }
                        (Value::Map(m), key) => {
                            let map = m.read().unwrap();
                            map.get(&key.to_string()).cloned().unwrap_or(Value::Nil)
                        }
                        (Value::String(s), Value::Int(i)) => {
                            if *i < 0 || *i as usize >= s.len() {
                                Value::Nil
                            } else {
                                Value::string(&s[*i as usize..*i as usize + 1])
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
                            if *i >= 0 && (*i as usize) < items.len() {
                                items[*i as usize] = v;
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
                                    let key = args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(m_clone.read().unwrap().contains_key(&key)))
                                })
                            } else if field == "remove" {
                                let m_clone = m.clone();
                                Value::native_fn("remove", move |args| {
                                    let key = args.first().map(|a| a.to_string()).unwrap_or_default();
                                    let removed = m_clone.write().unwrap().remove(&key).unwrap_or(Value::Nil);
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
                                    let idx = args[0].as_int().ok_or("insert: index must be int")? as usize;
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
                                    let idx = args.first().and_then(|a| a.as_int()).ok_or("remove: index must be int")? as usize;
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
                                        (Value::Int(x), Value::Float(y)) => {
                                            (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                                        }
                                        (Value::Float(x), Value::Int(y)) => {
                                            x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)
                                        }
                                        (Value::String(x), Value::String(y)) => x.cmp(y),
                                        _ => std::cmp::Ordering::Equal,
                                    });
                                    Ok(Value::Nil)
                                })
                            } else if field == "sum" {
                                let arr_clone = arr.clone();
                                Value::native_fn("sum", move |_| {
                                    let items = arr_clone.read().unwrap();
                                    let mut is_float = false;
                                    let mut int_sum = 0i64;
                                    let mut float_sum = 0.0f64;
                                    for item in items.iter() {
                                        match item {
                                            Value::Int(n) => {
                                                if is_float {
                                                    float_sum += *n as f64;
                                                } else {
                                                    int_sum += *n;
                                                }
                                            }
                                            Value::Float(f) => {
                                                if !is_float {
                                                    is_float = true;
                                                    float_sum = int_sum as f64 + *f;
                                                } else {
                                                    float_sum += *f;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if is_float {
                                        Ok(Value::Float(float_sum))
                                    } else {
                                        Ok(Value::Int(int_sum))
                                    }
                                })
                            } else if field == "enumerate" {
                                let arr_clone = arr.clone();
                                Value::native_fn("enumerate", move |_| {
                                    let items = arr_clone.read().unwrap();
                                    let mut enumerated = Vec::with_capacity(items.len());
                                    for (idx, item) in items.iter().enumerate() {
                                        enumerated.push(Value::array(vec![
                                            Value::Int(idx as i64),
                                            item.clone(),
                                        ]));
                                    }
                                    Ok(Value::array(enumerated))
                                })
                            } else if field == "rev" {
                                let arr_clone = arr.clone();
                                Value::native_fn("rev", move |_| {
                                    let mut items = arr_clone.read().unwrap().clone();
                                    items.reverse();
                                    Ok(Value::array(items))
                                })
                            } else if field == "join" {
                                let arr_clone = arr.clone();
                                Value::native_fn("join", move |args| {
                                    let sep = args
                                        .first()
                                        .map(|a| a.to_string())
                                        .unwrap_or_else(|| ",".to_string());
                                    let items = arr_clone.read().unwrap();
                                    let s: Vec<String> =
                                        items.iter().map(|it| it.to_string()).collect();
                                    Ok(Value::string(s.join(&sep)))
                                })
                            } else if field == "await_all" {
                                let arr_clone = arr.clone();
                                Value::native_fn("await_all", move |_| {
                                    let items = arr_clone.read().unwrap();
                                    let mut res = Vec::with_capacity(items.len());
                                    for item in items.iter() {
                                        match item {
                                            Value::Task(t) => res.push(t.read().unwrap().clone()),
                                            other => res.push(other.clone()),
                                        }
                                    }
                                    Ok(Value::array(res))
                                })
                            } else {
                                Value::Nil
                            }
                        }
                        Value::String(s) => {
                            if field == "len" {
                                Value::Int(s.len() as i64)
                            } else if field == "chars" {
                                let chars: Vec<Value> =
                                    s.chars().map(|c| Value::string(c.to_string())).collect();
                                Value::native_fn("chars", move |_| Ok(Value::array(chars.clone())))
                            } else if field == "lines" {
                                let lines: Vec<Value> =
                                    s.lines().map(Value::string).collect();
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
                                    let parts: Vec<Value> = s_clone
                                        .split(&sep)
                                        .map(Value::string)
                                        .collect();
                                    Ok(Value::array(parts))
                                })
                            } else if field == "contains" {
                                let s_clone = s.clone();
                                Value::native_fn("contains", move |args| {
                                    let sub = args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(s_clone.contains(&sub)))
                                })
                            } else if field == "starts_with" {
                                let s_clone = s.clone();
                                Value::native_fn("starts_with", move |args| {
                                    let prefix = args.first().map(|a| a.to_string()).unwrap_or_default();
                                    Ok(Value::Bool(s_clone.starts_with(&prefix)))
                                })
                            } else if field == "ends_with" {
                                let s_clone = s.clone();
                                Value::native_fn("ends_with", move |args| {
                                    let suffix = args.first().map(|a| a.to_string()).unwrap_or_default();
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
                                    let end = args.get(1).and_then(|a| a.as_int()).unwrap_or(char_count as i64);

                                    let start_idx = (if start < 0 { (char_count as i64 + start).max(0) } else { start.min(char_count as i64) }) as usize;
                                    let end_idx = (if end < 0 { (char_count as i64 + end).max(0) } else { end.min(char_count as i64) }) as usize;

                                    if start_idx >= end_idx {
                                        Ok(Value::string(""))
                                    } else {
                                        let sliced: String = s_clone.chars().skip(start_idx).take(end_idx - start_idx).collect();
                                        Ok(Value::string(sliced))
                                    }
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
                    self.registers[base + dst.0 as usize] =
                        Value::Closure(Arc::new(Closure { proto: child_proto }));
                }

                Instruction::Spawn { dst, expr } => {
                    let val = self.registers[base + expr.0 as usize].clone();
                    self.registers[base + dst.0 as usize] = Value::task(val);
                }

                Instruction::Await { dst, future } => {
                    let fut = self.registers[base + future.0 as usize].clone();
                    let res = match fut {
                        Value::Task(t) => t.read().unwrap().clone(),
                        other => other,
                    };
                    self.registers[base + dst.0 as usize] = res;
                }

                Instruction::IterInit { dst, src } => {
                    let val = self.registers[base + src.0 as usize].clone();
                    let it_state = match val {
                        Value::Array(arr) => {
                            let items = arr.read().unwrap().clone();
                            IteratorState::Array { items, index: 0 }
                        }
                        Value::String(s) => {
                            let chars = s.chars().collect();
                            IteratorState::String { chars, index: 0 }
                        }
                        Value::Map(m) => {
                            let entries: Vec<(String, Value)> = m
                                .read()
                                .unwrap()
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            IteratorState::Map { entries, index: 0 }
                        }
                        Value::Int(n) => IteratorState::Range {
                            current: 0,
                            end: n,
                            step: 1,
                        },
                        Value::Iterator(it) => {
                            self.registers[base + dst.0 as usize] = Value::Iterator(it);
                            continue;
                        }
                        _ => IteratorState::Array {
                            items: vec![val],
                            index: 0,
                        },
                    };
                    self.registers[base + dst.0 as usize] = Value::iterator(it_state);
                }

                Instruction::IterNext { dst, iter, offset } => {
                    let it_val = self.registers[base + iter.0 as usize].clone();
                    let mut advanced = false;
                    if let Value::Iterator(it) = it_val {
                        let mut state = it.write().unwrap();
                        match &mut *state {
                            IteratorState::Array { items, index } => {
                                if *index < items.len() {
                                    self.registers[base + dst.0 as usize] = items[*index].clone();
                                    *index += 1;
                                    advanced = true;
                                }
                            }
                            IteratorState::String { chars, index } => {
                                if *index < chars.len() {
                                    self.registers[base + dst.0 as usize] =
                                        Value::string(chars[*index].to_string());
                                    *index += 1;
                                    advanced = true;
                                }
                            }
                            IteratorState::Map { entries, index } => {
                                if *index < entries.len() {
                                    let (k, v) = &entries[*index];
                                    self.registers[base + dst.0 as usize] =
                                        Value::array(vec![Value::string(k.clone()), v.clone()]);
                                    *index += 1;
                                    advanced = true;
                                }
                            }
                            IteratorState::Range { current, end, step } => {
                                if (*step > 0 && *current < *end)
                                    || (*step < 0 && *current > *end)
                                {
                                    self.registers[base + dst.0 as usize] = Value::Int(*current);
                                    *current += *step;
                                    advanced = true;
                                }
                            }
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
