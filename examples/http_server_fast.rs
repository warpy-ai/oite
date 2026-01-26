//! High-Performance HTTP Server using Edge-Triggered I/O
//!
//! This server uses edge-triggered kqueue/epoll for maximum performance.
//!
//! Key optimizations:
//! - Edge-triggered I/O (EV_CLEAR/EPOLLET)
//! - Non-blocking sockets with TCP_NODELAY
//! - Pre-allocated buffers
//! - HTTP pipelining (multiple requests per read)
//! - Keep-alive connections
//!
//! Usage:
//!   cargo run --release --example http_server_fast
//!
//! Benchmark:
//!   wrk -t4 -c400 -d30s http://localhost:8080/

#![allow(dead_code)]
#![allow(unused_must_use)]
#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(target_os = "macos")]
use libc::{EV_ADD, EV_CLEAR, EV_DELETE, EV_ENABLE, EVFILT_READ, EVFILT_WRITE, kevent, kqueue};

#[cfg(target_os = "linux")]
use libc::{
    EPOLL_CLOEXEC, EPOLLERR, EPOLLET, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP, epoll_create1,
    epoll_ctl, epoll_event, epoll_wait,
};

// Pre-computed HTTP response (no allocations per request)
const HTTP_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Content-Length: 13\r\n\
Connection: keep-alive\r\n\
\r\n\
Hello, World!";

const BUF_SIZE: usize = 8192;
const MAX_EVENTS: usize = 1024;

/// Connection state with pre-allocated buffers
struct Connection {
    stream: TcpStream,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
    write_len: usize,
}

impl Connection {
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?; // Disable Nagle's algorithm

        Ok(Self {
            stream,
            read_buf: vec![0u8; BUF_SIZE],
            write_buf: Vec::with_capacity(BUF_SIZE),
            read_pos: 0,
            write_pos: 0,
            write_len: 0,
        })
    }

    fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    /// Read as much data as possible (edge-triggered requires draining)
    fn read_all(&mut self) -> io::Result<usize> {
        let mut total = 0;
        loop {
            // Ensure buffer has space
            if self.read_pos >= self.read_buf.len() {
                if self.read_buf.len() < 65536 {
                    self.read_buf.resize(self.read_buf.len() * 2, 0);
                } else {
                    // Buffer full, process what we have
                    break;
                }
            }

            match self.stream.read(&mut self.read_buf[self.read_pos..]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF")),
                Ok(n) => {
                    self.read_pos += n;
                    total += n;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }

    /// Process all complete HTTP requests in buffer (pipelining)
    fn process_requests(&mut self, count: &AtomicU64) -> usize {
        let mut responses = 0;
        let mut search_pos = 0;

        // Find complete requests (ending with \r\n\r\n)
        while search_pos + 4 <= self.read_pos {
            // Simple scan for end of headers
            let data = &self.read_buf[search_pos..self.read_pos];
            if let Some(pos) = find_header_end(data) {
                // Found complete request, queue response
                self.write_buf.extend_from_slice(HTTP_RESPONSE);
                self.write_len = self.write_buf.len();
                responses += 1;
                count.fetch_add(1, Ordering::Relaxed);
                search_pos += pos + 4;
            } else {
                break;
            }
        }

        // Compact buffer - move remaining data to start
        if search_pos > 0 {
            self.read_buf.copy_within(search_pos..self.read_pos, 0);
            self.read_pos -= search_pos;
        }

        responses
    }

    /// Write as much as possible (edge-triggered requires flushing)
    fn write_all(&mut self) -> io::Result<bool> {
        while self.write_pos < self.write_len {
            match self
                .stream
                .write(&self.write_buf[self.write_pos..self.write_len])
            {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "write zero")),
                Ok(n) => self.write_pos += n,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) => return Err(e),
            }
        }

        // All written, reset buffers
        self.write_buf.clear();
        self.write_pos = 0;
        self.write_len = 0;
        Ok(true)
    }
}

/// Fast scan for \r\n\r\n
fn find_header_end(data: &[u8]) -> Option<usize> {
    // Simple but fast scan
    (0..data.len().saturating_sub(3)).find(|&i| {
        data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
    })
}

// ============================================================================
// macOS kqueue implementation
// ============================================================================

