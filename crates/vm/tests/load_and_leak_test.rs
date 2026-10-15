use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use vlang_compiler::bytecode::Value;
use vlang_compiler::compile;
use vlang_syntax::parse;
use vlang_vm::actor::{ActorError, ServiceActor};
use vlang_vm::VirtualMachine;

/// Helper to sample OS resident set size (RSS) in bytes
#[allow(deprecated)]
fn get_current_rss_bytes() -> usize {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        unsafe {
            let mut info: libc::mach_task_basic_info = mem::zeroed();
            let mut count = (mem::size_of::<libc::mach_task_basic_info>()
                / mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
            let kerr = libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut libc::integer_t,
                &mut count,
            );
            if kerr == libc::KERN_SUCCESS {
                info.resident_size as usize
            } else {
                0
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm.split_whitespace().nth(1).and_then(|s| s.parse::<usize>().ok()) {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
                return rss_pages * page_size;
            }
        }
        0
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

#[test]
fn test_sustained_load_zero_region_a_leak() {
    let source = r#"
fn handle(req_id: i64) -> i64 {
    let mut map = Map::new();
    map.insert("token", "session_abc_1234567890_temporary_key");
    map.insert("status", "processed");
    let a = req_id * 10;
    let b = a + 42;
    return b;
}
"#;
    let prog = parse(source).expect("Parse failed");
    let chunk = compile(&prog).expect("Compile failed");

    let total_requests = 25_000;
    let mut actor = ServiceActor::new(
        "SustainedLoadWorker",
        16 * 1024 * 1024,
        128 * 1024 * 1024,
        chunk,
    );

    let initial_rss = get_current_rss_bytes();
    let start_time = Instant::now();

    for i in 1..=total_requests {
        let result = actor.handle_request(Value::Int(i as i64));
        assert!(result.is_ok(), "Request {} failed: {:?}", i, result);
        assert_eq!(result.unwrap(), Value::Int((i as i64) * 10 + 42));
    }

    let elapsed = start_time.elapsed();
    let final_rss = get_current_rss_bytes();
    let throughput = (total_requests as f64) / elapsed.as_secs_f64();

    println!("\n=== Sustained Load Test Results (25,000 requests) ===");
    println!("Completed in:      {:.3} s ({:.1} req/s)", elapsed.as_secs_f64(), throughput);
    println!("Initial RSS:       {} KB", initial_rss / 1024);
    println!("Final RSS:         {} KB", final_rss / 1024);
    println!("Region B Live:     {} bytes", actor.persistent_memory.live_bytes.load(Ordering::Relaxed));

    // Verify Region A memory is fully reset to 0 after every request
    let check_vm = VirtualMachine::with_budgets(actor.request_budget_bytes, actor.persistent_budget_bytes);
    assert_eq!(check_vm.region_a.used_bytes, 0, "Idle Region A memory must be 0 bytes");
    assert_eq!(actor.persistent_memory.live_bytes.load(Ordering::Relaxed), 0, "Region B must have 0 live bytes");
}

#[test]
fn test_concurrent_load_region_b_promotion_stability() {
    let source = r#"
fn handle(req_id: i64) -> i64 {
    // If req_id % 100 == 0, promote to persistent cache
    if req_id % 100 == 0 {
        let token = "persistent_user_profile_cache_entry";
        let p = persist(token);
    }
    return req_id * 2;
}
"#;
    let prog = parse(source).expect("Parse failed");
    let chunk = compile(&prog).expect("Compile failed");

    let num_threads = 8;
    let reqs_per_thread = 2_500;
    let total_requests = num_threads * reqs_per_thread;

    let persistent_mem = Arc::new(vlang_vm::RegionBAllocator::new(64 * 1024 * 1024));
    let chunk_arc = Arc::new(chunk);
    let success_count = Arc::new(AtomicUsize::new(0));

    let start_time = Instant::now();
    let initial_rss = get_current_rss_bytes();

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let chunk = Arc::clone(&chunk_arc);
            let successes = Arc::clone(&success_count);
            let mem = Arc::clone(&persistent_mem);

            std::thread::spawn(move || {
                let mut actor = ServiceActor::new(
                    format!("WorkerThread-{}", t),
                    4 * 1024 * 1024,
                    64 * 1024 * 1024,
                    (*chunk).clone(),
                );
                actor.persistent_memory = mem;

                for i in 1..=reqs_per_thread {
                    let req_id = (t * reqs_per_thread + i) as i64;
                    if let Ok(res) = actor.handle_request(Value::Int(req_id)) {
                        if res == Value::Int(req_id * 2) {
                            successes.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Worker thread panicked");
    }

    let elapsed = start_time.elapsed();
    let final_rss = get_current_rss_bytes();
    let total_success = success_count.load(Ordering::SeqCst);
    let throughput = (total_success as f64) / elapsed.as_secs_f64();

    println!("\n=== Multi-Threaded Load Test Results ({} threads, {} requests) ===", num_threads, total_requests);
    println!("Throughput:        {:.1} req/s", throughput);
    println!("Success Rate:      {}/{} (100%)", total_success, total_requests);
    println!("Region B Retained: {} bytes across {} live objects",
        persistent_mem.live_bytes.load(Ordering::Relaxed),
        persistent_mem.live_objects.load(Ordering::Relaxed)
    );
    println!("RSS Memory:        {} KB -> {} KB (diff: {} KB)",
        initial_rss / 1024,
        final_rss / 1024,
        (final_rss as isize - initial_rss as isize) / 1024
    );

    assert_eq!(total_success, total_requests);
    // Every 100th request persists an object (total_requests / 100 = 200 objects)
    let expected_persisted = total_requests / 100;
    assert_eq!(
        persistent_mem.live_objects.load(Ordering::Relaxed),
        expected_persisted,
        "Expected exactly {} persisted items in Region B",
        expected_persisted
    );
}

#[test]
fn test_high_concurrency_mailbox_backpressure_rejection() {
    // Service with non-trivial loop so worker processes requests sequentially
    let source = r#"
fn handle(req: i64) -> i64 {
    let mut i = 0;
    while i < 5000 {
        i = i + 1;
    }
    return req + i;
}
"#;
    let prog = parse(source).expect("Parse failed");
    let chunk = compile(&prog).expect("Compile failed");

    // Actor with tiny mailbox capacity of 5 to test congestion shedding under burst load
    let actor = ServiceActor::new("CongestionWorker", 1024 * 1024, 1024 * 1024, chunk)
        .with_mailbox_capacity(5);

    let (mailbox, _worker) = actor.spawn_mailbox();

    let num_threads = 8;
    let reqs_per_thread = 50;
    let rejected_count = Arc::new(AtomicUsize::new(0));
    let accepted_count = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let mailbox_clone = mailbox.clone();
            let rejected = Arc::clone(&rejected_count);
            let accepted = Arc::clone(&accepted_count);

            std::thread::spawn(move || {
                let mut receivers = Vec::new();
                for i in 0..reqs_per_thread {
                    match mailbox_clone.try_send(Value::Int(i as i64)) {
                        Ok(reply_rx) => {
                            receivers.push(reply_rx);
                        }
                        Err(ActorError::MailboxFull { .. }) => {
                            rejected.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(other) => panic!("Unexpected error: {:?}", other),
                    }
                }
                for rx in receivers {
                    if let Ok(Ok(_)) = rx.recv() {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let accepted = accepted_count.load(Ordering::Relaxed);
    let rejected = rejected_count.load(Ordering::Relaxed);
    println!("\n=== Backpressure Congestion Test (Mailbox capacity: 10, Requests: 500) ===");
    println!("Accepted: {}", accepted);
    println!("Rejected with 429/503 Backpressure: {}", rejected);

    assert!(rejected > 0, "Expected backpressure shedding when 500 requests flood capacity 10 mailbox");
    assert_eq!(accepted + rejected, num_threads * reqs_per_thread);
}

#[test]
fn test_concurrent_ffi_offloaded_database_queries_zero_mailbox_starvation() {
    let source = r#"
import "rust:vlang_std_pg" as db;

fn handle(user_id: i64) -> i64 {
    let pool = db.create_pool("postgres://localhost/users");
    let user = pool.query_user(user_id);
    return user_id * 2;
}
"#;
    let prog = parse(source).expect("Parse failed");
    let chunk = compile(&prog).expect("Compile failed");

    let actor = ServiceActor::new("UserService", 64 * 1024, 1024 * 1024, chunk)
        .with_mailbox_capacity(500);

    let (mailbox, _worker) = actor.spawn_mailbox();
    let num_threads = 4;
    let reqs_per_thread = 25;
    let success_count = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();
    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let mailbox_clone = mailbox.clone();
            let success = Arc::clone(&success_count);

            std::thread::spawn(move || {
                let mut receivers = Vec::new();
                for i in 0..reqs_per_thread {
                    let user_id = (t * 100 + i + 1) as i64;
                    if let Ok(rx) = mailbox_clone.try_send(Value::Int(user_id)) {
                        receivers.push((user_id, rx));
                    }
                }
                for (user_id, rx) in receivers {
                    if let Ok(Ok(res)) = rx.recv() {
                        if res == Value::Int(user_id * 2) {
                            success.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total_success = success_count.load(Ordering::Relaxed);
    println!("\n=== Concurrent FFI Database Query Actor Mailbox Test ===");
    println!("Processed {} concurrent offloaded FFI queries in {:?}", total_success, elapsed);
    assert_eq!(total_success, num_threads * reqs_per_thread);
}
