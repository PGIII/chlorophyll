#![warn(clippy::pedantic)]

pub mod db;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use chlorophyll_protocol::postcard::from_bytes;
use chlorophyll_protocol::{DataType, Packet, PacketCommand};
use chrono::{DateTime, Utc};
use tokio::net::UdpSocket;
use tracing::*;

/// Keep up to ~24 h of readings at ~1 reading/sensor/5 s (generous headroom).
pub const MAX_READINGS: usize = 100_000;

pub const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 1);
pub const PORT: u16 = 5000;
/// Re-send RequestSensorInfo every ~30 s (at 30 fps tick rate).
pub const REDISCOVER_TICKS: u64 = 900;

#[derive(Debug)]
pub struct DataEntry {
    pub data_type: DataType,
    pub sensor_id: u128,
    pub timestamp: DateTime<Utc>,
}

/// Broadcast `RequestSensorInfo` to the multicast group to find any online sensors.
pub async fn request_sensor_info(socket: &UdpSocket) -> color_eyre::Result<()> {
    use chlorophyll_protocol::postcard::to_allocvec;
    let packet = Packet::new(PacketCommand::RequestSensorInfo, 0);
    let data = to_allocvec(&packet).map_err(|e| color_eyre::eyre::eyre!("{e}"))?;
    let dest = SocketAddrV4::new(MULTICAST_ADDR, PORT);
    socket.send_to(&data, dest).await?;
    info!("Sent RequestSensorInfo to {}", dest);
    Ok(())
}

/// Drain all pending inbound packets and handle protocol logic.
///
/// - `SensorsInfo`  → record device address and optional sensor name
/// - `DataReading`  → append to `readings`
pub async fn process_packets(
    socket: &UdpSocket,
    known_devices: &mut HashMap<u128, SocketAddr>,
    sensor_names: &mut HashMap<u128, String>,
    readings: &mut Vec<DataEntry>,
) -> color_eyre::Result<()> {
    let mut buf = [0u8; 1500];
    loop {
        match socket.try_recv_from(&mut buf) {
            Ok((len, src)) => match from_bytes::<Packet>(&buf[..len]) {
                Ok(packet) => match packet.command().clone() {
                    PacketCommand::SensorsInfo(name) => {
                        let id = packet.id();
                        info!("SensorsInfo from {} (id={:x})", src, id);
                        known_devices.insert(id, src);
                        if let Some(n) = name {
                            info!("  sensor name: {n}");
                            sensor_names.insert(id, n);
                        }
                    }
                    PacketCommand::DataReading(data_type) => {
                        let now = Utc::now();
                        debug!(
                            "[{}] Got DataReading from {} (id={:x})",
                            now.format("%H:%M:%S%.3f"),
                            src,
                            packet.id()
                        );
                        let entry = DataEntry {
                            data_type,
                            sensor_id: packet.id(),
                            timestamp: now,
                        };
                        if readings.len() >= MAX_READINGS {
                            readings.remove(0);
                        }
                        readings.push(entry);
                    }
                    _ => {}
                },
                Err(e) => {
                    error!("Error parsing packet: {e}");
                }
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                error!("Error reading from socket: {e}");
                break;
            }
        }
    }
    Ok(())
}

/// Send a `SetName` packet directly to a known sensor's address.
/// The sensor's address must be known from a prior `SensorsInfo` response.
pub async fn send_set_name(socket: &UdpSocket, target: SocketAddr, sensor_id: u128, name: &str) -> color_eyre::Result<()> {
    use chlorophyll_protocol::postcard::to_allocvec;
    let packet = Packet::new(PacketCommand::SetName(name.to_string()), sensor_id);
    let data = to_allocvec(&packet).map_err(|e| color_eyre::eyre::eyre!("{e}"))?;
    socket.send_to(&data, target).await?;
    info!("Sent SetName(\"{name}\") to {} (sensor {:032x})", target, sensor_id);
    Ok(())
}
