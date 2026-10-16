use crate::bytecode::{Chunk, Opcode, Value};
use crate::error::CompileError;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use trulang_syntax::ast::*;
use trulang_syntax::span::Spanned;

#[derive(Default)]
struct ScopeContext {
    locals: HashMap<String, usize>,
    next_slot: usize,
}

pub struct CodeGenerator {
    pub chunk: Chunk,
    scopes: Vec<ScopeContext>,
    persistent_fields: HashSet<String>,
}

impl CodeGenerator {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            scopes: vec![ScopeContext::default()],
            persistent_fields: HashSet::new(),
        }
    }

    fn is_persistent_field(&self, name: &str) -> bool {
        self.persistent_fields.contains(name)
    }

    fn is_persistent_expr(&self, expr: &Spanned<Expr>) -> bool {
        match &expr.node {
            Expr::Ident(name) => self.is_persistent_field(name),
            Expr::MemberAccess { target, field, .. } => {
                if let Expr::Ident(tgt) = &target.node {
                    if tgt == "self" || tgt == "this" {
                        return self.is_persistent_field(field);
                    }
                }
                self.is_persistent_field(field)
            }
            _ => false,
        }
    }

    fn enter_scope(&mut self) {
        let next_slot = self.scopes.last().map(|s| s.next_slot).unwrap_or(0);
        self.scopes.push(ScopeContext {
            locals: HashMap::new(),
            next_slot,
        });
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_local(&mut self, name: &str) -> usize {
        let scope = self.scopes.last_mut().expect("No active scope");
        let slot = scope.next_slot;
        scope.locals.insert(name.to_string(), slot);
        scope.next_slot += 1;
        slot
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        for scope in self.scopes.iter().rev() {
            if let Some(&slot) = scope.locals.get(name) {
                return Some(slot);
            }
        }
        None
    }

    pub fn compile_program(&mut self, program: &Program) -> Result<(), CompileError> {
        for decl in &program.declarations {
            match &decl.node {
                Declaration::Service(svc) => self.compile_service(svc)?,
                Declaration::Function(func) => {
                    let entry_ip = self.chunk.code.len();
                    self.chunk.functions.insert(func.name.clone(), entry_ip);
                    self.compile_function(func)?;
                }
                Declaration::Struct(_) => {}
                Declaration::Import(imp) => {
                    self.chunk.imports.insert(imp.alias.clone(), imp.path.clone());
                }
                Declaration::Test(t) => {
                    self.compile_test(t, decl.span.line, program)?;
                }
            }
        }
        Ok(())
    }

    pub fn compile_test(&mut self, test_decl: &trulang_syntax::ast::TestDecl, line: usize, program: &Program) -> Result<(), CompileError> {
        use crate::bytecode::CompiledTest;
        let mut test_gen = CodeGenerator::new();
        test_gen.chunk.imports = self.chunk.imports.clone();

        // Jump over function declarations to test body
        let test_entry_jump = test_gen.chunk.emit(Opcode::Jump(0), line);

        // Compile functions declared in the file into this test chunk
        for decl in &program.declarations {
            if let Declaration::Function(func) = &decl.node {
                let fn_entry = test_gen.chunk.code.len();
                test_gen.chunk.functions.insert(func.name.clone(), fn_entry);
                test_gen.compile_function(func)?;
            }
        }

        let test_start = test_gen.chunk.code.len();
        test_gen.chunk.patch_jump(test_entry_jump, test_start);

        test_gen.enter_scope();
        for stmt in &test_decl.body.statements {
            test_gen.compile_statement(stmt)?;
        }
        test_gen.exit_scope();
        test_gen.chunk.emit(Opcode::LoadNull, line);
        test_gen.chunk.emit(Opcode::Return, line);

        self.chunk.tests.push(CompiledTest {
            name: test_decl.name.clone(),
            chunk: test_gen.chunk,
            line,
        });
        Ok(())
    }

    pub fn compile_service(&mut self, svc: &ServiceDecl) -> Result<(), CompileError> {
        // Track persistent fields declared on the service actor
        for field in &svc.fields {
            self.persistent_fields.insert(field.node.name.clone());
        }

        // Find budgets if declared
        for budget in &svc.budgets {
            if budget.node.region == BudgetRegion::Request {
                self.chunk.emit(
                    Opcode::AllocRegionA(budget.node.size_bytes, budget.node.max_size_bytes),
                    budget.span.line,
                );
            }
        }

        // Initialize service fields (e.g. let mut cache = Map::new())
        for field in &svc.fields {
            if let Some(ref init) = field.node.init {
                self.compile_expr(init)?;
                let slot = self.define_local(&field.node.name);
                self.chunk.emit(Opcode::SetLocal(slot), field.span.line);
            }
        }

        for method in &svc.methods {
            self.compile_function(&method.node)?;
        }

        Ok(())
    }

    pub fn compile_function(&mut self, func: &FunctionDecl) -> Result<(), CompileError> {
        self.enter_scope();

        for param in &func.params {
            self.define_local(&param.name);
        }

        for stmt in &func.body.statements {
            self.compile_statement(stmt)?;
        }

        // Implicit return null if not ending with return
        self.chunk.emit(Opcode::LoadNull, func.body.statements.last().map(|s| s.span.line).unwrap_or(1));
        self.chunk.emit(Opcode::Return, func.body.statements.last().map(|s| s.span.line).unwrap_or(1));

        self.exit_scope();
        Ok(())
    }

    pub fn compile_statement(&mut self, stmt: &Spanned<Statement>) -> Result<(), CompileError> {
        let line = stmt.span.line;

        match &stmt.node {
            Statement::Let { name, init, .. } => {
                self.compile_expr(init)?;
                let slot = self.define_local(name);
                self.chunk.emit(Opcode::SetLocal(slot), line);
            }
            Statement::Assign { target, value } => {
                match &target.node {
                    Expr::Ident(name) => {
                        self.compile_expr(value)?;
                        // Auto-promotion: promote transient data to Region B when written to actor persistent field
                        if self.is_persistent_field(name) && !matches!(value.node, Expr::Persist(_)) {
                            self.chunk.emit(Opcode::Persist, line);
                        }
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.emit(Opcode::SetLocal(slot), line);
                        }
                    }
                    Expr::MemberAccess { target: obj, field, .. } => {
                        self.compile_expr(value)?;
                        // Auto-promotion: promote transient data when written to actor persistent state or member
                        if (self.is_persistent_expr(obj) || self.is_persistent_field(field)) && !matches!(value.node, Expr::Persist(_)) {
                            self.chunk.emit(Opcode::Persist, line);
                        }
                        self.compile_expr(obj)?;
                        self.chunk.emit(Opcode::SetFieldNamed(field.clone()), line);
                    }
                    _ => {}
                }
            }
            Statement::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.compile_expr(expr)?;
                } else {
                    self.chunk.emit(Opcode::LoadNull, line);
                }
                // Run defers before returning
                self.chunk.emit(Opcode::RunDefers, line);
                self.chunk.emit(Opcode::Return, line);
            }
            Statement::Defer(expr) => {
                let jump_over = self.chunk.emit(Opcode::Jump(0), line);
                let defer_start = self.chunk.code.len();

                self.compile_expr(expr)?;
                self.chunk.emit(Opcode::Pop, line);
                self.chunk.emit(Opcode::Return, line);

                let after_defer = self.chunk.code.len();
                self.chunk.patch_jump(jump_over, after_defer);

                self.chunk.emit(Opcode::PushDefer(defer_start), line);
            }
            Statement::If { condition, then_branch, else_branch } => {
                self.compile_expr(condition)?;
                let jump_false = self.chunk.emit(Opcode::JumpIfFalse(0), line);

                self.enter_scope();
                for s in &then_branch.statements {
                    self.compile_statement(s)?;
                }
                self.exit_scope();

                if let Some(else_b) = else_branch {
                    let jump_end = self.chunk.emit(Opcode::Jump(0), line);
                    let else_start = self.chunk.code.len();
                    self.chunk.patch_jump(jump_false, else_start);

                    self.enter_scope();
                    for s in &else_b.statements {
                        self.compile_statement(s)?;
                    }
                    self.exit_scope();

                    let end_pos = self.chunk.code.len();
                    self.chunk.patch_jump(jump_end, end_pos);
                } else {
                    let end_pos = self.chunk.code.len();
                    self.chunk.patch_jump(jump_false, end_pos);
                }
            }
            Statement::While { condition, body } => {
                let loop_start = self.chunk.code.len();
                self.compile_expr(condition)?;
                let jump_exit = self.chunk.emit(Opcode::JumpIfFalse(0), line);

                self.enter_scope();
                for s in &body.statements {
                    self.compile_statement(s)?;
                }
                self.exit_scope();

                self.chunk.emit(Opcode::Jump(loop_start), line);
                let exit_pos = self.chunk.code.len();
                self.chunk.patch_jump(jump_exit, exit_pos);
            }
            Statement::TryCatch { try_block, error_name, catch_block, .. } => {
                let push_try = self.chunk.emit(Opcode::PushTry(0), line);

                self.enter_scope();
                for s in &try_block.statements {
                    self.compile_statement(s)?;
                }
                self.exit_scope();

                self.chunk.emit(Opcode::PopTry, line);
                let jump_end = self.chunk.emit(Opcode::Jump(0), line);

                let catch_start = self.chunk.code.len();
                self.chunk.patch_jump(push_try, catch_start);

                self.enter_scope();
                let err_slot = self.define_local(error_name);
                self.chunk.emit(Opcode::SetLocal(err_slot), line);

                for s in &catch_block.statements {
                    self.compile_statement(s)?;
                }
                self.exit_scope();

                let end_pos = self.chunk.code.len();
                self.chunk.patch_jump(jump_end, end_pos);
            }
            Statement::Expr(expr) => {
                self.compile_expr(expr)?;
                self.chunk.emit(Opcode::Pop, line);
            }
            Statement::Block(b) => {
                self.enter_scope();
                for s in &b.statements {
                    self.compile_statement(s)?;
                }
                self.exit_scope();
            }
        }
        Ok(())
    }

    pub fn compile_expr(&mut self, expr: &Spanned<Expr>) -> Result<(), CompileError> {
        let line = expr.span.line;

        match &expr.node {
            Expr::Literal(lit) => match lit {
                Literal::Int(n) => {
                    self.chunk.emit(Opcode::LoadInt(*n), line);
                }
                Literal::Float(fl) => {
                    self.chunk.emit(Opcode::LoadFloat(*fl), line);
                }
                Literal::Bool(b) => {
                    self.chunk.emit(Opcode::LoadBool(*b), line);
                }
                Literal::Null => {
                    self.chunk.emit(Opcode::LoadNull, line);
                }
                Literal::String(s) => {
                    let const_idx = self.chunk.add_constant(Value::String(Arc::new(s.clone())));
                    self.chunk.emit(Opcode::LoadConst(const_idx), line);
                }
            },
            Expr::Ident(name) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.chunk.emit(Opcode::GetLocal(slot), line);
                } else {
                    let const_idx = self.chunk.add_constant(Value::String(Arc::new(name.clone())));
                    self.chunk.emit(Opcode::LoadConst(const_idx), line);
                }
            }
            Expr::Binary { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                match op {
                    BinOp::Add => self.chunk.emit(Opcode::Add, line),
                    BinOp::Sub => self.chunk.emit(Opcode::Sub, line),
                    BinOp::Mul => self.chunk.emit(Opcode::Mul, line),
                    BinOp::Div => self.chunk.emit(Opcode::Div, line),
                    BinOp::Mod => self.chunk.emit(Opcode::Mod, line),
                    BinOp::Eq => self.chunk.emit(Opcode::Eq, line),
                    BinOp::NotEq => self.chunk.emit(Opcode::NotEq, line),
                    BinOp::Lt => self.chunk.emit(Opcode::Lt, line),
                    BinOp::LtEq => self.chunk.emit(Opcode::LtEq, line),
                    BinOp::Gt => self.chunk.emit(Opcode::Gt, line),
                    BinOp::GtEq => self.chunk.emit(Opcode::GtEq, line),
                    BinOp::And => self.chunk.emit(Opcode::Mul, line),
                    BinOp::Or => self.chunk.emit(Opcode::Add, line),
                };
            }
            Expr::Unary { op, expr: inner } => {
                self.compile_expr(inner)?;
                match op {
                    UnaryOp::Neg => self.chunk.emit(Opcode::Neg, line),
                    UnaryOp::Not => self.chunk.emit(Opcode::Not, line),
                };
            }
            Expr::Call { callee, args } => {
                // If Map::new()
                if let Expr::Path(segments) = &callee.node {
                    if segments.len() == 2 && segments[0] == "Map" && segments[1] == "new" {
                        self.chunk.emit(Opcode::NewMap, line);
                        return Ok(());
                    }
                }

                // If builtin print/println
                if let Expr::Ident(name) = &callee.node {
                    if name == "println" {
                        for arg in args {
                            self.compile_expr(arg)?;
                            self.chunk.emit(Opcode::Println, line);
                        }
                        self.chunk.emit(Opcode::LoadNull, line);
                        return Ok(());
                    } else if name == "print" {
                        for arg in args {
                            self.compile_expr(arg)?;
                            self.chunk.emit(Opcode::Print, line);
                        }
                        self.chunk.emit(Opcode::LoadNull, line);
                        return Ok(());
                    } else if name == "assert" {
                        if args.is_empty() || args.len() > 2 {
                            return Err(CompileError::new(
                                "assert expects 1 or 2 arguments: assert(condition, [msg])",
                                callee.span,
                            ));
                        }
                        self.compile_expr(&args[0])?;
                        let msg = if args.len() == 2 {
                            if let Expr::Literal(trulang_syntax::ast::Literal::String(s)) = &args[1].node {
                                Some(s.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        self.chunk.emit(Opcode::Assert(msg), line);
                        self.chunk.emit(Opcode::LoadNull, line);
                        return Ok(());
                    } else if name == "assert_eq" {
                        if args.len() != 2 && args.len() != 3 {
                            return Err(CompileError::new(
                                "assert_eq expects 2 or 3 arguments: assert_eq(actual, expected, [msg])",
                                callee.span,
                            ));
                        }
                        self.compile_expr(&args[0])?; // actual
                        self.compile_expr(&args[1])?; // expected
                        let msg = if args.len() == 3 {
                            if let Expr::Literal(trulang_syntax::ast::Literal::String(s)) = &args[2].node {
                                Some(s.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        self.chunk.emit(Opcode::AssertEq(msg), line);
                        self.chunk.emit(Opcode::LoadNull, line);
                        return Ok(());
                    }
                }

                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.compile_expr(callee)?;
                self.chunk.emit(Opcode::Call(args.len()), line);
            }
            Expr::MethodCall { receiver, method, args } => {
                let is_mutating_persistent_collection = self.is_persistent_expr(receiver)
                    && (method == "insert" || method == "set" || method == "push");

                for (i, arg) in args.iter().enumerate() {
                    self.compile_expr(arg)?;
                    let is_value_arg = (method == "push" && i == 0)
                        || ((method == "insert" || method == "set") && i == 1);
                    if is_mutating_persistent_collection && is_value_arg && !matches!(arg.node, Expr::Persist(_)) {
                        self.chunk.emit(Opcode::Persist, line);
                    }
                }
                self.compile_expr(receiver)?;
                self.chunk.emit(Opcode::CallMethod(method.clone(), args.len()), line);
            }
            Expr::MemberAccess { target, field, is_optional: _ } => {
                self.compile_expr(target)?;
                self.chunk.emit(Opcode::GetFieldNamed(field.clone()), line);
            }
            Expr::Persist(inner) => {
                self.compile_expr(inner)?;
                self.chunk.emit(Opcode::Persist, line);
            }
            Expr::Path(segments) => {
                let joined = segments.join("::");
                let const_idx = self.chunk.add_constant(Value::String(Arc::new(joined)));
                self.chunk.emit(Opcode::LoadConst(const_idx), line);
            }
        }
        Ok(())
    }
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}
