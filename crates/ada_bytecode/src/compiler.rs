use crate::instruction::{Constant, FunctionProto, Instruction, Reg};
use ada_ast::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Variable '{0}' not found")]
    VariableNotFound(String),
    #[error("Too many registers in function")]
    TooManyRegisters,
    #[error("Invalid assignment target")]
    InvalidAssignmentTarget,
}

struct Local {
    name: String,
    reg: Reg,
    depth: usize,
}

pub struct Compiler {
    proto: FunctionProto,
    locals: Vec<Local>,
    scope_depth: usize,
    next_reg: u16,
    max_reg: u16,
    loop_stack: Vec<LoopContext>,
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
        };
        comp.max_reg = comp.max_reg.max(arity);
        comp
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

    fn define_local(&mut self, name: String, reg: Reg) {
        self.locals.push(Local {
            name,
            reg,
            depth: self.scope_depth,
        });
    }

    fn resolve_local(&self, name: &str) -> Option<Reg> {
        for local in self.locals.iter().rev() {
            if local.name == name {
                return Some(local.reg);
            }
        }
        None
    }

    pub fn compile_program(program: &Program) -> Result<FunctionProto, CompileError> {
        let mut compiler = Compiler::new("<main>", 0);
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
            Stmt::Let {
                name, init, span, ..
            } => {
                let reg = self.alloc_reg()?;
                if let Some(init_expr) = init {
                    self.compile_expr(init_expr, reg)?;
                } else {
                    self.proto.emit(Instruction::LoadNil { dst: reg }, *span);
                }
                self.define_local(name.clone(), reg);
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
                for (i, p) in params.iter().enumerate() {
                    child.define_local(p.name.clone(), Reg(i as u16));
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
                self.proto.child_protos.push(child.proto);

                let fn_reg = self.alloc_reg()?;
                self.proto.emit(
                    Instruction::Closure {
                        dst: fn_reg,
                        proto_idx,
                    },
                    *span,
                );
                if self.scope_depth == 0 {
                    let name_idx = self.proto.add_name(name.clone());
                    self.proto.emit(
                        Instruction::SetGlobal {
                            name_idx,
                            src: fn_reg,
                        },
                        *span,
                    );
                }
                self.define_local(name.clone(), fn_reg);
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

                let cond_reg = self.alloc_reg()?;
                self.compile_expr(cond, cond_reg)?;

                let jump_exit_idx = self.proto.emit(
                    Instruction::JumpIfFalse {
                        cond: cond_reg,
                        offset: 0,
                    },
                    *span,
                );
                self.free_reg(cond_reg);

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
                if let Instruction::JumpIfFalse { offset, .. } =
                    &mut self.proto.instructions[jump_exit_idx]
                {
                    *offset = exit_offset;
                }

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
                        self.define_local(name.clone(), val_reg);
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
                            self.define_local(name.clone(), item_reg);
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
                if let Some(ctx) = self.loop_stack.last_mut() {
                    let jmp_idx = self.proto.emit(Instruction::Jump { offset: 0 }, *span);
                    ctx.break_jumps.push(jmp_idx);
                }
            }

            Stmt::Continue { span } => {
                if let Some(ctx) = self.loop_stack.last() {
                    let loop_offset =
                        (ctx.start_ip as i32) - (self.proto.instructions.len() as i32) - 1;
                    self.proto.emit(
                        Instruction::Jump {
                            offset: loop_offset,
                        },
                        *span,
                    );
                }
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
                if let Some(local_reg) = self.resolve_local(name) {
                    if local_reg != dst {
                        self.proto.emit(
                            Instruction::Move {
                                dst,
                                src: local_reg,
                            },
                            span,
                        );
                    }
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
                let val_reg = self.alloc_reg()?;
                self.compile_expr(value, val_reg)?;

                match &**target {
                    Expr::Ident(name, _) => {
                        if let Some(local_reg) = self.resolve_local(name) {
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
                                AssignOp::AddAssign => {
                                    self.proto.emit(
                                        Instruction::Add {
                                            dst: local_reg,
                                            lhs: local_reg,
                                            rhs: val_reg,
                                        },
                                        span,
                                    );
                                }
                                AssignOp::SubAssign => {
                                    self.proto.emit(
                                        Instruction::Sub {
                                            dst: local_reg,
                                            lhs: local_reg,
                                            rhs: val_reg,
                                        },
                                        span,
                                    );
                                }
                                AssignOp::MulAssign => {
                                    self.proto.emit(
                                        Instruction::Mul {
                                            dst: local_reg,
                                            lhs: local_reg,
                                            rhs: val_reg,
                                        },
                                        span,
                                    );
                                }
                                AssignOp::DivAssign => {
                                    self.proto.emit(
                                        Instruction::Div {
                                            dst: local_reg,
                                            lhs: local_reg,
                                            rhs: val_reg,
                                        },
                                        span,
                                    );
                                }
                                AssignOp::ModAssign => {
                                    self.proto.emit(
                                        Instruction::Mod {
                                            dst: local_reg,
                                            lhs: local_reg,
                                            rhs: val_reg,
                                        },
                                        span,
                                    );
                                }
                            }
                            self.proto.emit(
                                Instruction::Move {
                                    dst,
                                    src: local_reg,
                                },
                                span,
                            );
                        } else {
                            let name_idx = self.proto.add_name(name.clone());
                            self.proto.emit(
                                Instruction::SetGlobal {
                                    name_idx,
                                    src: val_reg,
                                },
                                span,
                            );
                            self.proto
                                .emit(Instruction::Move { dst, src: val_reg }, span);
                        }
                    }
                    Expr::FieldAccess { target, field, .. } => {
                        let t_reg = self.alloc_reg()?;
                        self.compile_expr(target, t_reg)?;
                        let field_idx = self.proto.add_name(field.clone());
                        self.proto.emit(
                            Instruction::SetField {
                                target: t_reg,
                                field_idx,
                                val: val_reg,
                            },
                            span,
                        );
                        self.proto
                            .emit(Instruction::Move { dst, src: val_reg }, span);
                        self.free_reg(t_reg);
                    }
                    Expr::Index { target, index, .. } => {
                        let t_reg = self.alloc_reg()?;
                        self.compile_expr(target, t_reg)?;
                        let i_reg = self.alloc_reg()?;
                        self.compile_expr(index, i_reg)?;
                        self.proto.emit(
                            Instruction::SetIndex {
                                target: t_reg,
                                index: i_reg,
                                val: val_reg,
                            },
                            span,
                        );
                        self.proto
                            .emit(Instruction::Move { dst, src: val_reg }, span);
                        self.free_reg(i_reg);
                        self.free_reg(t_reg);
                    }
                    _ => return Err(CompileError::InvalidAssignmentTarget),
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
                for (i, p) in params.iter().enumerate() {
                    child.define_local(p.name.clone(), Reg(i as u16));
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
                self.proto.child_protos.push(child.proto);

                self.proto
                    .emit(Instruction::Closure { dst, proto_idx }, span);
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                let cond_reg = self.alloc_reg()?;
                self.compile_expr(cond, cond_reg)?;

                let jmp_false = self.proto.emit(
                    Instruction::JumpIfFalse {
                        cond: cond_reg,
                        offset: 0,
                    },
                    span,
                );
                self.free_reg(cond_reg);

                self.compile_expr(then_branch, dst)?;

                let jmp_over = self.proto.emit(Instruction::Jump { offset: 0 }, span);

                // Patch jmp_false
                let else_start = self.proto.instructions.len();
                let else_offset = (else_start as i32) - (jmp_false as i32) - 1;
                if let Instruction::JumpIfFalse { offset, .. } =
                    &mut self.proto.instructions[jmp_false]
                {
                    *offset = else_offset;
                }

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

                for arm in arms {
                    let mut arm_skip_jumps = Vec::new();

                    match &arm.pattern {
                        Pattern::Wildcard => {
                            // Always matches
                        }
                        Pattern::Ident(name) => {
                            // Bind variable to target
                            self.define_local(name.clone(), t_reg);
                        }
                        Pattern::Literal(lit) => {
                            let lit_reg = self.alloc_reg()?;
                            match lit {
                                Literal::Int(n) => self.proto.emit(
                                    Instruction::LoadInt {
                                        dst: lit_reg,
                                        val: *n as i32,
                                    },
                                    arm.span,
                                ),
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
                        let g_reg = self.alloc_reg()?;
                        self.compile_expr(guard, g_reg)?;
                        let jmp = self.proto.emit(
                            Instruction::JumpIfFalse {
                                cond: g_reg,
                                offset: 0,
                            },
                            arm.span,
                        );
                        arm_skip_jumps.push(jmp);
                        self.free_reg(g_reg);
                    }

                    // Body
                    self.compile_expr(&arm.body, dst)?;
                    let jmp_end = self.proto.emit(Instruction::Jump { offset: 0 }, arm.span);
                    end_jumps.push(jmp_end);

                    // Patch arm skips to here
                    let next_arm_ip = self.proto.instructions.len();
                    for jmp in arm_skip_jumps {
                        let offset = (next_arm_ip as i32) - (jmp as i32) - 1;
                        if let Instruction::JumpIfFalse { offset: o, .. } =
                            &mut self.proto.instructions[jmp]
                        {
                            *o = offset;
                        }
                    }
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
                let r_inner = self.alloc_reg()?;
                self.compile_expr(expr, r_inner)?;
                self.proto
                    .emit(Instruction::Spawn { dst, expr: r_inner }, span);
                self.free_reg(r_inner);
            }
        }
        Ok(())
    }
}
