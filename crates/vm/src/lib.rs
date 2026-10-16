pub mod actor;
pub mod frame;
pub mod memory;
pub mod profiler;
pub mod test_runner;
pub mod vm;

pub use actor::ServiceActor;
pub use memory::{MemoryError, MemoryRegion, RegionAAllocator, RegionBAllocator};
pub use profiler::{MemoryEvent, MemoryEventKind, MemoryProfiler, ProfileNode};
pub use test_runner::{TestOutcome, TestResult, TestRunner};
pub use vm::{StepStatus, VirtualMachine, VmError};

#[cfg(test)]
mod tests {
    use super::*;
    use trulang_compiler::bytecode::{Opcode, Value};
    use trulang_compiler::compile;
    use trulang_syntax::parse;

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
        assert!(matches!(
            err,
            VmError::MemoryExceeded(MemoryError::RequestMemoryExceeded { .. })
        ));
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
        assert!(
            vm.region_b
                .total_allocated_bytes
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        );
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
        use trulang_compiler::bytecode::Opcode;
        let mut chunk = trulang_compiler::bytecode::Chunk::new();
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
        use trulang_compiler::bytecode::Opcode;
        let mut chunk = trulang_compiler::bytecode::Chunk::new();
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
        use trulang_compiler::bytecode::Opcode;

        // Construct cyclic data structure: map_a contains map_b, map_b contains map_a
        let map_a = Arc::new(Mutex::new(HashMap::new()));
        let map_b = Arc::new(Mutex::new(HashMap::new()));

        map_a
            .lock()
            .unwrap()
            .insert("b".to_string(), Value::Map(Arc::clone(&map_b)));
        map_b
            .lock()
            .unwrap()
            .insert("a".to_string(), Value::Map(Arc::clone(&map_a)));

        let mut chunk = trulang_compiler::bytecode::Chunk::new();
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
        let mut chunk = trulang_compiler::bytecode::Chunk::new();
        chunk.code.push(Opcode::LoadInt(42));
        chunk.code.push(Opcode::Return);

        // Actor with 0 capacity reject queue
        let mut actor =
            ServiceActor::new("SheddingWorker", 1024, 1024, chunk).with_mailbox_capacity(0);

