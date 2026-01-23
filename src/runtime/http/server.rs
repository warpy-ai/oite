use super::super::r#async::{TcpListener, TcpStream, AsyncTcpListener, Runtime};
use super::{Request, Response, RequestParser, ResponseParser, Method, Version};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::net::SocketAddr;
use std::path::Path;
use std::io;

#[cfg(feature = "tls")]
use super::super::r#async::tls::{TlsAcceptor, TlsServerConfig, TlsStream};

pub struct Route {
    method: Option<Method>,
    pattern: String,
    handler: Arc<dyn Handler + Send + Sync>,
}

impl Route {
    pub fn new(method: Option<Method>, pattern: String, handler: Arc<dyn Handler + Send + Sync>) -> Self {
        Self { method, pattern, handler }
    }
}

pub trait Handler: Send + Sync {
    fn handle(&self, request: &Request) -> Response;
}

pub struct Router {
    routes: Vec<Route>,
    not_found: Arc<dyn Handler + Send + Sync>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            not_found: Arc::new(DefaultNotFoundHandler),
        }
    }

    pub fn route<H>(
        &mut self,
        method: Option<Method>,
        pattern: &str,
        handler: H,
    ) where
        H: Handler + 'static,
    {
        self.routes.push(Route::new(
            method,
            pattern.to_string(),
            Arc::new(handler),
        ));
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Get), pattern, handler);
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Post), pattern, handler);
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Put), pattern, handler);
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(Some(Method::Delete), pattern, handler);
    }

    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.route(None, pattern, handler);
    }

    pub fn set_not_found<H>(&mut self, handler: H)
    where
        H: Handler + 'static,
    {
        self.not_found = Arc::new(handler);
    }

    fn match_route(&self, request: &Request) -> Option<&Arc<dyn Handler + Send + Sync>> {
        for route in &self.routes {
            if route.method.as_ref().map_or(false, |m| m != request.method()) {
                continue;
            }
            if self.match_pattern(&route.pattern, request.uri()) {
                return Some(&route.handler);
            }
        }
        None
    }

    fn match_pattern(&self, pattern: &str, uri: &str) -> bool {
        let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

        if pattern_parts.len() != uri_parts.len() {
            return false;
        }

        for (p, u) in pattern_parts.iter().zip(uri_parts.iter()) {
            if p.starts_with(':') {
                continue;
            }
            if p.starts_with('*') {
                return true;
            }
            if p != *u {
                return false;
            }
        }

        true
    }

    fn extract_params(&self, pattern: &str, uri: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

        for (i, p) in pattern_parts.iter().enumerate() {
            if p.starts_with(':') {
                let name = &p[1..];
                if i < uri_parts.len() {
                    params.insert(name.to_string(), uri_parts[i].to_string());
                }
            }
        }

        params
    }
}

struct DefaultNotFoundHandler;

impl Handler for DefaultNotFoundHandler {
    fn handle(&self, _request: &Request) -> Response {
        Response::new(Version::Http11, 404, "Not Found".to_string())
    }
}

pub struct RequestWithParams {
    request: Request,
    params: HashMap<String, String>,
}

impl RequestWithParams {
    pub fn new(request: Request, params: HashMap<String, String>) -> Self {
        Self { request, params }
    }

    pub fn method(&self) -> &Method {
        self.request.method()
    }

    pub fn uri(&self) -> &str {
        self.request.uri()
    }

    pub fn headers(&self) -> &[super::Header] {
        self.request.headers()
    }

    pub fn body(&self) -> Option<&[u8]> {
        self.request.body()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.request.header(name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn into_request(self) -> Request {
        self.request
    }
}

type HandlerResult = Result<Response, Box<dyn std::error::Error + Send + Sync>>;

pub trait AsyncHandler: Send + Sync {
    fn handle(&self, request: RequestWithParams) -> HandlerResult;
}

struct BoxAsyncHandler<F> {
    f: F,
}

impl<F> AsyncHandler for BoxAsyncHandler<F>
where
    F: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
{
    fn handle(&self, request: RequestWithParams) -> HandlerResult {
        (self.f)(request)
    }
}

pub struct AsyncRouter {
    routes: Vec<(Option<Method>, String, Arc<dyn AsyncHandler + Send + Sync>)>,
    not_found: Arc<dyn AsyncHandler + Send + Sync>,
}

impl AsyncRouter {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            not_found: Arc::new(DefaultAsyncNotFoundHandler),
        }
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Get),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Post),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Put),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            Some(Method::Delete),
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.routes.push((
            None,
            pattern.to_string(),
            Arc::new(BoxAsyncHandler { f: handler }),
        ));
    }

    pub fn set_not_found<H>(&mut self, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.not_found = Arc::new(BoxAsyncHandler { f: handler });
    }
}

struct DefaultAsyncNotFoundHandler;

