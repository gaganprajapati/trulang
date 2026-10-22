pub mod error;
pub mod handle;
pub mod marshal;
pub mod modules;
pub mod registry;
pub mod worker_pool;

pub use error::FfiError;
pub use handle::{
    create_pg_pool_handle, create_sqlite_handle, next_handle_id, PgPoolResource, SqliteResource,
    GLOBAL_DROPPED_HANDLES_COUNT,
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

    #[test]
    fn test_ffi_trace_span_and_otel_module() {
        let registry = FfiRegistry::new();
        assert!(registry.has_module("trulang/trace"));
        assert!(registry.has_module("otel"));

        let span_val = registry
            .invoke(
                "otel",
                "create_span",
                &[Value::String(Arc::new("checkout_flow".into()))],
                None,
            )
            .expect("create_span failed");

        match span_val {
            Value::Handle(h) => {
                assert_eq!(h.type_name, "TraceSpan");
                let trace_id = h.resource.call_method("trace_id", &[]).unwrap();
                let span_id = h.resource.call_method("span_id", &[]).unwrap();
                match (trace_id, span_id) {
                    (Value::String(t), Value::String(s)) => {
                        assert_eq!(t.len(), 32);
                        assert_eq!(s.len(), 16);
                    }
                    _ => panic!("Expected string IDs"),
                }

                let set_attr = h.resource.call_method(
                    "set_attribute",
                    &[
                        Value::String(Arc::new("http.status_code".into())),
                        Value::String(Arc::new("200".into())),
                    ],
                );
                assert_eq!(set_attr, Ok(Value::Bool(true)));

                let finish = h.resource.call_method("finish", &[]);
                assert_eq!(finish, Ok(Value::Bool(true)));
            }
            _ => panic!("Expected TraceSpan handle"),
        }
    }

    #[test]
    fn test_ffi_postgres_and_sqlite_modules() {
        let registry = FfiRegistry::new();
        assert!(registry.has_module("std::db"));
        assert!(registry.has_module("std::db::postgres"));
        assert!(registry.has_module("std::db::sqlite"));
        assert!(registry.has_module("sqlite"));

        // 1. Test SQLite via std::db
        let sqlite_val = registry
            .invoke(
                "std::db",
                "sqlite",
                &[Value::String(Arc::new(":memory:".into()))],
                None,
            )
            .expect("sqlite create failed");

        if let Value::Handle(h) = sqlite_val {
            assert_eq!(h.type_name, "Sqlite");

            // Execute DDL
            let exec_ddl = h
                .resource
                .call_method(
                    "execute",
                    &[Value::String(Arc::new(
                        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)".into(),
                    ))],
                )
                .expect("CREATE TABLE failed");
            if let Value::Object(ref o) = exec_ddl {
                assert_eq!(o.lock().unwrap().get("ok"), Some(&Value::Bool(true)));
            }

            // Insert parameterized
            let exec_ins = h
                .resource
                .call_method(
                    "execute",
                    &[
                        Value::String(Arc::new("INSERT INTO items VALUES (?, ?)".into())),
                        Value::Array(vec![
                            Value::Int(10),
                            Value::String(Arc::new("Widget".into())),
                        ]),
                    ],
                )
                .expect("INSERT failed");
            if let Value::Object(ref o) = exec_ins {
                assert_eq!(o.lock().unwrap().get("rows_affected"), Some(&Value::Int(1)));
            }

            // query_one
            let one = h
                .resource
                .call_method(
                    "query_one",
                    &[
                        Value::String(Arc::new("SELECT * FROM items WHERE id = ?".into())),
                        Value::Array(vec![Value::Int(10)]),
                    ],
                )
                .expect("query_one failed");
            if let Value::Object(ref o) = one {
                let g = o.lock().unwrap();
                assert_eq!(g.get("id"), Some(&Value::Int(10)));
                assert_eq!(
                    g.get("name"),
                    Some(&Value::String(Arc::new("Widget".into())))
                );
            } else {
                panic!("Expected Object from query_one");
            }

            // Checkpoint
            let cp = h.resource.call_method("checkpoint", &[]);
            assert_eq!(cp, Ok(Value::Bool(true)));

            // Close
            let close = h.resource.call_method("close", &[]);
            assert_eq!(close, Ok(Value::Bool(true)));
        } else {
            panic!("Expected Sqlite handle");
        }

        // 2. Test Postgres via std::db::postgres
        let pg_val = registry
            .invoke(
                "std::db::postgres",
                "create_pool",
                &[Value::String(Arc::new(
                    "postgres://localhost:5432/test".into(),
                ))],
                None,
            )
            .expect("postgres create failed");

        if let Value::Handle(h) = pg_val {
            assert_eq!(h.type_name, "PgPool");
            let q_one = h.resource.call_method(
                "query_one",
                &[
                    Value::String(Arc::new("SELECT id, name FROM users WHERE id = $1".into())),
                    Value::Array(vec![Value::Int(42)]),
                ],
            );
            assert!(q_one.is_ok());
        } else {
            panic!("Expected PgPool handle");
        }
    }
}
