//! Multi-Core HTTP Server using SO_REUSEPORT
//!
//! Architecture:
//! - Each worker thread accepts connections directly (kernel load balancing)
//! - SO_REUSEPORT allows multiple listeners on same port
//! - Edge-triggered kqueue/epoll per worker
//! - No channel overhead - direct accept
//!
//! Usage:
//!   cargo run --release --example http_server_reuseport
//!
//! Benchmark:
//!   wrk -t8 -c400 -d30s http://localhost:8080/

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

#[cfg(target_os = "macos")]
use libc::{EV_ADD, EV_CLEAR, EV_DELETE, EV_ENABLE, EVFILT_READ, EVFILT_WRITE, kevent, kqueue};

#[cfg(target_os = "linux")]
use libc::{
    EPOLL_CLOEXEC, EPOLLERR, EPOLLET, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP, epoll_create1,
    epoll_ctl, epoll_event, epoll_wait,
};

const HTTP_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Content-Length: 13\r\n\
Connection: keep-alive\r\n\
\r\n\
Hello, World!";

const BUF_SIZE: usize = 8192;
const MAX_EVENTS: usize = 1024;

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
        stream.set_nodelay(true)?;
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

    fn read_all(&mut self) -> io::Result<usize> {
        let mut total = 0;
        loop {
            if self.read_pos >= self.read_buf.len() {
                if self.read_buf.len() < 65536 {
                    self.read_buf.resize(self.read_buf.len() * 2, 0);
                } else {
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

    fn process_requests(&mut self, count: &AtomicU64) -> usize {
        let mut responses = 0;
        let mut search_pos = 0;

        while search_pos + 4 <= self.read_pos {
            let data = &self.read_buf[search_pos..self.read_pos];
            if let Some(pos) = find_header_end(data) {
                self.write_buf.extend_from_slice(HTTP_RESPONSE);
                self.write_len = self.write_buf.len();
                responses += 1;
                count.fetch_add(1, Ordering::Relaxed);
                search_pos += pos + 4;
            } else {
                break;
            }
        }

        if search_pos > 0 {
            self.read_buf.copy_within(search_pos..self.read_pos, 0);
            self.read_pos -= search_pos;
        }
        responses
    }

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
        self.write_buf.clear();
        self.write_pos = 0;
        self.write_len = 0;
        Ok(true)
    }
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
        {
            return Some(i);
        }
    }
    None
}

/// Create a listener with SO_REUSEPORT enabled
fn create_reuseport_listener(addr: &SocketAddr) -> io::Result<RawFd> {
    use libc::{
        AF_INET, SO_REUSEADDR, SO_REUSEPORT, SOCK_STREAM, SOL_SOCKET, bind, listen, setsockopt,
        sockaddr_in, socket,
    };

    unsafe {
        let fd = socket(AF_INET, SOCK_STREAM, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        // Enable SO_REUSEADDR
        let one: i32 = 1;
        if setsockopt(
            fd,
            SOL_SOCKET,
            SO_REUSEADDR,
            &one as *const i32 as *const _,
            4,
        ) < 0
        {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        // Enable SO_REUSEPORT - allows multiple processes/threads to bind to same port
        if setsockopt(
            fd,
            SOL_SOCKET,
            SO_REUSEPORT,
            &one as *const i32 as *const _,
            4,
        ) < 0
        {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        // Bind to address
        let sockaddr = sockaddr_in {
            sin_len: std::mem::size_of::<sockaddr_in>() as u8,
            sin_family: AF_INET as u8,
            sin_port: addr.port().to_be(),
            sin_addr: libc::in_addr { s_addr: 0 }, // INADDR_ANY
            sin_zero: [0; 8],
        };

        if bind(
            fd,
            &sockaddr as *const sockaddr_in as *const _,
            std::mem::size_of::<sockaddr_in>() as u32,
        ) < 0
        {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        // Listen with large backlog
        if listen(fd, 1024) < 0 {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        // Set non-blocking
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);

        Ok(fd)
    }
}

/// Accept a connection from the listener
fn accept_connection(listener_fd: RawFd) -> io::Result<TcpStream> {
    use std::os::unix::io::FromRawFd;

    unsafe {
        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as u32;

        let fd = libc::accept(listener_fd, &mut addr as *mut _ as *mut _, &mut len);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(TcpStream::from_raw_fd(fd))
    }
}

// Worker runs its own accept loop
struct Worker {
    id: usize,
    addr: SocketAddr,
    count: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    #[cfg(target_os = "macos")]
    fn run(self) {
        // Each worker creates its own listener with SO_REUSEPORT
        let listener_fd = match create_reuseport_listener(&self.addr) {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("Worker {} failed to create listener: {}", self.id, e);
                return;
            }
        };

        let kq = unsafe { kqueue() };
        if kq < 0 {
            unsafe {
                libc::close(listener_fd);
            }
            return;
        }

        // Register listener for accept events
        register_kqueue(kq, listener_fd, EVFILT_READ).ok();

        let mut connections: HashMap<RawFd, Connection> = HashMap::new();
        let mut events: Vec<libc::kevent> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

        while !self.shutdown.load(Ordering::Relaxed) {
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
                continue;
            }

            for i in 0..n as usize {
                let fd = events[i].ident as RawFd;
                let filter = events[i].filter;

                if fd == listener_fd {
                    // Accept new connections (drain all pending)
                    loop {
                        match accept_connection(listener_fd) {
                            Ok(stream) => {
                                if let Ok(conn) = Connection::new(stream) {
                                    let conn_fd = conn.fd();
                                    register_kqueue_rw(kq, conn_fd).ok();
                                    connections.insert(conn_fd, conn);
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(_) => break,
                        }
                    }
                } else if let Some(conn) = connections.get_mut(&fd) {
                    let mut should_close = false;

                    if filter == EVFILT_READ as i16 {
                        match conn.read_all() {
                            Ok(_) => {
                                conn.process_requests(&self.count);
                            }
                            Err(_) => should_close = true,
                        }
                    }

                    if filter == EVFILT_WRITE as i16 || conn.write_len > 0 {
                        if conn.write_all().is_err() {
                            should_close = true;
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
            libc::close(listener_fd);
            libc::close(kq);
        }
    }

    #[cfg(target_os = "linux")]
    fn run(self) {
        let listener_fd = match create_reuseport_listener(&self.addr) {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("Worker {} failed to create listener: {}", self.id, e);
                return;
            }
        };

        let epoll_fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            unsafe {
                libc::close(listener_fd);
            }
            return;
        }

        // Register listener
        register_epoll(epoll_fd, listener_fd, EPOLLIN as u32).ok();

        let mut connections: HashMap<RawFd, Connection> = HashMap::new();
        let mut events: Vec<epoll_event> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

        while !self.shutdown.load(Ordering::Relaxed) {
            let n = unsafe { epoll_wait(epoll_fd, events.as_mut_ptr(), MAX_EVENTS as i32, 1000) };
            if n < 0 {
                continue;
            }

            for i in 0..n as usize {
                let fd = events[i].u64 as RawFd;
                let ev = events[i].events;

                if fd == listener_fd {
                    loop {
                        match accept_connection(listener_fd) {
                            Ok(stream) => {
                                if let Ok(conn) = Connection::new(stream) {
                                    let conn_fd = conn.fd();
                                    register_epoll_rw(epoll_fd, conn_fd).ok();
                                    connections.insert(conn_fd, conn);
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(_) => break,
                        }
                    }
                } else if let Some(conn) = connections.get_mut(&fd) {
                    let mut should_close = false;

                    if ev & EPOLLIN as u32 != 0 {
                        match conn.read_all() {
                            Ok(_) => {
                                conn.process_requests(&self.count);
                            }
                            Err(_) => should_close = true,
                        }
                    }

                    if ev & EPOLLOUT as u32 != 0 || conn.write_len > 0 {
                        if conn.write_all().is_err() {
                            should_close = true;
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
            libc::close(listener_fd);
            libc::close(epoll_fd);
        }
    }
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
    if unsafe { kevent(kq, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
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
    if unsafe {
        kevent(
            kq,
            events.as_ptr(),
            2,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
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

#[cfg(target_os = "linux")]
fn register_epoll(epoll_fd: RawFd, fd: RawFd, events: u32) -> io::Result<()> {
    let mut event = epoll_event {
        events: events | EPOLLET as u32,
        u64: fd as u64,
    };
    if unsafe { epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn register_epoll_rw(epoll_fd: RawFd, fd: RawFd) -> io::Result<()> {
    let mut event = epoll_event {
        events: EPOLLIN as u32 | EPOLLOUT as u32 | EPOLLET as u32 | EPOLLRDHUP as u32,
        u64: fd as u64,
    };
    if unsafe { epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) } < 0 {
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

fn main() -> io::Result<()> {
    let num_workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    println!("==============================================");
    println!("  SO_REUSEPORT HTTP Server ({} workers)", num_workers);
    println!("  Kernel Load-Balanced + Edge-Triggered I/O");
    println!("==============================================");
    println!();

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();

    println!("Server listening on http://{}", addr);
    println!();
    println!("Benchmark:");
    println!("  wrk -t{} -c400 -d30s http://localhost:8080/", num_workers);
    println!();

    let count = Arc::new(AtomicU64::new(0));
    let shutdown = Arc::new(AtomicBool::new(false));

    let mut handles = Vec::new();

    for id in 0..num_workers {
        let worker = Worker {
            id,
            addr,
            count: count.clone(),
            shutdown: shutdown.clone(),
        };
        handles.push(thread::spawn(move || worker.run()));
    }

    // Stats thread
    let count_clone = count.clone();
    thread::spawn(move || {
        let mut last = 0u64;
        let start = std::time::Instant::now();
        loop {
            thread::sleep(std::time::Duration::from_secs(5));
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

    // Wait for workers
    for handle in handles {
        let _ = handle.join();
    }

    Ok(())
}
