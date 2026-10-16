use crate::error::FfiError;
use crate::handle::SpanResource;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use trulang_compiler::bytecode::Value;

pub fn create_span(args: &[Value]) -> Result<Value, FfiError> {
    let name = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        _ => "unnamed_span",
    };

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let trace_id = format!("{:032x}", nanos);
    let span_id = format!("{:016x}", nanos as u64);

    let span = SpanResource::new(name, trace_id, span_id);
    Ok(span.into_value())
}

pub fn new_trace_id(_args: &[Value]) -> Result<Value, FfiError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Value::String(Arc::new(format!("{:032x}", nanos))))
}

pub fn new_span_id(_args: &[Value]) -> Result<Value, FfiError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Value::String(Arc::new(format!("{:016x}", nanos as u64))))
}
