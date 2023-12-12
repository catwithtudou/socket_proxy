use std::net::{IpAddr, SocketAddr};

pub struct Config {
    pub socket5_server: SocketAddr,
    pub host: IpAddr,
    pub port: usize,
}