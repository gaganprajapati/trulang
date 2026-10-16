//! JIT Engine for TruLang.
//!
//! Provides on-the-fly compilation and execution of TruLang chunks,
//! delivering near-instant execution with native machine code speed.

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};
use trulang_compiler::bytecode::Chunk;
use crate::builder::{CodegenConfig, CodegenError, NativeBuilder};
use crate::c_backend::CBackend;

#[derive(Debug, Clone)]
pub struct JitResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

pub struct JitEngine;

impl JitEngine {
    /// Compiles and runs a TruLang Chunk with native JIT execution.
    pub fn execute(chunk: &Chunk) -> Result<JitResult, CodegenError> {
        let c_source = CBackend::emit(chunk);

        let temp_dir = std::env::temp_dir();
        static JIT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = JIT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_bin = temp_dir.join(format!("trulang_jit_{}_{}_{}", std::process::id(), timestamp, count));

        let config = CodegenConfig {
            release: false,
            output_path: Some(temp_bin.clone()),
        };

        // Compile to native binary
        let bin_path = NativeBuilder::build(&c_source, &config)?;

        // Execute natively
        let start = Instant::now();
        let output = Command::new(&bin_path)
            .output()
            .map_err(CodegenError::IoError)?;
        let duration = start.elapsed();

        // Clean up temporary binary
        let _ = fs::remove_file(&bin_path);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(0);

        Ok(JitResult {
            exit_code,
            stdout,
            stderr,
            duration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trulang_compiler::bytecode::Opcode;

    #[test]
    fn test_jit_execute_arithmetic() {
        let mut chunk = Chunk::new();
        chunk.emit(Opcode::LoadInt(100), 1);
        chunk.emit(Opcode::LoadInt(250), 2);
        chunk.emit(Opcode::Add, 3);
        chunk.emit(Opcode::Return, 4);

        let res = JitEngine::execute(&chunk).expect("JIT execution should succeed");
        assert_eq!(res.exit_code, 0);
        assert!(res.stdout.contains("Program exited with value: 350"));
        assert!(res.stdout.contains("Region A (Execution):"));
    }
}
