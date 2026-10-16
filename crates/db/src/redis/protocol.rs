//! Redis RESP2 / RESP3 Binary Serialization and Wire Framing Protocol.
//!
//! Implements high-throughput serialization and parsing for simple strings,
//! errors, integers, bulk strings, arrays, and null values.

use crate::error::DbError;
use std::io::{BufRead, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<Vec<u8>>),
    Array(Option<Vec<RespValue>>),
    Boolean(bool),
    Null,
}

impl RespValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RespValue::SimpleString(s) => Some(s.as_str()),
            RespValue::BulkString(Some(bytes)) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            RespValue::Integer(n) => Some(*n),
            _ => self.as_str()?.parse::<i64>().ok(),
        }
    }
}

/// Encodes an array of string arguments into a standard RESP command buffer:
/// `*<arg_count>\r\n$<len>\r\n<arg>\r\n...`
pub fn encode_command<W: Write>(writer: &mut W, args: &[&str]) -> Result<(), DbError> {
    writer.write_all(format!("*{}\r\n", args.len()).as_bytes())?;
    for arg in args {
        let bytes = arg.as_bytes();
        writer.write_all(format!("${}\r\n", bytes.len()).as_bytes())?;
        writer.write_all(bytes)?;
        writer.write_all(b"\r\n")?;
    }
    writer.flush()?;
    Ok(())
}

/// Reads and parses the next RESP value from a buffered TCP stream.
pub fn read_resp_value<R: BufRead>(reader: &mut R) -> Result<RespValue, DbError> {
    let mut prefix_buf = [0u8; 1];
    reader.read_exact(&mut prefix_buf)?;

    match prefix_buf[0] {
        b'+' => {
            let line = read_crlf_line(reader)?;
            Ok(RespValue::SimpleString(line))
        }
        b'-' => {
            let line = read_crlf_line(reader)?;
            Ok(RespValue::Error(line))
        }
        b':' => {
            let line = read_crlf_line(reader)?;
            let n = line
                .parse::<i64>()
                .map_err(|e| DbError::RedisProtocol(format!("Invalid integer in RESP: {}", e)))?;
            Ok(RespValue::Integer(n))
        }
        b'$' => {
            let len_line = read_crlf_line(reader)?;
            let len = len_line.parse::<i64>().map_err(|e| {
                DbError::RedisProtocol(format!("Invalid bulk string length in RESP: {}", e))
            })?;

            if len < 0 {
                return Ok(RespValue::BulkString(None));
            }

            let ulen = len as usize;
            let mut data = vec![0u8; ulen];
            reader.read_exact(&mut data)?;

            // Discard trailing \r\n
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
            if &crlf != b"\r\n" {
                return Err(DbError::RedisProtocol(
                    "Expected CRLF after bulk data".into(),
                ));
            }

            Ok(RespValue::BulkString(Some(data)))
        }
        b'*' => {
            let count_line = read_crlf_line(reader)?;
            let count = count_line.parse::<i64>().map_err(|e| {
                DbError::RedisProtocol(format!("Invalid array count in RESP: {}", e))
            })?;

            if count < 0 {
                return Ok(RespValue::Array(None));
            }

            let mut elements = Vec::with_capacity(count as usize);
            for _ in 0..count {
                elements.push(read_resp_value(reader)?);
            }
            Ok(RespValue::Array(Some(elements)))
        }
        b'_' => {
            // RESP3 Null: _\r\n
            let _ = read_crlf_line(reader)?;
            Ok(RespValue::Null)
        }
        b'#' => {
            // RESP3 Boolean: #t\r\n or #f\r\n
            let line = read_crlf_line(reader)?;
            match line.as_str() {
                "t" => Ok(RespValue::Boolean(true)),
                "f" => Ok(RespValue::Boolean(false)),
                other => Err(DbError::RedisProtocol(format!(
                    "Invalid RESP3 boolean: {}",
                    other
                ))),
            }
        }
        other => Err(DbError::RedisProtocol(format!(
            "Unexpected RESP type byte: '{}' ({})",
            other as char, other
        ))),
    }
}

fn read_crlf_line<R: BufRead>(reader: &mut R) -> Result<String, DbError> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.ends_with("\r\n") {
        line.truncate(line.len() - 2);
    } else if line.ends_with('\n') {
        line.truncate(line.len() - 1);
    }
    Ok(line)
}
