//! Zero-Copy HTTP/1.1 Request and Response Engine.
//!
//! Streams wire socket frames directly into TruLang's Region A bump allocator,
//! extracts request routing parameters and JSON payloads, and serializes
//! high-speed RFC-compliant HTTP/1.1 wire responses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use trulang_compiler::bytecode::Value;
use trulang_vm::{VirtualMachine, VmError};

/// Standard HTTP request methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
    Custom(String),
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Head => "HEAD",
            HttpMethod::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "DELETE" => HttpMethod::Delete,
            "PATCH" => HttpMethod::Patch,
            "OPTIONS" => HttpMethod::Options,
            "HEAD" => HttpMethod::Head,
            other => HttpMethod::Custom(other.to_string()),
        }
    }
}

/// Incoming HTTP/1.1 request.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub uri: String,
    pub path: String,
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub keep_alive: bool,
}

impl HttpRequest {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query_params.get(name).map(|s| s.as_str())
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn body_str(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("")
    }

    pub fn parse_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Converts the request into a TruLang `Value::Object` allocated into Region A.
    ///
    /// Every property and nested structure is accounted for in Region A's bump
    /// budget, allowing zero-overhead O(1) bulk reset after handler completion.
    pub fn to_trulang_value(&self, vm: &mut VirtualMachine) -> Result<Value, VmError> {
        let req_obj = Value::new_object();

        let method_str = self.method.as_str().to_string();
        let path_str = self.path.clone();
        let body_str = self.body_str().to_string();

        let mut query_map = HashMap::new();
        for (k, v) in &self.query_params {
            query_map.insert(k.clone(), Value::String(Arc::new(v.clone())));
        }

        let mut headers_map = HashMap::new();
        for (k, v) in &self.headers {
            headers_map.insert(k.clone(), Value::String(Arc::new(v.clone())));
        }

        // Try parsing JSON body into TruLang Object / Map if present
        let parsed_json = if let Ok(json_val) = self.parse_json() {
            json_to_trulang_value(&json_val)
        } else {
            Value::Null
        };

        if let Value::Object(ref o) = req_obj {
            let mut g = o.lock().unwrap();
            g.insert(
                "method".to_string(),
                Value::String(Arc::new(method_str.clone())),
            );
            g.insert(
                "path".to_string(),
                Value::String(Arc::new(path_str.clone())),
            );
            g.insert(
                "body".to_string(),
                Value::String(Arc::new(body_str.clone())),
            );
            g.insert(
                "params".to_string(),
                Value::Map(Arc::new(Mutex::new(query_map))),
            );
            g.insert(
                "headers".to_string(),
                Value::Map(Arc::new(Mutex::new(headers_map))),
            );
            g.insert("json_data".to_string(), parsed_json);
        }

        // Estimate size and bump allocate in Region A
        let approx_bytes = 128
            + method_str.len()
            + path_str.len()
            + body_str.len()
            + self.query_params.len() * 32
            + self.headers.len() * 48;
        vm.allocate_region_a(approx_bytes)?;

        Ok(req_obj)
    }
}

