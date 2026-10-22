//! In-Process Redis RESP Wire Protocol Mock Server.
//!
//! Provides a zero-dependency, deterministic TCP server speaking pure
//! Redis RESP protocol for testing and validation.

use super::protocol::{read_resp_value, RespValue};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct MockRedisServer {
    pub addr: SocketAddr,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl MockRedisServer {
    /// Launches an in-process mock Redis TCP server on an ephemeral port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Bind mock redis server");
        let addr = listener.local_addr().unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let kv_store: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let hash_store: Arc<Mutex<HashMap<String, HashMap<String, String>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let kv_clone = Arc::clone(&kv_store);
        let hash_clone = Arc::clone(&hash_store);

        let thread_handle = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();

            while running_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let kv = Arc::clone(&kv_clone);
                        let hashes = Arc::clone(&hash_clone);
                        thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            handle_redis_client(stream, kv, hashes);
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

    pub fn url(&self) -> String {
        format!("redis://{}", self.addr)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for MockRedisServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_redis_client(
    stream: TcpStream,
    kv: Arc<Mutex<HashMap<String, String>>>,
    hashes: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));

    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut reader = BufReader::new(read_stream);
    let mut writer = BufWriter::new(stream);

    while let Ok(msg) = read_resp_value(&mut reader) {
        let cmd_parts = match msg {
            RespValue::Array(Some(elems)) => {
                let mut parts = Vec::new();
                for elem in elems {
                    if let Some(s) = elem.as_str() {
                        parts.push(s.to_string());
                    }
                }
                parts
            }
            _ => break,
        };

        if cmd_parts.is_empty() {
            continue;
        }

        let cmd = cmd_parts[0].to_uppercase();
        match cmd.as_str() {
            "PING" => {
                let _ = writer.write_all(b"+PONG\r\n");
            }
            "AUTH" | "SELECT" => {
                let _ = writer.write_all(b"+OK\r\n");
            }
            "SET" => {
                if cmd_parts.len() >= 3 {
                    let key = cmd_parts[1].clone();
                    let val = cmd_parts[2].clone();
                    kv.lock().unwrap().insert(key, val);
                    let _ = writer.write_all(b"+OK\r\n");
                } else {
                    let _ = writer.write_all(b"-ERR wrong number of arguments for 'set'\r\n");
                }
            }
            "GET" => {
                if cmd_parts.len() >= 2 {
                    let key = &cmd_parts[1];
                    let guard = kv.lock().unwrap();
                    if let Some(val) = guard.get(key) {
                        let _ =
                            writer.write_all(format!("${}\r\n{}\r\n", val.len(), val).as_bytes());
                    } else {
                        let _ = writer.write_all(b"$-1\r\n");
                    }
                } else {
                    let _ = writer.write_all(b"-ERR wrong number of arguments for 'get'\r\n");
                }
            }
            "DEL" => {
                let mut count = 0;
                let mut guard = kv.lock().unwrap();
                for k in &cmd_parts[1..] {
                    if guard.remove(k).is_some() {
                        count += 1;
                    }
                }
                let _ = writer.write_all(format!(":{}\r\n", count).as_bytes());
            }
            "INCR" => {
                if cmd_parts.len() >= 2 {
                    let key = &cmd_parts[1];
                    let mut guard = kv.lock().unwrap();
                    let val = guard.entry(key.clone()).or_insert_with(|| "0".to_string());
                    let mut n: i64 = val.parse().unwrap_or(0);
                    n += 1;
                    *val = n.to_string();
                    let _ = writer.write_all(format!(":{}\r\n", n).as_bytes());
                } else {
                    let _ = writer.write_all(b"-ERR wrong number of arguments for 'incr'\r\n");
                }
            }
            "HSET" => {
                if cmd_parts.len() >= 4 {
                    let key = &cmd_parts[1];
                    let field = &cmd_parts[2];
                    let val = &cmd_parts[3];
                    let mut guard = hashes.lock().unwrap();
                    let h = guard.entry(key.clone()).or_default();
                    let is_new = h.insert(field.clone(), val.clone()).is_none();
                    let code = if is_new { 1 } else { 0 };
                    let _ = writer.write_all(format!(":{}\r\n", code).as_bytes());
                } else {
                    let _ = writer.write_all(b"-ERR wrong number of arguments for 'hset'\r\n");
                }
            }
            "HGET" => {
                if cmd_parts.len() >= 3 {
                    let key = &cmd_parts[1];
                    let field = &cmd_parts[2];
                    let guard = hashes.lock().unwrap();
                    if let Some(h) = guard.get(key) {
                        if let Some(val) = h.get(field) {
                            let _ = writer
                                .write_all(format!("${}\r\n{}\r\n", val.len(), val).as_bytes());
                        } else {
                            let _ = writer.write_all(b"$-1\r\n");
                        }
                    } else {
                        let _ = writer.write_all(b"$-1\r\n");
                    }
                } else {
                    let _ = writer.write_all(b"-ERR wrong number of arguments for 'hget'\r\n");
                }
            }
            _ => {
                let _ = writer.write_all(format!("-ERR unknown command '{}'\r\n", cmd).as_bytes());
            }
        }
        let _ = writer.flush();
    }
}
