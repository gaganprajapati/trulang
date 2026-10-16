# 🦀 Extending TruLang with the Rust Ecosystem (`crates.io`)

> **Architectural Guide & Developer Tutorial**  
> **Status:** Production Reference  
> **Language Version:** TruLang v0.1.0+

---

## 🌟 The TruLang Advantage: Zero Cold-Start

New programming languages typically face the **"Cold-Start Ecosystem Trap"**: developers love the language syntax and runtime model, but cannot adopt it for production microservices because they lack essential libraries:
- *Where is the PostgreSQL / MySQL client?*
- *Where is the Redis driver, JWT validator, or Argon2 / Bcrypt hasher?*
- *Where is the AWS SDK or S3 client?*

Building these libraries from scratch in a new language takes years.

### How History Solved This:
- **Python + C/C++:** Python dominated AI, ML, and Data Science because it could seamlessly call C/C++ libraries (**NumPy, PyTorch, TensorFlow, OpenCV**).
- **Elixir + Erlang:** Elixir succeeded because it ran on the BEAM VM and could call any Erlang OTP library on day one.
- **TypeScript + JavaScript:** TypeScript won the web by providing 100% interop with existing npm packages.

### TruLang's Unfair Advantage:
Because **TruLang’s compiler, virtual machine, and actor runtime are built in Rust**, TruLang has direct, zero-cost native access to the **150,000+ battle-tested, high-performance, memory-safe packages on [crates.io](https://crates.io)**.

---

## 🏗️ Architecture: The Safe FFI Bridge (`crates/ffi`)

TruLang interacts with Rust crates through three core architectural primitives in [`crates/ffi`](file:///Volumes/work/apps/TruLang/crates/ffi):

```
┌────────────────────────────────────────────────────────┐
│                   TruLang Code (.tr)                   │
│   import "crypto" as crypto;                           │
│   import "db" as pg;                                   │
│   let hash = crypto.hash_password("secret");           │
│   let pool = pg.create_pool("postgres://...");         │
└───────────────────────────┬────────────────────────────┘
                            │ Method Dispatch
                            ▼
┌────────────────────────────────────────────────────────┐
│             crates/ffi (Bridge Registry)               │
│                                                        │
│  • FfiRegistry: Maps TruLang module calls to Rust fn   │
│  • WorkerPool: Offloads heavy I/O to background threads│
│  • Handle<T>: Holds opaque stateful Rust structs       │
│  • Marshaller: Converts JSON / Rust primitives to      │
│                TruLang Region A Values                 │
└───────────────────────────┬────────────────────────────┘
                            │ Native Rust Call
                            ▼
┌────────────────────────────────────────────────────────┐
│                 Rust Crates on crates.io               │
│       bcrypt • ring • sqlx • redis • reqwest • image   │
└────────────────────────────────────────────────────────┘
```

1. **`FfiRegistry`:** A thread-safe dynamic registry that maps TruLang `import "<module>"` names to native Rust dispatch functions.
2. **`Value::Handle(Arc<Resource>)`:** Encapsulates stateful, long-lived Rust structs (like connection pools, socket handles, or ML models) inside TruLang. When the TruLang handle variable goes out of scope, Rust's deterministic **RAII `Drop`** destructor runs immediately—guaranteeing **zero resource leaks**.
3. **`WorkerPool`:** A background thread pool that executes CPU-bound or blocking Rust calls off the actor's thread, preventing mailbox starvation.

---

## 튜 Tutorial 1: Wrapping a Pure CPU Crate (`bcrypt`)

Suppose you want to provide native password hashing in TruLang using the Rust `bcrypt` crate.

### Step 1: Add the Crate to `crates/ffi/Cargo.toml`
```toml
# crates/ffi/Cargo.toml
[dependencies]
bcrypt = "0.15"
```

### Step 2: Implement the Module in Rust
Create `crates/ffi/src/modules/crypto.rs`:
```rust
use trulang_compiler::bytecode::Value;
use std::sync::Arc;
use crate::error::FfiError;

/// Hashes a plaintext password using bcrypt
pub fn hash_password(args: &[Value]) -> Result<Value, FfiError> {
    let password = match args.get(0) {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(FfiError::InvalidArgument("Expected string password".into())),
    };

    let cost = match args.get(1) {
        Some(Value::Int(c)) => *c as u32,
        _ => 10, // Default bcrypt cost
    };

    let hashed = bcrypt::hash(password, cost)
        .map_err(|e| FfiError::ExecutionFailed(format!("Bcrypt error: {}", e)))?;

    // Return as a TruLang String Value
    Ok(Value::String(Arc::new(hashed)))
}

/// Verifies a password against a hash
pub fn verify_password(args: &[Value]) -> Result<Value, FfiError> {
    let password = match args.get(0) {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(FfiError::InvalidArgument("Expected string password".into())),
    };
    let hash = match args.get(1) {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(FfiError::InvalidArgument("Expected string hash".into())),
    };

    let valid = bcrypt::verify(password, hash).unwrap_or(false);
    Ok(Value::Bool(valid))
}
```

### Step 3: Register in `FfiRegistry`
In `crates/ffi/src/registry.rs`:
```rust
registry.register("crypto", "hash_password", |args, _pool| {
    modules::crypto::hash_password(args)
});

registry.register("crypto", "verify_password", |args, _pool| {
    modules::crypto::verify_password(args)
});
```

### Step 4: Use Directly in TruLang (`user_service.tr`)
```trulang
import "crypto" as crypto;

service AuthService {
    memory_budget request = 16 MB;

    fn register_user(password: string) -> string {
        // Runs at pure compiled Rust speed
        let hash = crypto.hash_password(password, 10);
        return hash;
    }

    fn login(password: string, stored_hash: string) -> bool {
        return crypto.verify_password(password, stored_hash);
    }
}
```

---

## 튜 Tutorial 2: Stateful Client with RAII Destructors (`Redis`)

For stateful services (databases, caches, open sockets), you don't want to reconnect on every request. You need a **connection handle** stored in **Region B**.

### Step 1: Define a Native Resource Struct in Rust
```rust
// crates/ffi/src/handle.rs
use std::sync::Mutex;
use redis::Connection;

pub struct RedisResource {
    connection: Mutex<Connection>,
}

impl RedisResource {
    pub fn new(url: &str) -> Result<Self, FfiError> {
        let client = redis::Client::open(url)
            .map_err(|e| FfiError::ConnectionFailed(e.to_string()))?;
        let conn = client.get_connection()
            .map_err(|e| FfiError::ConnectionFailed(e.to_string()))?;
        Ok(Self { connection: Mutex::new(conn) })
    }
}

impl Drop for RedisResource {
    fn drop(&mut self) {
        // Automatically runs when the TruLang handle is cleared!
        // Guaranteed zero socket leak.
    }
}
```

### Step 2: Implement Method Dispatch
```rust
impl NativeResource for RedisResource {
    fn call_method(&self, method: &str, args: &[Value]) -> Result<Value, FfiError> {
        let mut conn = self.connection.lock().unwrap();
        match method {
            "get" => {
                let key: &str = extract_string(&args[0])?;
                let val: Option<String> = redis::cmd("GET").arg(key).query(&mut *conn).ok();
                match val {
                    Some(s) => Ok(Value::String(Arc::new(s))),
                    None => Ok(Value::Null),
                }
            }
            "set" => {
                let key: &str = extract_string(&args[0])?;
                let val: &str = extract_string(&args[1])?;
                let _: () = redis::cmd("SET").arg(key).arg(val).query(&mut *conn)
                    .map_err(|e| FfiError::ExecutionFailed(e.to_string()))?;
                Ok(Value::Bool(true))
            }
            _ => Err(FfiError::MethodNotFound(method.into())),
        }
    }
}
```

### Step 3: Use in TruLang with Persistent Caching (`cache_actor.tr`)
```trulang
import "redis" as redis;

service CacheActor {
    memory_budget request = 8 MB;
    memory_budget persistent = 128 MB;

    // Stored in Region B: survives across all incoming requests
    let client = redis.connect("redis://127.0.0.1:6379");

    fn get_cached_profile(user_id: string) -> string? {
        let key = "user:" + user_id;
        let data = client.get(key); // Fast C/Rust RESP query
        return data;
    }

    fn set_cached_profile(user_id: string, json_data: string) {
        let key = "user:" + user_id;
        client.set(key, json_data);
    }
}
```

---

## ⚡ Thread Pool Offloading: Non-Blocking Actors

One of Node.js's biggest weaknesses is that a slow C++ addon or heavy JSON parse freezes the Event Loop for all users.

In TruLang, the `WorkerPool` eliminates this problem:
```rust
// Heavy queries execute on background worker threads
let pool = registry.worker_pool();
let result = pool.spawn_blocking(move || {
    // Heavy Rust query or CPU computation runs here
    sqlx_pool.query("SELECT * FROM large_table")
})?;
```
The calling service actor remains unblocked, allowing its mailbox to continue accepting and queuing messages with zero jitter.

---

## 📊 Summary: Why Rust Interop Changes Everything for TruLang

| Challenge for New Languages | TruLang's Solution via Rust Interop |
| :--- | :--- |
| **No Database Drivers** | Wrap `sqlx` (PostgreSQL, MySQL, SQLite) in under 200 lines of Rust. |
| **No Crypto / Security** | Access `ring`, `argon2`, `jsonwebtoken`, and `rustls` immediately. |
| **No Data Serialization** | Use `serde_json`, `csv`, and `prost` (Protobuf) at native compiled speed. |
| **Memory Safety Risks** | Rust's borrow checker guarantees that native libraries are free of buffer overflows and data races. |
| **Memory Leaks** | TruLang's `Handle<T>` uses deterministic RAII destructors: when an actor stops, all foreign resources free instantly. |

---

*For contributions and registering new Rust standard modules, see [`CONTRIBUTING.md`](../CONTRIBUTING.md).*
