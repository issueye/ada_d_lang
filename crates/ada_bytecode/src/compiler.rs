use crate::instruction::{Constant, FunctionProto, GlobalWrite, Instruction, Reg, UpvalueDesc};
use ada_ast::*;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Variable '{name}' not found")]
    VariableNotFound { name: String, span: Span },
    #[error("Too many registers in function")]
    TooManyRegisters,
    #[error("Invalid assignment target")]
    InvalidAssignmentTarget { span: Span },
    #[error("Cannot assign to immutable variable '{name}' (declare it with `mut`)")]
    AssignToImmutable { name: String, span: Span },
    #[error("`{keyword}` used outside of a loop (only valid inside `while`/`for`)")]
    LoopKeywordOutsideLoop { keyword: String, span: Span },
    #[error("imports are parsed but not yet resolved (module '{path}')")]
    ImportUnsupported { path: String, span: Span },
}

impl CompileError {
    /// 出错位置（T1.8）。
    ///
    /// `TooManyRegisters` 是寄存器分配的内部上限，产生点没有可归属的用户表达式，
    /// 因此返回 `None`；其余都能给出行列。
    pub fn span(&self) -> Option<Span> {
        match self {
            CompileError::VariableNotFound { span, .. }
            | CompileError::InvalidAssignmentTarget { span }
            | CompileError::AssignToImmutable { span, .. }
            | CompileError::LoopKeywordOutsideLoop { span, .. }
            | CompileError::ImportUnsupported { span, .. } => Some(*span),
            CompileError::TooManyRegisters => None,
        }
    }
}

struct Local {
    name: String,
    reg: Reg,
    depth: usize,
    /// 是否已装箱为 `Value::Cell`（T2.2：被嵌套函数捕获的变量）
    boxed: bool,
    /// 是否可重新赋值（T3.1：`let` 为 false；`mut`、参数、循环变量、match 绑定为 true）
    is_mut: bool,
}

pub struct Compiler {
    proto: FunctionProto,
    locals: Vec<Local>,
    scope_depth: usize,
    next_reg: u16,
    max_reg: u16,
    loop_stack: Vec<LoopContext>,
    /// 仅顶层（模块）编译单元为 true：决定顶层绑定是否同时写入全局表
    is_module_scope: bool,
    /// 本函数中需要装箱的局部变量名（T2.2 预扫描结果）
    boxed_names: HashSet<String>,
    /// 本函数声明的 upvalue 名字，顺序即 `UpvalueGet`/`UpvalueSet` 的下标
    upvalue_names: Vec<String>,
    /// 与 `upvalue_names` 一一对应：该捕获是否可写（T3.1）
    upvalue_mutable: Vec<bool>,
    /// 本函数可见的**外层**名字（T2.2）→ 是否可写（T3.1）。
    /// 由父编译器在创建子编译器时传入；具体来源（父局部 / 父 upvalue）由父层
    /// 在子编译完成后解析，这样多级嵌套时中间层即使不直接引用也能把捕获链
    /// 与可变性传递下去。
    outer_names: HashMap<String, bool>,
}

struct LoopContext {
    start_ip: usize,
    break_jumps: Vec<usize>,
}

impl Compiler {
    pub fn new(name: impl Into<String>, arity: u16) -> Self {
        let mut comp = Self {
            proto: FunctionProto::new(name, arity),
            locals: Vec::new(),
            scope_depth: 0,
            next_reg: arity, // R0 .. R(arity-1) are parameters
            max_reg: arity,
            loop_stack: Vec::new(),
            is_module_scope: false,
            boxed_names: HashSet::new(),
            upvalue_names: Vec::new(),
            upvalue_mutable: Vec::new(),
            outer_names: HashMap::new(),
        };
        comp.max_reg = comp.max_reg.max(arity);
        comp
    }

    /// 准备编译一个函数体（T2.2）：
    /// 1. 预扫描出"被嵌套函数引用过的名字"，用于决定哪些局部变量要装箱；
    /// 2. 记录外层可见名字及其可变性（T3.1 用于跨闭包校验）。
    fn prepare_body(&mut self, body: &Expr, outer_names: HashMap<String, bool>) {
        self.boxed_names = collect_names_in_nested_fns(body);
        self.outer_names = outer_names;
    }

    /// 模块编译单元同样需要预扫描（顶层 `mut` 也可能被闭包捕获并写入）。
    fn prepare_program(&mut self, program: &Program) {
        let mut names = HashSet::new();
        for stmt in &program.stmts {
            collect_stmt(stmt, false, &mut names);
        }
        self.boxed_names = names;
    }

    /// 子函数可见的外层名字（T2.2）。
    ///
    /// 只有**已装箱**的局部变量能作为捕获来源——预扫描保证被嵌套函数引用的
    /// 局部变量一定已装箱；同时把本函数自己的 `outer_names` 继续向下传递，
    /// 以支持"中间层不直接引用"的多级捕获。
    fn visible_outer_names(&self) -> HashMap<String, bool> {
        let mut names = self.outer_names.clone();
        for local in &self.locals {
            if local.boxed {
                names.insert(local.name.clone(), local.is_mut);
            }
        }
        names
    }

    /// 把名字解析为 upvalue 下标（T2.2）：只登记名字与可变性，描述符由父层解析。
    fn resolve_upvalue(&mut self, name: &str) -> Option<u16> {
        let mutable = *self.outer_names.get(name)?;
        if let Some(idx) = self.upvalue_names.iter().position(|n| n == name) {
            return Some(idx as u16);
        }
        self.upvalue_names.push(name.to_string());
        self.upvalue_mutable.push(mutable);
        Some((self.upvalue_names.len() - 1) as u16)
    }

