//! In-Process PostgreSQL v3.0 Wire Protocol Mock Server.
//!
//! Provides a zero-dependency, deterministic TCP server speaking pure
//! PostgreSQL wire protocol for integration testing and verification.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub struct MockPostgresServer {
    pub addr: SocketAddr,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl MockPostgresServer {
    /// Launches an in-process mock PostgreSQL TCP server on an ephemeral port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Bind mock pg server");
        let addr = listener.local_addr().unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let thread_handle = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();

            while running_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            handle_client(stream);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            addr,
            running,
            thread_handle: Some(thread_handle),
        }
    }

    /// Returns the connection string for this mock server.
    pub fn url(&self) -> String {
        format!("postgres://postgres@{}", self.addr)
    }

    /// Stops the mock server and joins the listener thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for MockPostgresServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_client(mut stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));

    // 1. Read StartupMessage
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        return;
    }
    let total_len = i32::from_be_bytes(len_buf);
    if total_len > 4 {
        let mut rest = vec![0u8; (total_len - 4) as usize];
        let _ = stream.read_exact(&mut rest);
    }

    // 2. Send AuthenticationOk ('R', 0)
    let auth_ok = [b'R', 0, 0, 0, 8, 0, 0, 0, 0];
    if stream.write_all(&auth_ok).is_err() {
        return;
    }

    // 3. Send ParameterStatus
    send_parameter_status(&mut stream, "server_version", "16.0 (TruLang Wire)");
    send_parameter_status(&mut stream, "client_encoding", "UTF8");

    // 4. Send BackendKeyData
    let mut key_data = vec![b'K', 0, 0, 0, 12];
    key_data.extend_from_slice(&12345u32.to_be_bytes()); // PID
    key_data.extend_from_slice(&67890u32.to_be_bytes()); // Secret
    let _ = stream.write_all(&key_data);

    // 5. Send ReadyForQuery ('Z', 'I')
    let ready = [b'Z', 0, 0, 0, 5, b'I'];
    let _ = stream.write_all(&ready);
    let _ = stream.flush();

    // 6. Query Processing Loop
    loop {
        let mut type_buf = [0u8; 1];
        if stream.read_exact(&mut type_buf).is_err() {
            break;
        }

        match type_buf[0] {
            b'Q' => {
                let mut q_len_buf = [0u8; 4];
                if stream.read_exact(&mut q_len_buf).is_err() {
                    break;
                }
                let q_len = i32::from_be_bytes(q_len_buf);
                let mut q_bytes = vec![0u8; (q_len - 4) as usize];
                if stream.read_exact(&mut q_bytes).is_err() {
                    break;
                }
                let sql = String::from_utf8_lossy(&q_bytes)
                    .trim_matches('\0')
                    .to_string();

                if sql.contains("/* ping */") {
                    let _ = stream.write_all(&[b'I', 0, 0, 0, 4]); // EmptyQueryResponse
                    let _ = stream.write_all(&ready);
                    let _ = stream.flush();
                } else if sql.to_uppercase().starts_with("INSERT") {
                    send_command_complete(&mut stream, "INSERT 0 1");
                    let _ = stream.write_all(&ready);
                    let _ = stream.flush();
                } else if sql.to_uppercase().starts_with("UPDATE") {
                    send_command_complete(&mut stream, "UPDATE 2");
                    let _ = stream.write_all(&ready);
                    let _ = stream.flush();
                } else if sql.to_uppercase().starts_with("DELETE") {
                    send_command_complete(&mut stream, "DELETE 1");
                    let _ = stream.write_all(&ready);
                    let _ = stream.flush();
                } else {
                    // Default / Users query response
                    send_mock_users_result(&mut stream);
                    let _ = stream.write_all(&ready);
                    let _ = stream.flush();
                }
            }
            b'X' => {
                // Terminate
                break;
            }
            _ => break,
        }
    }
}

fn send_parameter_status(stream: &mut TcpStream, name: &str, value: &str) {
    let mut payload = Vec::new();
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(value.as_bytes());
    payload.push(0);

    let len = (payload.len() + 4) as i32;
    let _ = stream.write_all(b"S");
    let _ = stream.write_all(&len.to_be_bytes());
    let _ = stream.write_all(&payload);
}

fn send_command_complete(stream: &mut TcpStream, tag: &str) {
    let mut payload = tag.as_bytes().to_vec();
    payload.push(0);

    let len = (payload.len() + 4) as i32;
    let _ = stream.write_all(b"C");
    let _ = stream.write_all(&len.to_be_bytes());
    let _ = stream.write_all(&payload);
}

fn send_mock_users_result(stream: &mut TcpStream) {
    // 1. RowDescription ('T')
    let fields = [
        ("id", 20u32),     // int8
        ("name", 1043u32), // varchar
        ("email", 25u32),  // text
        ("active", 16u32), // bool
    ];

    let mut t_payload = Vec::new();
    t_payload.extend_from_slice(&(fields.len() as i16).to_be_bytes());

    for (name, oid) in &fields {
        t_payload.extend_from_slice(name.as_bytes());
        t_payload.push(0);
        t_payload.extend_from_slice(&0u32.to_be_bytes()); // table oid
        t_payload.extend_from_slice(&0u16.to_be_bytes()); // column attr
        t_payload.extend_from_slice(&oid.to_be_bytes()); // type oid
        t_payload.extend_from_slice(&(-1i16).to_be_bytes()); // type size
        t_payload.extend_from_slice(&(-1i32).to_be_bytes()); // type mod
        t_payload.extend_from_slice(&0i16.to_be_bytes()); // format (0 = text)
    }

    let t_len = (t_payload.len() + 4) as i32;
    let _ = stream.write_all(b"T");
    let _ = stream.write_all(&t_len.to_be_bytes());
    let _ = stream.write_all(&t_payload);

    // 2. DataRows ('D')
    let rows: Vec<Vec<&str>> = vec![
        vec!["1", "Alice Smith", "alice@example.com", "t"],
        vec!["2", "Bob Jones", "bob@example.com", "f"],
    ];

    for row in rows {
        let mut d_payload = Vec::new();
        d_payload.extend_from_slice(&(row.len() as i16).to_be_bytes());
        for val in row {
            let bytes = val.as_bytes();
            d_payload.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
            d_payload.extend_from_slice(bytes);
        }

        let d_len = (d_payload.len() + 4) as i32;
        let _ = stream.write_all(b"D");
        let _ = stream.write_all(&d_len.to_be_bytes());
        let _ = stream.write_all(&d_payload);
    }

    // 3. CommandComplete ('C')
    send_command_complete(stream, "SELECT 2");
}
