pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod symbols;
pub mod type_checker;
pub mod types;

use bytecode::Chunk;
use codegen::CodeGenerator;
use error::CompileError;
use type_checker::TypeChecker;
use vlang_syntax::ast::Program;

pub fn compile(program: &Program) -> Result<Chunk, CompileError> {
    let mut checker = TypeChecker::new();
    checker.check_program(program)?;

    let mut codegen = CodeGenerator::new();
    codegen.compile_program(program)?;
    Ok(codegen.chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vlang_syntax::parse;

    #[test]
    fn test_reject_mutation_without_let_mut() {
        let source = r#"
fn test() {
    let x = 10;
    x = 20;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let result = compile(&prog);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Cannot reassign immutable variable 'x'"));
    }

    #[test]
    fn test_allow_mutation_with_let_mut() {
        let source = r#"
fn test() {
    let mut x = 10;
    x = 20;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");
        assert!(!chunk.code.is_empty());
    }

    #[test]
    fn test_compile_try_catch_and_defer() {
        let source = r#"
fn handle() {
    defer println("cleanup");
    try {
        let x = 42;
    } catch (err: Exception) {
        println(err);
    }
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");
        assert!(!chunk.code.is_empty());
    }
}
