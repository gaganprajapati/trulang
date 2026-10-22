//! Dedicated Background Worker Thread for Embedded SQLite.
//!
//! Exclusively owns the `rusqlite::Connection` handle, serializes writes and reads
//! through a bounded channel, executes WAL initialization, and runs checkpoints on drain.

use crate::error::DbError;
use crossbeam_channel::{bounded, Receiver, Sender};
use rusqlite::Connection;
use std::sync::Arc;
use std::thread;
use trulang_compiler::bytecode::Value;

pub const SQLITE_CHANNEL_CAPACITY: usize = 1024;

pub enum SqliteTask {
    Query {
        sql: String,
        params: Vec<Value>,
        responder: Sender<Result<Value, DbError>>,
    },
    QueryOne {
        sql: String,
        params: Vec<Value>,
        responder: Sender<Result<Option<Value>, DbError>>,
    },
    Execute {
        sql: String,
        params: Vec<Value>,
        responder: Sender<Result<Value, DbError>>,
    },
    Checkpoint {
        responder: Sender<Result<(), DbError>>,
    },
    Close {
        responder: Sender<Result<(), DbError>>,
    },
}

pub struct SqliteWorker {
    pub task_sender: Sender<SqliteTask>,
    pub thread_handle: Option<thread::JoinHandle<()>>,
}

impl SqliteWorker {
    /// Spawns a dedicated background OS thread for the given database path.
    /// Runs WAL initialization pragmas and verifies connection readiness.
    pub fn spawn(path: String) -> Result<Self, DbError> {
        let (task_sender, task_receiver) = bounded::<SqliteTask>(SQLITE_CHANNEL_CAPACITY);
        let (init_tx, init_rx) = bounded::<Result<(), DbError>>(1);

        let thread_handle = thread::Builder::new()
            .name(format!("trulang-sqlite-{}", path))
            .spawn(move || {
                run_worker_loop(&path, task_receiver, init_tx);
            })
            .map_err(|e| DbError::Io(std::io::Error::other(e.to_string())))?;

        // Await worker initialization status
        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                task_sender,
                thread_handle: Some(thread_handle),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(DbError::Sqlite(
                "Worker initialization thread panicked".into(),
            )),
        }
    }
}

fn run_worker_loop(
    path: &str,
    receiver: Receiver<SqliteTask>,
    init_tx: Sender<Result<(), DbError>>,
) {
    let conn = if path == ":memory:" {
        Connection::open_in_memory()
    } else {
        Connection::open(path)
    };

    let mut conn = match conn {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(DbError::Sqlite(e.to_string())));
            return;
        }
    };

    // Initialize required SQLite Pragmas:
    // - journal_mode = WAL (Write-Ahead Logging for high concurrency)
    // - synchronous = NORMAL (Optimal durability/throughput trade-off)
    // - busy_timeout = 5000 (Avoid spurious lock timeouts)
    let pragma_init = conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    );

    if let Err(e) = pragma_init {
        let _ = init_tx.send(Err(DbError::Sqlite(e.to_string())));
        return;
    }

    // Signal successful initialization
    let _ = init_tx.send(Ok(()));

    // Actor Mailbox Processing Loop
    while let Ok(task) = receiver.recv() {
        match task {
            SqliteTask::Query {
                sql,
                params,
                responder,
            } => {
                let res = handle_query(&conn, &sql, &params);
                let _ = responder.send(res);
            }
            SqliteTask::QueryOne {
                sql,
                params,
                responder,
            } => {
                let res = handle_query_one(&conn, &sql, &params);
                let _ = responder.send(res);
            }
            SqliteTask::Execute {
                sql,
                params,
                responder,
            } => {
                let res = handle_execute(&mut conn, &sql, &params);
                let _ = responder.send(res);
            }
            SqliteTask::Checkpoint { responder } => {
                let res = conn
                    .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .map_err(|e| DbError::Sqlite(e.to_string()));
                let _ = responder.send(res);
            }
            SqliteTask::Close { responder } => {
                let res = conn
                    .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .map_err(|e| DbError::Sqlite(e.to_string()));
                let _ = responder.send(res);
                break; // Exit worker loop cleanly
            }
        }
    }
}

fn handle_query(conn: &Connection, sql: &str, params: &[Value]) -> Result<Value, DbError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| DbError::Sqlite(format!("SQL prepare error: {}", e)))?;

    let col_names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();

    let sqlite_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sqlite).collect();

    let rows = stmt
        .query_map(rusqlite::params_from_iter(sqlite_params), |row| {
            let obj = Value::new_object();
            if let Value::Object(ref o) = obj {
                let mut g = o.lock().unwrap();
                for (i, name) in col_names.iter().enumerate() {
                    let val_ref = row.get_ref(i)?;
                    g.insert(name.clone(), decode_sqlite_value(val_ref));
                }
            }
            Ok(obj)
        })
        .map_err(|e| DbError::Sqlite(e.to_string()))?;

    let mut result_rows = Vec::new();
    for row in rows {
        result_rows.push(row.map_err(|e| DbError::Sqlite(e.to_string()))?);
    }

    Ok(Value::Array(result_rows))
}

fn handle_query_one(
    conn: &Connection,
    sql: &str,
    params: &[Value],
) -> Result<Option<Value>, DbError> {
    let res = handle_query(conn, sql, params)?;
    if let Value::Array(mut rows) = res {
        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows.remove(0)))
        }
    } else {
        Ok(None)
    }
}

fn handle_execute(conn: &mut Connection, sql: &str, params: &[Value]) -> Result<Value, DbError> {
    let sqlite_params: Vec<rusqlite::types::Value> = params.iter().map(value_to_sqlite).collect();

    let changes = if params.is_empty() {
        conn.execute(sql, [])
            .map_err(|e| DbError::Sqlite(e.to_string()))?
    } else {
        conn.execute(sql, rusqlite::params_from_iter(sqlite_params))
            .map_err(|e| DbError::Sqlite(e.to_string()))?
    };

    let obj = Value::new_object();
    if let Value::Object(ref o) = obj {
        let mut g = o.lock().unwrap();
        g.insert("rows_affected".to_string(), Value::Int(changes as i64));
        g.insert("ok".to_string(), Value::Bool(true));
    }
    Ok(obj)
}

pub fn value_to_sqlite(val: &Value) -> rusqlite::types::Value {
    match val {
        Value::Null => rusqlite::types::Value::Null,
        Value::Bool(b) => rusqlite::types::Value::Integer(if *b { 1 } else { 0 }),
        Value::Int(n) => rusqlite::types::Value::Integer(*n),
        Value::Float(f) => rusqlite::types::Value::Real(*f),
        Value::String(s) => rusqlite::types::Value::Text(s.to_string()),
        Value::Array(_) | Value::Object(_) | Value::Map(_) => {
            rusqlite::types::Value::Text(val.to_string())
        }
        Value::Handle(h) => rusqlite::types::Value::Integer(h.id as i64),
    }
}

fn decode_sqlite_value(val: rusqlite::types::ValueRef<'_>) -> Value {
    match val {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(n) => Value::Int(n),
        rusqlite::types::ValueRef::Real(f) => Value::Float(f),
        rusqlite::types::ValueRef::Text(s) => {
            Value::String(Arc::new(String::from_utf8_lossy(s).into_owned()))
        }
        rusqlite::types::ValueRef::Blob(b) => {
            Value::String(Arc::new(String::from_utf8_lossy(b).into_owned()))
        }
    }
}
