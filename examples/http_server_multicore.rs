//! Multi-Core HTTP Server using Edge-Triggered I/O
//!
//! Architecture:
//! - Single acceptor thread distributes connections to workers
//! - One event loop per CPU core
//! - Edge-triggered kqueue/epoll
//! - Pre-allocated buffers, HTTP pipelining
//!
//! Usage:
//!   cargo run --release --example http_server_multicore
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
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
    (0..data.len().saturating_sub(3)).find(|&i| {
        data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
    })
}

// Worker receives connections via channel
struct Worker {
    id: usize,
    receiver: mpsc::Receiver<TcpStream>,
    count: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    #[cfg(target_os = "macos")]
    fn run(self) {
        let kq = unsafe { kqueue() };
        if kq < 0 {
            return;
        }

        // Create a pipe for wakeup when new connections arrive
        let mut pipe_fds = [0i32; 2];
        unsafe {
            libc::pipe(pipe_fds.as_mut_ptr());
        }
        let wake_read = pipe_fds[0];
        let wake_write = pipe_fds[1];
        unsafe {
            libc::fcntl(wake_read, libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(wake_write, libc::F_SETFL, libc::O_NONBLOCK);
        }
        register_kqueue(kq, wake_read, EVFILT_READ).ok();

        let mut connections: HashMap<RawFd, Connection> = HashMap::new();
        let mut events: Vec<libc::kevent> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

        while !self.shutdown.load(Ordering::Relaxed) {
            // Check for new connections (non-blocking)
            while let Ok(stream) = self.receiver.try_recv() {
                if let Ok(conn) = Connection::new(stream) {
                    let fd = conn.fd();
                    register_kqueue_rw(kq, fd).ok();
                    connections.insert(fd, conn);
                }
            }

            let timeout = libc::timespec {
                tv_sec: 0,
                tv_nsec: 10_000_000,
            }; // 10ms
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

                if fd == wake_read {
                    // Drain wake pipe
                    let mut buf = [0u8; 64];
                    while unsafe { libc::read(wake_read, buf.as_mut_ptr() as *mut _, 64) } > 0 {}
                    continue;
                }

                if let Some(conn) = connections.get_mut(&fd) {
                    let mut should_close = false;

                    if filter == EVFILT_READ {
                        match conn.read_all() {
                            Ok(_) => {
                                conn.process_requests(&self.count);
                            }
                            Err(_) => should_close = true,
                        }
                    }

                    if (filter == EVFILT_WRITE || conn.write_len > 0) && conn.write_all().is_err() {
                        should_close = true;
                    }

                    if should_close {
                        deregister_kqueue(kq, fd);
                        connections.remove(&fd);
                    }
                }
            }
        }

        unsafe {
            libc::close(wake_read);
            libc::close(wake_write);
            libc::close(kq);
        }
    }

    #[cfg(target_os = "linux")]
    fn run(self) {
        let epoll_fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return;
        }

        let mut connections: HashMap<RawFd, Connection> = HashMap::new();
        let mut events: Vec<epoll_event> = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];

        while !self.shutdown.load(Ordering::Relaxed) {
            while let Ok(stream) = self.receiver.try_recv() {
                if let Ok(conn) = Connection::new(stream) {
                    let fd = conn.fd();
                    register_epoll_rw(epoll_fd, fd).ok();
                    connections.insert(fd, conn);
                }
            }

            let n = unsafe { epoll_wait(epoll_fd, events.as_mut_ptr(), MAX_EVENTS as i32, 10) };
            if n < 0 {
                continue;
            }

            for i in 0..n as usize {
                let fd = events[i].u64 as RawFd;
                let ev = events[i].events;

                if let Some(conn) = connections.get_mut(&fd) {
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
    println!("  Multi-Core HTTP Server ({} workers)", num_workers);
    println!("  Edge-Triggered I/O + Channel Distribution");
    println!("==============================================");
    println!();

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;

    println!("Server listening on http://{}", addr);
    println!();
    println!("Benchmark:");
    println!("  wrk -t{} -c400 -d30s http://localhost:8080/", num_workers);
    println!();

    let count = Arc::new(AtomicU64::new(0));
    let shutdown = Arc::new(AtomicBool::new(false));

    // Create worker channels
    let mut senders: Vec<mpsc::Sender<TcpStream>> = Vec::new();
    let mut handles = Vec::new();

    for id in 0..num_workers {
        let (tx, rx) = mpsc::channel();
        senders.push(tx);

        let worker = Worker {
            id,
            receiver: rx,
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

    // Acceptor loop - round-robin distribute connections
    let mut next_worker = 0usize;
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                // Round-robin to workers
                if senders[next_worker].send(stream).is_err() {
                    // Worker died, skip
                }
                next_worker = (next_worker + 1) % num_workers;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_micros(100));
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }
}
