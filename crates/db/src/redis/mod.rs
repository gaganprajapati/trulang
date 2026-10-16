pub mod connection;
pub mod mock_server;
pub mod pipeline;
pub mod pool;
pub mod protocol;

pub use connection::RedisConnection;
pub use mock_server::MockRedisServer;
pub use pipeline::RedisPipeline;
pub use pool::{PooledRedisConnection, RedisConfig, RedisPool};
pub use protocol::RespValue;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redis_wire_protocol_end_to_end_mock() {
        let server = MockRedisServer::start();
        let pool = RedisPool::new(&server.url(), 4).expect("Connect to mock redis");

        // 1. PING
        let pong = pool.ping().expect("Ping failed");
        assert_eq!(
            pong,
            trulang_compiler::bytecode::Value::String(std::sync::Arc::new("PONG".to_string()))
        );

        // 2. SET and GET
        let set_res = pool.set("user:100", "Alice", None).expect("Set failed");
        assert_eq!(set_res, trulang_compiler::bytecode::Value::Bool(true));

        let get_res = pool.get("user:100").expect("Get failed");
        assert_eq!(
            get_res,
            trulang_compiler::bytecode::Value::String(std::sync::Arc::new("Alice".to_string()))
        );

        // 3. INCR
        let incr_res1 = pool.incr("counter").expect("Incr failed");
        assert_eq!(incr_res1, trulang_compiler::bytecode::Value::Int(1));
        let incr_res2 = pool.incr("counter").expect("Incr failed");
        assert_eq!(incr_res2, trulang_compiler::bytecode::Value::Int(2));

        // 4. HSET and HGET
        let hset_res = pool
            .hset("session:abc", "token", "xyz123")
            .expect("Hset failed");
        assert_eq!(hset_res, trulang_compiler::bytecode::Value::Bool(true));

        let hget_res = pool.hget("session:abc", "token").expect("Hget failed");
        assert_eq!(
            hget_res,
            trulang_compiler::bytecode::Value::String(std::sync::Arc::new("xyz123".to_string()))
        );

        // 5. DEL
        let del_res = pool.del(&["user:100", "counter"]).expect("Del failed");
        assert_eq!(del_res, trulang_compiler::bytecode::Value::Int(2));

        let get_after_del = pool.get("user:100").expect("Get failed");
        assert_eq!(get_after_del, trulang_compiler::bytecode::Value::Null);

        // 6. Pipeline
        let mut conn = pool.acquire().expect("Acquire conn for pipeline");
        let pipeline = RedisPipeline::new()
            .set("pipe:1", "first")
            .set("pipe:2", "second")
            .get("pipe:1")
            .get("pipe:2");
        let pipe_res = pipeline
            .execute(&mut conn)
            .expect("Pipeline execute failed");
        assert_eq!(pipe_res.len(), 4);
        assert_eq!(pipe_res[0], RespValue::SimpleString("OK".to_string()));
        assert_eq!(pipe_res[2], RespValue::BulkString(Some(b"first".to_vec())));
        assert_eq!(pipe_res[3], RespValue::BulkString(Some(b"second".to_vec())));
    }
}
