use crate::ast::*;
use crate::error::SyntaxError;
use crate::lexer::Token;
use crate::span::{Span, Spanned};
use crate::token::TokenKind;

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    recursion_depth: usize,
}

impl Parser {
    pub const MAX_RECURSION_DEPTH: usize = 64;

    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            cursor: 0,
            recursion_depth: 0,
        }
    }

    fn peek(&self) -> &TokenKind {
        self.tokens.get(self.cursor).map(|t| &t.kind).unwrap_or(&TokenKind::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens.get(self.cursor).map(|t| t.span).unwrap_or_default()
    }

    fn previous_span(&self) -> Span {
        if self.cursor > 0 {
            self.tokens.get(self.cursor - 1).map(|t| t.span).unwrap_or_default()
        } else {
            self.current_span()
        }
    }

    fn advance(&mut self) -> &Token {
        if self.cursor < self.tokens.len() {
            let t = &self.tokens[self.cursor];
            self.cursor += 1;
            t
        } else {
            self.tokens.last().unwrap()
        }
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Span, SyntaxError> {
        let span = self.current_span();
        if self.peek() == &expected {
            self.advance();
            Ok(span)
        } else {
            Err(SyntaxError::new(
                format!("Expected '{}', found '{}'", expected, self.peek()),
                span,
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), SyntaxError> {
        let span = self.current_span();
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok((name, span))
            }
            TokenKind::Request => {
                self.advance();
                Ok(("request".to_string(), span))
            }
            TokenKind::Persistent => {
                self.advance();
                Ok(("persistent".to_string(), span))
            }
            TokenKind::Test => {
                self.advance();
                Ok(("test".to_string(), span))
            }
            found => Err(SyntaxError::new(
                format!("Expected identifier, found '{}'", found),
                span,
            )),
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, SyntaxError> {
        let mut declarations = Vec::new();

        while self.peek() != &TokenKind::Eof {
            let decl = self.parse_declaration()?;
            declarations.push(decl);
        }

        Ok(Program { declarations })
    }

    fn parse_declaration(&mut self) -> Result<Spanned<Declaration>, SyntaxError> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::Service => {
                let service = self.parse_service_decl()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Declaration::Service(service), span))
            }
            TokenKind::Fn => {
                let func = self.parse_function_decl()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Declaration::Function(func), span))
            }
            TokenKind::Import => {
                let import_decl = self.parse_import_decl()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Declaration::Import(import_decl), span))
            }
            TokenKind::Test => {
                let test_decl = self.parse_test_decl()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Declaration::Test(test_decl), span))
            }
            _ => Err(SyntaxError::new(
                format!("Expected declaration (service, fn, import), found '{}'", self.peek()),
                start_span,
            )),
        }
    }

    fn parse_test_decl(&mut self) -> Result<TestDecl, SyntaxError> {
        self.expect(TokenKind::Test)?;
        let name = match self.peek() {
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.advance();
                s
            }
            found => {
                return Err(SyntaxError::new(
                    format!("Expected test name string, found '{}'", found),
                    self.current_span(),
                ));
            }
        };

        let body = self.parse_block()?;
        Ok(TestDecl { name, body })
    }

    fn parse_import_decl(&mut self) -> Result<ImportDecl, SyntaxError> {
        self.expect(TokenKind::Import)?;
        let path = match self.peek().clone() {
            TokenKind::StringLit(s) => {
                self.advance();
                s
            }
            TokenKind::Ident(s) => {
                self.advance();
                s
            }
            found => {
                return Err(SyntaxError::new(
                    format!("Expected import path string or identifier, found '{}'", found),
                    self.current_span(),
                ));
            }
        };

        self.expect(TokenKind::As)?;
        let (alias, _) = self.expect_ident()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(ImportDecl { path, alias })
    }

    fn parse_service_decl(&mut self) -> Result<ServiceDecl, SyntaxError> {
        self.expect(TokenKind::Service)?;
        let (name, _) = self.expect_ident()?;
        self.expect(TokenKind::OpenBrace)?;

        let mut budgets = Vec::new();
        let mut fields = Vec::new();
        let mut methods = Vec::new();

        while self.peek() != &TokenKind::CloseBrace && self.peek() != &TokenKind::Eof {
            match self.peek() {
                TokenKind::MemoryBudget => {
                    let budget = self.parse_memory_budget()?;
                    budgets.push(budget);
                }
                TokenKind::Let => {
                    let field = self.parse_field_decl()?;
                    fields.push(field);
                }
                TokenKind::Fn => {
                    let start_span = self.current_span();
                    let method = self.parse_function_decl()?;
                    let span = start_span.merge(self.previous_span());
                    methods.push(Spanned::new(method, span));
                }
                _ => {
                    return Err(SyntaxError::new(
                        format!("Unexpected token inside service: '{}'", self.peek()),
                        self.current_span(),
                    ));
                }
            }
        }

        self.expect(TokenKind::CloseBrace)?;
        Ok(ServiceDecl {
            name,
            budgets,
            fields,
            methods,
        })
    }

    fn parse_memory_budget(&mut self) -> Result<Spanned<MemoryBudgetDecl>, SyntaxError> {
        let start_span = self.expect(TokenKind::MemoryBudget)?;
        let region = match self.peek() {
            TokenKind::Request => {
                self.advance();
                BudgetRegion::Request
            }
            TokenKind::Persistent => {
                self.advance();
                BudgetRegion::Persistent
            }
            other => {
                return Err(SyntaxError::new(
                    format!("Expected 'request' or 'persistent' memory region, found '{}'", other),
                    self.current_span(),
                ));
            }
        };

        self.expect(TokenKind::Eq)?;

        let num = match self.peek() {
            TokenKind::IntLit(n) => {
                let val = *n;
                self.advance();
                val
            }
            other => {
                return Err(SyntaxError::new(
                    format!("Expected numeric memory budget value, found '{}'", other),
                    self.current_span(),
                ));
            }
        };

        let unit_multiplier = match self.peek() {
            TokenKind::ByteUnit(u) => {
                let m = match u.as_str() {
                    "B" => 1,
                    "KB" => 1024,
                    "MB" => 1024 * 1024,
                    "GB" => 1024 * 1024 * 1024,
                    _ => 1,
                };
                self.advance();
                m
            }
            _ => 1,
        };

        let max_size_bytes = if self.match_token(&TokenKind::Max) {
            let max_num = match self.peek() {
                TokenKind::IntLit(n) => {
                    let val = *n;
                    self.advance();
                    val
                }
                other => {
                    return Err(SyntaxError::new(
                        format!("Expected numeric ceiling value after 'max', found '{}'", other),
                        self.current_span(),
                    ));
                }
            };

            let max_unit_multiplier = match self.peek() {
                TokenKind::ByteUnit(u) => {
                    let m = match u.as_str() {
                        "B" => 1,
                        "KB" => 1024,
                        "MB" => 1024 * 1024,
                        "GB" => 1024 * 1024 * 1024,
                        _ => 1,
                    };
                    self.advance();
                    m
                }
                _ => 1,
            };

            let max_bytes = (max_num.max(0) as usize) * max_unit_multiplier;
            let base_bytes = (num.max(0) as usize) * unit_multiplier;
            if max_bytes < base_bytes {
                return Err(SyntaxError::new(
                    format!(
                        "Ceiling budget ({} bytes) cannot be less than base budget ({} bytes)",
                        max_bytes, base_bytes
                    ),
                    self.current_span(),
                ));
            }
            Some(max_bytes)
        } else {
            None
        };

        self.expect(TokenKind::Semicolon)?;
        let span = start_span.merge(self.previous_span());
        let size_bytes = (num.max(0) as usize) * unit_multiplier;

        Ok(Spanned::new(
            MemoryBudgetDecl {
                region,
                size_bytes,
                max_size_bytes,
            },
            span,
        ))
    }

    fn parse_field_decl(&mut self) -> Result<Spanned<FieldDecl>, SyntaxError> {
        let start_span = self.expect(TokenKind::Let)?;
        let is_mut = self.match_token(&TokenKind::Mut);
        let (name, _) = self.expect_ident()?;

        let ty = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let init = if self.match_token(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(TokenKind::Semicolon)?;
        let span = start_span.merge(self.previous_span());
        Ok(Spanned::new(
            FieldDecl { name, is_mut, ty, init },
            span,
        ))
    }

    fn parse_function_decl(&mut self) -> Result<FunctionDecl, SyntaxError> {
        self.expect(TokenKind::Fn)?;
        let (name, _) = self.expect_ident()?;
        self.expect(TokenKind::OpenParen)?;

        let mut params = Vec::new();
        while self.peek() != &TokenKind::CloseParen && self.peek() != &TokenKind::Eof {
            let is_mut = self.match_token(&TokenKind::Mut);
            let (param_name, _) = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;

            params.push(Param {
                name: param_name,
                is_mut,
                ty,
            });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::CloseParen)?;

        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        Ok(FunctionDecl {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_type(&mut self) -> Result<Type, SyntaxError> {
        let (base_name, _span) = self.expect_ident()?;
        let mut base_ty = match base_name.as_str() {
            "i64" | "I64" | "int" | "Int" => Type::I64,
            "f64" | "F64" | "float" | "Float" => Type::F64,
            "bool" | "Bool" => Type::Bool,
            "string" | "String" => Type::String,
            "void" | "Void" => Type::Void,
            _ => {
                // Check if generic, e.g., Map<String, User>
                if self.match_token(&TokenKind::Lt) {
                    let mut type_args = Vec::new();
                    while self.peek() != &TokenKind::Gt && self.peek() != &TokenKind::Eof {
                        type_args.push(self.parse_type()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::Gt)?;
                    Type::Generic(base_name, type_args)
                } else {
                    Type::Named(base_name)
                }
            }
        };

        // Check for nullable '?' suffix: T?
        if self.match_token(&TokenKind::Question) {
            base_ty = Type::Nullable(Box::new(base_ty));
        }

        Ok(base_ty)
    }

    fn parse_block(&mut self) -> Result<Block, SyntaxError> {
        self.expect(TokenKind::OpenBrace)?;
        let mut statements = Vec::new();

        while self.peek() != &TokenKind::CloseBrace && self.peek() != &TokenKind::Eof {
            statements.push(self.parse_statement()?);
        }

        self.expect(TokenKind::CloseBrace)?;
        Ok(Block { statements })
    }

    fn parse_statement(&mut self) -> Result<Spanned<Statement>, SyntaxError> {
        if self.recursion_depth >= Self::MAX_RECURSION_DEPTH {
            return Err(SyntaxError::new(
                format!(
                    "Parser recursion depth limit ({}) exceeded",
                    Self::MAX_RECURSION_DEPTH
                ),
                self.current_span(),
            ));
        }
        self.recursion_depth += 1;
        let res = self.parse_statement_inner();
        self.recursion_depth -= 1;
        res
    }

    fn parse_statement_inner(&mut self) -> Result<Spanned<Statement>, SyntaxError> {
        let start_span = self.current_span();

        match self.peek() {
            TokenKind::Let => {
                self.advance();
                let is_mut = self.match_token(&TokenKind::Mut);
                let (name, _) = self.expect_ident()?;
                let ty = if self.match_token(&TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(TokenKind::Eq)?;
                let init = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::Let { name, is_mut, ty, init }, span))
            }
            TokenKind::Return => {
                self.advance();
                let expr = if self.peek() != &TokenKind::Semicolon {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(TokenKind::Semicolon)?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::Return(expr), span))
            }
            TokenKind::Defer => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::Defer(expr), span))
            }
            TokenKind::If => {
                self.advance();
                let condition = self.parse_expr()?;
                let then_branch = self.parse_block()?;
                let else_branch = if self.match_token(&TokenKind::Else) {
                    if self.peek() == &TokenKind::If {
                        let nested_if = self.parse_statement()?;
                        Some(Block { statements: vec![nested_if] })
                    } else {
                        Some(self.parse_block()?)
                    }
                } else {
                    None
                };
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::If { condition, then_branch, else_branch }, span))
            }
            TokenKind::While => {
                self.advance();
                let condition = self.parse_expr()?;
                let body = self.parse_block()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::While { condition, body }, span))
            }
            TokenKind::Try => {
                self.advance();
                let try_block = self.parse_block()?;
                self.expect(TokenKind::Catch)?;
                self.expect(TokenKind::OpenParen)?;
                let (error_name, _) = self.expect_ident()?;
                let error_type = if self.match_token(&TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(TokenKind::CloseParen)?;
                let catch_block = self.parse_block()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::TryCatch { try_block, error_name, error_type, catch_block }, span))
            }
            TokenKind::OpenBrace => {
                let block = self.parse_block()?;
                let span = start_span.merge(self.previous_span());
                Ok(Spanned::new(Statement::Block(block), span))
            }
            _ => {
                // Expression or Assignment
                let expr = self.parse_expr()?;
                if self.match_token(&TokenKind::Eq) {
                    let value = self.parse_expr()?;
                    self.expect(TokenKind::Semicolon)?;
                    let span = start_span.merge(self.previous_span());
                    Ok(Spanned::new(Statement::Assign { target: expr, value }, span))
                } else {
                    self.expect(TokenKind::Semicolon)?;
                    let span = start_span.merge(self.previous_span());
                    Ok(Spanned::new(Statement::Expr(expr), span))
                }
            }
        }
    }

    pub fn parse_expr(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        if self.recursion_depth >= Self::MAX_RECURSION_DEPTH {
            return Err(SyntaxError::new(
                format!(
                    "Parser recursion depth limit ({}) exceeded",
                    Self::MAX_RECURSION_DEPTH
                ),
                self.current_span(),
            ));
        }
        self.recursion_depth += 1;
        let res = self.parse_logical_or();
        self.recursion_depth -= 1;
        res
    }

    fn parse_logical_or(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_logical_and()?;
        while self.match_token(&TokenKind::PipePipe) {
            let right = self.parse_logical_and()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op: BinOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_equality()?;
        while self.match_token(&TokenKind::AmpAmp) {
            let right = self.parse_equality()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op: BinOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_relational()?;
        loop {
            let op = if self.match_token(&TokenKind::EqEq) {
                BinOp::Eq
            } else if self.match_token(&TokenKind::BangEq) {
                BinOp::NotEq
            } else {
                break;
            };
            let right = self.parse_relational()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = if self.match_token(&TokenKind::Lt) {
                BinOp::Lt
            } else if self.match_token(&TokenKind::LtEq) {
                BinOp::LtEq
            } else if self.match_token(&TokenKind::Gt) {
                BinOp::Gt
            } else if self.match_token(&TokenKind::GtEq) {
                BinOp::GtEq
            } else {
                break;
            };
            let right = self.parse_additive()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = if self.match_token(&TokenKind::Plus) {
                BinOp::Add
            } else if self.match_token(&TokenKind::Minus) {
                BinOp::Sub
            } else {
                break;
            };
            let right = self.parse_multiplicative()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.match_token(&TokenKind::Star) {
                BinOp::Mul
            } else if self.match_token(&TokenKind::Slash) {
                BinOp::Div
            } else if self.match_token(&TokenKind::Percent) {
                BinOp::Mod
            } else {
                break;
            };
            let right = self.parse_unary()?;
            let span = left.span.merge(right.span);
            left = Spanned::new(
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let start_span = self.current_span();
        if self.match_token(&TokenKind::Bang) {
            let expr = self.parse_unary()?;
            let span = start_span.merge(expr.span);
            return Ok(Spanned::new(Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr) }, span));
        }
        if self.match_token(&TokenKind::Minus) {
            let expr = self.parse_unary()?;
            let span = start_span.merge(expr.span);
            return Ok(Spanned::new(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr) }, span));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_token(&TokenKind::OpenParen) {
                // Function call: expr(...)
                let mut args = Vec::new();
                while self.peek() != &TokenKind::CloseParen && self.peek() != &TokenKind::Eof {
                    args.push(self.parse_expr()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::CloseParen)?;
                let span = expr.span.merge(self.previous_span());
                expr = Spanned::new(Expr::Call { callee: Box::new(expr), args }, span);
            } else if self.match_token(&TokenKind::Dot) {
                // Member access or method call
                let (member, _) = self.expect_ident()?;
                if self.match_token(&TokenKind::OpenParen) {
                    let mut args = Vec::new();
                    while self.peek() != &TokenKind::CloseParen && self.peek() != &TokenKind::Eof {
                        args.push(self.parse_expr()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::CloseParen)?;
                    let span = expr.span.merge(self.previous_span());
                    expr = Spanned::new(Expr::MethodCall { receiver: Box::new(expr), method: member, args }, span);
                } else {
                    let span = expr.span.merge(self.previous_span());
                    expr = Spanned::new(Expr::MemberAccess { target: Box::new(expr), field: member, is_optional: false }, span);
                }
            } else if self.match_token(&TokenKind::QuestionDot) {
                // Optional chaining ?.
                let (member, _) = self.expect_ident()?;
                let span = expr.span.merge(self.previous_span());
                expr = Spanned::new(Expr::MemberAccess { target: Box::new(expr), field: member, is_optional: true }, span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Spanned<Expr>, SyntaxError> {
        let span = self.current_span();

        match self.peek().clone() {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Int(n)), span))
            }
            TokenKind::FloatLit(fl) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Float(fl)), span))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::String(s)), span))
            }
            TokenKind::True => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(true)), span))
            }
            TokenKind::False => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Bool(false)), span))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Spanned::new(Expr::Literal(Literal::Null), span))
            }
            TokenKind::Persist => {
                self.advance();
                self.expect(TokenKind::OpenParen)?;
                let inner = self.parse_expr()?;
                self.expect(TokenKind::CloseParen)?;
                let full_span = span.merge(self.previous_span());
                Ok(Spanned::new(Expr::Persist(Box::new(inner)), full_span))
            }
            TokenKind::Ident(name) => {
                self.advance();
                // Check if path separator '::' follows
                if self.match_token(&TokenKind::ColonColon) {
                    let mut segments = vec![name];
                    let (next_seg, _) = self.expect_ident()?;
                    segments.push(next_seg);
                    while self.match_token(&TokenKind::ColonColon) {
                        let (seg, _) = self.expect_ident()?;
                        segments.push(seg);
                    }
                    let full_span = span.merge(self.previous_span());
                    Ok(Spanned::new(Expr::Path(segments), full_span))
                } else {
                    Ok(Spanned::new(Expr::Ident(name), span))
                }
            }
            TokenKind::Request => {
                self.advance();
                Ok(Spanned::new(Expr::Ident("request".to_string()), span))
            }
            TokenKind::Persistent => {
                self.advance();
                Ok(Spanned::new(Expr::Ident("persistent".to_string()), span))
            }
            TokenKind::OpenParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::CloseParen)?;
                Ok(inner)
            }
            other => Err(SyntaxError::new(
                format!("Unexpected token in expression: '{}'", other),
                span,
            )),
        }
    }
}
