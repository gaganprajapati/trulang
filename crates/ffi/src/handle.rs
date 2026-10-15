use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use vlang_compiler::bytecode::{ExternalHandle, ExternalResource, Value};

static HANDLE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub static GLOBAL_DROPPED_HANDLES_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn next_handle_id() -> u64 {
    HANDLE_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Simulated PostgreSQL Connection Pool implementing ExternalResource
#[derive(Debug)]
pub struct PgPoolResource {
    pub id: u64,
    pub connection_url: String,
    pub is_closed: AtomicBool,
}

impl PgPoolResource {
    pub fn new(connection_url: impl Into<String>) -> Self {
        Self {
            id: next_handle_id(),
            connection_url: connection_url.into(),
            is_closed: AtomicBool::new(false),
        }
    }

    pub fn into_value(self) -> Value {
        let id = self.id;
        Value::new_handle(id, "PgPool", self)
    }
}

impl ExternalResource for PgPoolResource {
    fn type_name(&self) -> &'static str {
        "PgPool"
    }

    fn call_method(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        if self.is_closed.load(Ordering::SeqCst) {
            return Err("Cannot execute query on closed connection pool".into());
        }

        match method {
            "query_user" => {
                let user_id = args.first().cloned().unwrap_or(Value::Int(1));
                let user_obj = Value::new_object();
                if let Value::Object(ref o) = user_obj {
                    let mut g = o.lock().unwrap();
                    g.insert("id".to_string(), user_id);
                    g.insert("name".to_string(), Value::String(Arc::new("Alice Smith".to_string())));
                    g.insert("email".to_string(), Value::String(Arc::new("alice@example.com".to_string())));
                    g.insert("verified".to_string(), Value::Bool(true));
                }
                Ok(user_obj)
            }
            "execute" => {
                let resp = Value::new_object();
                if let Value::Object(ref o) = resp {
                    let mut g = o.lock().unwrap();
                    g.insert("rows_affected".to_string(), Value::Int(1));
                    g.insert("ok".to_string(), Value::Bool(true));
                }
                Ok(resp)
            }
            "close" => {
                self.is_closed.store(true, Ordering::SeqCst);
                Ok(Value::Bool(true))
            }
            _ => Err(format!("Unknown method '{}' on PgPool", method)),
        }
    }
}

impl Drop for PgPoolResource {
    fn drop(&mut self) {
        // Deterministic RAII cleanup
        self.is_closed.store(true, Ordering::SeqCst);
        GLOBAL_DROPPED_HANDLES_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn create_pg_pool_handle(conn_url: &str) -> Arc<ExternalHandle> {
    let res = PgPoolResource::new(conn_url);
    let id = res.id;
    Arc::new(ExternalHandle {
        id,
        type_name: "PgPool".to_string(),
        resource: Arc::new(res),
    })
}
