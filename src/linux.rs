use nix::libc;
use std::io::ErrorKind;
use std::net::SocketAddrV4;
use std::os::unix::prelude::AsRawFd;
use std::{io, mem, net::SocketAddrV6};

use libc::{c_void, socklen_t};
use nix::sys::socket::{getsockopt, sockopt::OriginalDst};

pub fn get_original_address_v4<F>(fd: &F) -> io::Result<SocketAddrV4>
where
    F: AsRawFd,
{
    let addr = getsockopt(fd.as_raw_fd(), OriginalDst).map_err(|e| match e {
        nix::Error::Sys(err) => io::Error::from(err),
        _ => io::Error::new(ErrorKind::Other, e),
    })?;
    let addr = SocketAddrV4::new(
        u32::from_be(addr.sin_addr.s_addr).into(),
        u16::from_be(addr.sin_port),
    );
    Ok(addr)
}

pub fn get_original_address_v6<F>(fd: &F) -> io::Result<SocketAddrV6>
where
    F: AsRawFd,
{
    let mut sockaddr: libc::sockaddr_in6 = unsafe { mem::zeroed() };
    let mut socklen = mem::size_of::<libc::sockaddr_in6>();
    let res = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_IPV6,
            libc::SOL_ORIGINAL_DST,
            &mut sockaddr as *mut _ as *mut c_void,
            &mut socklen as *mut _ as *mut socklen_t,
        )
    };
    if res != 0 {
        return Err(io::Error::new(ErrorKind::Other, "getsockopt failed"));
    }
    let addr = SocketAddrV6::new(
        sockaddr.sin6_addr.s6_addr.into(),
        u16::from_be(sockaddr.sin6_port),
        sockaddr.sin6_flowinfo,
        sockaddr.sin6_scope_id,
    );
    Ok(addr)
}
