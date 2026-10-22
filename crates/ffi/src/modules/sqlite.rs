use crate::error::FfiError;
use crate::handle::SqliteResource;
use trulang_compiler::bytecode::Value;

pub fn open(args: &[Value]) -> Result<Value, FfiError> {
    let path = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(FfiError::InvalidArgument(format!(
                "sqlite open expects a database path string, found {}",
                other
            )));
        }
        None => ":memory:",
    };

    let res = match trulang_db::SqliteConnection::open(path) {
        Ok(conn) => SqliteResource::with_conn(path, conn),
        Err(_) => SqliteResource::new(path),
    };
    Ok(res.into_value())
}
