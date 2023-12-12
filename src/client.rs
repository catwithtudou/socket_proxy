use bytes::Bytes;
use log::debug;
use std::sync::Arc;
use std::{
    borrow::Cow,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use crate::config::Config;
use crate::linux::{get_original_address_v4, get_original_address_v6};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

#[derive(Clone, Debug)]
pub enum Address {
    Ip(IpAddr),
    Domain(Box<str>),
}

impl From<[u8; 4]> for Address {
    fn from(value: [u8; 4]) -> Self {
        Address::Ip(IpAddr::V4(Ipv4Addr::from(value)))
    }
}

impl From<[u8; 16]> for Address {
    fn from(value: [u8; 16]) -> Self {
        Address::Ip(IpAddr::V6(Ipv6Addr::from(value)))
    }
}

impl From<String> for Address {
    fn from(value: String) -> Self {
        Address::Domain(value.into_boxed_str())
    }
}

pub struct Destination {
    pub host: Address,
    pub port: u16,
}

impl From<SocketAddr> for Destination {
    fn from(addr: SocketAddr) -> Self {
        Destination {
            host: Address::Ip(addr.ip()),
            port: addr.port(),
        }
    }
}

impl<'a> From<(&'a str, u16)> for Destination {
    fn from(addr: (&'a str, u16)) -> Self {
        let host = String::from(addr.0).into_boxed_str();
        Destination {
            host: Address::Domain(host),
            port: addr.1,
        }
    }
}

impl From<(Address, u16)> for Destination {
    fn from(addr_port: (Address, u16)) -> Self {
        Destination {
            host: addr_port.0,
            port: addr_port.1,
        }
    }
}

pub struct Client {
    config: Arc<Config>,
    left: TcpStream,
    src: SocketAddr,
    pub dest: Destination,
    from_port: u16,
    pending_data: Option<Bytes>,
}

fn normalize_socket_addr(socket: &SocketAddr) -> Cow<SocketAddr> {
    match socket {
        SocketAddr::V4(sock) => {
            let addr = sock.ip().to_ipv6_mapped();
            let sock = SocketAddr::new(addr.into(), sock.port());
            Cow::Owned(sock)
        }
        _ => Cow::Borrowed(socket),
    }
}

fn error_invalid_input<T>(msg: &'static str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, msg))
}

impl Client {
    pub async fn from_socket(mut peer_left: TcpStream, config: Arc<Config>) -> io::Result<Self> {
        let left_src = peer_left.peer_addr()?;
        let src_port = peer_left.local_addr()?.port();
        let dest = get_original_address_v4(&peer_left)
            .map(SocketAddr::V4)
            .or_else(|_| get_original_address_v6(&peer_left).map(SocketAddr::V6))
            .or_else(|_| peer_left.local_addr())?;
        #[cfg(not(target_os = "linux"))]
        let dest = peer_left.local_addr()?;
        let is_nated =
            normalize_socket_addr(&dest) != normalize_socket_addr(&peer_left.local_addr()?);

        debug!("local {} dest {}", peer_left.local_addr()?, dest);

        let dest = if cfg!(target_os = "linux") && is_nated {
            dest.into()
        } else {
            // TODO
            dest.into()
        };

        Ok(Client {
            dest,
            config: config,
            from_port: src_port,
            left: peer_left,
            src: left_src,
            pending_data: None,
        })
    }
}
