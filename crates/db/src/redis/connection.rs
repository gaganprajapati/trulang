//! Redis Physical TCP Connection Client.
//!
//! Handles command execution, socket timeouts, pipelining, and error unwrapping.

use super::protocol::*;
use crate::error::DbError;
use std::io::{BufReader, BufWriter};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct RedisConnection {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl std::fmt::Debug for RedisConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConnection").finish()
    }
}

impl RedisConnection {
    /// Establishes a direct TCP connection to a Redis / KeyDB / Valkey instance.
    pub fn connect(
        host: &str,
        port: u16,
        password: Option<&str>,
        database: Option<u8>,
        timeout: Option<Duration>,
    ) -> Result<Self, DbError> {
        let addr_str = format!("{}:{}", host, port);
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| DbError::InvalidUrl(format!("Failed to resolve '{}': {}", addr_str, e)))?
            .collect();

        if addrs.is_empty() {
            return Err(DbError::InvalidUrl(format!(
                "Could not resolve address '{}'",
                addr_str
            )));
        }

        let stream = match timeout {
            Some(d) => TcpStream::connect_timeout(&addrs[0], d)?,
            None => TcpStream::connect(&addrs[0])?,
        };

        stream.set_nodelay(true)?;
        if let Some(t) = timeout {
            stream.set_read_timeout(Some(t))?;
            stream.set_write_timeout(Some(t))?;
        }

        let read_stream = stream.try_clone()?;
        let mut conn = Self {
            reader: BufReader::new(read_stream),
            writer: BufWriter::new(stream),
        };

        // Authenticate if password provided
        if let Some(pwd) = password {
            let res = conn.execute_command(&["AUTH", pwd])?;
            if let RespValue::Error(err) = res {
                return Err(DbError::AuthenticationFailed(err));
            }
        }

        // Select database index if provided
        if let Some(db) = database {
            let db_str = db.to_string();
            let res = conn.execute_command(&["SELECT", &db_str])?;
            if let RespValue::Error(err) = res {
                return Err(DbError::RedisServerError(err));
            }
        }

        Ok(conn)
    }

    /// Sends a single command and reads the immediate response.
    pub fn execute_command(&mut self, args: &[&str]) -> Result<RespValue, DbError> {
        encode_command(&mut self.writer, args)?;
        let resp = read_resp_value(&mut self.reader)?;
        if let RespValue::Error(ref err) = resp {
            return Err(DbError::RedisServerError(err.clone()));
        }
        Ok(resp)
    }

    /// Pings the server (PING -> PONG).
    pub fn ping(&mut self) -> Result<String, DbError> {
        let resp = self.execute_command(&["PING"])?;
        match resp {
            RespValue::SimpleString(s) => Ok(s),
            RespValue::BulkString(Some(bytes)) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected PING response: {:?}",
                other
            ))),
        }
    }

    /// Fetches the string value of a key.
    pub fn get(&mut self, key: &str) -> Result<Option<String>, DbError> {
        let resp = self.execute_command(&["GET", key])?;
        match resp {
            RespValue::BulkString(Some(bytes)) => Ok(Some(
                String::from_utf8(bytes).map_err(|e| DbError::Serialization(e.to_string()))?,
            )),
            RespValue::BulkString(None) | RespValue::Null => Ok(None),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected GET response: {:?}",
                other
            ))),
        }
    }

    /// Sets the value of a key with an optional TTL in seconds.
    pub fn set(&mut self, key: &str, value: &str, ttl_secs: Option<u64>) -> Result<(), DbError> {
        if let Some(ttl) = ttl_secs {
            let ttl_str = ttl.to_string();
            self.execute_command(&["SET", key, value, "EX", &ttl_str])?;
        } else {
            self.execute_command(&["SET", key, value])?;
        }
        Ok(())
    }

    /// Deletes one or more keys.
    pub fn del(&mut self, keys: &[&str]) -> Result<usize, DbError> {
        let mut args = vec!["DEL"];
        args.extend_from_slice(keys);
        let resp = self.execute_command(&args)?;
        match resp {
            RespValue::Integer(n) => Ok(n as usize),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected DEL response: {:?}",
                other
            ))),
        }
    }

    /// Atomically increments key by 1.
    pub fn incr(&mut self, key: &str) -> Result<i64, DbError> {
        let resp = self.execute_command(&["INCR", key])?;
        match resp {
            RespValue::Integer(n) => Ok(n),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected INCR response: {:?}",
                other
            ))),
        }
    }

    /// Gets a field value from a Redis hash.
    pub fn hget(&mut self, key: &str, field: &str) -> Result<Option<String>, DbError> {
        let resp = self.execute_command(&["HGET", key, field])?;
        match resp {
            RespValue::BulkString(Some(bytes)) => Ok(Some(
                String::from_utf8(bytes).map_err(|e| DbError::Serialization(e.to_string()))?,
            )),
            RespValue::BulkString(None) | RespValue::Null => Ok(None),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected HGET response: {:?}",
                other
            ))),
        }
    }

    /// Sets a field in a Redis hash. Returns true if field is new.
    pub fn hset(&mut self, key: &str, field: &str, value: &str) -> Result<bool, DbError> {
        let resp = self.execute_command(&["HSET", key, field, value])?;
        match resp {
            RespValue::Integer(n) => Ok(n == 1),
            other => Err(DbError::RedisProtocol(format!(
                "Unexpected HSET response: {:?}",
                other
            ))),
        }
    }

    /// Checks if the connection is still alive.
    pub fn is_healthy(&mut self) -> bool {
        self.ping().map(|s| s == "PONG").unwrap_or(false)
    }
}
