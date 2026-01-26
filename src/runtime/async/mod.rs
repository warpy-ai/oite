//! Async runtime implementation (work in progress)

#![allow(dead_code)]
#![allow(unused_variables)]

use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub mod reactor;
pub mod runtime_impl;
pub mod task;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Interest {
    Readable,
    Writable,
}

pub struct Token(pub usize);

pub trait AsyncRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>>;
}

pub trait AsyncWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>>;

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>>;

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>>;
}

pub struct AsyncBufRead<T: AsyncRead> {
    inner: T,
    buf: Vec<u8>,
    consumed: usize,
}

impl<T: AsyncRead> AsyncBufRead<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            buf: Vec::with_capacity(8192),
            consumed: 0,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for AsyncBufRead<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.consumed >= this.buf.len() {
            this.buf.clear();
            this.consumed = 0;
            // Read into a temporary buffer first
            let mut temp_buf = vec![0u8; 8192];
            match Pin::new(&mut this.inner).poll_read(cx, &mut temp_buf) {
                Poll::Ready(Ok(0)) => return Poll::Ready(Ok(0)),
                Poll::Ready(Ok(n)) => {
                    this.buf.extend_from_slice(&temp_buf[..n]);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }

        let remaining = &this.buf[this.consumed..];
        let to_copy = std::cmp::min(buf.len(), remaining.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        this.consumed += to_copy;
        Poll::Ready(Ok(to_copy))
    }
}

pub trait AsyncTcpStream: AsyncRead + AsyncWrite + Unpin {
    fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr>;
    fn local_addr(&self) -> std::io::Result<std::net::SocketAddr>;
    fn shutdown(&self, how: std::net::Shutdown) -> std::io::Result<()>;
}

pub trait AsyncTcpListener: Unpin {
    type Stream: AsyncTcpStream + Unpin;
    fn accept(&self) -> AcceptFuture;
}

pub struct AcceptFuture {
    listener: std::net::TcpListener,
    waker: Option<Waker>,
}

impl AcceptFuture {
    pub fn new(listener: std::net::TcpListener) -> Self {
        Self {
            listener,
            waker: None,
        }
    }
}

impl std::future::Future for AcceptFuture {
    type Output = std::io::Result<(TcpStream, std::net::SocketAddr)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(true).ok();
                let fd = stream.as_raw_fd();
                let reactor = reactor::Reactor::new(fd, Interest::Readable);
                let tcp_stream = TcpStream {
                    stream,
                    inner: reactor,
                    interest: Interest::Readable,
                };
                Poll::Ready(Ok((tcp_stream, addr)))
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct TcpStream {
    stream: std::net::TcpStream,
    inner: reactor::Reactor,
    interest: Interest,
}

impl TcpStream {
    pub fn connect(addr: &std::net::SocketAddr) -> ConnectFuture {
        ConnectFuture::new(addr)
    }

    /// Async read convenience method
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Read;
        loop {
            match self.stream.read(buf) {
                Ok(n) => return Ok(n),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Yield to the executor
                    std::future::pending::<()>().await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Async write_all convenience method
    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut written = 0;
        while written < buf.len() {
            match self.stream.write(&buf[written..]) {
                Ok(n) => written += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Yield to the executor
                    std::future::pending::<()>().await;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl AsyncTcpStream for TcpStream {
    fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.stream.peer_addr()
    }

    fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.stream.local_addr()
    }

    fn shutdown(&self, how: std::net::Shutdown) -> std::io::Result<()> {
        self.stream.shutdown(how)
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        use std::io::Read;
        match self.get_mut().stream.read(buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        use std::io::Write;
        match self.get_mut().stream.write(buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<std::io::Result<()>> {
        use std::io::Write;
        match self.get_mut().stream.flush() {
            Ok(()) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<std::io::Result<()>> {
        // TCP shutdown for write direction
        match self.get_mut().stream.shutdown(std::net::Shutdown::Write) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl Unpin for TcpStream {}

pub struct ConnectFuture {
    stream: Option<std::net::TcpStream>,
    waker: Option<Waker>,
}

impl ConnectFuture {
    pub fn new(addr: &std::net::SocketAddr) -> Self {
        let stream = std::net::TcpStream::connect(addr).ok();
        Self {
            stream,
            waker: None,
        }
    }
}

impl std::future::Future for ConnectFuture {
    type Output = std::io::Result<TcpStream>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.stream.take() {
            Some(stream) => {
                stream.set_nonblocking(true)?;
                let reactor = reactor::Reactor::new(stream.as_raw_fd(), Interest::Writable);
                Ok(TcpStream {
                    stream,
                    inner: reactor,
                    interest: Interest::Writable,
                })
                .into()
            }
            None => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

pub struct TcpListener {
    listener: std::net::TcpListener,
}

impl TcpListener {
    pub fn bind(addr: &std::net::SocketAddr) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
}

impl AsyncTcpListener for TcpListener {
    type Stream = TcpStream;

    fn accept(&self) -> AcceptFuture {
        AcceptFuture::new(self.listener.try_clone().unwrap())
    }
}
