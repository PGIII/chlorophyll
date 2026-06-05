#![warn(clippy::pedantic)]

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use sensor_server::db::Db;
use sensor_server::{process_packets, request_sensor_info, send_set_name, MULTICAST_ADDR, PORT};
use tokio::net::UdpSocket;
use tracing::*;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();

    // set-name <hex_id> <name> — send SetName directly to a known sensor
    if args.get(1).map(String::as_str) == Some("set-name") {
        let id_hex = args.get(2).expect("usage: sensor_server set-name <hex_id> <name>");
        let name = args.get(3).expect("usage: sensor_server set-name <hex_id> <name>");
        let sensor_id = u128::from_str_radix(id_hex.trim_start_matches("0x"), 16)
            .expect("invalid sensor id hex");

        // Bind, discover, wait for SensorsInfo to learn the sensor's address, then send unicast.
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT)).await?;
        socket.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED)?;
        request_sensor_info(&socket).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let mut known_devices = HashMap::new();
        let mut sensor_names = HashMap::new();
        process_packets(&socket, &mut known_devices, &mut sensor_names, &mut vec![]).await?;

        let target = known_devices.get(&sensor_id).copied()
            .expect("sensor not found — is it online?");
        send_set_name(&socket, target, sensor_id, name).await?;
        return Ok(());
    }

    // Normal server mode
    let db_path = std::env::var("CHLOROPHYLL_DB").unwrap_or_else(|_| "chlorophyll.db".to_string());
    let db = Db::open(&db_path).await?;
    info!("Database opened at {db_path}");

    let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
    let socket = UdpSocket::bind(socket_addr).await?;
    socket.join_multicast_v4(MULTICAST_ADDR, Ipv4Addr::UNSPECIFIED)?;
    info!("Listening on {}:{}", MULTICAST_ADDR, PORT);

    request_sensor_info(&socket).await?;

    let mut known_devices = HashMap::new();
    let mut sensor_names: HashMap<u128, String> = HashMap::new();

    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let mut new_entries = Vec::new();
        if let Err(e) = process_packets(&socket, &mut known_devices, &mut sensor_names, &mut new_entries).await {
            error!("process_packets error: {e}");
        }
        for entry in &new_entries {
            if let Err(e) = db.insert_entry(entry).await {
                error!("DB insert error: {e}");
            }
        }
    }
}
