use std::{
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    u16,
};

use clap::{load_yaml, AppSettings};
use log::{error, info, LevelFilter};
use socket_proxy::{client::Client, config::Config};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    let yaml = load_yaml!("./cli.yaml");
    let app = clap::App::from_yaml(&yaml)
        .setting(AppSettings::ColoredHelp)
        .setting(AppSettings::UnifiedHelpMessage)
        .get_matches();
    let mut logger = env_logger::Builder::new();
    let log_level: &str = app.value_of("log-level").expect("need log level");
    logger
        .filter(None, log_level.parse().expect("unknown log level"))
        .filter_module("tokio_net", LevelFilter::Warn)
        .target(env_logger::Target::Stdout)
        .format(|buf, r| {
            writeln!(
                buf,
                "[{}] {}:{} {}",
                r.level(),
                r.file().unwrap_or("unknown"),
                r.line().unwrap_or(0),
                r.args()
            )
        })
        .init();
    info!("start");

    let host: IpAddr = app
        .value_of("host")
        .expect("missing host")
        .parse()
        .expect("invalid address");
    let port: usize = app
        .value_of("port")
        .expect("missing port")
        .parse()
        .expect("invalid port number");
    let socks_proxy_server: SocketAddr = app
        .value_of("socks5")
        .expect("missing socks5 server address")
        .parse()
        .expect("invalid socks5 address");
    let config = Arc::new(Config {
        socket5_server: socks_proxy_server,
        host,
        port,
    });
    // 开始监听
    let addr = SocketAddr::new(host, port as u16);
    let listener = TcpListener::bind(&addr).await.expect("failed to bind port");
    info!("listen on {}", addr);
    while let Ok((socks, _addr)) = listener.accept().await {
        let result = handle_client(socks, config.clone()).await;
        if let Err(err) = result {
            error!("handle client error {}", err);
        }
    }
}

async fn handle_client(peer_left: TcpStream, config: Arc<Config>) -> io::Result<()> {
    let mut client = Client::from_socket(peer_left, config).await?;
    let remote = if client.dest.port == 443 {
        client = client.retrieve_dest().await?;
        client.connect_remote_server().await?
    } else {
        client.connect_remote_server().await?
    };
    client.do_pipe(remote).await?;
    Ok(())
}