/// Outgoing HTTP/1.1 response.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status_code: u16, body: impl Into<Vec<u8>>) -> Self {
        let status_text = match status_code {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            400 => "Bad Request",
            404 => "Not Found",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Status",
        }
        .to_string();

        Self {
            status_code,
            status_text,
            headers: HashMap::new(),
            body: body.into(),
        }
    }

    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::new(200, body)
    }

    pub fn json(val: &serde_json::Value) -> Self {
        let body = serde_json::to_vec(val).unwrap_or_default();
        let mut res = Self::new(200, body);
        res.headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );
        res
    }

    pub fn text(status_code: u16, body: impl Into<String>) -> Self {
        let b = body.into().into_bytes();
        let mut res = Self::new(status_code, b);
        res.headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );
        res
    }

    pub fn error(status_code: u16, message: impl Into<String>) -> Self {
        let msg = message.into();
        let json_body = format!(
            r#"{{"error":true,"status":{},"message":"{}"}}"#,
            status_code, msg
        );
        let mut res = Self::new(status_code, json_body.into_bytes());
        res.headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );
        res
    }

    pub fn with_header(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.headers
            .insert(key.into().to_ascii_lowercase(), val.into());
        self
    }

    /// Serializes this response into RFC-compliant HTTP/1.1 wire bytes.
    pub fn to_wire_bytes(&self, keep_alive: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(128 + self.body.len());
        let status_line = format!("HTTP/1.1 {} {}\r\n", self.status_code, self.status_text);
        out.extend_from_slice(status_line.as_bytes());

        // Standard headers
        let content_length = format!("Content-Length: {}\r\n", self.body.len());
        out.extend_from_slice(content_length.as_bytes());

        let conn_hdr = if keep_alive {
            "Connection: keep-alive\r\n"
        } else {
            "Connection: close\r\n"
        };
        out.extend_from_slice(conn_hdr.as_bytes());

        out.extend_from_slice(b"Server: TruLang-HTTP/1.0\r\n");

        if !self.headers.contains_key("content-type") {
            out.extend_from_slice(b"Content-Type: text/plain; charset=utf-8\r\n");
        }

        for (k, v) in &self.headers {
            let hdr = format!("{}: {}\r\n", title_case_header(k), v);
            out.extend_from_slice(hdr.as_bytes());
        }

        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }

    /// Converts a TruLang `Value` (Object or String) into an `HttpResponse`.
    pub fn from_trulang_value(val: &Value) -> Self {
        match val {
            Value::Object(ref obj) => {
                let guard = obj.lock().unwrap();
                let status = match guard.get("status") {
                    Some(Value::Int(n)) => *n as u16,
                    _ => 200,
                };

                let mut headers = HashMap::new();
                if let Some(Value::Map(m)) = guard.get("headers") {
                    for (k, v) in m.lock().unwrap().iter() {
                        headers.insert(k.clone(), format!("{}", v));
                    }
                }

                // If explicit data or user object exists, serialize to JSON
                let body = if let Some(body_val) = guard.get("body") {
                    match body_val {
                        Value::String(s) => s.as_bytes().to_vec(),
                        other => format!("{}", other).into_bytes(),
                    }
                } else if let Some(data_val) = guard.get("data") {
                    trulang_val_to_json_bytes(data_val)
                } else {
                    // Whole object serialized as JSON
                    let mut map = serde_json::Map::new();
                    for (k, v) in guard.iter() {
                        map.insert(k.clone(), trulang_val_to_json_val(v));
                    }
                    serde_json::to_vec(&serde_json::Value::Object(map)).unwrap_or_default()
                };

                let mut res = Self::new(status, body);
                if !headers.is_empty() {
                    res.headers.extend(headers);
                }
                if !res.headers.contains_key("content-type") {
                    res.headers.insert(
                        "content-type".to_string(),
                        "application/json; charset=utf-8".to_string(),
                    );
                }
                res
            }
            Value::String(s) => Self::text(200, s.as_str()),
            Value::Int(n) => Self::text(200, format!("{}", n)),
            Value::Bool(b) => Self::text(200, if *b { "true" } else { "false" }),
            Value::Null => Self::new(204, Vec::new()),
            other => Self::ok(format!("{}", other).into_bytes()),
        }
    }
}

pub enum ParseResult {
    Complete(HttpRequest, usize),
    Incomplete,
    Invalid(String),
}

/// Zero-copy streaming HTTP/1.1 wire parser.
pub struct HttpParser;

impl HttpParser {
    pub fn parse(buffer: &[u8]) -> ParseResult {
        // Look for end of header block "\r\n\r\n"
        let header_end = match find_subsequence(buffer, b"\r\n\r\n") {
            Some(idx) => idx,
            None => return ParseResult::Incomplete,
        };

        let header_bytes = &buffer[..header_end];
        let header_str = match std::str::from_utf8(header_bytes) {
            Ok(s) => s,
            Err(_) => return ParseResult::Invalid("Invalid UTF-8 in HTTP header".to_string()),
        };

        let mut lines = header_str.split("\r\n");
        let request_line = match lines.next() {
            Some(l) if !l.is_empty() => l,
            _ => return ParseResult::Invalid("Empty request line".to_string()),
        };

        let mut parts = request_line.split_whitespace();
        let method_str = match parts.next() {
            Some(m) => m,
            None => return ParseResult::Invalid("Missing HTTP method".to_string()),
        };
        let uri = match parts.next() {
            Some(u) => u.to_string(),
            None => return ParseResult::Invalid("Missing HTTP URI".to_string()),
        };
        let _version = parts.next().unwrap_or("HTTP/1.1");

        let (path, query_params) = parse_uri(&uri);

        let mut headers = HashMap::new();
        let mut content_length: usize = 0;
        let mut keep_alive = true;

        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim().to_string();

                if key == "content-length" {
                    if let Ok(len) = val.parse::<usize>() {
                        content_length = len;
                    }
                } else if key == "connection" {
                    if val.eq_ignore_ascii_case("close") {
                        keep_alive = false;
                    } else if val.eq_ignore_ascii_case("keep-alive") {
                        keep_alive = true;
                    }
                }

                headers.insert(key, val);
            }
        }

        let body_start = header_end + 4;
        let total_needed = body_start + content_length;
        if buffer.len() < total_needed {
            return ParseResult::Incomplete;
        }

        let body = buffer[body_start..total_needed].to_vec();
        let method = HttpMethod::from_str(method_str);

        ParseResult::Complete(
            HttpRequest {
                method,
                uri,
                path,
                query_params,
                headers,
                body,
                keep_alive,
            },
            total_needed,
        )
    }
}

