use std::io::{self, ErrorKind};
use std::net::IpAddr;

use log::debug;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::client::{Address, Destination};

macro_rules! err {
    ($msg: expr) => {
        return Err(io::Error::new(ErrorKind::Other, $msg))
    };
}

pub async fn handshake<T>(
    remote: &mut TcpStream,
    dest: &Destination,
    data: Option<T>,
) -> io::Result<()>
where
    T: AsRef<[u8]>,
{
    // 执行 socks5 握手🤝
    // https://datatracker.ietf.org/doc/html/rfc1928#section-3
    do_handshake(remote, dest, data).await?;
    Ok(())
}

async fn do_handshake<T>(
    remote: &mut TcpStream,
    dest: &Destination,
    data: Option<T>,
) -> io::Result<()>
where
    T: AsRef<[u8]>,
{
    // +----+----------+----------+
    // |VER | NMETHODS | METHODS  |
    // +----+----------+----------+
    // | 1  |    1     | 1 to 255 |
    // +----+----------+----------+
    // we don't support user auth;
    remote.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut buf = vec![0; 2];
    remote.read_exact(&mut buf).await?;
    match buf[..] {
        [0x05, 0x00] => (),
        _ => err!(""),
    }
    let mut buf = Vec::new();
    build_request(&mut buf, dest);
    remote.write_all(&buf).await?;

    let mut buf = vec![0; 10];

    // 0x05,0x00,0x00,0x01,0x00,0x00,0x00,0x00,0x00,0x00
    remote.read_exact(&mut buf).await?;
    if buf[..2] != [0x05, 0x00] {
        err!("unexpected reply from server");
    }

    // 握手执行结束，将数据写回 stream
    if let Some(data) = data {
        debug!("Early data has been flushed into socket after finished socks5 handshake");
        remote.write_all(data.as_ref()).await?;
    }

    Ok(())
}

fn build_request(buf: &mut Vec<u8>, dest: &Destination) {
    // https://datatracker.ietf.org/doc/html/rfc1928#section-4
    buf.extend(&[0x05, 0x01, 0x00]);
    match dest.host {
        Address::Ip(ip) => match ip {
            IpAddr::V4(i) => {
                // the address is a version-4 IP address, with a length of 4 octets
                buf.extend_from_slice(&i.octets());
            }
            IpAddr::V6(ip) => {
                buf.push(0x04);
                // the address is a version-6 IP address, with a length of 16 octets.
                buf.extend_from_slice(&ip.octets());
            }
        },
        Address::Domain(ref name) => {
            buf.push(0x03);
            buf.push(name.len() as u8);
            buf.extend(name.as_bytes());
        }
    }
    //端口两字节
    buf.push((dest.port >> 8) as u8);
    buf.push(dest.port as u8);
}
