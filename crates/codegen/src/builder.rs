//! Native Binary Builder for TruLang.
//!
//! Discovers the host C compiler (clang, gcc, cc, or cl.exe) and compiles
//! generated native C source code into standalone executable binaries.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

static BUILD_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Error)]
pub enum CodegenError {
    #[error("No supported C compiler (clang, gcc, cc, or cl) found in PATH")]
    CompilerNotFound,
    #[error("Failed to write temporary source file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Native compilation failed with exit code {code}:\n{stderr}")]
    CompilationFailed { code: i32, stderr: String },
    #[error("Linker failed to produce output binary at {0}")]
    LinkerFailed(String),
}

#[derive(Debug, Clone, Default)]
pub struct CodegenConfig {
    pub release: bool,
    pub output_path: Option<PathBuf>,
}

pub struct NativeBuilder;

impl NativeBuilder {
    /// Detects available host C compiler in PATH or environment.
    pub fn detect_compiler() -> Result<String, CodegenError> {
        if let Ok(cc_env) = std::env::var("CC") {
            if Self::probe_compiler(&cc_env) {
                return Ok(cc_env);
            }
        }

        let candidates = ["clang", "gcc", "cc", "cl"];
        for candidate in candidates {
            if Self::probe_compiler(candidate) {
                return Ok(candidate.to_string());
            }
        }

        Err(CodegenError::CompilerNotFound)
    }

    fn probe_compiler(cmd: &str) -> bool {
        Command::new(cmd)
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    /// Compiles native C source code into a standalone executable binary.
    pub fn build(c_source: &str, config: &CodegenConfig) -> Result<PathBuf, CodegenError> {
        let compiler = Self::detect_compiler()?;
        let count = BUILD_COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let out_path = if let Some(ref path) = config.output_path {
            path.clone()
        } else {
            let mut p = std::env::current_dir().map_err(CodegenError::IoError)?;
            #[cfg(target_os = "windows")]
            p.push(format!("trulang_out_{}_{}.exe", std::process::id(), count));
            #[cfg(not(target_os = "windows"))]
            p.push(format!("trulang_out_{}_{}", std::process::id(), count));
            p
        };

        // Write C source to unique temporary file
        let temp_dir = std::env::temp_dir();
        let temp_src = temp_dir.join(format!(
            "trulang_src_{}_{}_{}.c",
            std::process::id(),
            count,
            nanos
        ));
        fs::write(&temp_src, c_source).map_err(CodegenError::IoError)?;

        let mut cmd = Command::new(&compiler);
        cmd.arg(&temp_src);
        cmd.arg("-o").arg(&out_path);

        if config.release {
            cmd.arg("-O3");
            cmd.arg("-DNDEBUG");
            #[cfg(not(target_os = "windows"))]
            {
                cmd.arg("-fomit-frame-pointer");
            }
        } else {
            cmd.arg("-O0");
            cmd.arg("-g");
        }

        #[cfg(not(target_os = "windows"))]
        {
            cmd.arg("-lm"); // Math library
        }

        let output = cmd.output().map_err(CodegenError::IoError)?;
        let _ = fs::remove_file(&temp_src);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(1);
            return Err(CodegenError::CompilationFailed { code, stderr });
        }

        if !out_path.exists() {
            return Err(CodegenError::LinkerFailed(out_path.display().to_string()));
        }

        Ok(out_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_compiler() {
        let compiler = NativeBuilder::detect_compiler();
        assert!(compiler.is_ok(), "Host compiler must be detected on system");
    }

    #[test]
    fn test_build_minimal_program() {
        let minimal_c = r#"
#include <stdio.h>
int main(void) {
    printf("Hello Native TruLang\n");
    return 0;
}
"#;
        let count = BUILD_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_bin =
            std::env::temp_dir().join(format!("test_bin_{}_{}", std::process::id(), count));
        let config = CodegenConfig {
            release: true,
            output_path: Some(temp_bin.clone()),
        };
        let res = NativeBuilder::build(minimal_c, &config);
        assert!(res.is_ok());
        let bin_path = res.unwrap();
        assert!(bin_path.exists());

        // Execute binary and verify output
        let out = Command::new(&bin_path)
            .output()
            .expect("Failed to execute built binary");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Hello Native TruLang"));

        let _ = fs::remove_file(&bin_path);
    }
}
