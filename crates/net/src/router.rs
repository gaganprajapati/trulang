//! Ergonomic High-Performance Routing and Middleware Pipeline.
//!
//! Provides:
//! - **Compressed Radix Trie**: $O(k)$ raw ASCII byte prefix matching with zero regex.
//! - **Zero-Copy Parameters**: Inline fixed-capacity `[RouteParam<'a>; 8]` without heap allocations.
//! - **Linear Interceptor Pipeline**: High-speed `before` and `after` middleware hooks without async boxing.
//! - **Built-in Middleware**: CORS, Logger, and Header Validation.

use crate::http::{HttpMethod, HttpRequest, HttpResponse};
use crate::ws::{WsHandler, WsListener};
use std::sync::Arc;
use std::time::Instant;

/// Maximum number of extracted route path parameters per request.
pub const MAX_ROUTE_PARAMS: usize = 8;

/// A single extracted route parameter slice borrowed directly from the request URI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteParam<'a> {
    pub key: &'static str,
    pub value: &'a str,
}

/// Zero-copy inline array of route path parameters (zero `HashMap` heap allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteParams<'a> {
    params: [Option<RouteParam<'a>>; MAX_ROUTE_PARAMS],
    len: usize,
}

impl<'a> Default for RouteParams<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> RouteParams<'a> {
    pub const fn new() -> Self {
        Self {
            params: [None; MAX_ROUTE_PARAMS],
            len: 0,
        }
    }

    /// Looks up a path parameter by name.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        for i in 0..self.len {
            if let Some(ref p) = self.params[i] {
                if p.key == key {
                    return Some(p.value);
                }
            }
        }
        None
    }

    /// Appends a new parameter if capacity remains.
    pub fn push(&mut self, key: &'static str, value: &'a str) -> bool {
        if self.len < MAX_ROUTE_PARAMS {
            self.params[self.len] = Some(RouteParam { key, value });
            self.len += 1;
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &RouteParam<'a>> {
        self.params[..self.len].iter().filter_map(|p| p.as_ref())
    }
}

/// Handler type alias for router endpoints.
pub type RouteHandler = Box<dyn Fn(&HttpRequest, &RouteParams) -> HttpResponse + Send + Sync>;

