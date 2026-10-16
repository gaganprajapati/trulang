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

pub use dual_region::DualRegionDbManager;
pub use error::DbError;
pub use postgres::{MockPostgresServer, PgConfig, PgConnection, PgPool};
pub use redis::{MockRedisServer, RedisConfig, RedisConnection, RedisPipeline, RedisPool};

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
        assert_eq!(
            manager
                .live_query_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            allocated
        );

        // End of request lifecycle: O(1) bulk reset
        manager.reset_request_arena();
        assert_eq!(
            manager
                .live_query_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            0,
            "Transient query bytes must be reset to 0"
        );
    }
}
