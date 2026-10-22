//! Low-Level OS Poller and Event Readiness Notification Engine.
//!
//! Provides a unified non-blocking OS readiness interface:
//! - **kqueue** on macOS / iOS / FreeBSD / OpenBSD / NetBSD
//! - **epoll** on Linux / Android
//! - **WSAPoll** on Windows
//!
//! Automatically handles non-blocking socket configuration,
//! SO_REUSEADDR, and TCP_NODELAY.

use std::io;
use std::time::Duration;

#[cfg(unix)]
pub type RawSocket = std::os::unix::io::RawFd;

#[cfg(windows)]
pub type RawSocket = std::os::windows::io::RawSocket;

pub trait AsSocket {
    fn as_socket(&self) -> RawSocket;
}

#[cfg(unix)]
impl<T: std::os::unix::io::AsRawFd> AsSocket for T {
    fn as_socket(&self) -> RawSocket {
        self.as_raw_fd()
    }
}

#[cfg(windows)]
impl<T: std::os::windows::io::AsRawSocket> AsSocket for T {
    fn as_socket(&self) -> RawSocket {
        self.as_raw_socket()
    }
}

pub fn get_socket_handle<T: AsSocket>(sock: &T) -> RawSocket {
    sock.as_socket()
}

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

/// Sets a file descriptor or socket to non-blocking mode.
pub fn set_nonblocking(fd: RawSocket) -> io::Result<()> {
    #[cfg(unix)]
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        #[link(name = "ws2_32")]
        extern "system" {
            fn ioctlsocket(s: usize, cmd: i32, argp: *mut u32) -> i32;
        }
        const FIONBIO: i32 = -2147195266i32;
        let mut mode: u32 = 1;
        let res = unsafe { ioctlsocket(fd as usize, FIONBIO, &mut mode) };
        if res != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Sets SO_REUSEADDR on a socket.
pub fn set_reuseaddr(fd: RawSocket) -> io::Result<()> {
    #[cfg(unix)]
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
        Ok(())
    }

    #[cfg(windows)]
    {
        let _ = fd;
        Ok(())
    }
}

