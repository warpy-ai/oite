use std::collections::{BTreeMap, VecDeque, HashMap};
use std::sync::{Arc, Mutex, Condvar, atomic::{AtomicUsize, Ordering}};
use std::task::{Context, Poll, Waker, Wake};
use std::time::{Duration, Instant};
use std::os::unix::io::{RawFd, IntoRawFd};
use std::thread;
use std::pin::Pin;
use std::future::Future;
use std::io;

pub mod reactor;
pub mod task;
pub mod runtime_impl;

pub use reactor::{Reactor, ReactorHandle};
pub use task::{Executor, Task, JoinSet, Timer};
pub use runtime_impl::Runtime;

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

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::io::Result<()>>;

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::io::Result<()>>;
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

impl<T: AsyncRead> AsyncRead for AsyncBufRead<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.consumed >= self.buf.len() {
            self.buf.clear();
            self.consumed = 0;
            let n = Pin::new(&mut self.inner).poll_read(cx, &mut self.buf)?;
            if n == 0 {
                return Poll::Ready(Ok(0));
            }
        }

        let remaining = &self.buf[self.consumed..];
        let to_copy = std::cmp::min(buf.len(), remaining.len());
        buf[..to_copy].copy_from_slice(&remaining[..to_copy]);
        self.consumed += to_copy;
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
    fn accept(&self) -> AcceptFuture<Self::Stream>;
}

pub struct AcceptFuture<T: AsyncTcpStream> {
    listener: std::net::TcpListener,
    waker: Option<Waker>,
}

impl<T: AsyncTcpStream> AcceptFuture<T> {
    pub fn new(listener: std::net::TcpListener) -> Self {
        Self {
            listener,
            waker: None,
        }
    }
}

impl<T: AsyncTcpStream> std::future::Future for AcceptFuture<T> {
    type Output = std::io::Result<(T, std::net::SocketAddr)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.listener.accept() {
            Ok((stream, addr)) => {
                let stream = unsafe { std::mem::transmute(stream) };
                Poll::Ready(Ok((stream, addr)))
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
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_close(cx)
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
                Ok(TcpStream { stream, inner: reactor, interest: Interest::Writable }).into()
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
}

impl AsyncTcpListener for TcpListener {
    type Stream = TcpStream;

    fn accept(&self) -> AcceptFuture<Self::Stream> {
        AcceptFuture::new(self.listener.try_clone().unwrap())
    }
}
