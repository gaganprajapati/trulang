//! Redis Connection Pool Living in Region B (Persistent Memory).
//!
//! Provides thread-safe connection pooling, automatic failover health checks,
//! and native TruLang value conversions for Redis key-value and hash operations.

use super::connection::RedisConnection;
use super::protocol::RespValue;
use crate::error::DbError;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trulang_compiler::bytecode::Value;

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    pub database: Option<u8>,
    pub max_connections: usize,
    pub timeout: Duration,
}

impl RedisConfig {
    /// Parses a standard Redis URL: `redis://[:password@]host[:port][/db]`
    pub fn parse(url: &str) -> Result<Self, DbError> {
        let stripped = url.strip_prefix("redis://").unwrap_or(url);

        let (auth, rest) = if let Some(idx) = stripped.find('@') {
            (&stripped[..idx], &stripped[idx + 1..])
        } else {
            ("", stripped)
        };

        let password = if let Some(idx) = auth.find(':') {
            let pwd = &auth[idx + 1..];
            if !pwd.is_empty() {
                Some(pwd.to_string())
            } else {
                None
            }
        } else if !auth.is_empty() {
            Some(auth.to_string())
        } else {
            None
        };

        let (host_port, db_part) = if let Some(idx) = rest.find('/') {
            (&rest[..idx], Some(&rest[idx + 1..]))
        } else {
            (rest, None)
        };

        let database = db_part
            .and_then(|s| s.split('?').next())
            .and_then(|s| s.parse::<u8>().ok());

        let (host, port) = if let Some(idx) = host_port.find(':') {
            let p: u16 = host_port[idx + 1..].parse().unwrap_or(6379);
            (host_port[..idx].to_string(), p)
        } else if !host_port.is_empty() {
            (host_port.to_string(), 6379)
        } else {
            ("127.0.0.1".to_string(), 6379)
        };

        Ok(Self {
            host,
            port,
            password,
            database,
            max_connections: 16,
            timeout: Duration::from_secs(5),
        })
    }
}

pub struct RedisPool {
    config: RedisConfig,
    pool: Mutex<VecDeque<RedisConnection>>,
    active_count: Mutex<usize>,
}

impl std::fmt::Debug for RedisPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPool")
            .field("config", &self.config)
            .field(
                "pooled_connections",
                &self.pool.lock().map(|p| p.len()).unwrap_or(0),
            )
            .field("active_count", &self.active_count)
            .finish()
    }
}

impl RedisPool {
    /// Creates a new pooled Redis client.
    pub fn new(url: &str, max_connections: usize) -> Result<Arc<Self>, DbError> {
        let mut config = RedisConfig::parse(url)?;
        if max_connections > 0 {
            config.max_connections = max_connections;
        }

        let pool = Arc::new(Self {
            config,
            pool: Mutex::new(VecDeque::new()),
            active_count: Mutex::new(0),
        });

        // Pre-warm a single connection if available
        if let Ok(conn) = pool.create_connection() {
            pool.pool.lock().unwrap().push_back(conn);
        }

        Ok(pool)
    }

    fn create_connection(&self) -> Result<RedisConnection, DbError> {
        RedisConnection::connect(
            &self.config.host,
            self.config.port,
            self.config.password.as_deref(),
            self.config.database,
            Some(self.config.timeout),
        )
    }

    pub fn acquire(&self) -> Result<PooledRedisConnection<'_>, DbError> {
        let mut pool = self.pool.lock().unwrap();
        let mut active = self.active_count.lock().unwrap();

        // 1. Pop existing healthy connection
        while let Some(mut conn) = pool.pop_front() {
            if conn.is_healthy() {
                *active += 1;
                return Ok(PooledRedisConnection {
                    conn: Some(conn),
                    pool: self,
                });
            }
        }

        // 2. Spawn new connection if under limit
        if *active < self.config.max_connections {
            let conn = self.create_connection()?;
            *active += 1;
            return Ok(PooledRedisConnection {
                conn: Some(conn),
                pool: self,
            });
        }

        Err(DbError::PoolExhausted(self.config.max_connections))
    }

    fn release(&self, conn: RedisConnection) {
        let mut active = self.active_count.lock().unwrap();
        if *active > 0 {
            *active -= 1;
        }
        let mut pool = self.pool.lock().unwrap();
        if pool.len() < self.config.max_connections {
            pool.push_back(conn);
        }
    }

    pub fn get(&self, key: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        match conn.get(key)? {
            Some(s) => Ok(Value::String(Arc::new(s))),
            None => Ok(Value::Null),
        }
    }

    pub fn set(&self, key: &str, val: &str, ttl_secs: Option<u64>) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        conn.set(key, val, ttl_secs)?;
        Ok(Value::Bool(true))
    }

    pub fn del(&self, keys: &[&str]) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let count = conn.del(keys)?;
        Ok(Value::Int(count as i64))
    }

    pub fn incr(&self, key: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let val = conn.incr(key)?;
        Ok(Value::Int(val))
    }

    pub fn hget(&self, key: &str, field: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        match conn.hget(key, field)? {
            Some(s) => Ok(Value::String(Arc::new(s))),
            None => Ok(Value::Null),
        }
    }

    pub fn hset(&self, key: &str, field: &str, val: &str) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let is_new = conn.hset(key, field, val)?;
        Ok(Value::Bool(is_new))
    }

    pub fn ping(&self) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let pong = conn.ping()?;
        Ok(Value::String(Arc::new(pong)))
    }

    /// Generic command execution mapping response to TruLang Value.
    pub fn execute(&self, args: &[&str]) -> Result<Value, DbError> {
        let mut conn = self.acquire()?;
        let resp = conn.execute_command(args)?;
        Ok(resp_to_trulang_value(&resp))
    }

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

    pub fn close(&self) {
        let mut pool = self.pool.lock().unwrap();
        pool.clear();
    }
}

pub struct PooledRedisConnection<'a> {
    conn: Option<RedisConnection>,
    pool: &'a RedisPool,
}

impl<'a> std::ops::Deref for PooledRedisConnection<'a> {
    type Target = RedisConnection;
    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().unwrap()
    }
}

impl<'a> std::ops::DerefMut for PooledRedisConnection<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn.as_mut().unwrap()
    }
}

impl<'a> Drop for PooledRedisConnection<'a> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }
}

fn resp_to_trulang_value(resp: &RespValue) -> Value {
    match resp {
        RespValue::SimpleString(s) => Value::String(Arc::new(s.clone())),
        RespValue::BulkString(Some(bytes)) => {
            Value::String(Arc::new(String::from_utf8_lossy(bytes).into_owned()))
        }
        RespValue::BulkString(None) | RespValue::Null => Value::Null,
        RespValue::Integer(n) => Value::Int(*n),
        RespValue::Boolean(b) => Value::Bool(*b),
        RespValue::Array(Some(elems)) => {
            Value::Array(elems.iter().map(resp_to_trulang_value).collect())
        }
        RespValue::Array(None) => Value::Null,
        RespValue::Error(err) => {
            let obj = Value::new_object();
            if let Value::Object(ref o) = obj {
                let mut g = o.lock().unwrap();
                g.insert("error".to_string(), Value::String(Arc::new(err.clone())));
            }
            obj
        }
    }
}
