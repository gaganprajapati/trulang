use crate::error::CompileError;
use crate::types::SemType;
use std::collections::HashMap;
use trulang_syntax::span::Span;

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub ty: SemType,
    pub is_mut: bool,
    pub function_depth: usize,
    pub is_field: bool,
}

pub struct SymbolTable {
    scopes: Vec<HashMap<String, Symbol>>,
    current_function_depth: usize,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            current_function_depth: 0,
        }
    }

    pub fn enter_function_scope(&mut self) {
        self.current_function_depth += 1;
        self.scopes.push(HashMap::new());
    }

    pub fn exit_function_scope(&mut self) {
        if self.current_function_depth > 0 {
            self.current_function_depth -= 1;
            self.scopes.pop();
        }
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define(&mut self, name: &str, ty: SemType, is_mut: bool, is_field: bool, span: Span) -> Result<(), CompileError> {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                return Err(CompileError::new(
                    format!("Variable '{}' is already defined in this scope", name),
                    span,
                ));
            }
            scope.insert(
                name.to_string(),
                Symbol {
                    name: name.to_string(),
                    ty,
                    is_mut,
                    function_depth: self.current_function_depth,
                    is_field,
                },
            );
            Ok(())
        } else {
            Err(CompileError::new("No active scope", span))
        }
    }

    pub fn resolve(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }

    pub fn check_mutation_allowed(&self, name: &str, span: Span) -> Result<(), CompileError> {
        let sym = self.resolve(name).ok_or_else(|| {
            CompileError::new(format!("Undefined variable '{}'", name), span)
        })?;

        if !sym.is_mut {
            return Err(CompileError::new(
                format!("Cannot reassign immutable variable '{}'", name),
                span,
            ).with_hint("Declare as 'let mut' to permit reassignment"));
        }

        // Rule 2.2: Nested functions cannot modify variables belonging to enclosing scopes
        if sym.function_depth < self.current_function_depth && !sym.is_field {
            return Err(CompileError::new(
                format!(
                    "Nested function cannot mutate variable '{}' declared in enclosing scope",
                    name
                ),
                span,
            ).with_hint("Functions must return updated values instead of mutating outer variables"));
        }

        Ok(())
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}
