pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod token;

use ast::Program;
use error::SyntaxError;
use lexer::Lexer;
use parser::Parser;

pub fn parse(source: &str) -> Result<Program, SyntaxError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_lex_basic_tokens() {
        let source = "let mut x: i64 = 42; defer close();";
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().expect("Tokenize failed");
        assert!(tokens.len() >= 9);
    }

    #[test]
    fn test_parse_prd_example_service() {
        let source = r#"
service UserService {
    memory_budget request = 16 MB;
    memory_budget persistent = 512 MB;

    let mut cache: Map<String, User> = Map::new();

    fn handle(request: Request) -> Response {
        defer log_request_completed(request.id);

        try {
            let user = database.find_user(request.user_id);
            if user != null {
                cache.insert(request.user_id, persist(user));
            }
            return Response.json(user);
        } catch (err: DatabaseError) {
            return Response.error(500, err.message);
        }
    }
}
"#;

        let program = parse(source).expect("Failed to parse PRD example service");
        assert_eq!(program.declarations.len(), 1);

        match &program.declarations[0].node {
            Declaration::Service(svc) => {
                assert_eq!(svc.name, "UserService");
                assert_eq!(svc.budgets.len(), 2);
                assert_eq!(svc.budgets[0].node.region, BudgetRegion::Request);
                assert_eq!(svc.budgets[0].node.size_bytes, 16 * 1024 * 1024);
                assert_eq!(svc.budgets[1].node.region, BudgetRegion::Persistent);
                assert_eq!(svc.budgets[1].node.size_bytes, 512 * 1024 * 1024);

                assert_eq!(svc.fields.len(), 1);
                assert_eq!(svc.fields[0].node.name, "cache");
                assert!(svc.fields[0].node.is_mut);

                assert_eq!(svc.methods.len(), 1);
                assert_eq!(svc.methods[0].node.name, "handle");
            }
            _ => panic!("Expected Service declaration"),
        }
    }

    #[test]
    fn test_parse_syntax_error_diagnostic() {
        let source = "service BrokenService {\n    let x =\n}";
        let result = parse(source);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let formatted = err.format_with_source(source, "test.tr");
        assert!(formatted.contains("test.tr:3:1"));
        assert!(formatted.contains("syntax error"));
    }

    #[test]
    fn test_parser_recursion_depth_limit() {
        // Construct deeply nested expression "((((... 80 times ... 42 ...))))"
        let mut source = String::from("service DeepService { fn handle() -> i64 { return ");
        for _ in 0..80 {
            source.push('(');
        }
        source.push_str("42");
        for _ in 0..80 {
            source.push(')');
        }
        source.push_str("; } }");

        let result = parse(&source);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("recursion depth limit (64) exceeded"));
    }

    #[test]
    fn test_parse_elastic_memory_budget() {
        let source = r#"
service ElasticService {
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 512 MB;
}
"#;
        let program = parse(source).expect("Failed to parse elastic service");
        match &program.declarations[0].node {
            Declaration::Service(svc) => {
                assert_eq!(svc.budgets.len(), 2);
                assert_eq!(svc.budgets[0].node.region, BudgetRegion::Request);
                assert_eq!(svc.budgets[0].node.size_bytes, 16 * 1024 * 1024);
                assert_eq!(svc.budgets[0].node.max_size_bytes, Some(64 * 1024 * 1024));
                assert_eq!(svc.budgets[1].node.region, BudgetRegion::Persistent);
                assert_eq!(svc.budgets[1].node.size_bytes, 512 * 1024 * 1024);
                assert_eq!(svc.budgets[1].node.max_size_bytes, None);
            }
            _ => panic!("Expected Service declaration"),
        }
    }

    #[test]
    fn test_parse_elastic_budget_invalid_ceiling() {
        let source = r#"
service InvalidCeilingService {
    memory_budget request = 64 MB max 16 MB;
}
"#;
        let result = parse(source);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Ceiling budget") && err.message.contains("cannot be less than base budget"));
    }

    #[test]
    fn test_parse_import_declaration() {
        let source = r#"
import "rust:trulang_std_pg" as db;
import "std::json" as json;

service AccountService {
    fn handle() -> i64 {
        return 0;
    }
}
"#;
        let program = parse(source).expect("Failed to parse imports");
        assert_eq!(program.declarations.len(), 3);
        match &program.declarations[0].node {
            Declaration::Import(imp) => {
                assert_eq!(imp.path, "rust:trulang_std_pg");
                assert_eq!(imp.alias, "db");
            }
            _ => panic!("Expected Import declaration"),
        }
        match &program.declarations[1].node {
            Declaration::Import(imp) => {
                assert_eq!(imp.path, "std::json");
                assert_eq!(imp.alias, "json");
            }
            _ => panic!("Expected Import declaration"),
        }
    }

    #[test]
    fn test_parse_test_declaration() {
        let source = r#"
test "should assert addition correctly" {
    let mut x = 40;
    x = x + 2;
    assert(x == 42);
    assert_eq(x, 42);
}
"#;
        let program = parse(source).expect("Failed to parse test declaration");
        assert_eq!(program.declarations.len(), 1);
        match &program.declarations[0].node {
            Declaration::Test(t) => {
                assert_eq!(t.name, "should assert addition correctly");
                assert_eq!(t.body.statements.len(), 4);
            }
            _ => panic!("Expected Test declaration"),
        }
    }
}
