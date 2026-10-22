//! High-Performance Native Database Wire Protocol Engine for TruLang.
//!
//! Provides pure, zero-dependency async clients for:
//! - **PostgreSQL**: Frontend/Backend Protocol v3.0 with binary framing & connection pooling.
//! - **Redis**: RESP2/RESP3 sub-millisecond protocol with pipelining & connection pooling.
//! - **Dual-Region Memory Architecture**:
//!   - Region B: Long-lived connection pools, prepared statements, and sockets.
//!   - Region A: Transient per-query row decoders, wire frames, and column buffers.

pub mod dual_region;
pub mod error;
pub mod postgres;
pub mod redis;
pub mod sqlite;

pub use dual_region::DualRegionDbManager;
pub use error::DbError;
pub use postgres::{MockPostgresServer, PgConfig, PgConnection, PgPool};
pub use redis::{MockRedisServer, RedisConfig, RedisConnection, RedisPipeline, RedisPool};
pub use sqlite::SqliteConnection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dual_region_database_lifecycle() {
        let manager = DualRegionDbManager::new(128 * 1024 * 1024);

        // Simulate incoming database query result
        let mut items = Vec::new();
        for i in 0..100 {
            let row = trulang_compiler::bytecode::Value::new_object();
            if let trulang_compiler::bytecode::Value::Object(ref o) = row {
                let mut g = o.lock().unwrap();
                g.insert("id".to_string(), trulang_compiler::bytecode::Value::Int(i));
                g.insert(
                    "token".to_string(),
                    trulang_compiler::bytecode::Value::String(std::sync::Arc::new(format!(
                        "token_{}",
                        i
                    ))),
                );
            }
            items.push(row);
        }
        let query_result = trulang_compiler::bytecode::Value::Array(items);

        let allocated = manager.track_query_result(&query_result);
        assert!(allocated > 0);
        assert_eq!(manager.transient_bytes(), allocated);
        assert_eq!(manager.persistent_bytes(), 0);

        // Select user 0 to cache/persist in Region B
        let user_to_persist = match &query_result {
            trulang_compiler::bytecode::Value::Array(items) => items[0].clone(),
            _ => panic!("Expected array"),
        };
        let promoted_bytes = manager.promote_to_persistent(&user_to_persist);
        assert!(promoted_bytes > 0);
        assert_eq!(manager.persistent_bytes(), promoted_bytes);
        assert_eq!(manager.transient_bytes(), allocated - promoted_bytes);

        // End of request lifecycle: O(1) bulk reset of Region A
        manager.reset_request_arena();
        assert_eq!(
            manager.transient_bytes(),
            0,
            "Transient query bytes must be reset to 0"
        );
        assert_eq!(
            manager.persistent_bytes(),
            promoted_bytes,
            "Persistent Region B bytes must be preserved after Region A arena reset"
        );
    }
}