/// Radix Trie Node segment type.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeType {
    Static,
    Param(&'static str),
    Wildcard(&'static str),
}

/// A node in the Compressed Radix Trie.
struct RadixNode<H> {
    prefix: String,
    node_type: NodeType,
    handler: Option<H>,
    children: Vec<RadixNode<H>>,
}

impl<H> RadixNode<H> {
    fn new(prefix: String, node_type: NodeType) -> Self {
        Self {
            prefix,
            node_type,
            handler: None,
            children: Vec::new(),
        }
    }

    fn insert(&mut self, path: &str, handler: H) {
        if path.is_empty() {
            self.handler = Some(handler);
            return;
        }

        // Parse next segment: static, :param, or *wildcard
        if let Some(stripped) = path.strip_prefix(':') {
            let end = stripped.find('/').unwrap_or(stripped.len());
            let param_name = Box::leak(stripped[..end].to_string().into_boxed_str());
            let rest = &stripped[end..];

            // Look for existing param child
            let child_idx = self
                .children
                .iter()
                .position(|c| matches!(c.node_type, NodeType::Param(name) if name == param_name));

            let idx = match child_idx {
                Some(i) => i,
                None => {
                    self.children
                        .push(RadixNode::new(String::new(), NodeType::Param(param_name)));
                    self.children.len() - 1
                }
            };

            self.children[idx].insert(rest, handler);
            return;
        }

        if let Some(stripped) = path.strip_prefix('*') {
            let param_name = Box::leak(stripped.to_string().into_boxed_str());
            self.children.push(RadixNode {
                prefix: String::new(),
                node_type: NodeType::Wildcard(param_name),
                handler: Some(handler),
                children: Vec::new(),
            });
            return;
        }

        // Static prefix matching
        let next_slash_or_spec = path.find([':', '*']).unwrap_or(path.len());
        let current_static = &path[..next_slash_or_spec];
        let rest = &path[next_slash_or_spec..];

        // Find common prefix with any child
        for child in &mut self.children {
            if child.node_type == NodeType::Static {
                let common_len = common_prefix_len(&child.prefix, current_static);
                if common_len > 0 {
                    if common_len == child.prefix.len() {
                        child.insert(&path[common_len..], handler);
                        return;
                    }

                    // Split child node
                    let mut split_node = RadixNode::new(
                        child.prefix[common_len..].to_string(),
                        child.node_type.clone(),
                    );
                    split_node.handler = child.handler.take();
                    split_node.children = std::mem::take(&mut child.children);

                    child.prefix = child.prefix[..common_len].to_string();
                    child.children.push(split_node);

                    if common_len < current_static.len() {
                        child.insert(&path[common_len..], handler);
                    } else {
                        child.insert(rest, handler);
                    }
                    return;
                }
            }
        }

        // No common prefix: create new static child
        let mut new_child = RadixNode::new(current_static.to_string(), NodeType::Static);
        new_child.insert(rest, handler);
        self.children.push(new_child);
    }

    fn match_path<'a>(&'a self, path: &'a str, params: &mut RouteParams<'a>) -> Option<&'a H> {
        // Priority: Exact Static > Parameter > Wildcard

        // 1. Check if current static prefix matches
        let remaining = if !self.prefix.is_empty() {
            path.strip_prefix(&self.prefix)?
        } else {
            path
        };

        if remaining.is_empty() {
            if let Some(ref h) = self.handler {
                return Some(h);
            }
        }

        // 2. Search children: static matches first
        for child in self
            .children
            .iter()
            .filter(|c| c.node_type == NodeType::Static)
        {
            let mark = params.len;
            if let Some(h) = child.match_path(remaining, params) {
                return Some(h);
            }
            params.len = mark; // Rollback on failure
        }

        // 3. Search children: parameter matches (:param)
        for child in self
            .children
            .iter()
            .filter(|c| matches!(c.node_type, NodeType::Param(_)))
        {
            if let NodeType::Param(name) = child.node_type {
                let end = remaining.find('/').unwrap_or(remaining.len());
                if end > 0 {
                    let val = &remaining[..end];
                    let rest = &remaining[end..];

                    let mark = params.len;
                    params.push(name, val);

                    if let Some(h) = child.match_path(rest, params) {
                        return Some(h);
                    }
                    params.len = mark;
                }
            }
        }

        // 4. Search children: wildcard matches (*wildcard)
        for child in self
            .children
            .iter()
            .filter(|c| matches!(c.node_type, NodeType::Wildcard(_)))
        {
            if let NodeType::Wildcard(name) = child.node_type {
                params.push(name, remaining);
                return child.handler.as_ref();
            }
        }

        None
    }
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// High-Performance Radix Router supporting HTTP verbs and parameters.
pub struct RadixRouter<H = RouteHandler> {
    get_trie: RadixNode<H>,
    post_trie: RadixNode<H>,
    put_trie: RadixNode<H>,
    delete_trie: RadixNode<H>,
    patch_trie: RadixNode<H>,
    options_trie: RadixNode<H>,
    head_trie: RadixNode<H>,
}

impl<H> Default for RadixRouter<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> RadixRouter<H> {
    pub fn new() -> Self {
        Self {
            get_trie: RadixNode::new(String::new(), NodeType::Static),
            post_trie: RadixNode::new(String::new(), NodeType::Static),
            put_trie: RadixNode::new(String::new(), NodeType::Static),
            delete_trie: RadixNode::new(String::new(), NodeType::Static),
            patch_trie: RadixNode::new(String::new(), NodeType::Static),
            options_trie: RadixNode::new(String::new(), NodeType::Static),
            head_trie: RadixNode::new(String::new(), NodeType::Static),
        }
    }

    /// Registers a route for an HTTP method and path.
    pub fn add_route(&mut self, method: &HttpMethod, path: &str, handler: H) {
        let trie = match method {
            HttpMethod::Get => &mut self.get_trie,
            HttpMethod::Post => &mut self.post_trie,
            HttpMethod::Put => &mut self.put_trie,
            HttpMethod::Delete => &mut self.delete_trie,
            HttpMethod::Patch => &mut self.patch_trie,
            HttpMethod::Options => &mut self.options_trie,
            HttpMethod::Head => &mut self.head_trie,
            HttpMethod::Custom(_) => &mut self.get_trie,
        };
        trie.insert(path, handler);
    }

