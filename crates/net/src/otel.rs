//! OpenTelemetry (OTel) Distributed Tracing Engine for TruLang.
//!
//! Implements W3C Trace Context (traceparent), span creation, propagation,
//! and zero-allocation Region A tracing context.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SPAN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// W3C Trace Context representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub trace_flags: u8,
}

impl SpanContext {
    /// Generates a new root trace context with 128-bit trace ID and 64-bit span ID.
    pub fn new_root() -> Self {
        let mut trace_bytes = [0u8; 16];
        fill_random_bytes(&mut trace_bytes);

        let mut span_bytes = [0u8; 8];
        fill_random_bytes(&mut span_bytes);

        Self {
            trace_id: hex::encode(trace_bytes),
            span_id: hex::encode(span_bytes),
            parent_span_id: None,
            trace_flags: 1, // Sampled
        }
    }

    /// Parses a W3C traceparent header: `00-{trace_id}-{span_id}-{trace_flags}`
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = parts[0];
        let trace_id = parts[1];
        let parent_id = parts[2];
        let flags = parts[3];

        if version != "00" || trace_id.len() != 32 || parent_id.len() != 16 || flags.len() != 2 {
            return None;
        }

        // Trace ID must not be all zeros
        if trace_id == "00000000000000000000000000000000" || parent_id == "0000000000000000" {
            return None;
        }

        let trace_flags = u8::from_str_radix(flags, 16).unwrap_or(1);

        // Generate a new child span ID for this request span
        let mut span_bytes = [0u8; 8];
        fill_random_bytes(&mut span_bytes);

        Some(Self {
            trace_id: trace_id.to_string(),
            span_id: hex::encode(span_bytes),
            parent_span_id: Some(parent_id.to_string()),
            trace_flags,
        })
    }

    /// Serializes to standard W3C traceparent header value.
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }

    /// Creates a child span context under the current context.
    pub fn child_span(&self) -> Self {
        let mut span_bytes = [0u8; 8];
        fill_random_bytes(&mut span_bytes);

        Self {
            trace_id: self.trace_id.clone(),
            span_id: hex::encode(span_bytes),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error(String),
}

/// An active or completed OpenTelemetry Span.
#[derive(Debug, Clone)]
pub struct TraceSpan {
    pub name: String,
    pub context: SpanContext,
    pub start_time_ns: u64,
    pub duration_ns: u64,
    pub attributes: HashMap<String, String>,
    pub status: SpanStatus,
}

impl TraceSpan {
    pub fn new(name: impl Into<String>, context: SpanContext) -> Self {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            name: name.into(),
            context,
            start_time_ns: now_ns,
            duration_ns: 0,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
        }
    }

    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(key.into(), value.into());
    }

    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    pub fn finish(&mut self) {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        if now_ns >= self.start_time_ns {
            self.duration_ns = now_ns - self.start_time_ns;
        }
        if self.status == SpanStatus::Unset {
            self.status = SpanStatus::Ok;
        }
    }
}

/// Generates pseudo-random or cryptographically random bytes.
fn fill_random_bytes(buf: &mut [u8]) {
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(buf).is_ok() {
            return;
        }
    }

    // Fallback using time and atomic counter
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut seed = nanos ^ (counter << 32);

    for b in buf.iter_mut() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *b = (seed >> 33) as u8;
    }
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let mut s = String::with_capacity(bytes.as_ref().len() * 2);
        for &b in bytes.as_ref() {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", b);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_w3c_traceparent_roundtrip() {
        let original_header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = SpanContext::from_traceparent(original_header).expect("Parse traceparent");

        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.parent_span_id, Some("00f067aa0ba902b7".to_string()));
        assert_eq!(ctx.trace_flags, 1);
        assert_ne!(ctx.span_id, "00f067aa0ba902b7"); // Generates new child span ID
        assert_eq!(ctx.span_id.len(), 16);

        let serialized = ctx.to_traceparent();
        assert!(serialized.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
        assert!(serialized.ends_with("-01"));
    }

    #[test]
    fn test_new_root_span_and_child() {
        let root = SpanContext::new_root();
        assert_eq!(root.trace_id.len(), 32);
        assert_eq!(root.span_id.len(), 16);
        assert_eq!(root.parent_span_id, None);

        let child = root.child_span();
        assert_eq!(child.trace_id, root.trace_id);
        assert_ne!(child.span_id, root.span_id);
        assert_eq!(child.parent_span_id, Some(root.span_id));
    }

    #[test]
    fn test_trace_span_lifecycle() {
        let root = SpanContext::new_root();
        let mut span = TraceSpan::new("GET /api/users", root);
        span.set_attribute("http.method", "GET");
        span.set_attribute("http.route", "/api/users");

        std::thread::sleep(std::time::Duration::from_millis(5));
        span.finish();

        assert!(span.duration_ns > 0);
        assert_eq!(span.status, SpanStatus::Ok);
        assert_eq!(span.attributes.get("http.method"), Some(&"GET".to_string()));
    }
}
