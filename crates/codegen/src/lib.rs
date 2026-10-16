//! TruLang Codegen: Native AOT & JIT Code Generation Backend.
//!
//! Provides dual-mode native code generation for TruLang:
//! 1. Production AOT Compilation (`trulang build [--release] file.tr [-o <out>]`):
//!    Generates optimized native C code preserving Region A bump arena and Region B
//!    persistent memory, invoked through host toolchains (clang, gcc, cc, or cl.exe)
//!    to produce standalone executables (Mach-O, ELF, PE) with 90-95%+ native performance.
//! 2. JIT Execution (`trulang run --jit file.tr`):
//!    On-the-fly native machine code compilation and execution.

pub mod builder;
pub mod c_backend;
pub mod jit;
pub mod runtime;

pub use builder::{CodegenConfig, CodegenError, NativeBuilder};
pub use c_backend::CBackend;
pub use jit::{JitEngine, JitResult};

#[cfg(test)]
mod tests {
    use super::*;
    use trulang_compiler::bytecode::{Chunk, Opcode};

    #[test]
    fn test_end_to_end_native_build_and_run() {
        let mut chunk = Chunk::new();
        // Calculate (15 * 4) - 20 = 40
        chunk.emit(Opcode::LoadInt(15), 1);
        chunk.emit(Opcode::LoadInt(4), 1);
        chunk.emit(Opcode::Mul, 1);
        chunk.emit(Opcode::LoadInt(20), 1);
        chunk.emit(Opcode::Sub, 1);
        chunk.emit(Opcode::Return, 1);

        let c_src = CBackend::emit(&chunk);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        static E2E_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = E2E_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_bin = std::env::temp_dir().join(format!(
            "trulang_e2e_test_{}_{}_{}",
            std::process::id(),
            nanos,
            count
        ));
        let config = CodegenConfig {
            release: true,
            output_path: Some(temp_bin.clone()),
        };

        let bin_path = NativeBuilder::build(&c_src, &config).expect("Native build must succeed");
        assert!(bin_path.exists());

        let out = std::process::Command::new(&bin_path)
            .output()
            .expect("Execution must succeed");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Program exited with value: 40"));

        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn test_native_loop_and_branching() {
        // Compute sum(1..=10) = 55 using while loop in bytecode
        // local 0: i = 1
        // local 1: sum = 0
        let mut chunk = Chunk::new();
        // local 0 = 1
        chunk.emit(Opcode::LoadInt(1), 1);
        chunk.emit(Opcode::SetLocal(0), 1);
        // local 1 = 0
        chunk.emit(Opcode::LoadInt(0), 1);
        chunk.emit(Opcode::SetLocal(1), 1);

        // loop_start: (offset 4)
        let loop_start = 4;
        chunk.emit(Opcode::GetLocal(0), 2);
        chunk.emit(Opcode::LoadInt(10), 2);
        chunk.emit(Opcode::LtEq, 2);
        let jump_exit = chunk.emit(Opcode::JumpIfFalse(0), 2); // patched later

        // sum = sum + i
        chunk.emit(Opcode::GetLocal(1), 3);
        chunk.emit(Opcode::GetLocal(0), 3);
        chunk.emit(Opcode::Add, 3);
        chunk.emit(Opcode::SetLocal(1), 3);

        // i = i + 1
        chunk.emit(Opcode::GetLocal(0), 4);
        chunk.emit(Opcode::LoadInt(1), 4);
        chunk.emit(Opcode::Add, 4);
        chunk.emit(Opcode::SetLocal(0), 4);

        // jump to loop_start
        chunk.emit(Opcode::Jump(loop_start), 5);

        // loop_exit:
        let loop_exit = chunk.code.len();
        chunk.patch_jump(jump_exit, loop_exit);

        // return sum
        chunk.emit(Opcode::GetLocal(1), 6);
        chunk.emit(Opcode::Return, 6);

        let res = JitEngine::execute(&chunk).expect("JIT loop should succeed");
        assert_eq!(res.exit_code, 0);
        assert!(res.stdout.contains("Program exited with value: 55"));
    }

    #[test]
    fn test_native_elastic_memory_and_persist() {
        let mut chunk = Chunk::new();
        // 1 KB base budget, 4 KB elastic ceiling
        chunk.emit(Opcode::AllocRegionA(1024, Some(4096)), 1);

        // NewMap, persist, and return
        chunk.emit(Opcode::NewMap, 2);
        chunk.emit(Opcode::Persist, 2);
        chunk.emit(Opcode::Return, 3);

        let res = JitEngine::execute(&chunk).expect("JIT execution should succeed");
        assert_eq!(res.exit_code, 0);
        assert!(res.stdout.contains("base: 1024 bytes, max: 4096 bytes"));
        assert!(res.stdout.contains("Region B (Persistent):"));
    }
}