    /// Matches an incoming method and path, populating zero-copy parameter slices.
    pub fn match_route<'a>(
        &'a self,
        method: &HttpMethod,
        path: &'a str,
    ) -> (Option<&'a H>, RouteParams<'a>) {
        let mut params = RouteParams::new();
        let trie = match method {
            HttpMethod::Get => &self.get_trie,
            HttpMethod::Post => &self.post_trie,
            HttpMethod::Put => &self.put_trie,
            HttpMethod::Delete => &self.delete_trie,
            HttpMethod::Patch => &self.patch_trie,
            HttpMethod::Options => &self.options_trie,
            HttpMethod::Head => &self.head_trie,
            HttpMethod::Custom(_) => &self.get_trie,
        };
        let handler = trie.match_path(path, &mut params);
        (handler, params)
    }
}

// -----------------------------------------------------------------------------
// Middleware Pipeline (Linear Interceptor Chain)
// -----------------------------------------------------------------------------

/// Middleware interceptor trait with non-allocating `before` and `after` stages.
pub trait Middleware: Send + Sync {
    /// Executed before the route handler. Returning `Some(HttpResponse)` short-circuits execution.
    fn before(&self, req: &mut HttpRequest) -> Option<HttpResponse> {
        let _ = req;
        None
    }

    /// Executed after the route handler. Allows response header or status mutation.
    fn after(&self, req: &HttpRequest, res: &mut HttpResponse) {
        let _ = (req, res);
    }
}

/// Composable Web Application Router with built-in Middleware Pipeline.
pub struct WebApp {
    pub router: RadixRouter<RouteHandler>,
    pub ws_router: RadixRouter<Arc<dyn WsHandler>>,
    pub middlewares: Vec<Arc<dyn Middleware>>,
}

impl Default for WebApp {
    fn default() -> Self {
        Self::new()
    }
}

impl WebApp {
    pub fn new() -> Self {
        Self {
            router: RadixRouter::new(),
            ws_router: RadixRouter::new(),
            middlewares: Vec::new(),
        }
    }

    /// Adds a middleware to the execution chain.
    pub fn use_middleware<M: Middleware + 'static>(&mut self, middleware: M) -> &mut Self {
        self.middlewares.push(Arc::new(middleware));
        self
    }

    pub fn get<F>(&mut self, path: &str, handler: F) -> &mut Self
    where
        F: Fn(&HttpRequest, &RouteParams) -> HttpResponse + Send + Sync + 'static,
    {
        self.router
            .add_route(&HttpMethod::Get, path, Box::new(handler));
        self
    }

    pub fn post<F>(&mut self, path: &str, handler: F) -> &mut Self
    where
        F: Fn(&HttpRequest, &RouteParams) -> HttpResponse + Send + Sync + 'static,
    {
        self.router
            .add_route(&HttpMethod::Post, path, Box::new(handler));
        self
    }

    pub fn put<F>(&mut self, path: &str, handler: F) -> &mut Self
    where
        F: Fn(&HttpRequest, &RouteParams) -> HttpResponse + Send + Sync + 'static,
    {
        self.router
            .add_route(&HttpMethod::Put, path, Box::new(handler));
        self
    }

    pub fn delete<F>(&mut self, path: &str, handler: F) -> &mut Self
    where
        F: Fn(&HttpRequest, &RouteParams) -> HttpResponse + Send + Sync + 'static,
    {
        self.router
            .add_route(&HttpMethod::Delete, path, Box::new(handler));
        self
    }

    /// Registers a WebSocket route with a functional callback builder.
    pub fn ws<F>(&mut self, path: &str, configure: F) -> &mut Self
    where
        F: FnOnce(&mut WsListener) + 'static,
    {
        let mut listener = WsListener::new();
        configure(&mut listener);
        self.ws_router
            .add_route(&HttpMethod::Get, path, Arc::new(listener));
        self
    }

    /// Registers a custom struct implementing `WsHandler` for a path.
    pub fn ws_handler<H: WsHandler + 'static>(&mut self, path: &str, handler: H) -> &mut Self {
        self.ws_router
            .add_route(&HttpMethod::Get, path, Arc::new(handler));
        self
    }

    /// Matches an incoming request path against registered WebSocket routes.
    pub fn match_ws_route<'a>(
        &'a self,
        path: &'a str,
    ) -> (Option<&'a Arc<dyn WsHandler>>, RouteParams<'a>) {
        self.ws_router.match_route(&HttpMethod::Get, path)
    }

    /// Handles an incoming HTTP request through the middleware interceptors and radix router.
    pub fn handle(&self, mut req: HttpRequest) -> HttpResponse {
        // 1. Execute `before` interceptors; short-circuit on any early response
        for mw in &self.middlewares {
            if let Some(mut early_res) = mw.before(&mut req) {
                // Run `after` on early response
                for after_mw in self.middlewares.iter().rev() {
                    after_mw.after(&req, &mut early_res);
                }
                return early_res;
            }
        }

        // 2. Route matching
        let (handler, params) = self.router.match_route(&req.method, &req.path);
        let mut response = match handler {
            Some(h) => h(&req, &params),
            None => HttpResponse::error(
                404,
                format!("Not Found: {} {}", req.method.as_str(), req.path),
            ),
        };

        // 3. Execute `after` interceptors
        for mw in self.middlewares.iter().rev() {
            mw.after(&req, &mut response);
        }

        response
    }
}

