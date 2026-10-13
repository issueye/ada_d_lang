use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }

    pub fn merge(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line,
            col: self.col,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_ann: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Literal(Literal),
    Ident(String),
    Wildcard,
    Compare { op: BinOp, value: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal, Span),
    Ident(String, Span),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    FieldAccess {
        target: Box<Expr>,
        field: String,
        optional: bool, // true if `?.`
        span: Span,
    },
    Pipe {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    NullCoalesce {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Array {
        items: Vec<Expr>,
        span: Span,
    },
    Map {
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
    StringInterp {
        parts: Vec<StringPart>,
        span: Span,
    },
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        span: Span,
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },
    Block {
        stmts: Vec<Stmt>,
        trailing: Option<Box<Expr>>,
        span: Span,
    },
    Match {
        target: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Await {
        expr: Box<Expr>,
        span: Span,
    },
    Spawn {
        expr: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s)
            | Expr::Ident(_, s)
            | Expr::Unary { span: s, .. }
            | Expr::Binary { span: s, .. }
            | Expr::Assign { span: s, .. }
            | Expr::Call { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::FieldAccess { span: s, .. }
            | Expr::Pipe { span: s, .. }
            | Expr::NullCoalesce { span: s, .. }
            | Expr::Array { span: s, .. }
            | Expr::Map { span: s, .. }
            | Expr::StringInterp { span: s, .. }
            | Expr::Closure { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::Block { span: s, .. }
            | Expr::Match { span: s, .. }
            | Expr::Await { span: s, .. }
            | Expr::Spawn { span: s, .. } => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForVar {
    Single(String),
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        is_mut: bool,
        type_ann: Option<String>,
        init: Option<Expr>,
        span: Span,
    },
    Fn {
        name: String,
        is_async: bool,
        params: Vec<Param>,
        ret_type: Option<String>,
        body: Expr,
        span: Span,
    },
    While {
        cond: Expr,
        body: Expr,
        span: Span,
    },
    For {
        var: ForVar,
        iter: Expr,
        body: Expr,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Expr {
        expr: Expr,
        has_semicolon: bool,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Fn { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span }
            | Stmt::Expr { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(line: usize, col: usize) -> Span {
        Span::new(10, 20, line, col)
    }

    #[test]
    fn span_new_记录起止位置与行列() {
        let s = Span::new(5, 15, 3, 7);
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 15);
        assert_eq!(s.line, 3);
        assert_eq!(s.col, 7);
    }

    #[test]
    fn span_merge_取最小起点与最大终点() {
        let a = Span::new(10, 20, 1, 5);
        let b = Span::new(15, 30, 2, 3);
        let merged = a.merge(b);
        assert_eq!(merged.start, 10);
        assert_eq!(merged.end, 30);
    }

    #[test]
    fn span_merge_保留左操作数的行列() {
        let a = Span::new(10, 20, 1, 5);
        let b = Span::new(15, 30, 9, 9);
        let merged = a.merge(b);
        // 行列取自 self（左操作数），不取右操作数
        assert_eq!(merged.line, 1);
        assert_eq!(merged.col, 5);
    }

    #[test]
    fn span_merge_反向顺序同样正确() {
        let a = Span::new(30, 40, 1, 1);
        let b = Span::new(5, 35, 2, 2);
        let merged = a.merge(b);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 40);
    }

    #[test]
    fn binop_display_覆盖全部运算符() {
        let cases = [
            (BinOp::Add, "+"),
            (BinOp::Sub, "-"),
            (BinOp::Mul, "*"),
            (BinOp::Div, "/"),
            (BinOp::Mod, "%"),
            (BinOp::Eq, "=="),
            (BinOp::Ne, "!="),
            (BinOp::Lt, "<"),
            (BinOp::Le, "<="),
            (BinOp::Gt, ">"),
            (BinOp::Ge, ">="),
            (BinOp::And, "&&"),
            (BinOp::Or, "||"),
            (BinOp::BitAnd, "&"),
            (BinOp::BitOr, "|"),
            (BinOp::BitXor, "^"),
            (BinOp::Shl, "<<"),
            (BinOp::Shr, ">>"),
        ];
        for (op, text) in cases {
            assert_eq!(op.to_string(), text);
        }
    }

    #[test]
    fn unaryop_display_覆盖全部运算符() {
        assert_eq!(UnaryOp::Neg.to_string(), "-");
        assert_eq!(UnaryOp::Not.to_string(), "!");
        assert_eq!(UnaryOp::BitNot.to_string(), "~");
    }

    #[test]
    fn assignop_变体可复制且可比较() {
        let ops = [
            AssignOp::Assign,
            AssignOp::AddAssign,
            AssignOp::SubAssign,
            AssignOp::MulAssign,
            AssignOp::DivAssign,
            AssignOp::ModAssign,
        ];
        // Copy + Eq：克隆后仍相等，且互不相同
        for (i, a) in ops.iter().enumerate() {
            let copied = *a;
            assert_eq!(*a, copied);
            for (j, b) in ops.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn literal_变体可克隆且可比较() {
        let lits = [
            Literal::Int(42),
            Literal::Float(3.5),
            Literal::String("hi".into()),
            Literal::Bool(true),
            Literal::Nil,
        ];
        for l in &lits {
            assert_eq!(l, &l.clone());
        }
        assert_ne!(Literal::Int(1), Literal::Int(2));
        assert_ne!(Literal::Nil, Literal::Bool(false));
    }

    #[test]
    fn pattern_变体可克隆且可比较() {
        let p = Pattern::Compare {
            op: BinOp::Gt,
            value: Box::new(Expr::Literal(Literal::Int(0), span(1, 1))),
        };
        assert_eq!(p, p.clone());
        assert_ne!(Pattern::Wildcard, Pattern::Ident("x".into()));
    }

    #[test]
    fn forvar_单变量与元组可比较() {
        assert_eq!(ForVar::Single("x".into()), ForVar::Single("x".into()));
        assert_ne!(
            ForVar::Single("x".into()),
            ForVar::Tuple(vec!["x".into(), "y".into()])
        );
    }

    #[test]
    fn param_类型标注可选且可比较() {
        let annotated = Param {
            name: "x".into(),
            type_ann: Some("int".into()),
            span: span(1, 1),
        };
        let plain = Param {
            name: "y".into(),
            type_ann: None,
            span: span(1, 1),
        };
        assert_eq!(annotated.type_ann.as_deref(), Some("int"));
        assert!(plain.type_ann.is_none());
        assert_ne!(annotated, plain);
    }

    #[test]
    fn stringpart_字面量与表达式可比较() {
        let lit = StringPart::Lit("abc".into());
        let expr = StringPart::Expr(Expr::Literal(Literal::Int(1), span(1, 1)));
        assert_eq!(lit, lit.clone());
        assert_ne!(lit, expr);
    }

    #[test]
    fn matcharm_结构与可比较性() {
        let arm = MatchArm {
            pattern: Pattern::Wildcard,
            guard: None,
            body: Expr::Literal(Literal::Nil, span(1, 1)),
            span: span(1, 1),
        };
        assert_eq!(arm, arm.clone());
        assert!(arm.guard.is_none());
    }

    #[test]
    fn program_默认为空语句列表() {
        let p = Program::default();
        assert!(p.stmts.is_empty());
    }

    #[test]
    fn expr_span_每个变体都返回构造时的位置() {
        let s = span(7, 13);
        let exprs: Vec<Expr> = vec![
            Expr::Literal(Literal::Int(1), s),
            Expr::Ident("x".into(), s),
            Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(Expr::Literal(Literal::Int(1), s)),
                span: s,
            },
            Expr::Binary {
                op: BinOp::Add,
                lhs: Box::new(Expr::Literal(Literal::Int(1), s)),
                rhs: Box::new(Expr::Literal(Literal::Int(2), s)),
                span: s,
            },
            Expr::Assign {
                target: Box::new(Expr::Ident("x".into(), s)),
                op: AssignOp::Assign,
                value: Box::new(Expr::Literal(Literal::Int(1), s)),
                span: s,
            },
            Expr::Call {
                callee: Box::new(Expr::Ident("f".into(), s)),
                args: vec![],
                span: s,
            },
            Expr::Index {
                target: Box::new(Expr::Ident("a".into(), s)),
                index: Box::new(Expr::Literal(Literal::Int(0), s)),
                span: s,
            },
            Expr::FieldAccess {
                target: Box::new(Expr::Ident("m".into(), s)),
                field: "k".into(),
                optional: false,
                span: s,
            },
            Expr::Pipe {
                lhs: Box::new(Expr::Ident("a".into(), s)),
                rhs: Box::new(Expr::Ident("b".into(), s)),
                span: s,
            },
            Expr::NullCoalesce {
                lhs: Box::new(Expr::Ident("a".into(), s)),
                rhs: Box::new(Expr::Literal(Literal::Int(0), s)),
                span: s,
            },
            Expr::Array {
                items: vec![],
                span: s,
            },
            Expr::Map {
                entries: vec![],
                span: s,
            },
            Expr::StringInterp {
                parts: vec![],
                span: s,
            },
            Expr::Closure {
                params: vec![],
                body: Box::new(Expr::Literal(Literal::Nil, s)),
                span: s,
            },
            Expr::If {
                cond: Box::new(Expr::Literal(Literal::Bool(true), s)),
                then_branch: Box::new(Expr::Literal(Literal::Int(1), s)),
                else_branch: None,
                span: s,
            },
            Expr::Block {
                stmts: vec![],
                trailing: None,
                span: s,
            },
            Expr::Match {
                target: Box::new(Expr::Literal(Literal::Int(1), s)),
                arms: vec![],
                span: s,
            },
            Expr::Await {
                expr: Box::new(Expr::Ident("t".into(), s)),
                span: s,
            },
            Expr::Spawn {
                expr: Box::new(Expr::Literal(Literal::Int(1), s)),
                span: s,
            },
        ];
        for e in &exprs {
            assert_eq!(e.span(), s, "变体 {:?} 的 span() 不正确", e);
        }
    }

    #[test]
    fn stmt_span_每个变体都返回构造时的位置() {
        let s = span(4, 8);
        let stmts: Vec<Stmt> = vec![
            Stmt::Let {
                name: "x".into(),
                is_mut: false,
                type_ann: None,
                init: None,
                span: s,
            },
            Stmt::Fn {
                name: "f".into(),
                is_async: false,
                params: vec![],
                ret_type: None,
                body: Expr::Literal(Literal::Nil, s),
                span: s,
            },
            Stmt::While {
                cond: Expr::Literal(Literal::Bool(true), s),
                body: Expr::Literal(Literal::Nil, s),
                span: s,
            },
            Stmt::For {
                var: ForVar::Single("i".into()),
                iter: Expr::Literal(Literal::Int(0), s),
                body: Expr::Literal(Literal::Nil, s),
                span: s,
            },
            Stmt::Return {
                value: None,
                span: s,
            },
            Stmt::Break { span: s },
            Stmt::Continue { span: s },
            Stmt::Expr {
                expr: Expr::Literal(Literal::Int(1), s),
                has_semicolon: false,
                span: s,
            },
        ];
        for st in &stmts {
            assert_eq!(st.span(), s, "语句 {:?} 的 span() 不正确", st);
        }
    }
}
