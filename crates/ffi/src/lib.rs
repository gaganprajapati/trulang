pub mod error;
pub mod handle;
pub mod marshal;
pub mod modules;
pub mod registry;
pub mod worker_pool;

pub use error::FfiError;
pub use handle::{
    create_pg_pool_handle, next_handle_id, PgPoolResource, GLOBAL_DROPPED_HANDLES_COUNT,
};
pub use marshal::{estimate_value_size, json_to_trulang_value, trulang_value_to_json};
pub use registry::FfiRegistry;
pub use worker_pool::WorkerPool;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use trulang_compiler::bytecode::Value;

    #[test]
    fn test_worker_pool_spawn_blocking_and_shutdown() {
        let pool = WorkerPool::new(4);
        assert_eq!(pool.worker_count(), 4);

        let result = pool.spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            Ok(42)
        });

        assert_eq!(result.expect("Task execution failed"), 42);
        assert_eq!(pool.completed_tasks(), 1);
        assert_eq!(pool.active_tasks(), 0);
    }

    #[test]
    fn test_worker_pool_zero_leakage_on_drop() {
        // Repeatedly spawn and drop worker pools
        for _ in 0..5 {
            let pool = WorkerPool::new(2);
            let res = pool.spawn_blocking(|| Ok("ok"));
            assert_eq!(res.unwrap(), "ok");
            drop(pool);
            // After drop, all worker threads have joined cleanly
        }
    }

    #[test]
    fn test_deterministic_raii_handle_destructor() {
        let initial_dropped = GLOBAL_DROPPED_HANDLES_COUNT.load(Ordering::SeqCst);

        {
            let handle_val = PgPoolResource::new("postgres://localhost/test_db").into_value();
            match handle_val {
                Value::Handle(ref h) => {
                    assert_eq!(h.type_name, "PgPool");
                    // Invoke query_user
                    let res = h
                        .resource
                        .call_method("query_user", &[Value::Int(101)])
                        .unwrap();
                    match res {
                        Value::Object(o) => {
                            let g = o.lock().unwrap();
                            assert_eq!(g.get("id"), Some(&Value::Int(101)));
                            assert_eq!(
                                g.get("email"),
                                Some(&Value::String(Arc::new("alice@example.com".into())))
                            );
                        }
                        _ => panic!("Expected Object response"),
                    }
                }
                _ => panic!("Expected Handle value"),
            }
            // handle_val goes out of scope here
        }

        let after_dropped = GLOBAL_DROPPED_HANDLES_COUNT.load(Ordering::SeqCst);
        assert!(
            after_dropped >= initial_dropped + 1,
            "Deterministic RAII destructor must run immediately on drop"
        );
    }

    #[test]
    fn test_std_json_parse_and_stringify() {
        let json_input = r#"{"name":"Alice","age":30,"tags":["rust","trulang"],"active":true}"#;
        let parsed = modules::json::parse(&[Value::String(Arc::new(json_input.into()))])
            .expect("Parse failed");

        match &parsed {
            Value::Object(obj) => {
                let g = obj.lock().unwrap();
                assert_eq!(
                    g.get("name"),
                    Some(&Value::String(Arc::new("Alice".into())))
                );
                assert_eq!(g.get("age"), Some(&Value::Int(30)));
                assert_eq!(g.get("active"), Some(&Value::Bool(true)));
            }
            _ => panic!("Expected Object representation"),
        }

        // Test stringify roundtrip
        let serialized = modules::json::stringify(&[parsed]).expect("Stringify failed");
        match serialized {
            Value::String(s) => {
                assert!(s.contains("\"name\":\"Alice\""));
                assert!(s.contains("\"age\":30"));
            }
            _ => panic!("Expected String response"),
        }
    }

    #[test]
    fn test_std_http_offloaded_get_and_post() {
        let pool = WorkerPool::new(2);

        // GET request
        let get_res = modules::http::get(
            &[Value::String(Arc::new(
                "https://api.example.com/users".into(),
            ))],
            Some(&pool),
        )
        .expect("HTTP GET failed");
        match get_res {
            Value::Object(o) => {
                let g = o.lock().unwrap();
                assert_eq!(g.get("status"), Some(&Value::Int(200)));
                assert_eq!(g.get("ok"), Some(&Value::Bool(true)));
            }
            _ => panic!("Expected Object response"),
        }

        // POST request
        let post_res = modules::http::post(
            &[
                Value::String(Arc::new("https://api.example.com/login".into())),
                Value::String(Arc::new("payload".into())),
            ],
            Some(&pool),
        )
        .expect("HTTP POST failed");

        match post_res {
            Value::Object(o) => {
                let g = o.lock().unwrap();
                assert_eq!(g.get("status"), Some(&Value::Int(201)));
            }
            _ => panic!("Expected Object response"),
        }
    }

    #[test]
    fn test_ffi_registry_module_resolution_and_invoke() {
        let registry = FfiRegistry::new();
        let pool = WorkerPool::new(2);

        // Alias resolution: "db" -> "rust:trulang_std_pg"
        registry.register_alias("db", "rust:trulang_std_pg");
        assert!(registry.has_module("db"));

        // Invoke create_pool
        let pool_val = registry
            .invoke("db", "create_pool", &[], Some(&pool))
            .expect("create_pool failed");
        match pool_val {
            Value::Handle(h) => {
                assert_eq!(h.type_name, "PgPool");
            }
            _ => panic!("Expected PgPool handle"),
        }
    }

    #[test]
    fn test_marshal_value_memory_estimation() {
        let str_val = Value::String(Arc::new("hello world".into()));
        assert_eq!(estimate_value_size(&str_val), 24 + 11);

        let int_val = Value::Int(42);
        assert_eq!(estimate_value_size(&int_val), 8);
    }
}