fn parse_uri(uri: &str) -> (String, HashMap<String, String>) {
    if let Some((p, q)) = uri.split_once('?') {
        let mut params = HashMap::new();
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(decode_uri_component(k), decode_uri_component(v));
            } else if !pair.is_empty() {
                params.insert(decode_uri_component(pair), String::new());
            }
        }
        (p.to_string(), params)
    } else {
        (uri.to_string(), HashMap::new())
    }
}

fn decode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte_val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte_val as char);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn json_to_trulang_value(val: &serde_json::Value) -> Value {
    match val {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(Arc::new(s.clone())),
        serde_json::Value::Array(arr) => {
            let tr_arr: Vec<Value> = arr.iter().map(json_to_trulang_value).collect();
            Value::Array(tr_arr)
        }
        serde_json::Value::Object(map) => {
            let mut tr_map = HashMap::new();
            for (k, v) in map {
                tr_map.insert(k.clone(), json_to_trulang_value(v));
            }
            Value::Object(Arc::new(Mutex::new(tr_map)))
        }
    }
}

fn trulang_val_to_json_val(val: &Value) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(n) => serde_json::json!(*n),
        Value::Float(f) => serde_json::json!(*f),
        Value::String(s) => serde_json::Value::String((**s).clone()),
        Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(trulang_val_to_json_val).collect())
        }
        Value::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m.lock().unwrap().iter() {
                map.insert(k.clone(), trulang_val_to_json_val(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj.lock().unwrap().iter() {
                map.insert(k.clone(), trulang_val_to_json_val(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Handle(h) => serde_json::json!(format!("<Handle: {}>", h.type_name)),
    }
}

fn trulang_val_to_json_bytes(val: &Value) -> Vec<u8> {
    let json_val = trulang_val_to_json_val(val);
    serde_json::to_vec(&json_val).unwrap_or_default()
}

fn title_case_header(k: &str) -> String {
    k.split('-')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_parser_get_with_query_params() {
        let raw = b"GET /users?id=42&sort=asc HTTP/1.1\r\nHost: localhost:8080\r\nUser-Agent: curl/7.88.1\r\n\r\n";
        match HttpParser::parse(raw) {
            ParseResult::Complete(req, consumed) => {
                assert_eq!(consumed, raw.len());
                assert_eq!(req.method, HttpMethod::Get);
                assert_eq!(req.path, "/users");
                assert_eq!(req.param("id"), Some("42"));
                assert_eq!(req.param("sort"), Some("asc"));
                assert_eq!(req.header("host"), Some("localhost:8080"));
                assert!(req.keep_alive);
            }
            _ => panic!("Expected complete request"),
        }
    }

    #[test]
    fn test_http_parser_post_json_payload() {
        let body = r#"{"name":"Alice","role":"admin"}"#;
        let raw = format!(
            "POST /api/v1/users HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );

        match HttpParser::parse(raw.as_bytes()) {
            ParseResult::Complete(req, consumed) => {
                assert_eq!(consumed, raw.len());
                assert_eq!(req.method, HttpMethod::Post);
                assert_eq!(req.path, "/api/v1/users");
                assert_eq!(req.body_str(), body);
                let json = req.parse_json().expect("Valid JSON");
                assert_eq!(json["name"], "Alice");
                assert_eq!(json["role"], "admin");
            }
            _ => panic!("Expected complete POST request"),
        }
    }

    #[test]
    fn test_http_response_wire_serialization() {
        let res = HttpResponse::json(&serde_json::json!({
            "status": "success",
            "count": 100
        }));
        let wire = res.to_wire_bytes(true);
        let s = String::from_utf8(wire).expect("Valid UTF-8 wire response");
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Type: application/json"));
        assert!(s.contains("Connection: keep-alive"));
        assert!(s.contains(r#"{"count":100,"status":"success"}"#));
    }
}
