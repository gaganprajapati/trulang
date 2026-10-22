//! Embedded SQLite Connection Client.
//!
//! Provides non-blocking actor dispatch to a dedicated background OS worker thread,
//! ensuring zero event-loop stalls, atomic serialization, and WAL mode concurrency.

use super::worker::{SqliteTask, SqliteWorker};
use crate::error::DbError;
use crossbeam_channel::{bounded, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use trulang_compiler::bytecode::Value;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct SqliteConnection {
    path: String,
    sender: Sender<SqliteTask>,
    thread_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    is_closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for SqliteConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteConnection")
            .field("path", &self.path)
            .field("is_closed", &self.is_closed.load(Ordering::Relaxed))
            .finish()
    }
}

impl SqliteConnection {
    /// Opens an embedded SQLite database file at `path` backed by a dedicated OS worker thread.
    pub fn open(path: &str) -> Result<Self, DbError> {
        let worker = SqliteWorker::spawn(path.to_string())?;
        Ok(Self {
            path: path.to_string(),
            sender: worker.task_sender,
            thread_handle: Arc::new(Mutex::new(worker.thread_handle)),
            is_closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Opens an in-memory embedded SQLite database backed by a dedicated OS worker thread.
    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::open(":memory:")
    }

    /// Returns the database path or URI.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Checks if this connection has been explicitly closed.
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }

    /// Executes a SQL query statement and decodes rows into a TruLang Array of Objects.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Value, DbError> {
        if self.is_closed() {
            return Err(DbError::ConnectionClosed);
        }

        let (resp_tx, resp_rx) = bounded(1);
        self.sender
            .send(SqliteTask::Query {
                sql: sql.to_string(),
                params: params.to_vec(),
                responder: resp_tx,
            })
            .map_err(|_| DbError::ConnectionClosed)?;

        resp_rx
            .recv_timeout(DEFAULT_TIMEOUT)
            .map_err(|_| DbError::Sqlite("SQLite query timed out".into()))?
    }

    /// Executes a SQL query statement and returns the first matching row, or None.
    pub fn query_one(&self, sql: &str, params: &[Value]) -> Result<Option<Value>, DbError> {
        if self.is_closed() {
            return Err(DbError::ConnectionClosed);
        }

        let (resp_tx, resp_rx) = bounded(1);
        self.sender
            .send(SqliteTask::QueryOne {
                sql: sql.to_string(),
                params: params.to_vec(),
                responder: resp_tx,
            })
            .map_err(|_| DbError::ConnectionClosed)?;

        resp_rx
            .recv_timeout(DEFAULT_TIMEOUT)
            .map_err(|_| DbError::Sqlite("SQLite query_one timed out".into()))?
    }

    /// Executes a SQL command statement (e.g. INSERT, UPDATE, DELETE, DDL).
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<Value, DbError> {
        if self.is_closed() {
            return Err(DbError::ConnectionClosed);
        }

        let (resp_tx, resp_rx) = bounded(1);
        self.sender
            .send(SqliteTask::Execute {
                sql: sql.to_string(),
                params: params.to_vec(),
                responder: resp_tx,
            })
            .map_err(|_| DbError::ConnectionClosed)?;

        resp_rx
            .recv_timeout(DEFAULT_TIMEOUT)
            .map_err(|_| DbError::Sqlite("SQLite execute timed out".into()))?
    }

    /// Flushes WAL pages to the database file (`PRAGMA wal_checkpoint(TRUNCATE)`).
    pub fn checkpoint(&self) -> Result<(), DbError> {
        if self.is_closed() {
            return Err(DbError::ConnectionClosed);
        }

        let (resp_tx, resp_rx) = bounded(1);
        self.sender
            .send(SqliteTask::Checkpoint { responder: resp_tx })
            .map_err(|_| DbError::ConnectionClosed)?;

        resp_rx
            .recv_timeout(DEFAULT_TIMEOUT)
            .map_err(|_| DbError::Sqlite("SQLite checkpoint timed out".into()))?
    }

    /// Safely checkpoints the WAL, drains remaining tasks, terminates the worker thread,
    /// and joins the background OS thread.
    pub fn close(&self) -> Result<(), DbError> {
        if self.is_closed.swap(true, Ordering::SeqCst) {
            return Ok(()); // Already closed
        }

        let (resp_tx, resp_rx) = bounded(1);
        let _ = self.sender.send(SqliteTask::Close { responder: resp_tx });
        let _ = resp_rx.recv_timeout(Duration::from_secs(5));

        if let Ok(mut handle_guard) = self.thread_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                let _ = handle.join();
            }
        }

        Ok(())
    }
}

impl Drop for SqliteConnection {
    fn drop(&mut self) {
        // Automatically close when the last reference drops
        if Arc::strong_count(&self.is_closed) == 1 && !self.is_closed() {
            let _ = self.close();
        }
    }
}
