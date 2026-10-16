pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod symbols;
pub mod type_checker;
pub mod types;

use bytecode::Chunk;
use codegen::CodeGenerator;
use error::CompileError;
use trulang_syntax::ast::Program;
use type_checker::TypeChecker;

pub fn compile(program: &Program) -> Result<Chunk, CompileError> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    compile_with_project_dir(program, current_dir)
}

pub fn compile_with_project_dir(
    program: &Program,
    project_dir: impl AsRef<std::path::Path>,
) -> Result<Chunk, CompileError> {
    let mut checker = TypeChecker::new();
    checker.check_program(program)?;

    let mut codegen = CodeGenerator::with_project_dir(project_dir.as_ref().to_path_buf());
    codegen.compile_program(program)?;
    Ok(codegen.chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trulang_syntax::parse;

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
        assert!(err
            .message
            .contains("Cannot reassign immutable variable 'x'"));
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
        assert!(chunk.code.contains(&bytecode::Opcode::AllocRegionA(
            16 * 1024 * 1024,
            Some(64 * 1024 * 1024)
        )));
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
        let persist_count = chunk
            .code
            .iter()
            .filter(|op| matches!(op, bytecode::Opcode::Persist))
            .count();
        // One for `last_key = key`, and one for `cache.insert(key, val)`
        assert_eq!(
            persist_count, 2,
            "Expected auto-promotion Persist opcodes for persistent field assign and insert"
        );
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

        let persist_count = chunk
            .code
            .iter()
            .filter(|op| matches!(op, bytecode::Opcode::Persist))
            .count();
        // Exactly one Persist opcode (no redundant duplicate)
        assert_eq!(
            persist_count, 1,
            "Expected exactly one Persist opcode when explicit persist() is used"
        );
    }

    #[test]
    fn test_compile_test_declaration_and_assert_opcodes() {
        let source = r#"
test "addition check" {
    let actual = 20 + 22;
    assert(actual > 0, "should be positive");
    assert_eq(actual, 42, "must equal 42");
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compilation failed");

        assert_eq!(chunk.tests.len(), 1);
        let compiled_test = &chunk.tests[0];
        assert_eq!(compiled_test.name, "addition check");
        assert!(compiled_test
            .chunk
            .code
            .iter()
            .any(|op| matches!(op, bytecode::Opcode::Assert(_))));
        assert!(compiled_test
            .chunk
            .code
            .iter()
            .any(|op| matches!(op, bytecode::Opcode::AssertEq(_))));
    }

    #[test]
    fn test_compile_with_project_dir_resolves_local_and_package_import() {
        let temp_dir = std::env::temp_dir().join(format!(
            "trulang_comp_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let pkg_dir = temp_dir.join("packages").join("github.com/trulang/math");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("lib.tr"),
            "fn add(a: i64, b: i64) -> i64 { return a + b; }",
        )
        .unwrap();

        let src = r#"
import "github.com/trulang/math" as math;

fn main() -> i64 {
    let sum = math.add(10, 20);
    return sum;
}
"#;
        let prog = parse(src).expect("Parse failed");
        let chunk = compile_with_project_dir(&prog, &temp_dir)
            .expect("Compilation with project dir failed");
        assert!(chunk.functions.contains_key("math::add"));
        assert!(chunk.functions.contains_key("math.add"));
        assert!(chunk.functions.contains_key("main"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
