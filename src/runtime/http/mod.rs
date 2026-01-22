pub mod server;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::str;

#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
    Patch,
    Extension(String),
}

impl Method {
    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Connect => "CONNECT",
            Method::Options => "OPTIONS",
            Method::Trace => "TRACE",
            Method::Patch => "PATCH",
            Method::Extension(s) => s.as_str(),
        }
    }
}

impl TryFrom<&[u8]> for Method {
    type Error = HttpError;

    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        match buf {
            b"GET" => Ok(Method::Get),
            b"HEAD" => Ok(Method::Head),
            b"POST" => Ok(Method::Post),
            b"PUT" => Ok(Method::Put),
            b"DELETE" => Ok(Method::Delete),
            b"CONNECT" => Ok(Method::Connect),
            b"OPTIONS" => Ok(Method::Options),
            b"TRACE" => Ok(Method::Trace),
            b"PATCH" => Ok(Method::Patch),
            _ => Ok(Method::Extension(String::from_utf8_lossy(buf).to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Version {
    Http10,
    Http11,
    Http2,
    Http3,
    Extension(String),
}

impl Version {
    pub fn as_str(&self) -> &str {
        match self {
            Version::Http10 => "HTTP/1.0",
            Version::Http11 => "HTTP/1.1",
            Version::Http2 => "HTTP/2",
            Version::Http3 => "HTTP/3",
            Version::Extension(s) => s.as_str(),
        }
    }
}

impl TryFrom<&[u8]> for Version {
    type Error = HttpError;

    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        match buf {
            b"HTTP/1.0" => Ok(Version::Http10),
            b"HTTP/1.1" => Ok(Version::Http11),
            b"HTTP/2" => Ok(Version::Http2),
            b"HTTP/3" => Ok(Version::Http3),
            _ => Ok(Version::Extension(String::from_utf8_lossy(buf).to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    name: String,
    value: String,
}

impl Header {
    pub fn new(name: String, value: String) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn into_parts(self) -> (String, String) {
        (self.name, self.value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    method: Method,
    uri: String,
    version: Version,
    headers: Vec<Header>,
    body: Option<Vec<u8>>,
}

impl Request {
    pub fn new(method: Method, uri: String, version: Version) -> Self {
        Self {
            method,
            uri,
            version,
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn add_header(&mut self, name: String, value: String) {
        self.headers.push(Header::new(name, value));
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = Some(body);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    version: Version,
    status: u16,
    reason: String,
    headers: Vec<Header>,
    body: Option<Vec<u8>>,
}

impl Response {
    pub fn new(version: Version, status: u16, reason: String) -> Self {
        Self {
            version,
            status,
            reason,
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn add_header(&mut self, name: String, value: String) {
        self.headers.push(Header::new(name, value));
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = Some(body);
    }

    pub fn status_text(status: u16) -> &'static str {
        match status {
            100 => "Continue",
            101 => "Switching Protocols",
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            203 => "Non-Authoritative Information",
            204 => "No Content",
            205 => "Reset Content",
            206 => "Partial Content",
            300 => "Multiple Choices",
            301 => "Moved Permanently",
            302 => "Found",
            303 => "See Other",
            304 => "Not Modified",
            305 => "Use Proxy",
            307 => "Temporary Redirect",
            308 => "Permanent Redirect",
            400 => "Bad Request",
            401 => "Unauthorized",
            402 => "Payment Required",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            406 => "Not Acceptable",
            407 => "Proxy Authentication Required",
            408 => "Request Timeout",
            409 => "Conflict",
            410 => "Gone",
            411 => "Length Required",
            412 => "Precondition Failed",
            413 => "Payload Too Large",
            414 => "URI Too Long",
            415 => "Unsupported Media Type",
            416 => "Range Not Satisfiable",
            417 => "Expectation Failed",
            418 => "I'm a teapot",
            421 => "Misdirected Request",
            422 => "Unprocessable Entity",
            426 => "Upgrade Required",
            428 => "Precondition Required",
            429 => "Too Many Requests",
            431 => "Request Header Fields Too Large",
            451 => "Unavailable For Legal Reasons",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            505 => "HTTP Version Not Supported",
            506 => "Variant Also Negotiates",
            507 => "Insufficient Storage",
            508 => "Loop Detected",
            510 => "Not Extended",
            511 => "Network Authentication Required",
            _ => "Unknown",
        }
    }
}

#[derive(Debug)]
pub enum HttpError {
    InvalidMethod,
    InvalidUri,
    InvalidVersion,
    InvalidHeader,
    InvalidRequest,
    InvalidResponse,
    Incomplete,
    TooLarge,
}

const MAX_HEADER_LINE: usize = 8192;
const MAX_HEADERS: usize = 256;
const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

pub struct RequestParser {
    buf: Vec<u8>,
    pos: usize,
    max_header_line: usize,
    max_headers: usize,
    max_body_size: usize,
}

impl RequestParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(16384),
            pos: 0,
            max_header_line: MAX_HEADER_LINE,
            max_headers: MAX_HEADERS,
            max_body_size: MAX_BODY_SIZE,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            pos: 0,
            max_header_line: MAX_HEADER_LINE,
            max_headers: MAX_HEADERS,
            max_body_size: MAX_BODY_SIZE,
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn parse(&mut self) -> Result<Option<Request>, HttpError> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }

        let end = self.find_end_of_line(self.pos)?;
        if end.is_none() {
            if self.buf.len() > self.max_header_line {
                return Err(HttpError::TooLarge);
            }
            return Ok(None);
        }
        let end = end.unwrap();

        let line = &self.buf[self.pos..end];
        self.pos = end + 2;

        let (method, uri, version) = self.parse_request_line(line)?;

        let mut request = Request::new(method, uri, version);
        let mut content_length: usize = 0;

        while self.pos < self.buf.len() {
            let end = self.find_end_of_line(self.pos)?;
            if end.is_none() {
                if self.buf.len() - self.pos > self.max_header_line {
                    return Err(HttpError::TooLarge);
                }
                break;
            }
            let end = end.unwrap();

            if self.pos == end {
                self.pos = end + 2;
                break;
            }

            let line = &self.buf[self.pos..end];
            self.pos = end + 2;

            if request.headers.len() >= self.max_headers {
                return Err(HttpError::TooLarge);
            }

            let (name, value) = self.parse_header_line(line)?;
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.parse().unwrap_or(0);
            }
            request.add_header(name, value);
        }

        if content_length > 0 {
            if content_length > self.max_body_size {
                return Err(HttpError::TooLarge);
            }
            let body_start = self.pos;
            let body_end = body_start + content_length;

            if body_end > self.buf.len() {
                return Ok(None);
            }

            let body = self.buf[body_start..body_end].to_vec();
            request.set_body(body);
            self.pos = body_end;
        }

        Ok(Some(request))
    }

    fn find_end_of_line(&self, start: usize) -> Result<Option<usize>, HttpError> {
        let mut pos = start;
        while pos < self.buf.len() {
            match self.buf[pos] {
                b'\r' => {
                    if pos + 1 < self.buf.len() && self.buf[pos + 1] == b'\n' {
                        return Ok(Some(pos));
                    }
                    return Err(HttpError::InvalidRequest);
                }
                b'\n' => return Ok(Some(pos)),
                _ => {
                    if pos - start > self.max_header_line {
                        return Err(HttpError::TooLarge);
                    }
                }
            }
            pos += 1;
        }
        Ok(None)
    }

    fn parse_request_line(&self, line: &[u8]) -> Result<(Method, String, Version), HttpError> {
        let mut parts = line.splitn(3, |&c| c == b' ');

        let method = parts.next().ok_or(HttpError::InvalidMethod)?;
        let method = Method::try_from(method)?;

        let uri = parts.next().ok_or(HttpError::InvalidUri)?;
        let uri = String::from_utf8_lossy(uri).to_string();

        let version = parts.next().ok_or(HttpError::InvalidVersion)?;
        let version = Version::try_from(version)?;

        Ok((method, uri, version))
    }

    fn parse_header_line(&self, line: &[u8]) -> Result<(String, String), HttpError> {
        let colon_pos = line
            .iter()
            .position(|&c| c == b':')
            .ok_or(HttpError::InvalidHeader)?;

        let name = String::from_utf8_lossy(&line[..colon_pos]).to_string();
        let value = if colon_pos + 2 < line.len() {
            String::from_utf8_lossy(&line[colon_pos + 2..]).to_string()
        } else {
            String::new()
        };

        Ok((name, value))
    }

    pub fn drain(&mut self, n: usize) {
        self.buf.drain(..n);
        self.pos = self.pos.saturating_sub(n);
    }
}

pub struct ResponseParser {
    buf: Vec<u8>,
    pos: usize,
    max_header_line: usize,
    max_headers: usize,
    max_body_size: usize,
    chunked: bool,
    chunk_remaining: usize,
}

impl ResponseParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(16384),
            pos: 0,
            max_header_line: MAX_HEADER_LINE,
            max_headers: MAX_HEADERS,
            max_body_size: MAX_BODY_SIZE,
            chunked: false,
            chunk_remaining: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            pos: 0,
            max_header_line: MAX_HEADER_LINE,
            max_headers: MAX_HEADERS,
            max_body_size: MAX_BODY_SIZE,
            chunked: false,
            chunk_remaining: 0,
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn parse(&mut self) -> Result<Option<Response>, HttpError> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }

        if self.chunked {
            return self.parse_chunked_body();
        }

        let end = self.find_end_of_line(self.pos)?;
        if end.is_none() {
            if self.buf.len() > self.max_header_line {
                return Err(HttpError::TooLarge);
            }
            return Ok(None);
        }
        let end = end.unwrap();

        let line = &self.buf[self.pos..end];
        self.pos = end + 2;

        let (version, status, reason) = self.parse_status_line(line)?;

        let mut response = Response::new(version, status, reason);
        let mut content_length: Option<usize> = None;

        while self.pos < self.buf.len() {
            let end = self.find_end_of_line(self.pos)?;
            if end.is_none() {
                if self.buf.len() - self.pos > self.max_header_line {
                    return Err(HttpError::TooLarge);
                }
                break;
            }
            let end = end.unwrap();

            if self.pos == end {
                self.pos = end + 2;
                break;
            }

            let line = &self.buf[self.pos..end];
            self.pos = end + 2;

            if response.headers.len() >= self.max_headers {
                return Err(HttpError::TooLarge);
            }

            let (name, value) = self.parse_header_line(line)?;
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.parse().unwrap_or(0));
            } else if name.eq_ignore_ascii_case("Transfer-Encoding")
                && value.to_lowercase().contains("chunked")
            {
                self.chunked = true;
            }
            response.add_header(name, value);
        }

        match (self.chunked, content_length) {
            (true, _) => {}
            (false, Some(len)) => {
                if len > self.max_body_size {
                    return Err(HttpError::TooLarge);
                }
                let body_start = self.pos;
                let body_end = body_start + len;

                if body_end > self.buf.len() {
                    return Ok(None);
                }

                let body = self.buf[body_start..body_end].to_vec();
                response.set_body(body);
                self.pos = body_end;
            }
            (false, None) => {
                if self.buf.len() > self.pos {
                    let body = self.buf[self.pos..].to_vec();
                    if body.len() > self.max_body_size {
                        return Err(HttpError::TooLarge);
                    }
                    response.set_body(body);
                    self.pos = self.buf.len();
                }
            }
        }

        Ok(Some(response))
    }

    fn parse_chunked_body(&mut self) -> Result<Option<Response>, HttpError> {
        if self.chunk_remaining == 0 {
            let end = self.find_end_of_line(self.pos)?;
            if end.is_none() {
                return Ok(None);
            }
            let end = end.unwrap();

            let line = &self.buf[self.pos..end];
            self.pos = end + 2;

            let size_str = str::from_utf8(line).map_err(|_| HttpError::InvalidResponse)?;
            let size = usize::from_str_radix(size_str.trim(), 16)
                .map_err(|_| HttpError::InvalidResponse)?;

            if size == 0 {
                return Ok(None);
            }

            self.chunk_remaining = size;
        }

        if self.chunk_remaining > 0 {
            let available = self.buf.len() - self.pos;
            let to_read = std::cmp::min(self.chunk_remaining, available);

            if to_read == 0 {
                return Ok(None);
            }

            self.chunk_remaining -= to_read;
            self.pos += to_read;

            if self.chunk_remaining == 0 {
                if self.pos + 2 <= self.buf.len() {
                    self.pos += 2;
                }
            }
        }

        Ok(None)
    }

    fn find_end_of_line(&self, start: usize) -> Result<Option<usize>, HttpError> {
        let mut pos = start;
        while pos < self.buf.len() {
            match self.buf[pos] {
                b'\r' => {
                    if pos + 1 < self.buf.len() && self.buf[pos + 1] == b'\n' {
                        return Ok(Some(pos));
                    }
                    return Err(HttpError::InvalidResponse);
                }
                b'\n' => return Ok(Some(pos)),
                _ => {
                    if pos - start > self.max_header_line {
                        return Err(HttpError::TooLarge);
                    }
                }
            }
            pos += 1;
        }
        Ok(None)
    }

    fn parse_status_line(&self, line: &[u8]) -> Result<(Version, u16, String), HttpError> {
        let mut parts = line.splitn(3, |&c| c == b' ');

        let version = parts.next().ok_or(HttpError::InvalidVersion)?;
        let version = Version::try_from(version)?;

        let status = parts.next().ok_or(HttpError::InvalidResponse)?;
        let status = str::from_utf8(status)
            .map_err(|_| HttpError::InvalidResponse)?
            .parse()
            .map_err(|_| HttpError::InvalidResponse)?;

        let reason = parts
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_else(|| Response::status_text(status).to_string());

        Ok((version, status, reason))
    }

    fn parse_header_line(&self, line: &[u8]) -> Result<(String, String), HttpError> {
        let colon_pos = line
            .iter()
            .position(|&c| c == b':')
            .ok_or(HttpError::InvalidHeader)?;

        let name = String::from_utf8_lossy(&line[..colon_pos]).to_string();
        let value = if colon_pos + 2 < line.len() {
            String::from_utf8_lossy(&line[colon_pos + 2..]).to_string()
        } else {
            String::new()
        };

        Ok((name, value))
    }

    pub fn drain(&mut self, n: usize) {
        self.buf.drain(..n);
        self.pos = self.pos.saturating_sub(n);
        self.chunk_remaining = self.chunk_remaining.saturating_sub(n);
    }
}
