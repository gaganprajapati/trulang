pub mod connection;
pub mod mock_server;
pub mod pool;
pub mod protocol;

pub use connection::{
    escape_pg_value, format_parameterized_query, PgConnection, PgExecuteResult, PgQueryResult,
};
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

        // Test parameterized query_one
        let user1 = pool
            .query_one(
                "SELECT id, name FROM users WHERE id = $1",
                &[trulang_compiler::bytecode::Value::Int(1)],
            )
            .expect("query_one should succeed");
        assert!(user1.is_some());
        if let Some(trulang_compiler::bytecode::Value::Object(ref obj)) = user1 {
            let g = obj.lock().unwrap();
            assert_eq!(
                g.get("id"),
                Some(&trulang_compiler::bytecode::Value::Int(1))
            );
        } else {
            panic!("Expected user object");
        }

        // Test parameterized execute
        let exec_p = pool
            .execute_params(
                "INSERT INTO users (id, name) VALUES ($1, $2)",
                &[
                    trulang_compiler::bytecode::Value::Int(4),
                    trulang_compiler::bytecode::Value::String(std::sync::Arc::new(
                        "Dave".to_string(),
                    )),
                ],
            )
            .expect("execute_params should succeed");
        if let trulang_compiler::bytecode::Value::Object(ref o) = exec_p {
            let g = o.lock().unwrap();
            assert_eq!(
                g.get("rows_affected"),
                Some(&trulang_compiler::bytecode::Value::Int(1))
            );
        }
    }

    #[test]
    fn test_format_parameterized_query() {
        use trulang_compiler::bytecode::Value;

        let sql = "SELECT * FROM users WHERE id = $1 AND name = $2 AND active = $3 AND age > $4";
        let params = vec![
            Value::Int(42),
            Value::String(std::sync::Arc::new("O'Connor".to_string())),
            Value::Bool(true),
            Value::Float(18.5),
        ];

        let formatted = format_parameterized_query(sql, &params).unwrap();
        assert_eq!(
            formatted,
            "SELECT * FROM users WHERE id = 42 AND name = 'O''Connor' AND active = TRUE AND age > 18.5"
        );

        // String with quote containing dollar sign should be preserved
        let sql2 = "SELECT '$1' FROM users WHERE id = $1";
        let params2 = vec![Value::Int(99)];
        let formatted2 = format_parameterized_query(sql2, &params2).unwrap();
        assert_eq!(formatted2, "SELECT '$1' FROM users WHERE id = 99");

        // Out of range parameter should error
        let sql_err = "SELECT * FROM users WHERE id = $2";
        assert!(format_parameterized_query(sql_err, &params2).is_err());
    }
}
