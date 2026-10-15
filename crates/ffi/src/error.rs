use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum FfiError {
    #[error("Foreign module '{0}' not found in FFI registry")]
    ModuleNotFound(String),

    #[error("Foreign method '{method}' not found on module '{module}'")]
    MethodNotFound { module: String, method: String },

    #[error("Invalid argument for foreign call: {0}")]
    InvalidArgument(String),

    #[error("Worker pool error: {0}")]
    WorkerPoolError(String),

    #[error("External handle error: {0}")]
    HandleError(String),

    #[error("JSON serialization/deserialization error: {0}")]
    JsonError(String),

    #[error("Memory budget exceeded during foreign marshaling: {0}")]
    MemoryBudgetExceeded(String),

    #[error("Native execution error: {0}")]
    NativeError(String),
}