        let res = actor.handle_request(Value::Null);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(matches!(err, actor::ActorError::MailboxFull { .. }));
    }

    #[test]
    fn test_vm_elastic_budget_burst_and_telemetry() {
        use std::sync::Arc;
        use trulang_supervisor::{SharedTelemetryRegion, TelemetryEventType};

        let temp_dir = std::env::temp_dir();
        let shm_path = temp_dir.join(format!("trulang_vm_burst_test_{}.dat", std::process::id()));
        let _ = std::fs::remove_file(&shm_path);

        let shm = Arc::new(
            SharedTelemetryRegion::create_or_open(&shm_path, std::process::id())
                .expect("SHM creation failed"),
        );

        // Base 16 bytes, ceiling 64 bytes
        let mut vm = VirtualMachine::with_elastic_budgets(16, Some(64), 1024);
        vm.attach_telemetry(shm.clone());

        // Allocate 3 integers (3 * 8 = 24 bytes) -> exceeds base 16, within ceiling 64
        vm.allocate_region_a(8).expect("Allocation 1 failed");
        vm.allocate_region_a(8).expect("Allocation 2 failed");
        assert_eq!(vm.region_a.burst_count, 0);

        // Third allocation bursts into headroom
        vm.allocate_region_a(8).expect("Burst allocation failed");
        assert_eq!(vm.region_a.burst_count, 1);
        assert!(vm.region_a.is_bursting());

        // Check telemetry record for ArenaBurst
        let records = shm.read_recent_records(10);
        let has_burst = records
            .iter()
            .any(|r| r.event_type == TelemetryEventType::ArenaBurst as u8);
        assert!(has_burst, "Expected ArenaBurst telemetry event in SHM");

        // Clean up
        let _ = std::fs::remove_file(&shm_path);
    }

    #[test]
    fn test_vm_elastic_budget_ceiling_exceeded() {
        // Base 16 bytes, ceiling 32 bytes
        let mut vm = VirtualMachine::with_elastic_budgets(16, Some(32), 1024);
        vm.allocate_region_a(16).expect("Base allocation failed");
        vm.allocate_region_a(16)
            .expect("Ceiling-limit allocation failed");

        // Exceeding ceiling (32 + 8 = 40 bytes) must fail
        let res = vm.allocate_region_a(8);
        assert!(res.is_err());
        match res.unwrap_err() {
            VmError::MemoryExceeded(MemoryError::RequestMemoryExceeded { budget, attempted }) => {
                assert_eq!(budget, 32);
                assert_eq!(attempted, 40);
            }
            other => panic!("Expected RequestMemoryExceeded, got {:?}", other),
        }
    }

    #[test]
    fn test_end_to_end_service_auto_promotion_and_elastic_budget() {
        let source = r#"
service AutoPromoteService {
    memory_budget request = 16 B max 128 B;
    memory_budget persistent = 1024 B;

    let mut cache = Map::new();

    fn handle(key: String, val: String) -> String {
        // Transparent auto-promotion: val promoted to Region B on insert
        cache.insert(key, val);
        return cache.get(key);
    }
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        // Execute service definition and methods
        let _ = vm.execute(&chunk).expect("Service execution failed");

        // Check that Region B received the persistent cache allocation
        assert!(
            vm.region_b
                .live_objects
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
        );
    }

    #[test]
    fn test_end_to_end_ffi_import_and_handle_execution() {
        let source = r#"
import "rust:trulang_std_pg" as db;
import "std::json" as json;

fn main() -> string {
    let pool = db.create_pool("postgres://localhost/testdb");
    let user = pool.query_user(42);
    let serialized = json.stringify(user);
    return serialized;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        let res = vm.execute(&chunk).expect("Execution failed");
        match res {
            Value::String(s) => {
                assert!(s.contains("\"id\":42"));
                assert!(s.contains("\"name\":\"Alice Smith\""));
                assert!(s.contains("\"email\":\"alice@example.com\""));
            }
            other => panic!(
                "Expected String result from json.stringify, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_vm_executes_imported_package_function() {
        let temp_dir = std::env::temp_dir().join(format!(
            "trulang_vm_pkg_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let pkg_dir = temp_dir.join("packages").join("github.com/trulang/math");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("lib.tr"),
            "fn multiply(x: i64, y: i64) -> i64 { return x * y; }",
        )
        .unwrap();

        let source = r#"
import "github.com/trulang/math" as math;

fn main() -> i64 {
    let result = math.multiply(6, 7);
    return result;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk =
            trulang_compiler::compile_with_project_dir(&prog, &temp_dir).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        let res = vm.execute(&chunk).expect("Execution failed");
        assert_eq!(res, Value::Int(42));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_test_runner_pass_and_assertion_failure() {
        let source = r#"
test "should pass basic arithmetic" {
    let a = 10;
    let b = 20;
    assert_eq(a + b, 30);
    assert(a < b);
}

test "should fail on inequality" {
    let x = 50;
    assert_eq(x, 99);
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");
        assert_eq!(chunk.tests.len(), 2);

        let results = TestRunner::run_all(&chunk.tests, None);
        assert_eq!(results.len(), 2);

        assert_eq!(results[0].name, "should pass basic arithmetic");
        assert!(results[0].is_passed());

        assert_eq!(results[1].name, "should fail on inequality");
        assert!(results[1].is_failed());
        match &results[1].outcome {
            TestOutcome::Failed { error, line, .. } => {
                assert!(error.contains("assert_eq"));
                assert!(error.contains("50"));
                assert!(error.contains("99"));
                assert_eq!(*line, 11);
            }
            _ => panic!("Expected failed test outcome"),
        }
    }

    #[test]
    fn test_end_to_end_postgres_wire_driver() {
        let _server = trulang_db::MockPostgresServer::start();
        let url = _server.url();

        let source = format!(
            r#"
import "trulang/postgres" as pg;
import "std::json" as json;

fn main() -> string {{
    let pool = pg.create_pool("{}");
    let rows = pool.query("SELECT id, name FROM users;");
    let serialized = json.stringify(rows);
    return serialized;
}}
"#,
            url
        );

        let prog = parse(&source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        let res = vm.execute(&chunk).expect("Execution failed");

        match res {
            Value::String(s) => {
                assert!(
                    s.contains("\"id\":\"1\"") || s.contains("\"id\":1") || s.contains("Alice")
                );
                assert!(s.contains("Bob"));
            }
            other => panic!(
                "Expected String result from json.stringify, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_end_to_end_redis_wire_driver() {
        let _server = trulang_db::MockRedisServer::start();
        let url = _server.url();

        let source = format!(
            r#"
import "trulang/redis" as redis;

fn main() -> string {{
    let client = redis.create_pool("{}");
    client.set("session:token", "xyz-987-token");
    let token = client.get("session:token");
    return token;
}}
"#,
            url
        );

        let prog = parse(&source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        let res = vm.execute(&chunk).expect("Execution failed");

        match res {
            Value::String(s) => {
                assert_eq!(&*s, "xyz-987-token");
            }
            other => panic!("Expected String result from redis.get, got {:?}", other),
        }
    }

    #[test]
    fn test_vm_profiler_tracks_allocations_and_persist() {
        let source = r#"
fn compute_data() -> i64 {
    let a = 100;
    let b = 200;
    return a + b;
}

fn main() -> i64 {
    let mut cache = { status: "cached", val: 42 };
    persist(cache);
    let result = compute_data();
    return result;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        vm.attach_profiler(MemoryProfiler::new());

        let res = vm.execute(&chunk).expect("Execution failed");
        assert_eq!(res, Value::Int(300));

        let profiler = vm.take_profiler().expect("Profiler was attached");
        assert!(
            profiler.total_region_a_bytes > 0,
            "Region A bytes must be tracked"
        );
        assert!(
            profiler.total_region_b_promoted > 0,
            "Region B persist bytes must be tracked"
        );
        assert!(
            !profiler.events.is_empty(),
            "Memory events must be recorded"
        );

        // Verify folded text format contains persist promotion tag
        let folded = profiler.render_folded_text();
        assert!(
            folded.contains("[persist->RegionB]"),
            "Folded text must identify persist promotion paths"
        );

        // Verify JSON output
        let json_str = profiler.render_json();
        assert!(
            json_str.contains("total_region_a_bytes"),
            "JSON must contain total_region_a_bytes"
        );
        assert!(
            json_str.contains("total_region_b_promoted"),
            "JSON must contain total_region_b_promoted"
        );

        // Verify SVG flamegraph output
        let svg_str = profiler.render_svg();
        assert!(svg_str.contains("<svg"), "Must render valid SVG");
        assert!(
            svg_str.contains("TruLang 1.0 Visual Memory Flamegraph"),
            "Must contain title"
        );
        assert!(
            svg_str.contains("REGION B (PERSIST)"),
            "Must display Region B badge"
        );
    }

    #[test]
    fn test_vm_step_instruction_and_debugger_inspection() {
        let source = r#"
fn add_nums(x: i64, y: i64) -> i64 {
    return x + y;
}

fn main() -> i64 {
    let a = 10;
    let b = 25;
    let res = add_nums(a, b);
    return res;
}
"#;
        let prog = parse(source).expect("Parse failed");
        let chunk = compile(&prog).expect("Compile failed");

        let mut vm = VirtualMachine::new();
        vm.init_execution(&chunk);

        assert!(!vm.is_terminated());
        assert_eq!(vm.current_function(&chunk), Some("main"));

        let mut step_count = 0;
        let mut final_value = None;

        while !vm.is_terminated() {
            let line = vm.current_line(&chunk);
            assert!(
                line.is_some(),
                "Current line should be available during stepping"
            );
            step_count += 1;

            match vm.step_instruction(&chunk).expect("Step failed") {
                StepStatus::Continued => {}
                StepStatus::Terminated(val) => {
                    final_value = Some(val);
                    break;
                }
            }
        }

        assert!(step_count > 5, "Should have executed multiple instructions");
        assert_eq!(final_value, Some(Value::Int(35)));
        assert!(vm.is_terminated());
    }
}