    /// 在父层视角解析子函数某个捕获的来源（T2.2）。
    ///
    /// - 命中本层已装箱局部变量 → `ParentLocal`
    /// - 否则通过 `resolve_upvalue` 在本层物化该捕获（可能继续向上传递）→ `ParentUpvalue`
    ///
    /// `span` 用于错误定位（T1.8）：捕获名解析失败时指向引用它的表达式。
    fn resolve_capture_source(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<UpvalueDesc, CompileError> {
        if let Some((reg, _, _)) = self.resolve_local(name) {
            return Ok(UpvalueDesc::ParentLocal(reg.0));
        }
        self.resolve_upvalue(name)
            .map(UpvalueDesc::ParentUpvalue)
            .ok_or_else(|| CompileError::VariableNotFound {
                name: name.to_string(),
                span,
            })
    }

    /// 子函数编译完成后，把它的 upvalue 名字列表解析成捕获描述符（顺序一致）。
    fn resolve_child_upvalues(
        &mut self,
        names: &[String],
        span: Span,
    ) -> Result<Vec<UpvalueDesc>, CompileError> {
        let mut descriptors = Vec::with_capacity(names.len());
        for name in names {
            descriptors.push(self.resolve_capture_source(name, span)?);
        }
        Ok(descriptors)
    }

    fn is_boxed(&self, name: &str) -> bool {
        self.boxed_names.contains(name)
    }

    /// 生成闭包创建指令（T2.2）：先把捕获值装入连续寄存器，再发 `MakeClosure`。
    /// T6.7：发出"条件为假则跳转"。条件是**比较表达式**时用融合指令
    /// （`JumpIfNotCompare*`），省掉一条比较指令、一个临时寄存器，以及一次
    /// "bool 写入 + truthiness 读取"；否则回退到"求值到临时寄存器 + `JumpIfFalse`"。
    ///
    /// 返回跳转指令下标，调用方用 [`Compiler::patch_conditional_jump`] 回填 `offset`。
    fn emit_jump_if_false(&mut self, cond: &Expr, span: Span) -> Result<usize, CompileError> {
        if let Expr::Binary { op, lhs, rhs, .. } = cond {
            if is_comparison(*op) {
                // 一侧是 i32 字面量 → 常量形态（`while i < 5` 最常见）
                if let Some((operand, value, fused_op)) = fuse_int_literal(*op, lhs, rhs) {
                    let r = self.alloc_reg()?;
                    self.compile_expr(operand, r)?;
                    let idx = self.proto.emit(
                        Instruction::JumpIfNotCompareConst {
                            op: fused_op,
                            src: r,
                            value,
                            offset: 0,
                        },
                        span,
                    );
                    self.free_reg(r);
                    return Ok(idx);
                }
                let r_lhs = self.alloc_reg()?;
                self.compile_expr(lhs, r_lhs)?;
                let r_rhs = self.alloc_reg()?;
                self.compile_expr(rhs, r_rhs)?;
                let idx = self.proto.emit(
                    Instruction::JumpIfNotCompare {
                        op: *op,
                        lhs: r_lhs,
                        rhs: r_rhs,
                        offset: 0,
                    },
                    span,
                );
                self.free_reg(r_rhs);
                self.free_reg(r_lhs);
                return Ok(idx);
            }
        }

        let cond_reg = self.alloc_reg()?;
        self.compile_expr(cond, cond_reg)?;
        let idx = self.proto.emit(
            Instruction::JumpIfFalse {
                cond: cond_reg,
                offset: 0,
            },
            span,
        );
        self.free_reg(cond_reg);
        Ok(idx)
    }

    /// 回填条件跳转的 `offset`（普通与两种融合形态通用）。
    fn patch_conditional_jump(&mut self, idx: usize, offset: i32) {
        match &mut self.proto.instructions[idx] {
            Instruction::JumpIfFalse { offset: o, .. }
            | Instruction::JumpIfTrue { offset: o, .. }
            | Instruction::JumpIfNotCompare { offset: o, .. }
            | Instruction::JumpIfNotCompareConst { offset: o, .. } => *o = offset,
            _ => {}
        }
    }
    fn emit_closure(
        &mut self,
        dst: Reg,
        proto_idx: u32,
        upvalues: &[UpvalueDesc],
        span: Span,
    ) -> Result<(), CompileError> {
        if upvalues.is_empty() {
            self.proto
                .emit(Instruction::Closure { dst, proto_idx }, span);
            return Ok(());
        }
        let start = Reg(self.next_reg);
        for desc in upvalues {
            let reg = self.alloc_reg()?;
            match desc {
                UpvalueDesc::ParentLocal(src) => {
                    self.proto.emit(
                        Instruction::Move {
                            dst: reg,
                            src: Reg(*src),
                        },
                        span,
                    );
                }
                UpvalueDesc::ParentUpvalue(idx) => {
                    self.proto.emit(
                        Instruction::UpvalueGet {
                            dst: reg,
                            idx: *idx,
                        },
                        span,
                    );
                }
            }
        }
        self.proto.emit(
            Instruction::MakeClosure {
                dst,
                proto_idx,
                upvalue_start: start,
                upvalue_count: upvalues.len() as u16,
            },
            span,
        );
        for _ in upvalues {
            self.next_reg -= 1;
        }
        Ok(())
    }

    /// 复合赋值共用的二元运算发射（T2.2）。
    fn emit_compound_op(
        &mut self,
        op: AssignOp,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
        span: Span,
    ) -> Result<(), CompileError> {
        let inst = match op {
            AssignOp::AddAssign => Instruction::Add { dst, lhs, rhs },
            AssignOp::SubAssign => Instruction::Sub { dst, lhs, rhs },
            AssignOp::MulAssign => Instruction::Mul { dst, lhs, rhs },
            AssignOp::DivAssign => Instruction::Div { dst, lhs, rhs },
            AssignOp::ModAssign => Instruction::Mod { dst, lhs, rhs },
            AssignOp::Assign => return Err(CompileError::InvalidAssignmentTarget { span }),
        };
        self.proto.emit(inst, span);
        Ok(())
    }

    /// 对装箱局部变量（Cell）执行赋值，并把表达式值写入 `dst`（T2.2）。
    fn emit_boxed_assign(
        &mut self,
        cell: Reg,
        op: AssignOp,
        value: Reg,
        dst: Reg,
        span: Span,
    ) -> Result<(), CompileError> {
        match op {
            AssignOp::Assign => {
                self.proto
                    .emit(Instruction::CellSet { cell, src: value }, span);
                // dst 恰好是变量槽本身时不能再写入（否则会把 Cell 覆盖掉）
                if dst != cell {
                    self.proto.emit(Instruction::Move { dst, src: value }, span);
                }
            }
            other => {
                let tmp = self.alloc_reg()?;
                self.proto
                    .emit(Instruction::CellGet { dst: tmp, cell }, span);
                self.emit_compound_op(other, tmp, tmp, value, span)?;
                self.proto
                    .emit(Instruction::CellSet { cell, src: tmp }, span);
                if dst != cell {
                    self.proto.emit(Instruction::Move { dst, src: tmp }, span);
                }
                self.free_reg(tmp);
            }
        }
        Ok(())
    }

    fn alloc_reg(&mut self) -> Result<Reg, CompileError> {
        let reg = Reg(self.next_reg);
        self.next_reg += 1;
        if self.next_reg > self.max_reg {
            self.max_reg = self.next_reg;
        }
        if self.next_reg > 10000 {
            return Err(CompileError::TooManyRegisters);
        }
        Ok(reg)
    }

    fn free_reg(&mut self, reg: Reg) {
        if reg.0 + 1 == self.next_reg {
            self.next_reg -= 1;
        }
    }

    fn enter_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn exit_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(last) = self.locals.last() {
            if last.depth > self.scope_depth {
                self.free_reg(last.reg);
                self.locals.pop();
            } else {
                break;
            }
        }
    }

    fn define_local(&mut self, name: String, reg: Reg, boxed: bool, is_mut: bool) {
        self.locals.push(Local {
            name,
            reg,
            depth: self.scope_depth,
            boxed,
            is_mut,
        });
    }

    /// 返回 (寄存器, 是否装箱, 是否可写)。
    fn resolve_local(&self, name: &str) -> Option<(Reg, bool, bool)> {
        for local in self.locals.iter().rev() {
            if local.name == name {
                return Some((local.reg, local.boxed, local.is_mut));
            }
        }
        None
    }

    pub fn compile_program(program: &Program) -> Result<FunctionProto, CompileError> {
        let mut compiler = Compiler::new("<main>", 0);
        compiler.is_module_scope = true;
        compiler.prepare_program(program);
        let ret_reg = compiler.alloc_reg()?;

        for stmt in &program.stmts {
            compiler.compile_stmt(stmt, Some(ret_reg))?;
        }

        // Default return last value or nil
        compiler
            .proto
            .emit(Instruction::Return { src: Some(ret_reg) }, Span::default());
        compiler.proto.register_count = compiler.max_reg;
        Ok(compiler.proto)
    }

