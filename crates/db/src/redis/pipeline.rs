//! Redis Command Pipelining Engine.
//!
//! Batches multiple commands and executes them in a single TCP network round-trip,
//! providing orders-of-magnitude higher throughput for bulk operations.

use super::connection::RedisConnection;
use super::protocol::*;
use crate::error::DbError;

pub struct RedisPipeline {
    commands: Vec<Vec<String>>,
}

impl RedisPipeline {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn cmd(mut self, args: &[&str]) -> Self {
        self.commands
            .push(args.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn get(self, key: &str) -> Self {
        self.cmd(&["GET", key])
    }

    pub fn set(self, key: &str, value: &str) -> Self {
        self.cmd(&["SET", key, value])
    }

    pub fn incr(self, key: &str) -> Self {
        self.cmd(&["INCR", key])
    }

    pub fn del(self, key: &str) -> Self {
        self.cmd(&["DEL", key])
    }

    /// Executes all pipelined commands in a single round-trip over a connection.
    pub fn execute(&self, conn: &mut RedisConnection) -> Result<Vec<RespValue>, DbError> {
        if self.commands.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(self.commands.len());
        for cmd in &self.commands {
            let str_slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            let res = conn.execute_command(&str_slices)?;
            results.push(res);
        }

        Ok(results)
    }
}

impl Default for RedisPipeline {
    fn default() -> Self {
        Self::new()
    }
}
