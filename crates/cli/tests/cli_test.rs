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

#[test]
fn test_cli_test_command() {
    let test_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("tests")
        .join("math_test.tr");

    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["test", test_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang test");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("running 3 test(s)"));
    assert!(stdout.contains("3 passed; 0 failed"));
}

#[test]
fn test_cli_test_filter() {
    let test_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("tests")
        .join("math_test.tr");

    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["test", test_path.to_str().unwrap(), "--filter", "factorial"])
        .output()
        .expect("Failed to run trulang test with filter");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("running 1 test(s)"));
    assert!(stdout.contains("test factorial computation"));
    assert!(stdout.contains("1 passed; 0 failed"));
}

#[test]
fn test_cli_serve_command() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let gateway_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("http_api_gateway.tr");

    // Pick an available port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut child = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args([
            "serve",
            gateway_path.to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--host",
            "127.0.0.1",
        ])
        .spawn()
        .expect("Failed to start trulang serve");

    // Allow time for server to bind and start listening
    let mut connected = false;
    let mut client = None;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Ok(c) = TcpStream::connect(format!("127.0.0.1:{}", port)) {
            client = Some(c);
            connected = true;
            break;
        }
    }

    assert!(connected, "Failed to connect to trulang serve on port {}", port);
    let mut stream = client.unwrap();

    stream
        .write_all(b"GET /api/v1/users?id=cli_test_user HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("Write to trulang serve failed");

    let mut response_buf = Vec::new();
    stream.read_to_end(&mut response_buf).expect("Read from trulang serve failed");
    let resp = String::from_utf8(response_buf).expect("Valid UTF-8 response");

    assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(resp.contains(r#""status":"success""#));
    assert!(resp.contains(r#""user":"cli_test_user""#));

    // Terminate child process
    let _ = child.kill();
    let _ = child.wait();
}