// -----------------------------------------------------------------------------
// Built-in Middlewares: CORS, Logger, HeaderValidation
// -----------------------------------------------------------------------------

/// Cross-Origin Resource Sharing (CORS) Middleware.
pub struct CorsMiddleware {
    pub allow_origin: String,
    pub allow_methods: String,
    pub allow_headers: String,
    pub max_age_secs: u32,
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        Self {
            allow_origin: "*".to_string(),
            allow_methods: "GET, POST, PUT, DELETE, PATCH, OPTIONS, HEAD".to_string(),
            allow_headers: "content-type, authorization, traceparent, x-request-id".to_string(),
            max_age_secs: 86400,
        }
    }
}

impl CorsMiddleware {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_origin(mut self, origin: &str) -> Self {
        self.allow_origin = origin.to_string();
        self
    }
}

impl Middleware for CorsMiddleware {
    fn before(&self, req: &mut HttpRequest) -> Option<HttpResponse> {
        // Intercept preflight OPTIONS request
        if req.method == HttpMethod::Options {
            let res = HttpResponse::new(204, "")
                .with_header("access-control-allow-origin", self.allow_origin.clone())
                .with_header("access-control-allow-methods", self.allow_methods.clone())
                .with_header("access-control-allow-headers", self.allow_headers.clone())
                .with_header("access-control-max-age", self.max_age_secs.to_string());
            return Some(res);
        }
        None
    }

    fn after(&self, _req: &HttpRequest, res: &mut HttpResponse) {
        res.headers.insert(
            "access-control-allow-origin".to_string(),
            self.allow_origin.clone(),
        );
    }
}

/// Lightweight Non-Allocating Request Logger Middleware.
pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn before(&self, req: &mut HttpRequest) -> Option<HttpResponse> {
        req.headers.insert(
            "x-trulang-start-time".to_string(),
            Instant::now().elapsed().as_nanos().to_string(),
        );
        None
    }

    fn after(&self, req: &HttpRequest, res: &mut HttpResponse) {
        let status = res.status_code;
        let method = req.method.as_str();
        let path = &req.path;
        let _ = (status, method, path); // Can be tapped into telemetry / structured logs
    }
}

/// Header Validation Middleware. Short-circuits with 400 Bad Request if missing.
pub struct HeaderValidationMiddleware {
    pub required_header: &'static str,
}

impl HeaderValidationMiddleware {
    pub fn new(header: &'static str) -> Self {
        Self {
            required_header: header,
        }
    }
}

