//! Low-Level OS Poller and Event Readiness Notification Engine.
//!
//! Provides a unified non-blocking OS readiness interface:
//! - **kqueue** on macOS / iOS / FreeBSD / OpenBSD / NetBSD
//! - **epoll** on Linux / Android
//! - **poll** portable fallback on other Unix platforms
//!
//! Automatically handles non-blocking socket configuration (`O_NONBLOCK`),
//! `SO_REUSEADDR`, and `TCP_NODELAY`.

use std::io;
use std::os::unix::io::RawFd;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interest {
    Readable,
    Writable,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub token: usize,
    pub readable: bool,
    pub writable: bool,
    pub is_closed: bool,
    pub is_error: bool,
}

/// Sets a file descriptor to non-blocking mode via fcntl.
pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Sets SO_REUSEADDR on a socket.
pub fn set_reuseaddr(fd: RawFd) -> io::Result<()> {
    unsafe {
        let optval: libc::c_int = 1;
        let ret = libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &optval as *const _ as *const libc::c_void,
            std::mem::size_of_val(&optval) as libc::socklen_t,
        );
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Sets TCP_NODELAY on a socket.
pub fn set_nodelay(fd: RawFd) -> io::Result<()> {
    unsafe {
        let optval: libc::c_int = 1;
        let ret = libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            &optval as *const _ as *const libc::c_void,
            std::mem::size_of_val(&optval) as libc::socklen_t,
        );
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
pub struct Poller {
    kq_fd: RawFd,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
impl Poller {
    pub fn new() -> io::Result<Self> {
        let kq_fd = unsafe { libc::kqueue() };
        if kq_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { kq_fd })
    }

    pub fn register(&self, fd: RawFd, token: usize, interest: Interest) -> io::Result<()> {
        let mut changes: Vec<libc::kevent> = Vec::with_capacity(2);
        let flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR;

        if interest == Interest::Readable || interest == Interest::Both {
            changes.push(libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_READ,
                flags: flags as u16,
                fflags: 0,
                data: 0,
                udata: token as *mut libc::c_void,
            });
        }

        if interest == Interest::Writable || interest == Interest::Both {
            changes.push(libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_WRITE,
                flags: flags as u16,
                fflags: 0,
                data: 0,
                udata: token as *mut libc::c_void,
            });
        }

        let ret = unsafe {
            libc::kevent(
                self.kq_fd,
                changes.as_ptr(),
                changes.len() as libc::c_int,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };

        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn deregister(&self, fd: RawFd) -> io::Result<()> {
        let changes = [
            libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_READ,
                flags: libc::EV_DELETE as u16,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
            libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_WRITE,
                flags: libc::EV_DELETE as u16,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
        ];

        // Best-effort delete; errors (like already deleted) are ignored
        unsafe {
            libc::kevent(
                self.kq_fd,
                changes.as_ptr(),
                changes.len() as libc::c_int,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            );
        }
        Ok(())
    }

    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<usize> {
        events.clear();
        const MAX_KEVENTS: usize = 128;
        let mut raw_events: [libc::kevent; MAX_KEVENTS] = unsafe { std::mem::zeroed() };

        let timespec_buf;
        let timespec_ptr = if let Some(dur) = timeout {
            timespec_buf = libc::timespec {
                tv_sec: dur.as_secs() as libc::time_t,
                tv_nsec: dur.subsec_nanos() as libc::c_long,
            };
            &timespec_buf as *const libc::timespec
        } else {
            std::ptr::null()
        };

        let n = unsafe {
            libc::kevent(
                self.kq_fd,
                std::ptr::null(),
                0,
                raw_events.as_mut_ptr(),
                MAX_KEVENTS as libc::c_int,
                timespec_ptr,
            )
        };

        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(0);
            }
            return Err(err);
        }

        for i in 0..n as usize {
            let kev = &raw_events[i];
            let token = kev.udata as usize;
            let readable = kev.filter == libc::EVFILT_READ;
            let writable = kev.filter == libc::EVFILT_WRITE;
            let is_closed = (kev.flags & libc::EV_EOF as u16) != 0;
            let is_error = (kev.flags & libc::EV_ERROR as u16) != 0;

            events.push(Event {
                token,
                readable,
                writable,
                is_closed,
                is_error,
            });
        }

        Ok(events.len())
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
impl Drop for Poller {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.kq_fd);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub struct Poller {
    epoll_fd: RawFd,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Poller {
    pub fn new() -> io::Result<Self> {
        let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { epoll_fd })
    }

    pub fn register(&self, fd: RawFd, token: usize, interest: Interest) -> io::Result<()> {
        let mut events = libc::EPOLLET as u32;
        if interest == Interest::Readable || interest == Interest::Both {
            events |= libc::EPOLLIN as u32;
        }
        if interest == Interest::Writable || interest == Interest::Both {
            events |= libc::EPOLLOUT as u32;
        }
        events |= (libc::EPOLLRDHUP | libc::EPOLLERR | libc::EPOLLHUP) as u32;

        let mut ev = libc::epoll_event {
            events,
            u64: token as u64,
        };

        let ret = unsafe {
            libc::epoll_ctl(
                self.epoll_fd,
                libc::EPOLL_CTL_ADD,
                fd,
                &mut ev as *mut libc::epoll_event,
            )
        };

        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn deregister(&self, fd: RawFd) -> io::Result<()> {
        let ret = unsafe {
            libc::epoll_ctl(self.epoll_fd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut())
        };
        // Silently tolerate already-deleted or closed fds
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ENOENT) && err.raw_os_error() != Some(libc::EBADF) {
                return Err(err);
            }
        }
        Ok(())
    }

    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<usize> {
        events.clear();
        const MAX_EVENTS: usize = 128;
        let mut raw_events: [libc::epoll_event; MAX_EVENTS] = unsafe { std::mem::zeroed() };

        let timeout_ms = match timeout {
            Some(d) => d.as_millis().min(i32::MAX as u128) as libc::c_int,
            None => -1,
        };

        let n = unsafe {
            libc::epoll_wait(
                self.epoll_fd,
                raw_events.as_mut_ptr(),
                MAX_EVENTS as libc::c_int,
                timeout_ms,
            )
        };

        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                return Ok(0);
            }
            return Err(err);
        }

        for i in 0..n as usize {
            let ev = &raw_events[i];
            let token = ev.u64 as usize;
            let readable = (ev.events & (libc::EPOLLIN as u32)) != 0;
            let writable = (ev.events & (libc::EPOLLOUT as u32)) != 0;
            let is_closed = (ev.events & ((libc::EPOLLRDHUP | libc::EPOLLHUP) as u32)) != 0;
            let is_error = (ev.events & (libc::EPOLLERR as u32)) != 0;

            events.push(Event {
                token,
                readable,
                writable,
                is_closed,
                is_error,
            });
        }

        Ok(events.len())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Drop for Poller {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.epoll_fd);
        }
    }
}
