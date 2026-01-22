use super::{Interest, Token};
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

#[cfg(target_os = "linux")]
mod sys {
    use super::*;
    use libc::{
        epoll_create1, epoll_ctl, epoll_event, epoll_wait, EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT,
        EPOLL_CLOEXEC,
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

    pub fn add_fd(epoll_fd: RawFd, fd: RawFd, interest: &super::Interest) -> io::Result<()> {
        let mut event = epoll_event {
            events: match interest {
                super::Interest::Readable => EPOLLIN | EPOLLERR | EPOLLHUP,
                super::Interest::Writable => EPOLLOUT | EPOLLERR | EPOLLHUP,
            },
            u64: fd as u64,
        };

        unsafe {
            if epoll_ctl(epoll_fd, 1, fd, &event) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    pub fn wait(epoll_fd: RawFd, events: &mut [epoll_event], timeout: i32) -> io::Result<usize> {
        let n = unsafe { epoll_wait(epoll_fd, events.as_mut_ptr(), events.len() as i32, timeout) };
        if n < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::*;
    use libc::{kevent, kqueue, EVFILT_READ, EVFILT_WRITE, EV_ADD, EV_ENABLE, EV_ONESHOT};
    use std::os::unix::io::RawFd;

    pub fn create_kqueue() -> io::Result<RawFd> {
        let fd = unsafe { kqueue() };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }

    pub fn add_fd(kq: RawFd, fd: RawFd, interest: &super::Interest) -> io::Result<()> {
        let filter = match interest {
            super::Interest::Readable => EVFILT_READ,
            super::Interest::Writable => EVFILT_WRITE,
        };

        let mut event = kevent {
            ident: fd as usize,
            filter,
            flags: EV_ADD | EV_ENABLE | EV_ONESHOT,
            fflags: 0,
            data: 0,
            udata: 0,
        };

        unsafe {
            if kevent(kq, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    pub fn wait(kq: RawFd, events: &mut [kevent], timeout: i32) -> io::Result<usize> {
        let ts = if timeout < 0 {
            std::ptr::null()
        } else {
            let secs = timeout / 1000;
            let nsecs = (timeout % 1000) * 1_000_000;
            let ts = libc::timespec {
                tv_sec: secs as i64,
                tv_nsec: nsecs as i64,
            };
            &ts as *const libc::timespec
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
            Err(io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
}

pub struct ReactorHandle {
    epoll_fd: RawFd,
}

impl ReactorHandle {
    pub fn new() -> io::Result<Self> {
        let epoll_fd = sys::create_kqueue()?;
        Ok(Self { epoll_fd })
    }

    pub fn add(&self, reactor: &Reactor) -> io::Result<()> {
        sys::add_fd(self.epoll_fd, reactor.fd, &reactor.interest)
    }

    pub fn wait(&self, timeout_ms: i32) -> io::Result<Vec<(Token, Interest)>> {
        const MAX_EVENTS: usize = 1024;
        let mut events = vec![unsafe { std::mem::zeroed() }; MAX_EVENTS];
        let n = sys::wait(self.epoll_fd, &mut events, timeout_ms)?;

        let mut ready = Vec::with_capacity(n);
        for i in 0..n {
            let fd = events[i].ident as RawFd;
            let interest = if events[i].filter == EVFILT_READ as i16 {
                super::Interest::Readable
            } else {
                super::Interest::Writable
            };
            ready.push((Token(fd as usize), interest));
        }
        Ok(ready)
    }
}

impl Drop for ReactorHandle {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.epoll_fd);
        }
    }
}