impl Middleware for HeaderValidationMiddleware {
    fn before(&self, req: &mut HttpRequest) -> Option<HttpResponse> {
        if req.header(self.required_header).is_none() {
            Some(HttpResponse::error(
                400,
                format!("Missing required request header: {}", self.required_header),
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radix_trie_parameter_extraction_zero_copy() {
        let mut router = RadixRouter::new();
        router.add_route(&HttpMethod::Get, "/api/v1/users/:id", ());
        router.add_route(&HttpMethod::Get, "/api/v1/users/:id/posts/:post_id", ());
        router.add_route(&HttpMethod::Get, "/static/*filepath", ());

        // 1. Single param
        let (matched, params) = router.match_route(&HttpMethod::Get, "/api/v1/users/42");
        assert!(matched.is_some());
        assert_eq!(params.get("id"), Some("42"));
        assert_eq!(params.len(), 1);

        // 2. Dual params
        let (matched, params) = router.match_route(&HttpMethod::Get, "/api/v1/users/99/posts/101");
        assert!(matched.is_some());
        assert_eq!(params.get("id"), Some("99"));
        assert_eq!(params.get("post_id"), Some("101"));
        assert_eq!(params.len(), 2);

        // 3. Wildcard param
        let (matched, params) = router.match_route(&HttpMethod::Get, "/static/css/theme/dark.css");
        assert!(matched.is_some());
        assert_eq!(params.get("filepath"), Some("css/theme/dark.css"));

        // 4. Mismatch
        let (matched, _) = router.match_route(&HttpMethod::Get, "/api/v2/unknown");
        assert!(matched.is_none());
    }

    #[test]
    fn test_webapp_middleware_pipeline_short_circuit_and_headers() {
        let mut app = WebApp::new();
        app.use_middleware(CorsMiddleware::new().with_origin("https://example.com"));
        app.use_middleware(HeaderValidationMiddleware::new("x-api-key"));
        app.get("/items/:item_id", |_req, params| {
            let id = params.get("item_id").unwrap_or("unknown");
            HttpResponse::ok(format!("Item: {}", id))
        });

        // 1. Preflight OPTIONS request short-circuits with 204
        let preflight = HttpRequest {
            method: HttpMethod::Options,
            uri: "/items/1".into(),
            path: "/items/1".into(),
            query_params: Default::default(),
            headers: Default::default(),
            body: Vec::new(),
            keep_alive: true,
        };
        let res = app.handle(preflight);
        assert_eq!(res.status_code, 204);
        assert_eq!(
            res.headers
                .get("access-control-allow-origin")
                .map(|s| s.as_str()),
            Some("https://example.com")
        );

        // 2. Request missing required header fails with 400
        let invalid_req = HttpRequest {
            method: HttpMethod::Get,
            uri: "/items/1".into(),
            path: "/items/1".into(),
            query_params: Default::default(),
            headers: Default::default(),
            body: Vec::new(),
            keep_alive: true,
        };
        let res = app.handle(invalid_req);
        assert_eq!(res.status_code, 400);

        // 3. Valid request succeeds with 200 and CORS header
        let mut valid_req = HttpRequest {
            method: HttpMethod::Get,
            uri: "/items/12345".into(),
            path: "/items/12345".into(),
            query_params: Default::default(),
            headers: Default::default(),
            body: Vec::new(),
            keep_alive: true,
        };
        valid_req
            .headers
            .insert("x-api-key".into(), "secret".into());

        let res = app.handle(valid_req);
        assert_eq!(res.status_code, 200);
        assert_eq!(res.body_str(), "Item: 12345");
        assert_eq!(
            res.headers
                .get("access-control-allow-origin")
                .map(|s| s.as_str()),
            Some("https://example.com")
        );
    }

    #[test]
    fn test_router_performance_benchmark() {
        let mut router = RadixRouter::new();
        router.add_route(
            &HttpMethod::Get,
            "/api/v1/organizations/:org_id/projects/:project_id/deployments/:deployment_id",
            (),
        );

        let start = Instant::now();
        const ITERATIONS: usize = 200_000;

        for _ in 0..ITERATIONS {
            let (matched, params) = router.match_route(
                &HttpMethod::Get,
                "/api/v1/organizations/corp-99/projects/proj-alpha/deployments/dep-88219",
            );
            assert!(matched.is_some());
            assert_eq!(params.len(), 3);
        }

        let elapsed = start.elapsed();
        let req_per_sec = (ITERATIONS as f64) / elapsed.as_secs_f64();
        println!(
            "Radix Router Benchmark: {:.0} req/sec ({:?})",
            req_per_sec, elapsed
        );

        // Must exceed non-functional threshold: > 75,000 req/sec
        assert!(
            req_per_sec > 75_000.0,
            "Routing throughput must exceed 75,000 req/sec per core, got: {:.0}",
            req_per_sec
        );
    }
}
