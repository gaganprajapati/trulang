//! PostgreSQL Connection Pool Living in Region B (Persistent Memory).
//!
//! Provides thread-safe connection pooling, health checks, query execution,
//! and type decoding into TruLang values.

use super::connection::{PgConnection, PgQueryResult};
use super::protocol::*;
use crate::error::DbError;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trulang_compiler::bytecode::Value;

#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub password: Option<String>,
    pub max_connections: usize,
    pub connection_timeout: Duration,
}

impl PgConfig {
    /// Parses a standard PostgreSQL connection URI:
    /// `postgres://user:password@host:port/database`
    pub fn parse(url: &str) -> Result<Self, DbError> {
        let stripped = url
            .strip_prefix("postgres://")
            .or_else(|| url.strip_prefix("postgresql://"))
            .unwrap_or(url);

        let (user_pass, rest) = if let Some(idx) = stripped.find('@') {
            (&stripped[..idx], &stripped[idx + 1..])
        } else {
            ("", stripped)
        };

        let (user, password) = if let Some(idx) = user_pass.find(':') {
            (
                user_pass[..idx].to_string(),
                Some(user_pass[idx + 1..].to_string()),
            )
        } else if !user_pass.is_empty() {
            (user_pass.to_string(), None)
        } else {
            ("postgres".to_string(), None)
        };

        let (host_port, database) = if let Some(idx) = rest.find('/') {
            (
                &rest[..idx],
                rest[idx + 1..].split('?').next().unwrap_or("postgres"),
            )
        } else {
            (rest, "postgres")
        };

        let (host, port) = if let Some(idx) = host_port.find(':') {
            let p: u16 = host_port[idx + 1..].parse().unwrap_or(5432);
            (host_port[..idx].to_string(), p)
        } else if !host_port.is_empty() {
            (host_port.to_string(), 5432)
        } else {
            ("localhost".to_string(), 5432)
        };

        Ok(Self {
            host,
            port,
            user,
            database: database.to_string(),
            password,
            max_connections: 16,
            connection_timeout: Duration::from_secs(5),
        })
    }
}

pub struct PgPool {
    config: PgConfig,
    pool: Mutex<VecDeque<PgConnection>>,
    active_count: Mutex<usize>,
}

impl std::fmt::Debug for PgPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgPool")
            .field("config", &self.config)
            .field(
                "pooled_connections",
                &self.pool.lock().map(|p| p.len()).unwrap_or(0),
            )
            .field("active_count", &self.active_count)
            .finish()
    }
}

impl PgPool {
    /// Creates a new connection pool for PostgreSQL.
    pub fn new(url: &str, max_connections: usize) -> Result<Arc<Self>, DbError> {
        let mut config = PgConfig::parse(url)?;
        if max_connections > 0 {
            config.max_connections = max_connections;
        }

        let pool = Arc::new(Self {
            config,
            pool: Mutex::new(VecDeque::new()),
            active_count: Mutex::new(0),
        });

        // Pre-warm a single connection if reachable
        if let Ok(conn) = pool.create_connection() {
            pool.pool.lock().unwrap().push_back(conn);
        }

        Ok(pool)
    }

    fn create_connection(&self) -> Result<PgConnection, DbError> {
        PgConnection::connect(
            &self.config.host,
            self.config.port,
            &self.config.user,
            &self.config.database,
            self.config.password.as_deref(),
            Some(self.config.connection_timeout),
        )
    }

    /// Acquires a pooled connection or instantiates a new one up to `max_connections`.
    pub fn acquire(&self) -> Result<PooledPgConnection<'_>, DbError> {
        let mut pool = self.pool.lock().unwrap();
        let mut active = self.active_count.lock().unwrap();

        // 1. Try to take existing healthy connection
        while let Some(mut conn) = pool.pop_front() {
            if conn.is_healthy() {
                *active += 1;
                return Ok(PooledPgConnection {
                    conn: Some(conn),
                    pool: self,
                });
            }
        }

        // 2. Spawn new connection if under max limit
        if *active < self.config.max_connections {
            let conn = self.create_connection()?;
            *active += 1;
            return Ok(PooledPgConnection {
                conn: Some(conn),
                pool: self,
            });
        }

