pub mod actor;
pub mod frame;
pub mod memory;
pub mod vm;

pub use actor::ServiceActor;
pub use memory::{MemoryError, MemoryRegion, RegionAAllocator, RegionBAllocator};
pub use vm::{VirtualMachine, VmError};

#[cfg(test)]
mod tests {
    use super::*;
    use vlang_compiler::compile;
    use vlang_compiler::bytecode::{Opcode, Value};
    use vlang_syntax::parse;

    #[test]
    fn test_vm_execute_arithmetic() {
        let source = r#"
fn main() -> i64 {
    let a = 10;
    let b = 25;
    return a + b * 2;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk).expect("VM Execution failed");
        assert_eq!(result, Value::Int(60));
    }

    #[test]
    fn test_vm_execute_if_else() {
        let source = r#"
fn main() -> i64 {
    let mut x = 5;
    if x > 3 {
        x = 100;
    } else {
        x = 200;
    }
    return x;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk).expect("VM Execution failed");
        assert_eq!(result, Value::Int(100));
    }

    #[test]
    fn test_vm_while_loop() {
        let source = r#"
fn main() -> i64 {
    let mut sum = 0;
    let mut i = 1;
    while i <= 10 {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk).expect("VM Execution failed");
        assert_eq!(result, Value::Int(55));
    }

    #[test]
    fn test_vm_memory_budget_overflow_caught() {
        let source = r#"
fn main() {
    let a = 1;
    let b = 2;
    let c = 3;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        // Tiny 8-byte Region A budget to test overflow protection
        let mut vm = VirtualMachine::with_budgets(8, 1024);
        let result = vm.execute(&chunk);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::MemoryExceeded(MemoryError::RequestMemoryExceeded { .. })));
    }

    #[test]
    fn test_vm_persist_promotes_to_region_b() {
        let source = r#"
fn main() {
    let user = "Alice";
    let persistent_user = persist(user);
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new();
        vm.execute(&chunk).expect("VM execution failed");
        assert!(vm.region_b.total_allocated_bytes.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_vm_map_operations() {
        let source = r#"
fn main() -> i64 {
    let cache = Map::new();
    cache.insert("user_1", 42);
    let val = cache.get("user_1");
    return val;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new();
        let res = vm.execute(&chunk).expect("Execution failed");
        assert_eq!(res, Value::Int(42));
    }

    #[test]
    fn test_service_actor_repeated_requests_reclaims_region_a() {
        let source = r#"
fn handle(req: i64) -> i64 {
    let temp = "A temporary request token that consumes Region A memory";
    return req * 2;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut actor = ServiceActor::new("TestWorker", 1024 * 1024, 16 * 1024 * 1024, chunk);

        // Process 50 consecutive requests through actor
        for i in 1..=50 {
            let res = actor.handle_request(Value::Int(i)).expect("Handler failed");
            assert_eq!(res, Value::Int(i * 2));
        }
        // Since Region A resets per request, the actor does not leak or accumulate memory
    }

    #[test]
    fn test_vm_arithmetic_overflow_min_div_neg_one() {
        use vlang_compiler::bytecode::Opcode;
        let mut chunk = vlang_compiler::bytecode::Chunk::new();
        // i64::MIN / -1 would cause CPU hardware exception without checked arithmetic
        chunk.code.push(Opcode::LoadInt(i64::MIN));
        chunk.code.push(Opcode::LoadInt(-1));
        chunk.code.push(Opcode::Div);
        chunk.code.push(Opcode::Return);

        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            VmError::ArithmeticOverflow("Integer division overflow".into())
        );
    }

    #[test]
    fn test_vm_arithmetic_overflow_addition() {
        use vlang_compiler::bytecode::Opcode;
        let mut chunk = vlang_compiler::bytecode::Chunk::new();
        chunk.code.push(Opcode::LoadInt(i64::MAX));
        chunk.code.push(Opcode::LoadInt(1));
        chunk.code.push(Opcode::Add);
        chunk.code.push(Opcode::Return);

        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            VmError::ArithmeticOverflow("Integer addition overflow".into())
        );
    }

    #[test]
    fn test_vm_instruction_step_limit_preemption() {
        let source = r#"
fn main() -> i64 {
    let mut i = 0;
    while true {
        i = i + 1;
    }
    return i;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        let mut vm = VirtualMachine::new().with_instruction_limit(500);
        let result = vm.execute(&chunk);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::TimeLimitExceeded(_)));
    }

    #[test]
    fn test_vm_persist_detects_and_rejects_cycles() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        use vlang_compiler::bytecode::Opcode;

        // Construct cyclic data structure: map_a contains map_b, map_b contains map_a
        let map_a = Arc::new(Mutex::new(HashMap::new()));
        let map_b = Arc::new(Mutex::new(HashMap::new()));

        map_a.lock().unwrap().insert("b".to_string(), Value::Map(Arc::clone(&map_b)));
        map_b.lock().unwrap().insert("a".to_string(), Value::Map(Arc::clone(&map_a)));

        let mut chunk = vlang_compiler::bytecode::Chunk::new();
        chunk.constants.push(Value::Map(map_a));
        chunk.code.push(Opcode::LoadConst(0));
        chunk.code.push(Opcode::Persist);
        chunk.code.push(Opcode::Return);

        let mut vm = VirtualMachine::new();
        let result = vm.execute(&chunk);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::CyclicPersistentData(_)));
    }

    #[test]
    fn test_service_actor_mailbox_backpressure_rejection() {
        let mut chunk = vlang_compiler::bytecode::Chunk::new();
        chunk.code.push(Opcode::LoadInt(42));
        chunk.code.push(Opcode::Return);

        // Actor with 0 capacity reject queue
        let mut actor = ServiceActor::new("SheddingWorker", 1024, 1024, chunk)
            .with_mailbox_capacity(0);

        let res = actor.handle_request(Value::Null);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(matches!(err, actor::ActorError::MailboxFull { .. }));
    }
}
