use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PostgreSQL protocol error: {0}")]
    PgProtocol(String),

    #[error("PostgreSQL server error: {code} - {message}")]
    PgServerError { code: String, message: String },

    #[error("Redis protocol error: {0}")]
    RedisProtocol(String),

    #[error("Redis server error: {0}")]
    RedisServerError(String),

    #[error("Connection pool exhausted: max {0} connections active")]
    PoolExhausted(usize),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Invalid connection URL: {0}")]
    InvalidUrl(String),

    #[error("Serialization/Deserialization error: {0}")]
    Serialization(String),

    #[error("SQLite error: {0}")]
    Sqlite(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        DbError::Sqlite(err.to_string())
    }
}