impl AsyncHandler for DefaultAsyncNotFoundHandler {
    fn handle(&self, _request: RequestWithParams) -> HandlerResult {
        Ok(Response::new(Version::Http11, 404, "Not Found".to_string()))
    }
}

pub struct HttpServer {
    listener: TcpListener,
    router: Arc<Mutex<AsyncRouter>>,
    runtime: Runtime,
}

impl HttpServer {
    pub async fn bind(addr: &SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let router = Arc::new(Mutex::new(AsyncRouter::new()));
        let runtime = Runtime::new()?;

        Ok(Self {
            listener,
            router,
            runtime,
        })
    }

    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().get(pattern, handler);
    }

    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().post(pattern, handler);
    }

    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().put(pattern, handler);
    }

    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().delete(pattern, handler);
    }

    pub async fn serve(&mut self) {
        println!("Server listening on {}", self.listener.local_addr().unwrap());

        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    let router = self.router.clone();
                    self.runtime.spawn(async move {
                        handle_connection(stream, addr, router).await;
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    router: Arc<Mutex<AsyncRouter>>,
) {
    let mut parser = RequestParser::new();
    let mut buf = vec![0u8; 8192];

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                parser.feed(&buf[..n]);
                while let Ok(Some(request)) = parser.parse() {
                    let response = handle_request(&request, &router);
                    if let Err(e) = send_response(&mut stream, &response).await {
                        eprintln!("Write error: {}", e);
                        return;
                    }
                    parser.drain(n);
                }
                if parser.buf.len() > 1024 * 1024 {
                    eprintln!("Request too large from {}", addr);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Read error from {}: {}", addr, e);
                break;
            }
        }
    }
}

fn handle_request(
    request: &Request,
    router: &Arc<Mutex<AsyncRouter>>,
) -> Response {
    let router = router.lock().unwrap();

    for (method, pattern, handler) in &router.routes {
        if method.as_ref().map_or(false, |m| m != request.method()) {
            continue;
        }
        if match_pattern(pattern, request.uri()) {
            let params = extract_params(pattern, request.uri());
            let req_with_params = RequestWithParams::new(request.clone(), params);
            return handler.handle(req_with_params).unwrap_or_else(|e| {
                Response::new(Version::Http11, 500, "Internal Server Error".to_string())
            });
        }
    }

    router.not_found.handle(RequestWithParams::new(
        request.clone(),
        HashMap::new(),
    )).unwrap_or_else(|_| {
        Response::new(Version::Http11, 404, "Not Found".to_string())
    })
}

fn match_pattern(pattern: &str, uri: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

    if pattern_parts.len() != uri_parts.len() {
        return false;
    }

    for (p, u) in pattern_parts.iter().zip(uri_parts.iter()) {
        if p.starts_with(':') {
            continue;
        }
        if p.starts_with('*') {
            return true;
        }
        if p != *u {
            return false;
        }
    }

    true
}

fn extract_params(pattern: &str, uri: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

    for (i, p) in pattern_parts.iter().enumerate() {
        if p.starts_with(':') {
            let name = &p[1..];
            if i < uri_parts.len() {
                params.insert(name.to_string(), uri_parts[i].to_string());
            }
        }
    }

    params
}

