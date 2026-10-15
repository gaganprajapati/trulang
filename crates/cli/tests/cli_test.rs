use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn get_example_fibonacci_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("fibonacci.tr")
}

#[test]
fn test_cli_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .arg("version")
        .output()
        .expect("Failed to run trulang version");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trulang 0.1.0"));
}

#[test]
fn test_cli_check_valid_file() {
    let fib_path = get_example_fibonacci_path();
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["check", fib_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang check");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Check passed"));
}

#[test]
fn test_cli_run_vm_mode() {
    let fib_path = get_example_fibonacci_path();
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["run", fib_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Program exited with value: 55"));
    assert!(stdout.contains("Region A (Execution):"));
}

#[test]
fn test_cli_run_jit_mode() {
    let fib_path = get_example_fibonacci_path();
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["run", "--jit", fib_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang run --jit");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Program exited with value: 55"));
    assert!(stdout.contains("Native JIT Execution completed in"));
}

#[test]
fn test_cli_build_and_execute_standalone() {
    let fib_path = get_example_fibonacci_path();
    let temp_dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let out_bin = temp_dir.join(format!("trulang_cli_test_bin_{}", nanos));

    // 1. Build standalone executable
    let build_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["build", "--release", fib_path.to_str().unwrap(), "-o", out_bin.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang build");
    assert!(build_out.status.success());
    assert!(out_bin.exists());

    // 2. Run standalone binary directly without trulang
    let run_out = Command::new(&out_bin)
        .output()
        .expect("Failed to execute compiled standalone binary");
    assert!(run_out.status.success());
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    assert!(stdout.contains("Program exited with value: 55"));
    assert!(stdout.contains("Region A (Execution):"));

    let _ = fs::remove_file(&out_bin);
}

#[test]
fn test_cli_build_emit_c() {
    let fib_path = get_example_fibonacci_path();
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["build", "--emit-c", fib_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang build --emit-c");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trulang_region_a_init"));
    assert!(stdout.contains("int main(int argc, char** argv)"));
}
