use crate::error::FfiError;
use crate::marshal::{json_to_vlang_value, vlang_value_to_json};
use std::sync::Arc;
use vlang_compiler::bytecode::Value;

pub fn parse(args: &[Value]) -> Result<Value, FfiError> {
    let input = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => {
            return Err(FfiError::InvalidArgument(format!(
                "json.parse expects a string argument, found {}",
                other
            )));
        }
        None => return Err(FfiError::InvalidArgument("json.parse requires 1 argument".into())),
    };

    let parsed: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| FfiError::JsonError(format!("Invalid JSON syntax: {}", e)))?;

    Ok(json_to_vlang_value(&parsed))
}

pub fn stringify(args: &[Value]) -> Result<Value, FfiError> {
    let input = match args.first() {
        Some(v) => v,
        None => return Err(FfiError::InvalidArgument("json.stringify requires 1 argument".into())),
    };

    let json_val = vlang_value_to_json(input);
    let serialized = serde_json::to_string(&json_val)
        .map_err(|e| FfiError::JsonError(format!("Failed to serialize to JSON: {}", e)))?;

    Ok(Value::String(Arc::new(serialized)))
}
