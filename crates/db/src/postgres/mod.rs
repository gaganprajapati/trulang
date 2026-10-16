pub mod connection;
pub mod mock_server;
pub mod pool;
pub mod protocol;

pub use connection::{PgConnection, PgExecuteResult, PgQueryResult};
pub use mock_server::MockPostgresServer;
pub use pool::{decode_query_result_to_trulang, PgConfig, PgPool, PooledPgConnection};
pub use protocol::{BackendMessage, DataRow, FieldDescription};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_wire_protocol_end_to_end_mock() {
        let server = MockPostgresServer::start();
        let pool = PgPool::new(&server.url(), 4).expect("Connect to mock pg server");

        let res = pool
            .query("SELECT id, name, email, active FROM users")
            .expect("Query failed");
        if let trulang_compiler::bytecode::Value::Array(rows) = res {
            assert_eq!(rows.len(), 2);
            if let trulang_compiler::bytecode::Value::Object(ref row0) = rows[0] {
                let g = row0.lock().unwrap();
                assert_eq!(
                    g.get("id"),
                    Some(&trulang_compiler::bytecode::Value::Int(1))
                );
                assert_eq!(
                    g.get("active"),
                    Some(&trulang_compiler::bytecode::Value::Bool(true))
                );
            } else {
                panic!("Expected Object row");
            }
        } else {
            panic!("Expected Array of rows");
        }

        let exec_res = pool
            .execute("INSERT INTO users VALUES (3, 'Charlie')")
            .expect("Execute failed");
        if let trulang_compiler::bytecode::Value::Object(ref o) = exec_res {
            let g = o.lock().unwrap();
            assert_eq!(
                g.get("rows_affected"),
                Some(&trulang_compiler::bytecode::Value::Int(1))
            );
            assert_eq!(
                g.get("ok"),
                Some(&trulang_compiler::bytecode::Value::Bool(true))
            );
        }

        let stats = pool.stats();
        if let trulang_compiler::bytecode::Value::Object(ref s) = stats {
            let g = s.lock().unwrap();
            assert_eq!(
                g.get("max_connections"),
                Some(&trulang_compiler::bytecode::Value::Int(4))
            );
        }
    }
}