/// Sets TCP_NODELAY on a socket.
pub fn set_nodelay(fd: RawSocket) -> io::Result<()> {
    #[cfg(unix)]
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
        Ok(())
    }

    #[cfg(windows)]
    {
        #[link(name = "ws2_32")]
        extern "system" {
            fn setsockopt(s: usize, level: i32, optname: i32, optval: *const i8, optlen: i32) -> i32;
        }
        const IPPROTO_TCP: i32 = 6;
        const TCP_NODELAY: i32 = 0x0001;
        let optval: i32 = 1;
        let res = unsafe {
            setsockopt(
                fd as usize,
                IPPROTO_TCP,
                TCP_NODELAY,
                &optval as *const _ as *const i8,
                std::mem::size_of::<i32>() as i32,
            )
        };
        if res != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// kqueue Backend (macOS, iOS, BSD)
// -----------------------------------------------------------------------------
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
pub struct Poller {
    kq_fd: libc::c_int,
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

    pub fn register(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
        let mut changes: Vec<libc::kevent> = Vec::with_capacity(2);
        let flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR;

        if interest == Interest::Readable || interest == Interest::Both {
            changes.push(libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_READ,
                flags,
                fflags: 0,
                data: 0,
                udata: token as *mut libc::c_void,
            });
        }

        if interest == Interest::Writable || interest == Interest::Both {
            changes.push(libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_WRITE,
                flags,
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

    /// Modifies the registered interest for an existing file descriptor.
    pub fn modify(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
        let mut changes: Vec<libc::kevent> = Vec::with_capacity(2);
        let add_flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR;

        match interest {
            Interest::Both => {
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_READ,
                    flags: add_flags,
                    fflags: 0,
                    data: 0,
                    udata: token as *mut libc::c_void,
                });
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_WRITE,
                    flags: add_flags,
                    fflags: 0,
                    data: 0,
                    udata: token as *mut libc::c_void,
                });
            }
            Interest::Readable => {
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_READ,
                    flags: add_flags,
                    fflags: 0,
                    data: 0,
                    udata: token as *mut libc::c_void,
                });
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_WRITE,
                    flags: libc::EV_DELETE,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                });
            }
            Interest::Writable => {
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_WRITE,
                    flags: add_flags,
                    fflags: 0,
                    data: 0,
                    udata: token as *mut libc::c_void,
                });
                changes.push(libc::kevent {
                    ident: fd as usize,
                    filter: libc::EVFILT_READ,
                    flags: libc::EV_DELETE,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                });
            }
        }

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

    pub fn deregister(&self, fd: RawSocket) -> io::Result<()> {
        let changes = [
            libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_READ,
                flags: libc::EV_DELETE,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
            libc::kevent {
                ident: fd as usize,
                filter: libc::EVFILT_WRITE,
                flags: libc::EV_DELETE,
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

        for kev in raw_events.iter().take(n as usize) {
            let token = kev.udata as usize;
            let readable = kev.filter == libc::EVFILT_READ;
            let writable = kev.filter == libc::EVFILT_WRITE;
            let is_closed = (kev.flags & libc::EV_EOF) != 0;
            let is_error = (kev.flags & libc::EV_ERROR) != 0;

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

// -----------------------------------------------------------------------------
// epoll Backend (Linux, Android)
// -----------------------------------------------------------------------------
#[cfg(any(target_os = "linux", target_os = "android"))]
pub struct Poller {
    epoll_fd: RawSocket,
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

    pub fn register(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
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

    /// Modifies the registered interest for an existing file descriptor.
    pub fn modify(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
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
                libc::EPOLL_CTL_MOD,
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

    pub fn deregister(&self, fd: RawSocket) -> io::Result<()> {
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

// -----------------------------------------------------------------------------
// WSAPoll Backend (Windows)
// -----------------------------------------------------------------------------
#[cfg(target_os = "windows")]
pub struct Poller {
    sockets: std::sync::Mutex<std::collections::HashMap<RawSocket, (usize, Interest)>>,
}

#[cfg(target_os = "windows")]
impl Poller {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            sockets: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    pub fn register(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
        let mut map = self.sockets.lock().unwrap();
        map.insert(fd, (token, interest));
        Ok(())
    }

    /// Modifies the registered interest for an existing file descriptor.
    pub fn modify(&self, fd: RawSocket, token: usize, interest: Interest) -> io::Result<()> {
        let mut map = self.sockets.lock().unwrap();
        map.insert(fd, (token, interest));
        Ok(())
    }

    pub fn deregister(&self, fd: RawSocket) -> io::Result<()> {
        let mut map = self.sockets.lock().unwrap();
        map.remove(&fd);
        Ok(())
    }

    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<usize> {
        events.clear();
        let map = self.sockets.lock().unwrap().clone();
        if map.is_empty() {
            if let Some(dur) = timeout {
                std::thread::sleep(dur);
            }
            return Ok(0);
        }

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct WSAPOLLFD {
            fd: usize,
            events: i16,
            revents: i16,
        }

        #[link(name = "ws2_32")]
        extern "system" {
            fn WSAPoll(fd_array: *mut WSAPOLLFD, fds: u32, timeout: i32) -> i32;
        }

        const POLLRDNORM: i16 = 0x0100;
        const POLLRDBAND: i16 = 0x0200;
        const POLLIN: i16 = POLLRDNORM | POLLRDBAND;
        const POLLWRNORM: i16 = 0x0010;
        const POLLOUT: i16 = POLLWRNORM;
        const POLLERR: i16 = 0x0001;
        const POLLHUP: i16 = 0x0002;

        let mut poll_fds: Vec<WSAPOLLFD> = Vec::with_capacity(map.len());
        let mut fd_tokens: Vec<(RawSocket, usize)> = Vec::with_capacity(map.len());

        for (&sock, &(token, interest)) in &map {
            let mut req_events = 0i16;
            if interest == Interest::Readable || interest == Interest::Both {
                req_events |= POLLIN;
            }
            if interest == Interest::Writable || interest == Interest::Both {
                req_events |= POLLOUT;
            }
            poll_fds.push(WSAPOLLFD {
                fd: sock as usize,
                events: req_events,
                revents: 0,
            });
            fd_tokens.push((sock, token));
        }

        let timeout_ms = match timeout {
            Some(d) => d.as_millis().min(i32::MAX as u128) as i32,
            None => -1,
        };

        let ret = unsafe {
            WSAPoll(poll_fds.as_mut_ptr(), poll_fds.len() as u32, timeout_ms)
        };

        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        for (i, pfd) in poll_fds.iter().enumerate() {
            if pfd.revents != 0 {
                let (_, token) = fd_tokens[i];
                let readable = (pfd.revents & POLLIN) != 0;
                let writable = (pfd.revents & POLLOUT) != 0;
                let is_closed = (pfd.revents & POLLHUP) != 0;
                let is_error = (pfd.revents & POLLERR) != 0;

                events.push(Event {
                    token,
                    readable,
                    writable,
                    is_closed,
                    is_error,
                });
            }
        }

        Ok(events.len())
    }
}
