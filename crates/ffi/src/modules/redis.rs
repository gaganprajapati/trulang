use crate::error::FfiError;
use crate::handle::RedisPoolResource;
use trulang_compiler::bytecode::Value;

pub fn create_client(args: &[Value]) -> Result<Value, FfiError> {
    create_pool(args)
}

pub fn create_pool(args: &[Value]) -> Result<Value, FfiError> {
    let conn_url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(FfiError::InvalidArgument(format!(
                "create_pool expects a connection string (redis://...), found {}",
                other
            )));
        }
        None => "redis://127.0.0.1:6379",
    };

    let max_conns = args
        .get(1)
        .and_then(|v| match v {
            Value::Int(n) if *n > 0 => Some(*n as usize),
            _ => None,
        })
        .unwrap_or(16);

    let pool = match trulang_db::RedisPool::new(conn_url, max_conns) {
        Ok(p) => RedisPoolResource::with_pool(conn_url, p),
        Err(_) => RedisPoolResource::new(conn_url),
    };
    Ok(pool.into_value())
}
