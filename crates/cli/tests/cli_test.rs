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
    assert!(stdout.contains("trulang 1.0.0"));
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
        .args([
            "build",
            "--release",
            fib_path.to_str().unwrap(),
            "-o",
            out_bin.to_str().unwrap(),
        ])
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

    // Pick an available ephemeral port
    let port = 50000
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 10000) as u16;

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
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Ok(c) = TcpStream::connect(format!("127.0.0.1:{}", port)) {
            client = Some(c);
            connected = true;
            break;
        }
    }

    assert!(
        connected,
        "Failed to connect to trulang serve on port {}",
        port
    );
    let mut stream = client.unwrap();

    stream
        .write_all(b"GET /api/v1/users?id=cli_test_user HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("Write to trulang serve failed");

    let mut response_buf = Vec::new();
    stream
        .read_to_end(&mut response_buf)
        .expect("Read from trulang serve failed");
    let resp = String::from_utf8(response_buf).expect("Valid UTF-8 response");

    assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(resp.contains(r#""status":"success""#));
    assert!(resp.contains(r#""user":"cli_test_user""#));
    assert!(resp.to_ascii_lowercase().contains("traceparent: 00-"));

    // Verify /metrics endpoint
    let mut metrics_stream =
        TcpStream::connect(format!("127.0.0.1:{}", port)).expect("Connect metrics failed");
    metrics_stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("Write to /metrics failed");
    let mut metrics_buf = Vec::new();
    metrics_stream
        .read_to_end(&mut metrics_buf)
        .expect("Read /metrics failed");
    let metrics_resp = String::from_utf8_lossy(&metrics_buf);
    assert!(metrics_resp.contains("trulang_http_requests_total"));
    assert!(metrics_resp.contains("trulang_uptime_seconds"));

    // Verify /healthz endpoint
    let mut healthz_stream =
        TcpStream::connect(format!("127.0.0.1:{}", port)).expect("Connect healthz failed");
    healthz_stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("Write to /healthz failed");
    let mut healthz_buf = Vec::new();
    healthz_stream
        .read_to_end(&mut healthz_buf)
        .expect("Read /healthz failed");
    let healthz_resp = String::from_utf8_lossy(&healthz_buf);
    assert!(healthz_resp.contains(r#"{"status":"ok""#));

    // Terminate child process
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_cli_pkg_lifecycle() {
    let base_temp = std::env::temp_dir().join(format!(
        "trulang_cli_pkg_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&base_temp).unwrap();

    let project_dir = base_temp.join("my_microservice");

    // 1. trulang new my_microservice
    let new_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args([
            "new",
            "my_microservice",
            "--path",
            project_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute trulang new");
    assert!(
        new_out.status.success(),
        "trulang new failed: {}",
        String::from_utf8_lossy(&new_out.stderr)
    );
    assert!(project_dir.join("Tru.toml").exists());
    assert!(project_dir.join("src").join("main.tr").exists());
    assert!(project_dir.join("src").join("main_test.tr").exists());

    // 2. trulang test on the newly scaffolded project
    let test_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["test", project_dir.to_str().unwrap()])
        .output()
        .expect("Failed to execute trulang test on scaffold");
    assert!(
        test_out.status.success(),
        "trulang test failed: {}",
        String::from_utf8_lossy(&test_out.stderr)
    );
    let test_str = String::from_utf8_lossy(&test_out.stdout);
    assert!(
        test_str.contains("passed"),
        "Expected tests to pass in scaffolded project"
    );

    // 3. Create a local library package and use trulang add
    let lib_dir = base_temp.join("math_lib");
    let lib_src = lib_dir.join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(
        lib_dir.join("Tru.toml"),
        "[package]\nname = \"math_lib\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        lib_src.join("lib.tr"),
        "fn calculate(factor: i64) -> i64 {\n    return factor * 10;\n}\n",
    )
    .unwrap();

    let add_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .current_dir(&project_dir)
        .args(["add", "math_lib", "--path", lib_dir.to_str().unwrap()])
        .output()
        .expect("Failed to execute trulang add");
    assert!(
        add_out.status.success(),
        "trulang add failed: {}",
        String::from_utf8_lossy(&add_out.stderr)
    );
    assert!(project_dir.join("Tru.lock").exists());

    // 4. Update src/main.tr to import and use math_lib
    let user_code = r#"
import "math_lib" as math;

fn main() -> i64 {
    let result = math.calculate(5);
    println("Calculated result:");
    println(result);
    return result;
}
"#;
    fs::write(project_dir.join("src").join("main.tr"), user_code).unwrap();

    let run_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .current_dir(&project_dir)
        .args(["run", "src/main.tr"])
        .output()
        .expect("Failed to execute trulang run with imported package");
    assert!(
        run_out.status.success(),
        "trulang run failed: {}",
        String::from_utf8_lossy(&run_out.stderr)
    );
    let run_str = String::from_utf8_lossy(&run_out.stdout);
    assert!(
        run_str.contains("Program exited with value: 50"),
        "Expected 50 from math_lib, got: {}",
        run_str
    );

    // 5. trulang install (re-verifies lockfile integrity)
    let install_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .current_dir(&project_dir)
        .arg("install")
        .output()
        .expect("Failed to execute trulang install");
    assert!(
        install_out.status.success(),
        "trulang install failed: {}",
        String::from_utf8_lossy(&install_out.stderr)
    );

    // 6. trulang publish
    let pub_out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .current_dir(&project_dir)
        .arg("publish")
        .output()
        .expect("Failed to execute trulang publish");
    assert!(
        pub_out.status.success(),
        "trulang publish failed: {}",
        String::from_utf8_lossy(&pub_out.stderr)
    );
    let pub_str = String::from_utf8_lossy(&pub_out.stdout);
    assert!(pub_str.contains("ready for distribution"));
    assert!(pub_str.contains("Cryptographic Integrity (SHA-256)"));

    let _ = fs::remove_dir_all(&base_temp);
}

#[test]
fn test_cli_check_database_demo() {
    let demo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("database_demo.tr");
    let out = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args(["check", demo_path.to_str().unwrap()])
        .output()
        .expect("Failed to run trulang check");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Check passed"));
}

#[test]
fn test_cli_profile_command() {
    let fib_path = get_example_fibonacci_path();
    let temp_dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let svg_out = temp_dir.join(format!("profile_test_{}.svg", nanos));
    let json_out = temp_dir.join(format!("profile_test_{}.json", nanos));
    let text_out = temp_dir.join(format!("profile_test_{}.txt", nanos));

    // 1. Profile in SVG format (default)
    let out_svg = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args([
            "profile",
            fib_path.to_str().unwrap(),
            "-o",
            svg_out.to_str().unwrap(),
            "-f",
            "svg",
        ])
        .output()
        .expect("Failed to run trulang profile svg");
    assert!(out_svg.status.success());
    assert!(svg_out.exists());
    let svg_content = fs::read_to_string(&svg_out).expect("Read svg failed");
    assert!(svg_content.contains("<svg"));
    assert!(svg_content.contains("TruLang 1.0 Visual Memory Flamegraph"));
    assert!(svg_content.contains("REGION A (BUMP)"));

    // 2. Profile in JSON format
    let out_json = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args([
            "profile",
            fib_path.to_str().unwrap(),
            "-o",
            json_out.to_str().unwrap(),
            "-f",
            "json",
        ])
        .output()
        .expect("Failed to run trulang profile json");
    assert!(out_json.status.success());
    assert!(json_out.exists());
    let json_content = fs::read_to_string(&json_out).expect("Read json failed");
    assert!(json_content.contains("\"total_region_a_bytes\""));

    // 3. Profile in folded text format
    let out_txt = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .args([
            "profile",
            fib_path.to_str().unwrap(),
            "-o",
            text_out.to_str().unwrap(),
            "-f",
            "text",
        ])
        .output()
        .expect("Failed to run trulang profile text");
    assert!(out_txt.status.success());
    assert!(text_out.exists());

    let _ = fs::remove_file(svg_out);
    let _ = fs::remove_file(json_out);
    let _ = fs::remove_file(text_out);
}

#[test]
fn test_cli_dap_command() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_trulang"))
        .arg("dap")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn trulang dap");

    let mut stdin = child.stdin.take().expect("Child stdin");
    let stdout = child.stdout.take().expect("Child stdout");
    let mut reader = BufReader::new(stdout);

    // Send initialize request
    let init_req = serde_json::json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "vscode",
            "adapterID": "trulang"
        }
    })
    .to_string();
    let init_framed = format!("Content-Length: {}\r\n\r\n{}", init_req.len(), init_req);
    stdin
        .write_all(init_framed.as_bytes())
        .expect("Write init failed");
    stdin.flush().expect("Flush init failed");

    // Read initialize response
    let mut line = String::new();
    let mut content_len = 0;
    while reader.read_line(&mut line).unwrap() > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_len = rest.trim().parse::<usize>().unwrap();
        }
        line.clear();
    }
    assert!(content_len > 0);
    let mut body = vec![0u8; content_len];
    std::io::Read::read_exact(&mut reader, &mut body).unwrap();
    let resp_str = String::from_utf8(body).unwrap();
    assert!(resp_str.contains("\"command\":\"initialize\""));
    assert!(resp_str.contains("\"success\":true"));

    // Send disconnect request
    let disc_req = serde_json::json!({
        "seq": 2,
        "type": "request",
        "command": "disconnect"
    })
    .to_string();
    let disc_framed = format!("Content-Length: {}\r\n\r\n{}", disc_req.len(), disc_req);
    stdin
        .write_all(disc_framed.as_bytes())
        .expect("Write disc failed");
    stdin.flush().expect("Flush disc failed");

    let status = child.wait().expect("Child wait failed");
    assert!(status.success());
}