    pub fn compile_stmt(&mut self, stmt: &Stmt, dst: Option<Reg>) -> Result<(), CompileError> {
        match stmt {
            Stmt::Import { kind, path, span } => {
                if let Some(module) = path.strip_prefix("std:") {
                    // T8.14：`std:` 导入编译为运行时指令，由 VM 注册模块，
                    // 因此 Engine 与裸用 Vm 两条路径都可用。
                    match kind {
                        ImportKind::Namespace(alias) => {
                            let module_idx = self.proto.add_name(module);
                            let alias_idx = self.proto.add_name(alias.as_str());
                            self.proto.emit(
                                Instruction::ImportModule {
                                    module_idx,
                                    alias_idx,
                                },
                                *span,
                            );
                        }
                        ImportKind::Named(names) => {
                            let module_idx = self.proto.add_name(module);
                            for name in names {
                                let member_idx = self.proto.add_name(name.as_str());
                                let alias_idx = self.proto.add_name(name.as_str());
                                self.proto.emit(
                                    Instruction::ImportNamed {
                                        module_idx,
                                        member_idx,
                                        alias_idx,
                                    },
                                    *span,
                                );
                            }
                        }
                    }
                } else {
                    // 文件 import 需要文件系统上下文，由 Engine 在编译前解析（T8.8）。
                    return Err(CompileError::ImportUnsupported {
                        path: path.clone(),
                        span: *span,
                    });
                }
            }
            Stmt::Let {
                name,
                is_mut,
                init,
                span,
                ..
            } => {
                let reg = self.alloc_reg()?;
                let boxed = self.is_boxed(name);
                if let Some(init_expr) = init {
                    self.compile_expr(init_expr, reg)?;
                } else {
                    self.proto.emit(Instruction::LoadNil { dst: reg }, *span);
                }
                if boxed {
                    // T2.2：被嵌套函数捕获的变量就地装箱，供闭包共享读写
                    self.proto
                        .emit(Instruction::MakeCell { dst: reg, src: reg }, *span);
                }
                // T3.1：`let` 绑定不可重新赋值（`mut` 才可）
                self.define_local(name.clone(), reg, boxed, *is_mut);
                // T2.4：模块顶层的绑定同时写入全局表，使 REPL 跨行求值可见；
                // 函数体内的顶层局部变量不会外泄（is_module_scope 为 false）。
                if self.is_module_scope && self.scope_depth == 0 {
                    let name_idx = self.proto.add_name(name.clone());
                    if boxed {
                        // 全局表里存值而不是 Cell；闭包后续修改 Cell 时该镜像可能过期
                        // （同一脚本内始终走局部 Cell，见 SPEC_LANGUAGE §5.1）
                        let tmp = self.alloc_reg()?;
                        self.proto.emit(
                            Instruction::CellGet {
                                dst: tmp,
                                cell: reg,
                            },
                            *span,
                        );
                        self.proto.emit(
                            Instruction::SetGlobal {
                                name_idx,
                                src: tmp,
                                // T8.3：`let` 声明由脚本接管并标记为不可变，
                                // 因此跨轮求值（REPL）不能再改写它
                                kind: global_write_kind(*is_mut),
                            },
                            *span,
                        );
                        self.free_reg(tmp);
                    } else {
                        self.proto.emit(
                            Instruction::SetGlobal {
                                name_idx,
                                src: reg,
                                kind: global_write_kind(*is_mut),
                            },
                            *span,
                        );
                    }
                }
                if let Some(d) = dst {
                    self.proto
                        .emit(Instruction::Move { dst: d, src: reg }, *span);
                }
            }

            Stmt::Fn {
                name,
                is_async,
                params,
                body,
                span,
                ..
            } => {
                let mut child = Compiler::new(name, params.len() as u16);
                child.proto.is_async = *is_async;
                child.prepare_body(body, self.visible_outer_names());
                for (i, p) in params.iter().enumerate() {
                    let reg = Reg(i as u16);
                    let boxed = child.is_boxed(&p.name);
                    if boxed {
                        child
                            .proto
                            .emit(Instruction::MakeCell { dst: reg, src: reg }, *span);
                    }
                    child.define_local(p.name.clone(), reg, boxed, true);
                }
                let body_reg = child.alloc_reg()?;
                child.compile_expr(body, body_reg)?;
                child.proto.emit(
                    Instruction::Return {
                        src: Some(body_reg),
                    },
                    body.span(),
                );
                child.proto.register_count = child.max_reg;

                let proto_idx = self.proto.child_protos.len() as u32;
                let upvalues = self.resolve_child_upvalues(&child.upvalue_names, *span)?;
                let mut child_proto = child.proto;
                child_proto.upvalues = upvalues.clone();
                self.proto.child_protos.push(child_proto);

                let fn_reg = self.alloc_reg()?;
                self.emit_closure(fn_reg, proto_idx, &upvalues, *span)?;
                if self.scope_depth == 0 {
                    let name_idx = self.proto.add_name(name.clone());
                    self.proto.emit(
                        Instruction::SetGlobal {
                            name_idx,
                            src: fn_reg,
                            // 顶层 `fn` 声明：函数名由脚本接管，此后不可改写
                            kind: GlobalWrite::DeclareImmutable,
                        },
                        *span,
                    );
                }
                // T8.3：函数名是**不可变**绑定（与 `let` 一致）。此前这里是可变局部，
                // 于是同一脚本里 `f = 2` 能改写函数名，而跨轮求值又被全局规则拦住——
                // 两处口径不一致。需要"换实现"时重新写一个 `fn` 声明（声明会遮蔽，不报错）。
                self.define_local(name.clone(), fn_reg, false, false);
                if let Some(d) = dst {
                    self.proto.emit(
                        Instruction::Move {
                            dst: d,
                            src: fn_reg,
                        },
                        *span,
                    );
                }
            }

            Stmt::While { cond, body, span } => {
                let start_ip = self.proto.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_ip,
                    break_jumps: Vec::new(),
                });

                // T6.7：条件是比较表达式时直接发出融合的条件跳转
                let jump_exit_idx = self.emit_jump_if_false(cond, *span)?;

                let body_reg = self.alloc_reg()?;
                self.compile_expr(body, body_reg)?;
                self.free_reg(body_reg);

                // Loop back
                let loop_offset = (start_ip as i32) - (self.proto.instructions.len() as i32) - 1;
                self.proto.emit(
                    Instruction::Jump {
                        offset: loop_offset,
                    },
                    *span,
                );

                // Patch exit jump
                let exit_ip = self.proto.instructions.len();
                let exit_offset = (exit_ip as i32) - (jump_exit_idx as i32) - 1;
                self.patch_conditional_jump(jump_exit_idx, exit_offset);

                // Patch break jumps
                if let Some(ctx) = self.loop_stack.pop() {
                    for brk_idx in ctx.break_jumps {
                        let brk_offset = (exit_ip as i32) - (brk_idx as i32) - 1;
                        if let Instruction::Jump { offset } = &mut self.proto.instructions[brk_idx]
                        {
                            *offset = brk_offset;
                        }
                    }
                }

                if let Some(d) = dst {
                    self.proto.emit(Instruction::LoadNil { dst: d }, *span);
                }
            }