#[cfg(target_os = "macos")]
fn run_server(
    listener: TcpListener,
    count: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<()> {
    let kq = unsafe { kqueue() };
    if kq < 0 {
        return Err(io::Error::last_os_error());
    }

    // Register listener for accept events
    let listener_fd = listener.as_raw_fd();
    register_kqueue(kq, listener_fd, EVFILT_READ)?;

    let mut connections: HashMap<RawFd, Connection> = HashMap::new();
    let mut events: Vec<libc::kevent> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

    while !shutdown.load(Ordering::Relaxed) {
        let timeout = libc::timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        let n = unsafe {
            kevent(
                kq,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                MAX_EVENTS as i32,
                &timeout,
            )
        };

        if n < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }

        for i in 0..n as usize {
            let fd = events[i].ident as RawFd;
            let filter = events[i].filter;

            if fd == listener_fd {
                // Accept new connections
                loop {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            let conn = Connection::new(stream)?;
                            let conn_fd = conn.fd();
                            register_kqueue_rw(kq, conn_fd)?;
                            connections.insert(conn_fd, conn);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => eprintln!("Accept error: {}", e),
                    }
                }
            } else if let Some(conn) = connections.get_mut(&fd) {
                let mut should_close = false;

                if filter == EVFILT_READ {
                    match conn.read_all() {
                        Ok(_) => {
                            conn.process_requests(&count);
                        }
                        Err(_) => should_close = true,
                    }
                }

                if filter == EVFILT_WRITE || conn.write_len > 0 {
                    match conn.write_all() {
                        Ok(_) => {}
                        Err(_) => should_close = true,
                    }
                }

                if should_close {
                    deregister_kqueue(kq, fd);
                    connections.remove(&fd);
                }
            }
        }
    }

    unsafe {
        libc::close(kq);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn register_kqueue(kq: RawFd, fd: RawFd, filter: i16) -> io::Result<()> {
    let event = libc::kevent {
        ident: fd as usize,
        filter,
        flags: EV_ADD | EV_ENABLE | EV_CLEAR,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let result = unsafe { kevent(kq, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn register_kqueue_rw(kq: RawFd, fd: RawFd) -> io::Result<()> {
    let events = [
        libc::kevent {
            ident: fd as usize,
            filter: EVFILT_READ,
            flags: EV_ADD | EV_ENABLE | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        },
        libc::kevent {
            ident: fd as usize,
            filter: EVFILT_WRITE,
            flags: EV_ADD | EV_ENABLE | EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        },
    ];
    let result = unsafe {
        kevent(
            kq,
            events.as_ptr(),
            2,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn deregister_kqueue(kq: RawFd, fd: RawFd) {
    let events = [
        libc::kevent {
            ident: fd as usize,
            filter: EVFILT_READ,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        },
        libc::kevent {
            ident: fd as usize,
            filter: EVFILT_WRITE,
            flags: EV_DELETE,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        },
    ];
    unsafe {
        kevent(
            kq,
            events.as_ptr(),
            2,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        );
    }
}

// ============================================================================
// Linux epoll implementation
// ============================================================================

#[cfg(target_os = "linux")]
fn run_server(
    listener: TcpListener,
    count: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<()> {
    let epoll_fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
    if epoll_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // Register listener
    let listener_fd = listener.as_raw_fd();
    register_epoll(epoll_fd, listener_fd, EPOLLIN as u32)?;

    let mut connections: HashMap<RawFd, Connection> = HashMap::new();
    let mut events: Vec<epoll_event> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

    while !shutdown.load(Ordering::Relaxed) {
        let n = unsafe { epoll_wait(epoll_fd, events.as_mut_ptr(), MAX_EVENTS as i32, 1000) };

        if n < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }

        for i in 0..n as usize {
            let fd = events[i].u64 as RawFd;
            let ev = events[i].events;

            if fd == listener_fd {
                // Accept new connections
                loop {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            let conn = Connection::new(stream)?;
                            let conn_fd = conn.fd();
                            register_epoll_rw(epoll_fd, conn_fd)?;
                            connections.insert(conn_fd, conn);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => eprintln!("Accept error: {}", e),
                    }
                }
            } else if let Some(conn) = connections.get_mut(&fd) {
                let mut should_close = false;

                if ev & EPOLLIN as u32 != 0 {
                    match conn.read_all() {
                        Ok(_) => {
                            conn.process_requests(&count);
                        }
                        Err(_) => should_close = true,
                    }
                }

                if ev & EPOLLOUT as u32 != 0 || conn.write_len > 0 {
                    match conn.write_all() {
                        Ok(_) => {}
                        Err(_) => should_close = true,
                    }
                }

                if ev & (EPOLLRDHUP | EPOLLERR | EPOLLHUP) as u32 != 0 {
                    should_close = true;
                }

                if should_close {
                    deregister_epoll(epoll_fd, fd);
                    connections.remove(&fd);
                }
            }
        }
    }

    unsafe {
        libc::close(epoll_fd);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn register_epoll(epoll_fd: RawFd, fd: RawFd, events: u32) -> io::Result<()> {
    let mut event = epoll_event {
        events: events | EPOLLET as u32,
        u64: fd as u64,
    };
    let result = unsafe { epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn register_epoll_rw(epoll_fd: RawFd, fd: RawFd) -> io::Result<()> {
    let events = EPOLLIN as u32 | EPOLLOUT as u32 | EPOLLET as u32 | EPOLLRDHUP as u32;
    let mut event = epoll_event {
        events,
        u64: fd as u64,
    };
    let result = unsafe { epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn deregister_epoll(epoll_fd: RawFd, fd: RawFd) {
    unsafe {
        epoll_ctl(epoll_fd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut());
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() -> io::Result<()> {
    println!("==============================================");
    println!("  High-Performance HTTP Server");
    println!("  Edge-Triggered I/O (kqueue/epoll)");
    println!("==============================================");
    println!();

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;

    println!("Server listening on http://{}", addr);
    println!();
    println!("Benchmark with:");
    println!("  wrk -t4 -c400 -d30s http://localhost:8080/");
    println!();

    let count = Arc::new(AtomicU64::new(0));
    let shutdown = Arc::new(AtomicBool::new(false));

    // Stats thread
    let count_clone = count.clone();
    std::thread::spawn(move || {
        let mut last = 0u64;
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let current = count_clone.load(Ordering::Relaxed);
            let rps = (current - last) / 5;
            let elapsed = start.elapsed().as_secs().max(1);
            let avg = current / elapsed;
            println!(
                "[Stats] Total: {} | Last 5s: {} req/s | Avg: {} req/s",
                current, rps, avg
            );
            last = current;
        }
    });

    // Run server
    run_server(listener, count, shutdown)?;

    Ok(())
}