        Err(DbError::PoolExhausted(self.config.max_connections))
    }

    fn release(&self, conn: PgConnection) {
        let mut active = self.active_count.lock().unwrap();
        if *active > 0 {
            *active -= 1;
        }
        let mut pool = self.pool.lock().unwrap();
        if pool.len() < self.config.max_connections {
            pool.push_back(conn);
        }
    }

    /// Executes a SQL query and decodes results into a TruLang Array of Objects.
    pub fn query(&self, sql: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let res = conn.query(sql)?;
        Ok(decode_query_result_to_trulang(&res))
    }

    /// Executes a SQL command (INSERT/UPDATE/DELETE/DDL) and returns metadata object.
    pub fn execute(&self, sql: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let res = conn.execute(sql)?;

        let obj = Value::new_object();
        if let Value::Object(ref o) = obj {
            let mut g = o.lock().unwrap();
            g.insert(
                "rows_affected".to_string(),
                Value::Int(res.rows_affected as i64),
            );
            g.insert(
                "command_tag".to_string(),
                Value::String(Arc::new(res.command_tag)),
            );
            g.insert("ok".to_string(), Value::Bool(true));
        }
        Ok(obj)
    }

    /// Returns pool metrics and status information.
    pub fn stats(&self) -> Value {
        let pool = self.pool.lock().unwrap();
        let active = self.active_count.lock().unwrap();

        let obj = Value::new_object();
        if let Value::Object(ref o) = obj {
            let mut g = o.lock().unwrap();
            g.insert(
                "idle_connections".to_string(),
                Value::Int(pool.len() as i64),
            );
            g.insert("active_connections".to_string(), Value::Int(*active as i64));
            g.insert(
                "max_connections".to_string(),
                Value::Int(self.config.max_connections as i64),
            );
        }
        obj
    }

    /// Closes all pooled connections.
    pub fn close(&self) {
        let mut pool = self.pool.lock().unwrap();
        while let Some(mut conn) = pool.pop_front() {
            let _ = conn.close();
        }
    }
}

pub struct PooledPgConnection<'a> {
    conn: Option<PgConnection>,
    pool: &'a PgPool,
}

impl<'a> std::ops::Deref for PooledPgConnection<'a> {
    type Target = PgConnection;
    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().unwrap()
    }
}

impl<'a> std::ops::DerefMut for PooledPgConnection<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn.as_mut().unwrap()
    }
}

impl<'a> Drop for PooledPgConnection<'a> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }
}

/// Decodes PostgreSQL raw text-format data rows into typed TruLang values.
pub fn decode_query_result_to_trulang(res: &PgQueryResult) -> Value {
    let mut row_objects = Vec::with_capacity(res.rows.len());

    for row in &res.rows {
        let obj = Value::new_object();
        if let Value::Object(ref o) = obj {
            let mut g = o.lock().unwrap();
            for (i, field) in res.fields.iter().enumerate() {
                let val = match row.values.get(i).and_then(|v| v.as_ref()) {
                    Some(bytes) => decode_field_value(field.type_oid, bytes),
                    None => Value::Null,
                };
                g.insert(field.name.clone(), val);
            }
        }
        row_objects.push(obj);
    }

    Value::Array(row_objects)
}

fn decode_field_value(type_oid: u32, bytes: &[u8]) -> Value {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Value::String(Arc::new(String::from_utf8_lossy(bytes).into_owned())),
    };

    match type_oid {
        OID_BOOL => match text {
            "t" | "true" | "TRUE" | "1" => Value::Bool(true),
            _ => Value::Bool(false),
        },
        OID_INT2 | OID_INT4 | OID_INT8 => {
            if let Ok(n) = text.parse::<i64>() {
                Value::Int(n)
            } else {
                Value::String(Arc::new(text.to_string()))
            }
        }
        OID_FLOAT4 | OID_FLOAT8 => {
            if let Ok(f) = text.parse::<f64>() {
                Value::Float(f)
            } else {
                Value::String(Arc::new(text.to_string()))
            }
        }
        _ => Value::String(Arc::new(text.to_string())),
    }
}
