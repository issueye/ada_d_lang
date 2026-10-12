use ada_ast::Span;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Reg(pub u16);

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "R{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst {
        dst: Reg,
        const_idx: u32,
    },
    LoadNil {
        dst: Reg,
    },
    LoadBool {
        dst: Reg,
        val: bool,
    },
    LoadInt {
        dst: Reg,
        val: i32,
    },
    Move {
        dst: Reg,
        src: Reg,
    },

    // Arithmetic
    Add {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Sub {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Mul {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Div {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Mod {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },

    // Unary
    Neg {
        dst: Reg,
        src: Reg,
    },
    Not {
        dst: Reg,
        src: Reg,
    },
    BitNot {
        dst: Reg,
        src: Reg,
    },

    // Comparison
    Eq {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Ne {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Lt {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Le {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Gt {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Ge {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },

    // Bitwise
    BitAnd {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    BitOr {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    BitXor {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Shl {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Shr {
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },

    // Jumps & Flow
    Jump {
        offset: i32,
    },
    JumpIfFalse {
        cond: Reg,
        offset: i32,
    },
    JumpIfTrue {
        cond: Reg,
        offset: i32,
    },
    JumpIfNil {
        cond: Reg,
        offset: i32,
    },

    // Calls & Returns
    Call {
        dst: Reg,
        callee: Reg,
        arg_start: Reg,
        arg_count: u16,
    },
    Return {
        src: Option<Reg>,
    },

    // Globals
    GetGlobal {
        dst: Reg,
        name_idx: u32,
    },
    SetGlobal {
        name_idx: u32,
        src: Reg,
    },

    // Data structures
    NewArray {
        dst: Reg,
        start: Reg,
        count: u16,
    },
    NewMap {
        dst: Reg,
    },
    MapSet {
        map: Reg,
        key: Reg,
        val: Reg,
    },
    GetIndex {
        dst: Reg,
        target: Reg,
        index: Reg,
    },
    SetIndex {
        target: Reg,
        index: Reg,
        val: Reg,
    },
    GetField {
        dst: Reg,
        target: Reg,
        field_idx: u32,
        optional: bool,
    },
    SetField {
        target: Reg,
        field_idx: u32,
        val: Reg,
    },

    // String operations
    StringConcat {
        dst: Reg,
        start: Reg,
        count: u16,
    },

    // Closures
    Closure {
        dst: Reg,
        proto_idx: u32,
    },

    // Async & Concurrency
    Await {
        dst: Reg,
        future: Reg,
    },
    Spawn {
        dst: Reg,
        expr: Reg,
    },

    // Iterators
    IterInit {
        dst: Reg,
        src: Reg,
    },
    IterNext {
        dst: Reg,
        iter: Reg,
        offset: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FunctionProto {
    pub name: String,
    pub arity: u16,
    pub is_async: bool,
    pub register_count: u16,
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Constant>,
    pub names: Vec<String>,
    pub child_protos: Vec<FunctionProto>,
    pub spans: Vec<Span>,
}

impl FunctionProto {
    pub fn new(name: impl Into<String>, arity: u16) -> Self {
        Self {
            name: name.into(),
            arity,
            is_async: false,
            register_count: 0,
            instructions: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            child_protos: Vec::new(),
            spans: Vec::new(),
        }
    }

    pub fn add_constant(&mut self, val: Constant) -> u32 {
        for (i, c) in self.constants.iter().enumerate() {
            if c == &val {
                return i as u32;
            }
        }
        self.constants.push(val);
        (self.constants.len() - 1) as u32
    }

    pub fn add_name(&mut self, name: impl Into<String>) -> u32 {
        let name_str = name.into();
        for (i, n) in self.names.iter().enumerate() {
            if n == &name_str {
                return i as u32;
            }
        }
        self.names.push(name_str);
        (self.names.len() - 1) as u32
    }

    pub fn emit(&mut self, inst: Instruction, span: Span) -> usize {
        self.instructions.push(inst);
        self.spans.push(span);
        self.instructions.len() - 1
    }
}
