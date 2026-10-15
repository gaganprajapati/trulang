use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use vlang_compiler::bytecode::Value;

/// Computes the approximate memory footprint of a VLang Value.
/// Used to enforce Region A memory budgets when importing foreign data.
pub fn estimate_value_size(val: &Value) -> usize {
    match val {
        Value::Null => 8,
        Value::Bool(_) => 8,
        Value::Int(_) => 8,
        Value::Float(_) => 8,
        Value::String(s) => 24 + s.len(),
        Value::Array(arr) => 24 + arr.iter().map(estimate_value_size).sum::<usize>(),
        Value::Map(m) => {
            let base = 64;
            if let Ok(guard) = m.lock() {
                base + guard
                    .iter()
                    .map(|(k, v)| k.len() + 16 + estimate_value_size(v))
                    .sum::<usize>()
            } else {
                base
            }
        }
        Value::Object(o) => {
            let base = 64;
            if let Ok(guard) = o.lock() {
                base + guard
                    .iter()
                    .map(|(k, v)| k.len() + 16 + estimate_value_size(v))
                    .sum::<usize>()
            } else {
                base
            }
        }
        Value::Handle(_) => 32,
    }
}

/// Converts a `serde_json::Value` directly into VLang's internal `Value` representation.
pub fn json_to_vlang_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(num) => {
            if let Some(i) = num.as_i64() {
                Value::Int(i)
            } else if let Some(f) = num.as_f64() {
                Value::Float(f)
            } else {
                Value::Float(0.0)
            }
        }
        serde_json::Value::String(s) => Value::String(Arc::new(s.clone())),
        serde_json::Value::Array(arr) => {
            let items = arr.iter().map(json_to_vlang_value).collect();
            Value::Array(items)
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::with_capacity(obj.len());
            for (k, v) in obj {
                map.insert(k.clone(), json_to_vlang_value(v));
            }
            Value::Object(Arc::new(Mutex::new(map)))
        }
    }
}

/// Serializes a VLang `Value` to a `serde_json::Value`.
pub fn vlang_value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::json!(i),
        Value::Float(f) => serde_json::json!(f),
        Value::String(s) => serde_json::Value::String((**s).clone()),
        Value::Array(arr) => {
            let json_arr: Vec<serde_json::Value> = arr.iter().map(vlang_value_to_json).collect();
            serde_json::Value::Array(json_arr)
        }
        Value::Map(m) => {
            let mut json_obj = serde_json::Map::new();
            if let Ok(guard) = m.lock() {
                for (k, v) in guard.iter() {
                    json_obj.insert(k.clone(), vlang_value_to_json(v));
                }
            }
            serde_json::Value::Object(json_obj)
        }
        Value::Object(o) => {
            let mut json_obj = serde_json::Map::new();
            if let Ok(guard) = o.lock() {
                for (k, v) in guard.iter() {
                    json_obj.insert(k.clone(), vlang_value_to_json(v));
                }
            }
            serde_json::Value::Object(json_obj)
        }
        Value::Handle(h) => serde_json::json!({
            "__handle_id": h.id,
            "__type": h.type_name,
        }),
    }
}
