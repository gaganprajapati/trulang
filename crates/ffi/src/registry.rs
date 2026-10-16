use crate::error::FfiError;
use crate::modules::{http, json, pg};
use crate::worker_pool::WorkerPool;
use std::collections::HashMap;
use std::sync::RwLock;
use trulang_compiler::bytecode::Value;

pub type FfiFn = Box<dyn Fn(&[Value], Option<&WorkerPool>) -> Result<Value, FfiError> + Send + Sync>;

pub struct FfiRegistry {
    aliases: RwLock<HashMap<String, String>>,
    custom_modules: RwLock<HashMap<String, HashMap<String, FfiFn>>>,
}

impl FfiRegistry {
    pub fn new() -> Self {
        let registry = Self {
            aliases: RwLock::new(HashMap::new()),
            custom_modules: RwLock::new(HashMap::new()),
        };

        // Register default aliases
        registry.register_alias("json", "std::json");
        registry.register_alias("http", "std::http");
        registry.register_alias("trulang/http", "std::http");
        registry.register_alias("sql", "std::sql");

        registry
    }

    pub fn register_alias(&self, alias: impl Into<String>, target: impl Into<String>) {
        let mut guard = self.aliases.write().unwrap();
        guard.insert(alias.into(), target.into());
    }

    pub fn resolve_module(&self, name_or_alias: &str) -> String {
        let guard = self.aliases.read().unwrap();
        guard.get(name_or_alias).cloned().unwrap_or_else(|| name_or_alias.to_string())
    }

    pub fn has_module(&self, name_or_alias: &str) -> bool {
        let resolved = self.resolve_module(name_or_alias);
        match resolved.as_str() {
            "std::json" | "std::http" | "std::sql" | "rust:trulang_std_pg" => true,
            other => self.custom_modules.read().unwrap().contains_key(other),
        }
    }

    pub fn register_custom_function(
        &self,
        module: impl Into<String>,
        method: impl Into<String>,
        func: FfiFn,
    ) {
        let mut guard = self.custom_modules.write().unwrap();
        let methods = guard.entry(module.into()).or_default();
        methods.insert(method.into(), func);
    }

    pub fn invoke(
        &self,
        target: &str,
        method: &str,
        args: &[Value],
        pool: Option<&WorkerPool>,
    ) -> Result<Value, FfiError> {
        let resolved = self.resolve_module(target);

        match resolved.as_str() {
            "std::json" => match method {
                "parse" => json::parse(args),
                "stringify" => json::stringify(args),
                _ => Err(FfiError::MethodNotFound {
                    module: resolved,
                    method: method.to_string(),
                }),
            },
            "std::http" => match method {
                "get" => http::get(args, pool),
                "post" => http::post(args, pool),
                "json" => {
                    let resp = Value::new_object();
                    if let Value::Object(ref o) = resp {
                        let mut g = o.lock().unwrap();
                        g.insert("status".to_string(), Value::Int(200));
                        g.insert("data".to_string(), args.first().cloned().unwrap_or(Value::Null));
                    }
                    Ok(resp)
                }
                "text" => {
                    let resp = Value::new_object();
                    if let Value::Object(ref o) = resp {
                        let mut g = o.lock().unwrap();
                        let status = args.first().cloned().unwrap_or(Value::Int(200));
                        let body = args.get(1).cloned().unwrap_or(Value::Null);
                        g.insert("status".to_string(), status);
                        g.insert("body".to_string(), body);
                    }
                    Ok(resp)
                }
                "error" => {
                    let resp = Value::new_object();
                    if let Value::Object(ref o) = resp {
                        let mut g = o.lock().unwrap();
                        let status = args.first().cloned().unwrap_or(Value::Int(500));
                        let msg = args.get(1).cloned().unwrap_or(Value::Null);
                        g.insert("status".to_string(), status);
                        g.insert("error".to_string(), msg);
                    }
                    Ok(resp)
                }
                _ => Err(FfiError::MethodNotFound {
                    module: resolved,
                    method: method.to_string(),
                }),
            },
            "rust:trulang_std_pg" | "std::sql" => match method {
                "create_pool" => pg::create_pool(args),
                _ => Err(FfiError::MethodNotFound {
                    module: resolved,
                    method: method.to_string(),
                }),
            },
            other => {
                let guard = self.custom_modules.read().unwrap();
                if let Some(methods) = guard.get(other) {
                    if let Some(func) = methods.get(method) {
                        return func(args, pool);
                    }
                    return Err(FfiError::MethodNotFound {
                        module: other.to_string(),
                        method: method.to_string(),
                    });
                }
                Err(FfiError::ModuleNotFound(other.to_string()))
            }
        }
    }
}

impl Default for FfiRegistry {
    fn default() -> Self {
        Self::new()
    }
}