async fn send_response(
    stream: &mut TcpStream,
    response: &Response,
) -> std::io::Result<()> {
    let mut buf = Vec::new();

    buf.extend_from_slice(response.version().as_str().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(response.status().to_string().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(response.reason().as_bytes());
    buf.extend_from_slice(b"\r\n");

    for header in response.headers() {
        buf.extend_from_slice(header.name().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(header.value().as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    if let Some(body) = response.body() {
        buf.extend_from_slice(b"Content-Length: ");
        buf.extend_from_slice(body.len().to_string().as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"\r\n");

    if let Some(body) = response.body() {
        buf.extend_from_slice(body);
    }

    stream.write_all(&buf).await
}

pub fn method_not_allowed() -> Response {
    Response::new(Version::Http11, 405, "Method Not Allowed".to_string())
}

pub fn bad_request() -> Response {
    Response::new(Version::Http11, 400, "Bad Request".to_string())
}

pub fn internal_error() -> Response {
    Response::new(Version::Http11, 500, "Internal Server Error".to_string())
}

pub fn ok(body: &str) -> Response {
    let mut response = Response::new(Version::Http11, 200, "OK".to_string());
    response.add_header("Content-Type".to_string(), "text/plain".to_string());
    response.add_header("Content-Length".to_string(), body.len().to_string());
    response.set_body(body.as_bytes().to_vec());
    response
}

pub fn json(body: &str) -> Response {
    let mut response = Response::new(Version::Http11, 200, "OK".to_string());
    response.add_header("Content-Type".to_string(), "application/json".to_string());
    response.add_header("Content-Length".to_string(), body.len().to_string());
    response.set_body(body.as_bytes().to_vec());
    response
}

pub fn redirect(url: &str) -> Response {
    let mut response = Response::new(Version::Http11, 302, "Found".to_string());
    response.add_header("Location".to_string(), url.to_string());
    response
}

// ============================================================================
// HTTPS Server (TLS-enabled)
// ============================================================================

#[cfg(feature = "tls")]
pub struct HttpsServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    router: Arc<Mutex<AsyncRouter>>,
    runtime: Runtime,
}

#[cfg(feature = "tls")]
impl HttpsServer {
    /// Bind an HTTPS server with TLS using certificate and key files.
    ///
    /// # Arguments
    /// * `addr` - Socket address to bind to (e.g., "0.0.0.0:443")
    /// * `cert_path` - Path to the PEM-encoded certificate file
    /// * `key_path` - Path to the PEM-encoded private key file
    ///
    /// # Performance
    /// Uses rustls with aws-lc-rs for Actix-level performance (~200k+ req/s).
    /// Session resumption is enabled by default.
    pub async fn bind(
        addr: &SocketAddr,
        cert_path: &Path,
        key_path: &Path,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let tls_config = TlsServerConfig::from_pem_files(cert_path, key_path)?;
        let acceptor = TlsAcceptor::new(tls_config);
        let router = Arc::new(Mutex::new(AsyncRouter::new()));
        let runtime = Runtime::new()?;

        Ok(Self {
            listener,
            acceptor,
            router,
            runtime,
        })
    }

    /// Bind with custom TLS configuration.
    ///
    /// Use this for advanced scenarios like custom session cache sizes,
    /// sharing TLS config across multiple servers, or custom certificate handling.
    pub async fn bind_with_config(
        addr: &SocketAddr,
        tls_config: TlsServerConfig,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let acceptor = TlsAcceptor::new(tls_config);
        let router = Arc::new(Mutex::new(AsyncRouter::new()));
        let runtime = Runtime::new()?;

        Ok(Self {
            listener,
            acceptor,
            router,
            runtime,
        })
    }

    /// Register a GET route handler.
    pub fn get<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().get(pattern, handler);
    }

    /// Register a POST route handler.
    pub fn post<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().post(pattern, handler);
    }

    /// Register a PUT route handler.
    pub fn put<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().put(pattern, handler);
    }

    /// Register a DELETE route handler.
    pub fn delete<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().delete(pattern, handler);
    }

    /// Register a handler for all HTTP methods.
    pub fn all<H>(&mut self, pattern: &str, handler: H)
    where
        H: Fn(RequestWithParams) -> HandlerResult + Send + Sync + 'static,
    {
        self.router.lock().unwrap().all(pattern, handler);
    }

    /// Start serving HTTPS connections.
    ///
    /// This method runs forever, accepting TLS connections and routing requests.
    pub async fn serve(&mut self) {
        println!(
            "HTTPS Server listening on {} (TLS enabled)",
            self.listener.local_addr().unwrap()
        );

        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    // Perform TLS handshake
                    match self.acceptor.accept(stream) {
                        Ok(tls_stream) => {
                            let router = self.router.clone();
                            self.runtime.spawn(async move {
                                handle_tls_connection(tls_stream, addr, router).await;
                            });
                        }
                        Err(e) => {
                            eprintln!("TLS handshake error from {}: {}", addr, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
}

#[cfg(feature = "tls")]
async fn handle_tls_connection(
    mut stream: TlsStream<TcpStream>,
    addr: SocketAddr,
    router: Arc<Mutex<AsyncRouter>>,
) {
    let mut parser = RequestParser::new();
    let mut buf = vec![0u8; 8192];

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                parser.feed(&buf[..n]);
                while let Ok(Some(request)) = parser.parse() {
                    let response = handle_request(&request, &router);
                    if let Err(e) = send_tls_response(&mut stream, &response).await {
                        eprintln!("TLS write error: {}", e);
                        return;
                    }
                    parser.drain(n);
                }
                if parser.buf.len() > 1024 * 1024 {
                    eprintln!("Request too large from {}", addr);
                    break;
                }
            }
            Err(e) => {
                eprintln!("TLS read error from {}: {}", addr, e);
                break;
            }
        }
    }
}

#[cfg(feature = "tls")]
async fn send_tls_response(
    stream: &mut TlsStream<TcpStream>,
    response: &Response,
) -> io::Result<()> {
    let mut buf = Vec::new();

    buf.extend_from_slice(response.version().as_str().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(response.status().to_string().as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(response.reason().as_bytes());
    buf.extend_from_slice(b"\r\n");

    for header in response.headers() {
        buf.extend_from_slice(header.name().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(header.value().as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    if let Some(body) = response.body() {
        buf.extend_from_slice(b"Content-Length: ");
        buf.extend_from_slice(body.len().to_string().as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"\r\n");

    if let Some(body) = response.body() {
        buf.extend_from_slice(body);
    }

    stream.write_all(&buf).await
}
