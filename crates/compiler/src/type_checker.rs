use crate::error::CompileError;
use crate::symbols::SymbolTable;
use crate::types::SemType;
use vlang_syntax::ast::*;
use vlang_syntax::span::{Span, Spanned};

pub struct TypeChecker {
    symbols: SymbolTable,
    current_return_type: Option<SemType>,
    in_service: bool,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            current_return_type: None,
            in_service: false,
        }
    }

    pub fn check_program(&mut self, program: &Program) -> Result<(), CompileError> {
        for decl in &program.declarations {
            match &decl.node {
                Declaration::Service(svc) => self.check_service(svc, decl.span)?,
                Declaration::Function(func) => self.check_function(func, decl.span)?,
                Declaration::Struct(_) => {}
            }
        }
        Ok(())
    }

    fn check_service(&mut self, svc: &ServiceDecl, _span: Span) -> Result<(), CompileError> {
        self.in_service = true;
        self.symbols.enter_scope();

        // Check fields
        for field in &svc.fields {
            let ty = match (&field.node.ty, &field.node.init) {
                (Some(ast_ty), Some(init)) => {
                    let declared = SemType::from_ast(ast_ty);
                    let init_ty = self.check_expr(init)?;
                    if !init_ty.is_assignable_to(&declared) {
                        return Err(CompileError::new(
                            format!(
                                "Field '{}' has type '{}' but was initialized with '{}'",
                                field.node.name, declared, init_ty
                            ),
                            init.span,
                        ));
                    }
                    declared
                }
                (Some(ast_ty), None) => SemType::from_ast(ast_ty),
                (None, Some(init)) => self.check_expr(init)?,
                (None, None) => {
                    return Err(CompileError::new(
                        format!("Field '{}' must have a type annotation or initializer", field.node.name),
                        field.span,
                    ));
                }
            };

            self.symbols.define(&field.node.name, ty, field.node.is_mut, true, field.span)?;
        }

        // Check methods
        for method in &svc.methods {
            self.check_function(&method.node, method.span)?;
        }

        self.symbols.exit_scope();
        self.in_service = false;
        Ok(())
    }

    fn check_function(&mut self, func: &FunctionDecl, _span: Span) -> Result<(), CompileError> {
        self.symbols.enter_function_scope();

        let ret_ty = func.return_type.as_ref().map(SemType::from_ast).unwrap_or(SemType::Void);
        let old_ret = self.current_return_type.replace(ret_ty.clone());

        for param in &func.params {
            let p_ty = SemType::from_ast(&param.ty);
            self.symbols.define(&param.name, p_ty, param.is_mut, false, Span::default())?;
        }

        self.check_block(&func.body)?;

        self.current_return_type = old_ret;
        self.symbols.exit_function_scope();
        Ok(())
    }

    fn check_block(&mut self, block: &Block) -> Result<(), CompileError> {
        self.symbols.enter_scope();
        for stmt in &block.statements {
            self.check_statement(stmt)?;
        }
        self.symbols.exit_scope();
        Ok(())
    }

    fn check_statement(&mut self, stmt: &Spanned<Statement>) -> Result<(), CompileError> {
        match &stmt.node {
            Statement::Let { name, is_mut, ty, init } => {
                let init_ty = self.check_expr(init)?;
                let final_ty = if let Some(ast_ty) = ty {
                    let declared = SemType::from_ast(ast_ty);
                    if !init_ty.is_assignable_to(&declared) {
                        return Err(CompileError::new(
                            format!(
                                "Variable '{}' declared with type '{}' but assigned '{}'",
                                name, declared, init_ty
                            ),
                            init.span,
                        ));
                    }
                    declared
                } else {
                    init_ty
                };

                self.symbols.define(name, final_ty, *is_mut, false, stmt.span)?;
            }
            Statement::Assign { target, value } => {
                let val_ty = self.check_expr(value)?;

                match &target.node {
                    Expr::Ident(name) => {
                        self.symbols.check_mutation_allowed(name, target.span)?;
                        let sym = self.symbols.resolve(name).unwrap();
                        if !val_ty.is_assignable_to(&sym.ty) {
                            return Err(CompileError::new(
                                format!(
                                    "Cannot assign type '{}' to variable '{}' of type '{}'",
                                    val_ty, name, sym.ty
                                ),
                                value.span,
                            ));
                        }
                    }
                    Expr::MemberAccess { target: obj, field: _, is_optional: _ } => {
                        let _obj_ty = self.check_expr(obj)?;
                    }
                    _ => {
                        return Err(CompileError::new("Invalid assignment target", target.span));
                    }
                }
            }
            Statement::Return(opt_expr) => {
                let actual_ty = if let Some(expr) = opt_expr {
                    self.check_expr(expr)?
                } else {
                    SemType::Void
                };

                if let Some(expected) = &self.current_return_type {
                    if !actual_ty.is_assignable_to(expected) {
                        return Err(CompileError::new(
                            format!(
                                "Function expects return type '{}', but returned '{}'",
                                expected, actual_ty
                            ),
                            stmt.span,
                        ));
                    }
                }
            }
            Statement::Defer(expr) => {
                self.check_expr(expr)?;
            }
            Statement::If { condition, then_branch, else_branch } => {
                let cond_ty = self.check_expr(condition)?;
                if cond_ty != SemType::Bool && cond_ty != SemType::Any {
                    return Err(CompileError::new(
                        format!("If condition must be a bool, found '{}'", cond_ty),
                        condition.span,
                    ));
                }
                self.check_block(then_branch)?;
                if let Some(else_b) = else_branch {
                    self.check_block(else_b)?;
                }
            }
            Statement::While { condition, body } => {
                let cond_ty = self.check_expr(condition)?;
                if cond_ty != SemType::Bool && cond_ty != SemType::Any {
                    return Err(CompileError::new(
                        format!("While condition must be a bool, found '{}'", cond_ty),
                        condition.span,
                    ));
                }
                self.check_block(body)?;
            }
            Statement::TryCatch { try_block, error_name, error_type, catch_block } => {
                self.check_block(try_block)?;
                self.symbols.enter_scope();
                let err_ty = error_type.as_ref().map(SemType::from_ast).unwrap_or(SemType::Named("Exception".to_string()));
                self.symbols.define(error_name, err_ty, false, false, Span::default())?;
                self.check_block(catch_block)?;
                self.symbols.exit_scope();
            }
            Statement::Expr(expr) => {
                self.check_expr(expr)?;
            }
            Statement::Block(b) => {
                self.check_block(b)?;
            }
        }
        Ok(())
    }

    pub fn check_expr(&mut self, expr: &Spanned<Expr>) -> Result<SemType, CompileError> {
        match &expr.node {
            Expr::Literal(lit) => Ok(match lit {
                Literal::Int(_) => SemType::I64,
                Literal::Float(_) => SemType::F64,
                Literal::String(_) => SemType::String,
                Literal::Bool(_) => SemType::Bool,
                Literal::Null => SemType::Null,
            }),
            Expr::Ident(name) => {
                if let Some(sym) = self.symbols.resolve(name) {
                    Ok(sym.ty.clone())
                } else {
                    // Check builtins or global services
                    match name.as_str() {
                        "database" | "Response" | "Map" => Ok(SemType::Named(name.clone())),
                        _ => Err(CompileError::new(
                            format!("Undefined variable '{}'", name),
                            expr.span,
                        )),
                    }
                }
            }
            Expr::Binary { op, left, right } => {
                let l_ty = self.check_expr(left)?;
                let r_ty = self.check_expr(right)?;

                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if l_ty == SemType::I64 && r_ty == SemType::I64 {
                            Ok(SemType::I64)
                        } else if l_ty == SemType::F64 && r_ty == SemType::F64 {
                            Ok(SemType::F64)
                        } else if *op == BinOp::Add && l_ty == SemType::String && r_ty == SemType::String {
                            Ok(SemType::String)
                        } else if l_ty == SemType::Any || r_ty == SemType::Any {
                            Ok(SemType::Any)
                        } else {
                            Err(CompileError::new(
                                format!("Cannot apply arithmetic operator to '{}' and '{}'", l_ty, r_ty),
                                expr.span,
                            ))
                        }
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        if l_ty == r_ty || l_ty.is_assignable_to(&r_ty) || r_ty.is_assignable_to(&l_ty) {
                            Ok(SemType::Bool)
                        } else {
                            Err(CompileError::new(
                                format!("Cannot compare incompatible types '{}' and '{}'", l_ty, r_ty),
                                expr.span,
                            ))
                        }
                    }
                    BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                        if (l_ty == SemType::I64 && r_ty == SemType::I64)
                            || (l_ty == SemType::F64 && r_ty == SemType::F64)
                            || l_ty == SemType::Any
                            || r_ty == SemType::Any
                        {
                            Ok(SemType::Bool)
                        } else {
                            Err(CompileError::new(
                                format!("Relational operator requires numeric types, found '{}' and '{}'", l_ty, r_ty),
                                expr.span,
                            ))
                        }
                    }
                    BinOp::And | BinOp::Or => {
                        if (l_ty == SemType::Bool || l_ty == SemType::Any)
                            && (r_ty == SemType::Bool || r_ty == SemType::Any)
                        {
                            Ok(SemType::Bool)
                        } else {
                            Err(CompileError::new(
                                format!("Logical operator requires bool operands, found '{}' and '{}'", l_ty, r_ty),
                                expr.span,
                            ))
                        }
                    }
                }
            }
            Expr::Unary { op, expr: inner } => {
                let inner_ty = self.check_expr(inner)?;
                match op {
                    UnaryOp::Neg => {
                        if inner_ty == SemType::I64 || inner_ty == SemType::F64 || inner_ty == SemType::Any {
                            Ok(inner_ty)
                        } else {
                            Err(CompileError::new(
                                format!("Unary '-' requires numeric operand, found '{}'", inner_ty),
                                expr.span,
                            ))
                        }
                    }
                    UnaryOp::Not => {
                        if inner_ty == SemType::Bool || inner_ty == SemType::Any {
                            Ok(SemType::Bool)
                        } else {
                            Err(CompileError::new(
                                format!("Unary '!' requires bool operand, found '{}'", inner_ty),
                                expr.span,
                            ))
                        }
                    }
                }
            }
            Expr::Call { callee, args } => {
                for arg in args {
                    self.check_expr(arg)?;
                }
                match &callee.node {
                    Expr::Ident(name) => match name.as_str() {
                        "log_request_completed" | "print" | "println" => Ok(SemType::Void),
                        _ => Ok(SemType::Any),
                    },
                    _ => {
                        self.check_expr(callee)?;
                        Ok(SemType::Any)
                    }
                }
            }
            Expr::MethodCall { receiver, method: _, args } => {
                let _recv_ty = self.check_expr(receiver)?;
                for arg in args {
                    self.check_expr(arg)?;
                }
                Ok(SemType::Any)
            }
            Expr::MemberAccess { target, field: _, is_optional } => {
                let target_ty = self.check_expr(target)?;
                if *is_optional {
                    Ok(SemType::Nullable(Box::new(SemType::Any)))
                } else {
                    match target_ty {
                        SemType::Nullable(inner) => Ok(*inner),
                        other => Ok(other),
                    }
                }
            }
            Expr::Persist(inner) => {
                let inner_ty = self.check_expr(inner)?;
                // PRD Section 8.2: 'persist(x)' relocates/deep-copies object to Region B
                Ok(inner_ty)
            }
            Expr::Path(segments) => {
                Ok(SemType::Named(segments.join("::")))
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
