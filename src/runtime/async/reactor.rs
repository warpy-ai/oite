use super::{Interest, Token};
#[cfg(target_os = "macos")]
use libc::{EVFILT_READ, EVFILT_WRITE};
use std::collections::HashMap;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

pub struct Reactor {
    fd: RawFd,
    interest: Interest,
    state: Mutex<ReactorState>,
}

struct ReactorState {
    readable: Option<Waker>,
    writable: Option<Waker>,
}

impl Reactor {
    pub fn new(fd: RawFd, interest: Interest) -> Self {
        Self {
            fd,
            interest,
            state: Mutex::new(ReactorState {
                readable: None,
                writable: None,
            }),
        }
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// ============================================================================
// Linux epoll implementation - Edge-Triggered for high performance
// ============================================================================

#[cfg(target_os = "linux")]
mod sys {
    use super::*;
    use libc::{
        EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLET, EPOLLHUP,
        EPOLLIN, EPOLLOUT, EPOLLRDHUP, epoll_create1, epoll_ctl, epoll_event, epoll_wait,
    };
    use std::os::unix::io::RawFd;

    pub fn create_epoll() -> io::Result<RawFd> {
        let fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }

    /// Add fd with edge-triggered mode for maximum performance
    pub fn add_fd(epoll_fd: RawFd, fd: RawFd, interest: &super::Interest) -> io::Result<()> {
        // EPOLLET = edge-triggered (notify only on state change, not level)
        // This reduces syscall overhead significantly
        let base_events = EPOLLET as u32 | EPOLLERR as u32 | EPOLLHUP as u32 | EPOLLRDHUP as u32;

        let events = match interest {
            super::Interest::Readable => base_events | EPOLLIN as u32,
            super::Interest::Writable => base_events | EPOLLOUT as u32,
        };

        let mut event = epoll_event {
            events,
            u64: fd as u64,
        };

        let result = unsafe { epoll_ctl(epoll_fd, EPOLL_CTL_ADD, fd, &mut event) };
        if result < 0 {
            let err = io::Error::last_os_error();
            // If already exists, try to modify instead
            if err.raw_os_error() == Some(libc::EEXIST) {
                let result = unsafe { epoll_ctl(epoll_fd, EPOLL_CTL_MOD, fd, &mut event) };
                if result < 0 {
                    return Err(io::Error::last_os_error());
                }
            } else {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Add fd for both read AND write events (common for HTTP keep-alive)
    pub fn add_fd_rw(epoll_fd: RawFd, fd: RawFd) -> io::Result<()> {
        let events = EPOLLET as u32
            | EPOLLIN as u32
            | EPOLLOUT as u32
            | EPOLLERR as u32
            | EPOLLHUP as u32
            | EPOLLRDHUP as u32;

        let mut event = epoll_event {
            events,
            u64: fd as u64,
        };

        let result = unsafe { epoll_ctl(epoll_fd, EPOLL_CTL_ADD, fd, &mut event) };
        if result < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EEXIST) {
                let result = unsafe { epoll_ctl(epoll_fd, EPOLL_CTL_MOD, fd, &mut event) };
                if result < 0 {
                    return Err(io::Error::last_os_error());
                }
            } else {
                return Err(err);
            }
        }
        Ok(())
    }

    pub fn remove_fd(epoll_fd: RawFd, fd: RawFd) -> io::Result<()> {
        let result = unsafe { epoll_ctl(epoll_fd, EPOLL_CTL_DEL, fd, std::ptr::null_mut()) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn wait(epoll_fd: RawFd, events: &mut [epoll_event], timeout: i32) -> io::Result<usize> {
        let n = unsafe { epoll_wait(epoll_fd, events.as_mut_ptr(), events.len() as i32, timeout) };
        if n < 0 {
            let err = io::Error::last_os_error();
            // EINTR is not an error, just retry
            if err.raw_os_error() == Some(libc::EINTR) {
                return Ok(0);
            }
            Err(err)
        } else {
            Ok(n as usize)
        }
    }
}

// ============================================================================
// macOS kqueue implementation - Edge-Triggered with EV_CLEAR
// ============================================================================

#[cfg(target_os = "macos")]
mod sys {
    use super::*;
    use libc::{
        EV_ADD, EV_CLEAR, EV_DELETE, EV_ENABLE, EV_EOF, EV_ERROR, EVFILT_READ, EVFILT_WRITE,
        kevent, kqueue,
    };
    use std::os::unix::io::RawFd;

    pub fn create_kqueue() -> io::Result<RawFd> {
        let fd = unsafe { kqueue() };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }

    /// Add fd with edge-triggered mode (EV_CLEAR)
    /// EV_CLEAR = edge-triggered: after event delivered, reset state
    pub fn add_fd(kq: RawFd, fd: RawFd, interest: &super::Interest) -> io::Result<()> {
        let filter = match interest {
            super::Interest::Readable => EVFILT_READ,
            super::Interest::Writable => EVFILT_WRITE,
        };

        // EV_CLEAR makes it edge-triggered (reset state after delivery)
        let event = kevent {
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

    /// Add fd for both read AND write events
    pub fn add_fd_rw(kq: RawFd, fd: RawFd) -> io::Result<()> {
        let events = [
            kevent {
                ident: fd as usize,
                filter: EVFILT_READ,
                flags: EV_ADD | EV_ENABLE | EV_CLEAR,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
            kevent {
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

    pub fn remove_fd(kq: RawFd, fd: RawFd) -> io::Result<()> {
        // Remove both read and write filters
        let events = [
            kevent {
                ident: fd as usize,
                filter: EVFILT_READ,
                flags: EV_DELETE,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
            kevent {
                ident: fd as usize,
                filter: EVFILT_WRITE,
                flags: EV_DELETE,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
        ];

        // Ignore errors (fd might not have both filters registered)
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
        Ok(())
    }

    pub fn wait(kq: RawFd, events: &mut [kevent], timeout: i32) -> io::Result<usize> {
        let ts = if timeout < 0 {
            std::ptr::null()
        } else {
            let secs = timeout / 1000;
            let nsecs = (timeout % 1000) * 1_000_000;
            &libc::timespec {
                tv_sec: secs as i64,
                tv_nsec: nsecs as i64,
            } as *const libc::timespec
        };

        let n = unsafe {
            kevent(
                kq,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                ts,
            )
        };

        if n < 0 {
            let err = io::Error::last_os_error();
            // EINTR is not an error
            if err.raw_os_error() == Some(libc::EINTR) {
                return Ok(0);
            }
            Err(err)
        } else {
            Ok(n as usize)
        }
    }
}

// ============================================================================
// ReactorHandle - Platform-specific I/O multiplexing (Edge-Triggered)
// ============================================================================

/// macOS: kqueue-based reactor handle with edge-triggered events
#[cfg(target_os = "macos")]
pub struct ReactorHandle {
    kq_fd: RawFd,
}

#[cfg(target_os = "macos")]
impl ReactorHandle {
    pub fn new() -> io::Result<Self> {
        let kq_fd = sys::create_kqueue()?;
        Ok(Self { kq_fd })
    }

    pub fn add(&self, reactor: &Reactor) -> io::Result<()> {
        sys::add_fd(self.kq_fd, reactor.fd, &reactor.interest)
    }

    /// Register fd for both read and write events (optimal for HTTP)
    pub fn add_rw(&self, fd: RawFd) -> io::Result<()> {
        sys::add_fd_rw(self.kq_fd, fd)
    }

    /// Register fd for specific interest
    pub fn register(&self, fd: RawFd, interest: &Interest) -> io::Result<()> {
        sys::add_fd(self.kq_fd, fd, interest)
    }

    /// Unregister fd
    pub fn deregister(&self, fd: RawFd) -> io::Result<()> {
        sys::remove_fd(self.kq_fd, fd)
    }

    pub fn wait(&self, timeout_ms: i32) -> io::Result<Vec<(Token, Interest)>> {
        const MAX_EVENTS: usize = 1024;
        let mut events: [libc::kevent; MAX_EVENTS] = unsafe { std::mem::zeroed() };
        let n = sys::wait(self.kq_fd, &mut events, timeout_ms)?;

        let mut ready = Vec::with_capacity(n);
        for i in 0..n {
            let fd = events[i].ident as RawFd;
            let interest = if events[i].filter == EVFILT_READ as i16 {
                Interest::Readable
            } else {
                Interest::Writable
            };
            ready.push((Token(fd as usize), interest));
        }
        Ok(ready)
    }

    /// Get the raw kqueue fd (for advanced use)
    pub fn raw_fd(&self) -> RawFd {
        self.kq_fd
    }
}

#[cfg(target_os = "macos")]
impl Drop for ReactorHandle {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.kq_fd);
        }
    }
}

/// Linux: epoll-based reactor handle with edge-triggered events
#[cfg(target_os = "linux")]
pub struct ReactorHandle {
    epoll_fd: RawFd,
}

#[cfg(target_os = "linux")]
impl ReactorHandle {
    pub fn new() -> io::Result<Self> {
        let epoll_fd = sys::create_epoll()?;
        Ok(Self { epoll_fd })
    }

    pub fn add(&self, reactor: &Reactor) -> io::Result<()> {
        sys::add_fd(self.epoll_fd, reactor.fd, &reactor.interest)
    }

    /// Register fd for both read and write events (optimal for HTTP)
    pub fn add_rw(&self, fd: RawFd) -> io::Result<()> {
        sys::add_fd_rw(self.epoll_fd, fd)
    }

    /// Register fd for specific interest
    pub fn register(&self, fd: RawFd, interest: &Interest) -> io::Result<()> {
        sys::add_fd(self.epoll_fd, fd, interest)
    }

    /// Unregister fd
    pub fn deregister(&self, fd: RawFd) -> io::Result<()> {
        sys::remove_fd(self.epoll_fd, fd)
    }

    pub fn wait(&self, timeout_ms: i32) -> io::Result<Vec<(Token, Interest)>> {
        use libc::{EPOLLIN, EPOLLOUT, epoll_event};

        const MAX_EVENTS: usize = 1024;
        let mut events: [epoll_event; MAX_EVENTS] = unsafe { std::mem::zeroed() };
        let n = sys::wait(self.epoll_fd, &mut events, timeout_ms)?;

        let mut ready = Vec::with_capacity(n);
        for i in 0..n {
            let fd = events[i].u64 as RawFd;
            // Edge-triggered can report both read and write ready
            if events[i].events as u32 & EPOLLIN as u32 != 0 {
                ready.push((Token(fd as usize), Interest::Readable));
            }
            if events[i].events as u32 & EPOLLOUT as u32 != 0 {
                ready.push((Token(fd as usize), Interest::Writable));
            }
        }
        Ok(ready)
    }

    /// Get the raw epoll fd (for advanced use)
    pub fn raw_fd(&self) -> RawFd {
        self.epoll_fd
    }
}

#[cfg(target_os = "linux")]
impl Drop for ReactorHandle {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.epoll_fd);
        }
    }
}
