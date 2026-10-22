//! Dual-Region Database Memory Manager.
//!
//! Enforces the TruLang dual-region allocation architecture for database drivers:
//! - **Region B (Persistent State):** Long-lived connection pools, prepared statements,
//!   and authentication tokens are retained across requests.
//! - **Region A (Transient Request):** Wire socket frames, parsed row tuples, and query
//!   result objects are allocated into Region A bump memory and cleared in O(1) time
//!   when the request finishes.

use std::sync::atomic::{AtomicUsize, Ordering};
use trulang_compiler::bytecode::Value;

pub struct DualRegionDbManager {
    pub persistent_budget_bytes: usize,
    pub live_query_bytes: AtomicUsize,
    pub persistent_bytes_used: AtomicUsize,
    pub total_queries_tracked: AtomicUsize,
}

impl DualRegionDbManager {
    pub fn new(persistent_budget_bytes: usize) -> Self {
        Self {
            persistent_budget_bytes,
            live_query_bytes: AtomicUsize::new(0),
            persistent_bytes_used: AtomicUsize::new(0),
            total_queries_tracked: AtomicUsize::new(0),
        }
    }

    /// Computes transient bytes for query response objects allocated in Region A.
    pub fn track_query_result(&self, result: &Value) -> usize {
        let bytes = estimate_query_value_size(result);
        self.live_query_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.total_queries_tracked.fetch_add(1, Ordering::Relaxed);
        bytes
    }

    /// Promotes a transient database row/model from Region A into persistent Region B.
    pub fn promote_to_persistent(&self, val: &Value) -> usize {
        let bytes = estimate_query_value_size(val);
        self.persistent_bytes_used
            .fetch_add(bytes, Ordering::Relaxed);
        let mut cur = self.live_query_bytes.load(Ordering::Relaxed);
        while let Err(actual) = self.live_query_bytes.compare_exchange_weak(
            cur,
            cur.saturating_sub(bytes),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            cur = actual;
        }
        bytes
    }

    /// Resets transient memory accounting (Region A) at the end of a request lifecycle.
    pub fn reset_request_arena(&self) {
        self.live_query_bytes.store(0, Ordering::Relaxed);
    }

    /// Returns live transient bytes currently allocated in Region A.
    pub fn transient_bytes(&self) -> usize {
        self.live_query_bytes.load(Ordering::Relaxed)
    }

    /// Returns persistent bytes promoted into Region B.
    pub fn persistent_bytes(&self) -> usize {
        self.persistent_bytes_used.load(Ordering::Relaxed)
    }
}

pub fn estimate_query_value_size(val: &Value) -> usize {
    match val {
        Value::Null => 8,
        Value::Bool(_) => 8,
        Value::Int(_) | Value::Float(_) => 16,
        Value::String(s) => 32 + s.len(),
        Value::Array(items) => 32 + items.iter().map(estimate_query_value_size).sum::<usize>(),
        Value::Object(obj) => {
            let guard = obj.lock().unwrap();
            let mut total = 64;
            for (k, v) in guard.iter() {
                total += 32 + k.len() + estimate_query_value_size(v);
            }
            total
        }
        Value::Map(m) => {
            let guard = m.lock().unwrap();
            let mut total = 64;
            for (k, v) in guard.iter() {
                total += 32 + k.len() + estimate_query_value_size(v);
            }
            total
        }
        Value::Handle(_) => 48,
    }
}
