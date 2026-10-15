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

    #[test]
    fn test_compile_elastic_memory_budget_opcode() {
        let source = r#"
service ElasticSvc {
    memory_budget request = 16 MB max 64 MB;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");
        assert!(chunk.code.contains(&bytecode::Opcode::AllocRegionA(16 * 1024 * 1024, Some(64 * 1024 * 1024))));
    }

    #[test]
    fn test_compile_auto_promotion_on_persistent_assign_and_insert() {
        let source = r#"
service CacheService {
    let mut cache = Map::new();
    let mut last_key = "";

    fn handle(key: String, val: String) {
        last_key = key;
        cache.insert(key, val);
    }
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");
        
        // Count Opcode::Persist in compiled bytecode
        let persist_count = chunk.code.iter().filter(|op| matches!(op, bytecode::Opcode::Persist)).count();
        // One for `last_key = key`, and one for `cache.insert(key, val)`
        assert_eq!(persist_count, 2, "Expected auto-promotion Persist opcodes for persistent field assign and insert");
    }

    #[test]
    fn test_compile_no_redundant_persist_when_explicit() {
        let source = r#"
service CacheServiceExplicit {
    let mut cache = Map::new();

    fn handle(key: String, val: String) {
        cache.insert(key, persist(val));
    }
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");
        
        let persist_count = chunk.code.iter().filter(|op| matches!(op, bytecode::Opcode::Persist)).count();
        // Exactly one Persist opcode (no redundant duplicate)
        assert_eq!(persist_count, 1, "Expected exactly one Persist opcode when explicit persist() is used");
    }
}
