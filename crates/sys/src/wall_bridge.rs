use std::io;
use std::mem::{size_of, zeroed};
use std::net::TcpListener;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// The wall bootstrap's inherited socket, next to bwrap's status pipe.
pub const BRIDGE_FD: i32 = 4;

/// Duplicates the bootstrap socket. Both descriptors close when the CLI starts.
pub fn inherited_bridge() -> io::Result<UnixStream> {
    // SAFETY: fcntl validates the integer descriptor and duplicates it to a fresh owned descriptor.
    let fd = unsafe { libc::fcntl(BRIDGE_FD, libc::F_DUPFD_CLOEXEC, BRIDGE_FD + 1) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful duplication returned a fresh owned descriptor.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: marks the original inherited descriptor close-on-exec without taking ownership.
    if unsafe { libc::fcntl(BRIDGE_FD, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stream)
}

struct Message {
    // cmsghdr requires native alignment.
    control: [usize; 4],
    iov: libc::iovec,
}

impl Message {
    fn new(label: &mut u8) -> Self {
        Self { control: [0; 4], iov: libc::iovec { iov_base: std::ptr::from_mut(label).cast(), iov_len: 1 } }
    }

    fn header(&mut self) -> libc::msghdr {
        // SAFETY: a zeroed msghdr has no address or flags. Its buffers live through the one syscall.
        let mut msg: libc::msghdr = unsafe { zeroed() };
        msg.msg_iov = &mut self.iov;
        msg.msg_iovlen = 1;
        msg.msg_control = self.control.as_mut_ptr().cast();
        // SAFETY: the padded length of one descriptor fits the aligned control buffer.
        msg.msg_controllen = unsafe { libc::CMSG_SPACE(size_of::<i32>() as u32) } as usize;
        msg
    }
}

/// Sends one listener with a one-byte label on a connected Unix socket.
pub fn send_listener(pair: &UnixStream, listener: &TcpListener, mut label: u8) -> io::Result<()> {
    let mut buffers = Message::new(&mut label);
    let msg = buffers.header();
    // SAFETY: the aligned buffer holds one rights header and one descriptor, and all buffers stay live.
    let sent = unsafe {
        let c = libc::CMSG_FIRSTHDR(&msg);
        (*c).cmsg_level = libc::SOL_SOCKET;
        (*c).cmsg_type = libc::SCM_RIGHTS;
        (*c).cmsg_len = libc::CMSG_LEN(size_of::<i32>() as u32) as usize;
        libc::CMSG_DATA(c).cast::<i32>().write(listener.as_raw_fd());
        libc::sendmsg(pair.as_raw_fd(), &msg, libc::MSG_NOSIGNAL)
    };
    if sent == 1 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Receives the bootstrap's labelled listener, or EOF at the end of the list.
pub fn recv_listener(pair: BorrowedFd<'_>) -> io::Result<Option<(u8, TcpListener)>> {
    let mut label = 0;
    let mut buffers = Message::new(&mut label);
    let mut msg = buffers.header();
    // SAFETY: the writable message buffers stay live; the kernel supplies the ancillary header.
    let n = unsafe { libc::recvmsg(pair.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    match n {
        0 => return Ok(None),
        -1 => return Err(io::Error::last_os_error()),
        _ => {}
    }
    // SAFETY: a complete SCM_RIGHTS record contains fresh descriptors owned by the receiver.
    let fd = unsafe {
        let c = libc::CMSG_FIRSTHDR(&msg);
        if c.is_null()
            || (*c).cmsg_type != libc::SCM_RIGHTS
            || (*c).cmsg_level != libc::SOL_SOCKET
            || (*c).cmsg_len != libc::CMSG_LEN(size_of::<i32>() as u32) as usize
            || msg.msg_flags & libc::MSG_CTRUNC != 0
        {
            return Err(io::Error::other("expected one wall listener"));
        }
        OwnedFd::from_raw_fd(libc::CMSG_DATA(c).cast::<i32>().read())
    };
    Ok(Some((label, TcpListener::from(fd))))
}