            Stmt::For {
                var,
                iter,
                body,
                span,
            } => {
                let iter_reg = self.alloc_reg()?;
                self.compile_expr(iter, iter_reg)?;

                let it_reg = self.alloc_reg()?;
                self.proto.emit(
                    Instruction::IterInit {
                        dst: it_reg,
                        src: iter_reg,
                    },
                    *span,
                );

                let start_ip = self.proto.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_ip,
                    break_jumps: Vec::new(),
                });

                let val_reg = self.alloc_reg()?;
                let jump_exit_idx = self.proto.emit(
                    Instruction::IterNext {
                        dst: val_reg,
                        iter: it_reg,
                        offset: 0,
                    },
                    *span,
                );

                self.enter_scope();
                match var {
                    ForVar::Single(name) => {
                        // T2.2：循环变量被闭包捕获时每轮重新装箱，
                        // 因此每次迭代的闭包各自持有一个独立槽（类似 JS 的 let）
                        let boxed = self.is_boxed(name);
                        if boxed {
                            self.proto.emit(
                                Instruction::MakeCell {
                                    dst: val_reg,
                                    src: val_reg,
                                },
                                *span,
                            );
                        }
                        self.define_local(name.clone(), val_reg, boxed, true);
                    }
                    ForVar::Tuple(names) => {
                        for (i, name) in names.iter().enumerate() {
                            let item_reg = self.alloc_reg()?;
                            let idx_reg = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::LoadInt {
                                    dst: idx_reg,
                                    val: i as i32,
                                },
                                *span,
                            );
                            self.proto.emit(
                                Instruction::GetIndex {
                                    dst: item_reg,
                                    target: val_reg,
                                    index: idx_reg,
                                },
                                *span,
                            );
                            self.free_reg(idx_reg);
                            let boxed = self.is_boxed(name);
                            if boxed {
                                self.proto.emit(
                                    Instruction::MakeCell {
                                        dst: item_reg,
                                        src: item_reg,
                                    },
                                    *span,
                                );
                            }
                            self.define_local(name.clone(), item_reg, boxed, true);
                        }
                    }
                }

                let body_reg = self.alloc_reg()?;
                self.compile_expr(body, body_reg)?;
                self.free_reg(body_reg);
                self.exit_scope();

                // Jump back to loop start
                let loop_offset = (start_ip as i32) - (self.proto.instructions.len() as i32) - 1;
                self.proto.emit(
                    Instruction::Jump {
                        offset: loop_offset,
                    },
                    *span,
                );

                // Patch exit jump
                let exit_ip = self.proto.instructions.len();
                let exit_offset = (exit_ip as i32) - (jump_exit_idx as i32) - 1;
                if let Instruction::IterNext { offset, .. } =
                    &mut self.proto.instructions[jump_exit_idx]
                {
                    *offset = exit_offset;
                }

                if let Some(ctx) = self.loop_stack.pop() {
                    for brk_idx in ctx.break_jumps {
                        let brk_offset = (exit_ip as i32) - (brk_idx as i32) - 1;
                        if let Instruction::Jump { offset } = &mut self.proto.instructions[brk_idx]
                        {
                            *offset = brk_offset;
                        }
                    }
                }

                self.free_reg(val_reg);
                self.free_reg(it_reg);
                self.free_reg(iter_reg);

                if let Some(d) = dst {
                    self.proto.emit(Instruction::LoadNil { dst: d }, *span);
                }
            }

            Stmt::Return { value, span } => {
                let ret_reg = if let Some(v) = value {
                    let r = self.alloc_reg()?;
                    self.compile_expr(v, r)?;
                    Some(r)
                } else {
                    None
                };
                self.proto.emit(Instruction::Return { src: ret_reg }, *span);
            }

            Stmt::Break { span } => {
                // T8.2：循环外的 `break` 此前被静默忽略（函数直接返回 nil），
                // 与 M3 的"不静默"原则冲突，现在报编译错误。
                let Some(ctx) = self.loop_stack.last_mut() else {
                    return Err(CompileError::LoopKeywordOutsideLoop {
                        keyword: "break".to_string(),
                        span: *span,
                    });
                };
                let jmp_idx = self.proto.emit(Instruction::Jump { offset: 0 }, *span);
                ctx.break_jumps.push(jmp_idx);
            }

            Stmt::Continue { span } => {
                let Some(ctx) = self.loop_stack.last() else {
                    return Err(CompileError::LoopKeywordOutsideLoop {
                        keyword: "continue".to_string(),
                        span: *span,
                    });
                };
                let loop_offset =
                    (ctx.start_ip as i32) - (self.proto.instructions.len() as i32) - 1;
                self.proto.emit(
                    Instruction::Jump {
                        offset: loop_offset,
                    },
                    *span,
                );
            }

            Stmt::Expr { expr, .. } => {
                let r = dst.unwrap_or(self.alloc_reg()?);
                self.compile_expr(expr, r)?;
                if dst.is_none() {
                    self.free_reg(r);
                }
            }
        }
        Ok(())
    }

    // ==========================================
    // Expression Compilation
    // ==========================================

    pub fn compile_expr(&mut self, expr: &Expr, dst: Reg) -> Result<(), CompileError> {
        let span = expr.span();

        match expr {
            Expr::Literal(lit, _) => match lit {
                Literal::Int(n) => {
                    if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                        self.proto.emit(
                            Instruction::LoadInt {
                                dst,
                                val: *n as i32,
                            },
                            span,
                        );
                    } else {
                        let c_idx = self.proto.add_constant(Constant::Int(*n));
                        self.proto.emit(
                            Instruction::LoadConst {
                                dst,
                                const_idx: c_idx,
                            },
                            span,
                        );
                    }
                }
                Literal::Float(f) => {
                    let c_idx = self.proto.add_constant(Constant::Float(*f));
                    self.proto.emit(
                        Instruction::LoadConst {
                            dst,
                            const_idx: c_idx,
                        },
                        span,
                    );
                }
                Literal::String(s) => {
                    let c_idx = self.proto.add_constant(Constant::String(s.clone()));
                    self.proto.emit(
                        Instruction::LoadConst {
                            dst,
                            const_idx: c_idx,
                        },
                        span,
                    );
                }
                Literal::Bool(b) => {
                    self.proto
                        .emit(Instruction::LoadBool { dst, val: *b }, span);
                }
                Literal::Nil => {
                    self.proto.emit(Instruction::LoadNil { dst }, span);
                }
            },

            Expr::Ident(name, _) => {
                if let Some((local_reg, boxed, _)) = self.resolve_local(name) {
                    if boxed {
                        if local_reg == dst {
                            // 目标就是变量槽本身：借道临时寄存器，避免覆盖 Cell
                            let tmp = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::CellGet {
                                    dst: tmp,
                                    cell: local_reg,
                                },
                                span,
                            );
                            self.proto.emit(Instruction::Move { dst, src: tmp }, span);
                            self.free_reg(tmp);
                        } else {
                            self.proto.emit(
                                Instruction::CellGet {
                                    dst,
                                    cell: local_reg,
                                },
                                span,
                            );
                        }
                    } else if local_reg != dst {
                        self.proto.emit(
                            Instruction::Move {
                                dst,
                                src: local_reg,
                            },
                            span,
                        );
                    }
                } else if let Some(idx) = self.resolve_upvalue(name) {
                    self.proto.emit(Instruction::UpvalueGet { dst, idx }, span);
                } else {
                    let name_idx = self.proto.add_name(name.clone());
                    self.proto
                        .emit(Instruction::GetGlobal { dst, name_idx }, span);
                }
            }

            Expr::Unary { op, expr, .. } => {
                let src = self.alloc_reg()?;
                self.compile_expr(expr, src)?;
                match op {
                    UnaryOp::Neg => self.proto.emit(Instruction::Neg { dst, src }, span),
                    UnaryOp::Not => self.proto.emit(Instruction::Not { dst, src }, span),
                    UnaryOp::BitNot => self.proto.emit(Instruction::BitNot { dst, src }, span),
                };
                self.free_reg(src);
            }

            Expr::Binary { op, lhs, rhs, .. } => {
                // T6.6 指令融合：`x + 2` / `i % 2 == 0` / `k < x` 这类"一侧是整数字面量"
                // 的表达式直接编成一条 `BinaryConst`，省掉一条 `LoadInt` 与一次派发。
                // 这也**减少指令数**（确定性门禁可度量），不只是省时间。
                if let Some((operand, value, fused_op)) = fuse_int_literal(*op, lhs, rhs) {
                    let r_operand = self.alloc_reg()?;
                    self.compile_expr(operand, r_operand)?;
                    self.proto.emit(
                        Instruction::BinaryConst {
                            dst,
                            op: fused_op,
                            src: r_operand,
                            value,
                        },
                        span,
                    );
                    self.free_reg(r_operand);
                    return Ok(());
                }

                let r_lhs = self.alloc_reg()?;
                self.compile_expr(lhs, r_lhs)?;
                let r_rhs = self.alloc_reg()?;
                self.compile_expr(rhs, r_rhs)?;

                match op {
                    BinOp::Add => self.proto.emit(
                        Instruction::Add {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Sub => self.proto.emit(
                        Instruction::Sub {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Mul => self.proto.emit(
                        Instruction::Mul {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Div => self.proto.emit(
                        Instruction::Div {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Mod => self.proto.emit(
                        Instruction::Mod {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Eq => self.proto.emit(
                        Instruction::Eq {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Ne => self.proto.emit(
                        Instruction::Ne {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Lt => self.proto.emit(
                        Instruction::Lt {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Le => self.proto.emit(
                        Instruction::Le {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Gt => self.proto.emit(
                        Instruction::Gt {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Ge => self.proto.emit(
                        Instruction::Ge {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::BitAnd => self.proto.emit(
                        Instruction::BitAnd {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::BitOr => self.proto.emit(
                        Instruction::BitOr {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::BitXor => self.proto.emit(
                        Instruction::BitXor {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Shl => self.proto.emit(
                        Instruction::Shl {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::Shr => self.proto.emit(
                        Instruction::Shr {
                            dst,
                            lhs: r_lhs,
                            rhs: r_rhs,
                        },
                        span,
                    ),
                    BinOp::And => {
                        // Short-circuiting AND: if lhs false -> false, else rhs
                        self.proto.emit(Instruction::Move { dst, src: r_lhs }, span);
                        let jmp = self.proto.emit(
                            Instruction::JumpIfFalse {
                                cond: dst,
                                offset: 0,
                            },
                            span,
                        );
                        self.compile_expr(rhs, dst)?;
                        let after_ip = self.proto.instructions.len();
                        let offset = (after_ip as i32) - (jmp as i32) - 1;
                        if let Instruction::JumpIfFalse { offset: o, .. } =
                            &mut self.proto.instructions[jmp]
                        {
                            *o = offset;
                        }
                        self.free_reg(r_rhs);
                        self.free_reg(r_lhs);
                        return Ok(());
                    }
                    BinOp::Or => {
                        // Short-circuiting OR: if lhs true -> true, else rhs
                        self.proto.emit(Instruction::Move { dst, src: r_lhs }, span);
                        let jmp = self.proto.emit(
                            Instruction::JumpIfTrue {
                                cond: dst,
                                offset: 0,
                            },
                            span,
                        );
                        self.compile_expr(rhs, dst)?;
                        let after_ip = self.proto.instructions.len();
                        let offset = (after_ip as i32) - (jmp as i32) - 1;
                        if let Instruction::JumpIfTrue { offset: o, .. } =
                            &mut self.proto.instructions[jmp]
                        {
                            *o = offset;
                        }
                        self.free_reg(r_rhs);
                        self.free_reg(r_lhs);
                        return Ok(());
                    }
                };

                self.free_reg(r_rhs);
                self.free_reg(r_lhs);
            }

            Expr::Assign {
                target, op, value, ..
            } => {
                // T6.6b：`x += 1`（复合赋值 + 整数字面量，目标是普通局部变量）直接编成
                // 一条 `BinaryConst`——循环计数器每轮省掉一条 `LoadInt` 与一个寄存器。
                // 非字面量、或目标是字段/索引/全局/闭包捕获时仍走通用路径。
                if !matches!(op, AssignOp::Assign) {
                    if let (Expr::Ident(name, _), Some(constant)) = (&**target, int_literal(value))
                    {
                        if let Some((local_reg, boxed, is_mut)) = self.resolve_local(name) {
                            if !is_mut {
                                return Err(CompileError::AssignToImmutable {
                                    name: name.clone(),
                                    span,
                                });
                            }
                            if !boxed {
                                if let Some(bin) = compound_binop(*op) {
                                    self.proto.emit(
                                        Instruction::BinaryConst {
                                            dst: local_reg,
                                            op: bin,
                                            src: local_reg,
                                            value: constant,
                                        },
                                        span,
                                    );
                                    self.proto.emit(
                                        Instruction::Move {
                                            dst,
                                            src: local_reg,
                                        },
                                        span,
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

                let val_reg = self.alloc_reg()?;
                self.compile_expr(value, val_reg)?;

                match &**target {
                    Expr::Ident(name, _) => {
                        if let Some((local_reg, boxed, is_mut)) = self.resolve_local(name) {
                            // T3.1：`let`（非 mut）绑定不可重新赋值
                            if !is_mut {
                                return Err(CompileError::AssignToImmutable {
                                    name: name.clone(),
                                    span,
                                });
                            }
                            if boxed {
                                self.emit_boxed_assign(local_reg, *op, val_reg, dst, span)?;
                            } else {
                                match op {
                                    AssignOp::Assign => {
                                        self.proto.emit(
                                            Instruction::Move {
                                                dst: local_reg,
                                                src: val_reg,
                                            },
                                            span,
                                        );
                                    }
                                    compound => {
                                        self.emit_compound_op(
                                            *compound, local_reg, local_reg, val_reg, span,
                                        )?;
                                    }
                                }
                                self.proto.emit(
                                    Instruction::Move {
                                        dst,
                                        src: local_reg,
                                    },
                                    span,
                                );
                            }
                        } else if let Some(idx) = self.resolve_upvalue(name) {
                            // T3.1：捕获来的不可变绑定同样不可写
                            if !self.upvalue_mutable[idx as usize] {
                                return Err(CompileError::AssignToImmutable {
                                    name: name.clone(),
                                    span,
                                });
                            }
                            match op {
                                AssignOp::Assign => {
                                    self.proto
                                        .emit(Instruction::UpvalueSet { idx, src: val_reg }, span);
                                    self.proto
                                        .emit(Instruction::Move { dst, src: val_reg }, span);
                                }
                                compound => {
                                    let tmp = self.alloc_reg()?;
                                    self.proto
                                        .emit(Instruction::UpvalueGet { dst: tmp, idx }, span);
                                    self.emit_compound_op(*compound, tmp, tmp, val_reg, span)?;
                                    self.proto
                                        .emit(Instruction::UpvalueSet { idx, src: tmp }, span);
                                    self.proto.emit(Instruction::Move { dst, src: tmp }, span);
                                    self.free_reg(tmp);
                                }
                            }
                        } else {
                            let name_idx = self.proto.add_name(name.clone());
                            // T8.4 修复：复合赋值此前在这里被丢弃——`total += 1` 会编译成
                            // `total = 1`（静默错值）。先读回当前全局值，再应用运算。
                            let result_reg = if matches!(op, AssignOp::Assign) {
                                val_reg
                            } else {
                                let tmp = self.alloc_reg()?;
                                self.proto
                                    .emit(Instruction::GetGlobal { dst: tmp, name_idx }, span);
                                self.emit_compound_op(*op, tmp, tmp, val_reg, span)?;
                                tmp
                            };
                            self.proto.emit(
                                Instruction::SetGlobal {
                                    name_idx,
                                    src: result_reg,
                                    // 普通赋值（未声明名的首次赋值，或跨轮次写入）
                                    kind: GlobalWrite::Assign,
                                },
                                span,
                            );
                            self.proto.emit(
                                Instruction::Move {
                                    dst,
                                    src: result_reg,
                                },
                                span,
                            );
                            if result_reg != val_reg {
                                self.free_reg(result_reg);
                            }
                        }
                    }
                    Expr::FieldAccess { target, field, .. } => {
                        let t_reg = self.alloc_reg()?;
                        self.compile_expr(target, t_reg)?;
                        let field_idx = self.proto.add_name(field.clone());
                        // T8.4 修复：`cfg.n += 1` 此前编译成 `cfg.n = 1`（静默错值）
                        let result_reg = if matches!(op, AssignOp::Assign) {
                            val_reg
                        } else {
                            let tmp = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::GetField {
                                    dst: tmp,
                                    target: t_reg,
                                    field_idx,
                                    optional: false,
                                },
                                span,
                            );
                            self.emit_compound_op(*op, tmp, tmp, val_reg, span)?;
                            tmp
                        };
                        self.proto.emit(
                            Instruction::SetField {
                                target: t_reg,
                                field_idx,
                                val: result_reg,
                            },
                            span,
                        );
                        self.proto.emit(
                            Instruction::Move {
                                dst,
                                src: result_reg,
                            },
                            span,
                        );
                        self.free_reg(t_reg);
                        if result_reg != val_reg {
                            self.free_reg(result_reg);
                        }
                    }
                    Expr::Index { target, index, .. } => {
                        let t_reg = self.alloc_reg()?;
                        self.compile_expr(target, t_reg)?;
                        let i_reg = self.alloc_reg()?;
                        self.compile_expr(index, i_reg)?;
                        // T8.4 修复：`m[k] += 1` 此前编译成 `m[k] = 1`（计数器惯用法
                        // 会静默失效）
                        let result_reg = if matches!(op, AssignOp::Assign) {
                            val_reg
                        } else {
                            let tmp = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::GetIndex {
                                    dst: tmp,
                                    target: t_reg,
                                    index: i_reg,
                                },
                                span,
                            );
                            self.emit_compound_op(*op, tmp, tmp, val_reg, span)?;
                            tmp
                        };
                        self.proto.emit(
                            Instruction::SetIndex {
                                target: t_reg,
                                index: i_reg,
                                val: result_reg,
                            },
                            span,
                        );
                        self.proto.emit(
                            Instruction::Move {
                                dst,
                                src: result_reg,
                            },
                            span,
                        );
                        self.free_reg(i_reg);
                        self.free_reg(t_reg);
                        if result_reg != val_reg {
                            self.free_reg(result_reg);
                        }
                    }
                    _ => return Err(CompileError::InvalidAssignmentTarget { span }),
                }

                self.free_reg(val_reg);
            }

            Expr::Call { callee, args, .. } => {
                let r_callee = self.alloc_reg()?;
                self.compile_expr(callee, r_callee)?;

                let arg_start = Reg(self.next_reg);
                for arg in args {
                    let r_arg = self.alloc_reg()?;
                    self.compile_expr(arg, r_arg)?;
                }

                self.proto.emit(
                    Instruction::Call {
                        dst,
                        callee: r_callee,
                        arg_start,
                        arg_count: args.len() as u16,
                    },
                    span,
                );

                // Free argument registers and callee register
                for _ in args {
                    self.next_reg -= 1;
                }
                self.free_reg(r_callee);
            }

            Expr::Pipe { lhs, rhs, .. } => {
                // Desugar pipe:
                // x |> f(a, b) -> f(x, a, b)
                // x |> f       -> f(x)
                match &**rhs {
                    Expr::Call {
                        callee,
                        args,
                        span: s,
                    } => {
                        let mut new_args = vec![(**lhs).clone()];
                        new_args.extend(args.iter().cloned());
                        let desugared = Expr::Call {
                            callee: callee.clone(),
                            args: new_args,
                            span: *s,
                        };
                        self.compile_expr(&desugared, dst)?;
                    }
                    _ => {
                        let desugared = Expr::Call {
                            callee: rhs.clone(),
                            args: vec![(**lhs).clone()],
                            span,
                        };
                        self.compile_expr(&desugared, dst)?;
                    }
                }
            }

            Expr::NullCoalesce { lhs, rhs, .. } => {
                // Evaluate lhs into dst. If not nil, keep it; if nil, evaluate rhs into dst
                self.compile_expr(lhs, dst)?;
                let jmp_idx = self.proto.emit(
                    Instruction::JumpIfNil {
                        cond: dst,
                        offset: 0,
                    },
                    span,
                );

                // If not nil, jump over rhs
                let jmp_over_idx = self.proto.emit(Instruction::Jump { offset: 0 }, span);

                // Patch JumpIfNil to here
                let rhs_start = self.proto.instructions.len();
                let nil_offset = (rhs_start as i32) - (jmp_idx as i32) - 1;
                if let Instruction::JumpIfNil { offset, .. } = &mut self.proto.instructions[jmp_idx]
                {
                    *offset = nil_offset;
                }

                self.compile_expr(rhs, dst)?;

                // Patch jmp_over to after rhs
                let after_rhs = self.proto.instructions.len();
                let over_offset = (after_rhs as i32) - (jmp_over_idx as i32) - 1;
                if let Instruction::Jump { offset } = &mut self.proto.instructions[jmp_over_idx] {
                    *offset = over_offset;
                }
            }

            Expr::FieldAccess {
                target,
                field,
                optional,
                ..
            } => {
                let t_reg = self.alloc_reg()?;
                self.compile_expr(target, t_reg)?;
                let field_idx = self.proto.add_name(field.clone());
                self.proto.emit(
                    Instruction::GetField {
                        dst,
                        target: t_reg,
                        field_idx,
                        optional: *optional,
                    },
                    span,
                );
                self.free_reg(t_reg);
            }

            Expr::Index { target, index, .. } => {
                let t_reg = self.alloc_reg()?;
                self.compile_expr(target, t_reg)?;
                let i_reg = self.alloc_reg()?;
                self.compile_expr(index, i_reg)?;
                self.proto.emit(
                    Instruction::GetIndex {
                        dst,
                        target: t_reg,
                        index: i_reg,
                    },
                    span,
                );
                self.free_reg(i_reg);
                self.free_reg(t_reg);
            }

            Expr::Array { items, .. } => {
                let start_reg = Reg(self.next_reg);
                for item in items {
                    let r = self.alloc_reg()?;
                    self.compile_expr(item, r)?;
                }
                self.proto.emit(
                    Instruction::NewArray {
                        dst,
                        start: start_reg,
                        count: items.len() as u16,
                    },
                    span,
                );
                for _ in items {
                    self.next_reg -= 1;
                }
            }

            Expr::Map { entries, .. } => {
                self.proto.emit(Instruction::NewMap { dst }, span);
                for (k, v) in entries {
                    let k_reg = self.alloc_reg()?;
                    self.compile_expr(k, k_reg)?;
                    let v_reg = self.alloc_reg()?;
                    self.compile_expr(v, v_reg)?;
                    self.proto.emit(
                        Instruction::MapSet {
                            map: dst,
                            key: k_reg,
                            val: v_reg,
                        },
                        span,
                    );
                    self.free_reg(v_reg);
                    self.free_reg(k_reg);
                }
            }

            Expr::StringInterp { parts, .. } => {
                let start_reg = Reg(self.next_reg);
                for part in parts {
                    let r = self.alloc_reg()?;
                    match part {
                        StringPart::Lit(s) => {
                            let c_idx = self.proto.add_constant(Constant::String(s.clone()));
                            self.proto.emit(
                                Instruction::LoadConst {
                                    dst: r,
                                    const_idx: c_idx,
                                },
                                span,
                            );
                        }
                        StringPart::Expr(e) => {
                            self.compile_expr(e, r)?;
                        }
                    }
                }
                self.proto.emit(
                    Instruction::StringConcat {
                        dst,
                        start: start_reg,
                        count: parts.len() as u16,
                    },
                    span,
                );
                for _ in parts {
                    self.next_reg -= 1;
                }
            }

            Expr::Closure { params, body, .. } => {
                let mut child = Compiler::new("<closure>", params.len() as u16);
                child.prepare_body(body, self.visible_outer_names());
                for (i, p) in params.iter().enumerate() {
                    let reg = Reg(i as u16);
                    let boxed = child.is_boxed(&p.name);
                    if boxed {
                        child
                            .proto
                            .emit(Instruction::MakeCell { dst: reg, src: reg }, span);
                    }
                    child.define_local(p.name.clone(), reg, boxed, true);
                }
                let body_reg = child.alloc_reg()?;
                child.compile_expr(body, body_reg)?;
                child.proto.emit(
                    Instruction::Return {
                        src: Some(body_reg),
                    },
                    body.span(),
                );
                child.proto.register_count = child.max_reg;

                let proto_idx = self.proto.child_protos.len() as u32;
                let upvalues = self.resolve_child_upvalues(&child.upvalue_names, span)?;
                let mut child_proto = child.proto;
                child_proto.upvalues = upvalues.clone();
                self.proto.child_protos.push(child_proto);

                self.emit_closure(dst, proto_idx, &upvalues, span)?;
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                // T6.7：`if <比较>` 直接发出融合的条件跳转
                let jmp_false = self.emit_jump_if_false(cond, span)?;

                self.compile_expr(then_branch, dst)?;

                let jmp_over = self.proto.emit(Instruction::Jump { offset: 0 }, span);

                // Patch jmp_false
                let else_start = self.proto.instructions.len();
                let else_offset = (else_start as i32) - (jmp_false as i32) - 1;
                self.patch_conditional_jump(jmp_false, else_offset);

                if let Some(eb) = else_branch {
                    self.compile_expr(eb, dst)?;
                } else {
                    self.proto.emit(Instruction::LoadNil { dst }, span);
                }

                // Patch jmp_over
                let end_ip = self.proto.instructions.len();
                let over_offset = (end_ip as i32) - (jmp_over as i32) - 1;
                if let Instruction::Jump { offset } = &mut self.proto.instructions[jmp_over] {
                    *offset = over_offset;
                }
            }

            Expr::Block {
                stmts, trailing, ..
            } => {
                self.enter_scope();
                for stmt in stmts {
                    self.compile_stmt(stmt, None)?;
                }
                if let Some(t) = trailing {
                    self.compile_expr(t, dst)?;
                } else {
                    self.proto.emit(Instruction::LoadNil { dst }, span);
                }
                self.exit_scope();
            }

            Expr::Match { target, arms, .. } => {
                let t_reg = self.alloc_reg()?;
                self.compile_expr(target, t_reg)?;

                let mut end_jumps = Vec::new();
                // T3.2：是否存在"不可反驳"的 arm（`_` 或无 guard 的标识符绑定）
                let mut has_irrefutable = false;

                for arm in arms {
                    let mut arm_skip_jumps = Vec::new();

                    match &arm.pattern {
                        Pattern::Wildcard => {
                            // Always matches
                            has_irrefutable = true;
                        }
                        Pattern::Ident(name) => {
                            if arm.guard.is_none() {
                                has_irrefutable = true;
                            }
                            // 绑定到独立寄存器，避免装箱污染匹配目标（T2.2）
                            let bind_reg = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::Move {
                                    dst: bind_reg,
                                    src: t_reg,
                                },
                                arm.span,
                            );
                            let boxed = self.is_boxed(name);
                            if boxed {
                                self.proto.emit(
                                    Instruction::MakeCell {
                                        dst: bind_reg,
                                        src: bind_reg,
                                    },
                                    arm.span,
                                );
                            }
                            self.define_local(name.clone(), bind_reg, boxed, true);
                        }
                        Pattern::Literal(lit) => {
                            let lit_reg = self.alloc_reg()?;
                            match lit {
                                Literal::Int(n) => {
                                    // 超过 i32 的字面量必须走常量池，否则会被截断：
                                    // `match 5000000000 { 5000000000 => … }` 此前会静默失配。
                                    // 与表达式路径（compile_expr 的 Literal::Int）保持同一规则。
                                    if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                                        self.proto.emit(
                                            Instruction::LoadInt {
                                                dst: lit_reg,
                                                val: *n as i32,
                                            },
                                            arm.span,
                                        )
                                    } else {
                                        let c = self.proto.add_constant(Constant::Int(*n));
                                        self.proto.emit(
                                            Instruction::LoadConst {
                                                dst: lit_reg,
                                                const_idx: c,
                                            },
                                            arm.span,
                                        )
                                    }
                                }
                                Literal::Float(f) => {
                                    let c = self.proto.add_constant(Constant::Float(*f));
                                    self.proto.emit(
                                        Instruction::LoadConst {
                                            dst: lit_reg,
                                            const_idx: c,
                                        },
                                        arm.span,
                                    )
                                }
                                Literal::String(s) => {
                                    let c = self.proto.add_constant(Constant::String(s.clone()));
                                    self.proto.emit(
                                        Instruction::LoadConst {
                                            dst: lit_reg,
                                            const_idx: c,
                                        },
                                        arm.span,
                                    )
                                }
                                Literal::Bool(b) => self.proto.emit(
                                    Instruction::LoadBool {
                                        dst: lit_reg,
                                        val: *b,
                                    },
                                    arm.span,
                                ),
                                Literal::Nil => self
                                    .proto
                                    .emit(Instruction::LoadNil { dst: lit_reg }, arm.span),
                            };
                            let cmp_reg = self.alloc_reg()?;
                            self.proto.emit(
                                Instruction::Eq {
                                    dst: cmp_reg,
                                    lhs: t_reg,
                                    rhs: lit_reg,
                                },
                                arm.span,
                            );
                            let jmp = self.proto.emit(
                                Instruction::JumpIfFalse {
                                    cond: cmp_reg,
                                    offset: 0,
                                },
                                arm.span,
                            );
                            arm_skip_jumps.push(jmp);
                            self.free_reg(cmp_reg);
                            self.free_reg(lit_reg);
                        }
                        Pattern::Compare { op, value } => {
                            let val_reg = self.alloc_reg()?;
                            self.compile_expr(value, val_reg)?;
                            let cmp_reg = self.alloc_reg()?;
                            match op {
                                BinOp::Gt => self.proto.emit(
                                    Instruction::Gt {
                                        dst: cmp_reg,
                                        lhs: t_reg,
                                        rhs: val_reg,
                                    },
                                    arm.span,
                                ),
                                BinOp::Ge => self.proto.emit(
                                    Instruction::Ge {
                                        dst: cmp_reg,
                                        lhs: t_reg,
                                        rhs: val_reg,
                                    },
                                    arm.span,
                                ),
                                BinOp::Lt => self.proto.emit(
                                    Instruction::Lt {
                                        dst: cmp_reg,
                                        lhs: t_reg,
                                        rhs: val_reg,
                                    },
                                    arm.span,
                                ),
                                BinOp::Le => self.proto.emit(
                                    Instruction::Le {
                                        dst: cmp_reg,
                                        lhs: t_reg,
                                        rhs: val_reg,
                                    },
                                    arm.span,
                                ),
                                _ => unreachable!(),
                            };
                            let jmp = self.proto.emit(
                                Instruction::JumpIfFalse {
                                    cond: cmp_reg,
                                    offset: 0,
                                },
                                arm.span,
                            );
                            arm_skip_jumps.push(jmp);
                            self.free_reg(cmp_reg);
                            self.free_reg(val_reg);
                        }
                    }

                    if let Some(guard) = &arm.guard {
                        // T6.7：guard 是比较表达式时同样走融合跳转
                        let jmp = self.emit_jump_if_false(guard, arm.span)?;
                        arm_skip_jumps.push(jmp);
                    }

                    // Body
                    self.compile_expr(&arm.body, dst)?;
                    let jmp_end = self.proto.emit(Instruction::Jump { offset: 0 }, arm.span);
                    end_jumps.push(jmp_end);

                    // Patch arm skips to here
                    let next_arm_ip = self.proto.instructions.len();
                    for jmp in arm_skip_jumps {
                        let offset = (next_arm_ip as i32) - (jmp as i32) - 1;
                        // T6.7：guard 的比较跳转是融合形态，必须用通用回填
                        self.patch_conditional_jump(jmp, offset);
                    }
                }

                // T3.2：没有不可反驳的 arm 时显式报错，而不是留下未赋值的匹配目标
                if !has_irrefutable {
                    self.proto.emit(Instruction::MatchFail, span);
                }

                // Patch all end jumps to end
                let match_end_ip = self.proto.instructions.len();
                for jmp in end_jumps {
                    let offset = (match_end_ip as i32) - (jmp as i32) - 1;
                    if let Instruction::Jump { offset: o } = &mut self.proto.instructions[jmp] {
                        *o = offset;
                    }
                }

                self.free_reg(t_reg);
            }

            Expr::Await { expr, .. } => {
                let r_fut = self.alloc_reg()?;
                self.compile_expr(expr, r_fut)?;
                self.proto
                    .emit(Instruction::Await { dst, future: r_fut }, span);
                self.free_reg(r_fut);
            }

            Expr::Spawn { expr, .. } => {
                // T4.1：`spawn f(a, b)` 只捕获调用目标与实参，不在此处执行；
                // 非调用表达式（`spawn 50`）在 spawn 点求值并直接成为完成态任务。
                match &**expr {
                    Expr::Call { callee, args, .. } => {
                        let r_callee = self.alloc_reg()?;
                        self.compile_expr(callee, r_callee)?;

                        let arg_start = Reg(self.next_reg);
                        for arg in args {
                            let r_arg = self.alloc_reg()?;
                            self.compile_expr(arg, r_arg)?;
                        }

                        self.proto.emit(
                            Instruction::Spawn {
                                dst,
                                callee: r_callee,
                                arg_start,
                                arg_count: args.len() as u16,
                            },
                            span,
                        );

                        for _ in args {
                            self.next_reg -= 1;
                        }
                        self.free_reg(r_callee);
                    }
                    other => {
                        let r_inner = self.alloc_reg()?;
                        self.compile_expr(other, r_inner)?;
                        self.proto
                            .emit(Instruction::SpawnValue { dst, src: r_inner }, span);
                        self.free_reg(r_inner);
                    }
                }
            }
        }
        Ok(())
    }
}

// ==========================================
// T2.2 预扫描：哪些局部变量需要装箱
// ==========================================

/// 收集 `body` 内部**嵌套函数**中出现过的所有标识符。
///
/// 用途：决定本函数的哪些局部变量需要装箱成 `Value::Cell`。
/// 这是一个**上近似**：多装箱只多一点开销，不会改变语义；而漏装箱会让闭包
/// 捕获到值拷贝（写入不共享），因此宁可多。
fn collect_names_in_nested_fns(body: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_expr(body, false, &mut out);
    out
}

fn collect_expr(expr: &Expr, in_fn: bool, out: &mut HashSet<String>) {
    match expr {
        Expr::Literal(..) => {}
        Expr::Ident(name, _) => {
            if in_fn {
                out.insert(name.clone());
            }
        }
        Expr::Unary { expr, .. } => collect_expr(expr, in_fn, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_expr(lhs, in_fn, out);
            collect_expr(rhs, in_fn, out);
        }
        Expr::Assign { target, value, .. } => {
            collect_expr(target, in_fn, out);
            collect_expr(value, in_fn, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_expr(callee, in_fn, out);
            for arg in args {
                collect_expr(arg, in_fn, out);
            }
        }
        Expr::Index { target, index, .. } => {
            collect_expr(target, in_fn, out);
            collect_expr(index, in_fn, out);
        }
        Expr::FieldAccess { target, .. } => collect_expr(target, in_fn, out),
        Expr::Pipe { lhs, rhs, .. } | Expr::NullCoalesce { lhs, rhs, .. } => {
            collect_expr(lhs, in_fn, out);
            collect_expr(rhs, in_fn, out);
        }
        Expr::Array { items, .. } => {
            for item in items {
                collect_expr(item, in_fn, out);
            }
        }
        Expr::Map { entries, .. } => {
            for (key, value) in entries {
                collect_expr(key, in_fn, out);
                collect_expr(value, in_fn, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let StringPart::Expr(expr) = part {
                    collect_expr(expr, in_fn, out);
                }
            }
        }
        // 进入嵌套函数：其内部的一切标识符都算"被捕获的候选"
        Expr::Closure { body, .. } => collect_expr(body, true, out),
        Expr::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_expr(cond, in_fn, out);
            collect_expr(then_branch, in_fn, out);
            if let Some(else_branch) = else_branch {
                collect_expr(else_branch, in_fn, out);
            }
        }
        Expr::Block {
            stmts, trailing, ..
        } => {
            for stmt in stmts {
                collect_stmt(stmt, in_fn, out);
            }
            if let Some(trailing) = trailing {
                collect_expr(trailing, in_fn, out);
            }
        }
        Expr::Match { target, arms, .. } => {
            collect_expr(target, in_fn, out);
            for arm in arms {
                if let Pattern::Compare { value, .. } = &arm.pattern {
                    collect_expr(value, in_fn, out);
                }
                if let Some(guard) = &arm.guard {
                    collect_expr(guard, in_fn, out);
                }
                collect_expr(&arm.body, in_fn, out);
            }
        }
        Expr::Await { expr, .. } | Expr::Spawn { expr, .. } => collect_expr(expr, in_fn, out),
    }
}

fn collect_stmt(stmt: &Stmt, in_fn: bool, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                collect_expr(init, in_fn, out);
            }
        }
        Stmt::Fn { body, .. } => collect_expr(body, true, out),
        Stmt::While { cond, body, .. } => {
            collect_expr(cond, in_fn, out);
            collect_expr(body, in_fn, out);
        }
        Stmt::For { iter, body, .. } => {
            collect_expr(iter, in_fn, out);
            collect_expr(body, in_fn, out);
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                collect_expr(value, in_fn, out);
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
        Stmt::Expr { expr, .. } => collect_expr(expr, in_fn, out),
        Stmt::Import { .. } => {}
    }
}

/// T8.3：把"是否 `mut`"翻译成全局写入种类。///
/// **声明**（`let`/`mut`）让脚本接管该名字：可以从内建名手里夺过来，也会重置
/// 之前的不可变标记。**赋值**（`Assign`）才受"不可变全局 / 内建名"保护。
fn global_write_kind(is_mut: bool) -> GlobalWrite {
    if is_mut {
        GlobalWrite::DeclareMut
    } else {
        GlobalWrite::DeclareImmutable
    }
}

/// T6.6：识别"一侧是 i32 范围内的整数字面量"的二元表达式。
///
/// 返回 `(要编译成寄存器的另一侧, 常量, 融合后的运算符)`。
///
/// 规则：
/// - 字面量在右侧且运算符可融合 → 直接融合（`x + 2`、`i % 2`）；
/// - 字面量在左侧时，仅当**交换两侧仍等价**才融合：`Add`/`Mul`/`Eq`/`Ne` 可交换，
///   比较运算按镜像处理（`2 < x` ≡ `x > 2`）；`Sub`/`Div`/`Mod` 不可镜像，放弃。
/// - 只处理算术与比较：短路运算符（`&&`/`||`）与位运算不在此列。
fn fuse_int_literal<'a>(op: BinOp, lhs: &'a Expr, rhs: &'a Expr) -> Option<(&'a Expr, i32, BinOp)> {
    let allowed = |op: BinOp| {
        matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
        )
    };
    if !allowed(op) {
        return None;
    }

    if let Some(value) = int_literal(rhs) {
        return Some((lhs, value, op));
    }

    let value = int_literal(lhs)?;
    let mirrored = match op {
        BinOp::Add | BinOp::Mul | BinOp::Eq | BinOp::Ne => op,
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        // `k - x`、`k / x`、`k % x` 没有等价的"常量在右"形式
        _ => return None,
    };
    Some((rhs, value, mirrored))
}

/// 取 i32 范围内的整数字面量（超出 i32 时不能用融合指令，退回通用路径）。
fn int_literal(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::Literal(Literal::Int(n), _) if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 => {
            Some(*n as i32)
        }
        _ => None,
    }
}

/// 是否为比较运算符（T6.7：只有比较条件才走"比较 + 跳转"融合）。
fn is_comparison(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    )
}

/// T6.6b：复合赋值运算符 → 对应的二元运算符（用于 `x += 1` 的常量融合）。
/// `Assign` 本身不是二元运算，返回 `None`。
fn compound_binop(op: AssignOp) -> Option<BinOp> {
    match op {
        AssignOp::AddAssign => Some(BinOp::Add),
        AssignOp::SubAssign => Some(BinOp::Sub),
        AssignOp::MulAssign => Some(BinOp::Mul),
        AssignOp::DivAssign => Some(BinOp::Div),
        AssignOp::ModAssign => Some(BinOp::Mod),
        AssignOp::Assign => None,
    }
}
