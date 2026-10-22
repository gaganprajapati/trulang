use crate::error::FfiError;
use crate::handle::PgPoolResource;
use trulang_compiler::bytecode::Value;

pub fn create_pool(args: &[Value]) -> Result<Value, FfiError> {
    let conn_url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(FfiError::InvalidArgument(format!(
                "create_pool expects a connection string, found {}",
                other
            )));
        }
        None => "postgres://localhost:5432/trulang_db",
    };

    let max_conns = args
        .get(1)
        .and_then(|v| match v {
            Value::Int(n) if *n > 0 => Some(*n as usize),
            Value::Object(o) => {
                let g = o.lock().unwrap();
                match g.get("max_connections") {
                    Some(Value::Int(n)) if *n > 0 => Some(*n as usize),
                    _ => None,
                }
            }
            Value::Map(m) => {
                let g = m.lock().unwrap();
                match g.get("max_connections") {
                    Some(Value::Int(n)) if *n > 0 => Some(*n as usize),
                    _ => None,
                }
            }
            _ => None,
        })
        .unwrap_or(16);

    let pool = match trulang_db::PgPool::new(conn_url, max_conns) {
        Ok(p) => PgPoolResource::with_pool(conn_url, p),
        Err(_) => PgPoolResource::new(conn_url),
    };
    Ok(pool.into_value())
}
