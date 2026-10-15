use crate::error::FfiError;
use crate::handle::PgPoolResource;
use vlang_compiler::bytecode::Value;

pub fn create_pool(args: &[Value]) -> Result<Value, FfiError> {
    let conn_url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(FfiError::InvalidArgument(format!(
                "create_pool expects a connection string, found {}",
                other
            )));
        }
        None => "postgres://localhost:5432/vlang_db",
    };

    let pool = PgPoolResource::new(conn_url);
    Ok(pool.into_value())
}
