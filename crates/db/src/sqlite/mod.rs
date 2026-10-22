//! Embedded SQLite Native Driver for TruLang.
//!
//! Utilizes an Actor Mailbox pattern with a dedicated background OS worker thread
//! per database and bounded crossbeam channels to guarantee zero blocking of
//! the async event loop and serial execution without `SQLITE_BUSY` errors.

pub mod connection;
pub mod worker;

pub use connection::SqliteConnection;
pub use worker::{SqliteTask, SqliteWorker};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use trulang_compiler::bytecode::Value;

    #[test]
    fn test_sqlite_in_memory_crud() {
        let conn = SqliteConnection::open_in_memory().expect("Open in-memory sqlite");

        // 1. Create table
        conn.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, score REAL, active INTEGER)",
            &[],
        )
        .expect("Create table failed");

        // 2. Insert rows using parameters
        let res1 = conn
            .execute(
                "INSERT INTO users (id, name, score, active) VALUES (?, ?, ?, ?)",
                &[
                    Value::Int(1),
                    Value::String(Arc::new("Alice".to_string())),
                    Value::Float(95.5),
                    Value::Bool(true),
                ],
            )
            .expect("Insert 1 failed");

        if let Value::Object(ref o) = res1 {
            let g = o.lock().unwrap();
            assert_eq!(g.get("rows_affected"), Some(&Value::Int(1)));
            assert_eq!(g.get("ok"), Some(&Value::Bool(true)));
        }

        let res2 = conn
            .execute(
                "INSERT INTO users (id, name, score, active) VALUES (?, ?, ?, ?)",
                &[
                    Value::Int(2),
                    Value::String(Arc::new("Bob".to_string())),
                    Value::Float(82.0),
                    Value::Bool(false),
                ],
            )
            .expect("Insert 2 failed");
        if let Value::Object(ref o) = res2 {
            let g = o.lock().unwrap();
            assert_eq!(g.get("rows_affected"), Some(&Value::Int(1)));
        }

        // 3. Query all users
        let rows = conn
            .query(
                "SELECT id, name, score, active FROM users ORDER BY id ASC",
                &[],
            )
            .expect("Query failed");

        if let Value::Array(list) = rows {
            assert_eq!(list.len(), 2);

            // Verify Alice
            if let Value::Object(ref obj) = list[0] {
                let g = obj.lock().unwrap();
                assert_eq!(g.get("id"), Some(&Value::Int(1)));
                assert_eq!(
                    g.get("name"),
                    Some(&Value::String(Arc::new("Alice".to_string())))
                );
                assert_eq!(g.get("score"), Some(&Value::Float(95.5)));
                assert_eq!(g.get("active"), Some(&Value::Int(1)));
            } else {
                panic!("Expected Object row");
            }

            // Verify Bob
            if let Value::Object(ref obj) = list[1] {
                let g = obj.lock().unwrap();
                assert_eq!(g.get("id"), Some(&Value::Int(2)));
                assert_eq!(
                    g.get("name"),
                    Some(&Value::String(Arc::new("Bob".to_string())))
                );
            }
        } else {
            panic!("Expected Array of rows");
        }

        // 4. Query one user
        let user1 = conn
            .query_one("SELECT id, name FROM users WHERE id = ?", &[Value::Int(1)])
            .expect("query_one failed");
        assert!(user1.is_some());
        if let Some(Value::Object(ref obj)) = user1 {
            let g = obj.lock().unwrap();
            assert_eq!(g.get("id"), Some(&Value::Int(1)));
            assert_eq!(
                g.get("name"),
                Some(&Value::String(Arc::new("Alice".to_string())))
            );
        }

        // 5. Query non-existent user returns None
        let user99 = conn
            .query_one("SELECT id, name FROM users WHERE id = ?", &[Value::Int(99)])
            .expect("query_one non-existent");
        assert!(user99.is_none());

        // 6. Checkpoint and Close
        conn.checkpoint().expect("Checkpoint failed");
        conn.close().expect("Close failed");
        assert!(conn.is_closed());

        // Subsequent operations after close must fail with ConnectionClosed
        assert!(conn.query("SELECT 1", &[]).is_err());
    }

    #[test]
    fn test_sqlite_file_wal_checkpoint() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("trulang_test_{}.db", std::process::id()));
        let db_str = db_path.to_str().unwrap();

        let conn = SqliteConnection::open(db_str).expect("Open file sqlite");
        conn.execute("CREATE TABLE test_wal (k TEXT, v TEXT)", &[])
            .expect("Create table failed");

        for i in 0..50 {
            conn.execute(
                "INSERT INTO test_wal VALUES (?, ?)",
                &[
                    Value::String(Arc::new(format!("key_{}", i))),
                    Value::String(Arc::new(format!("val_{}", i))),
                ],
            )
            .expect("Insert failed");
        }

        // Verify WAL checkpoint
        conn.checkpoint().expect("Checkpoint failed");

        let count_row = conn
            .query_one("SELECT COUNT(*) as cnt FROM test_wal", &[])
            .expect("Count query")
            .expect("Count row");

        if let Value::Object(ref o) = count_row {
            let g = o.lock().unwrap();
            assert_eq!(g.get("cnt"), Some(&Value::Int(50)));
        }

        conn.close().expect("Close failed");

        // Clean up temp file
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_str));
        let _ = std::fs::remove_file(format!("{}-shm", db_str));
    }
}
