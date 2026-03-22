use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use sensor_server::{process_packets, send_discover, MULTICAST_ADDR, PORT};
use tokio::net::UdpSocket;
use tracing::*;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
    let socket = UdpSocket::bind(socket_addr).await?;
    socket.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED)?;
    info!("Listening on {}:{}", MULTICAST_ADDR, PORT);

    send_discover(&socket).await?;

    let mut known_devices = HashMap::new();
    let mut readings = Vec::new();

    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        if let Err(e) = process_packets(&socket, &mut known_devices, &mut readings).await {
            error!("process_packets error: {e}");
        }
    }
}
