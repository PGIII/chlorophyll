use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use chlorophyll_protocol::postcard::from_bytes;
use chlorophyll_protocol::Packet;
use chrono::Utc;
use socket2::{Domain, Socket, Type};
use tokio::sync::broadcast;

use crate::config::ClientConfig;
use crate::registry::{dispatch, Registry};
use crate::reading::Reading;

/// Re-send `RequestSensorInfo` roughly this often (one tick per `recv_from` timeout).
const REQUEST_INFO_INTERVAL: Duration = Duration::from_secs(30);

fn bind_multicast(group: Ipv4Addr, port: u16) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    socket.bind(&addr.into())?;
    let socket: UdpSocket = socket.into();
    socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    Ok(socket)
}

/// Blocking receive loop, run on a dedicated thread.
///
/// tokio's async UDP readiness for this multicast socket does not fire on macOS, so we
/// use a blocking `recv_from` with a read timeout instead (see lambic's sensors/mod.rs).
pub fn run(
    cfg: ClientConfig,
    registry: Arc<Mutex<Registry>>,
    tx: broadcast::Sender<Reading>,
) {
    let socket = match bind_multicast(cfg.group, cfg.port) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("chlorophyll-client: cannot join multicast {}:{} : {e:#}", cfg.group, cfg.port);
            return;
        }
    };
    tracing::info!("chlorophyll-client: listening on multicast {}:{}", cfg.group, cfg.port);

    if let Err(e) = send_request_sensor_info(&socket, cfg) {
        tracing::warn!("chlorophyll-client: initial RequestSensorInfo failed: {e:#}");
    }

    let mut buf = [0u8; 1500];
    let mut last_request = Instant::now();

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, _src)) => {
                let now = Utc::now();
                match from_bytes::<Packet>(&buf[..len]) {
                    Ok(packet) => {
                        let mut reg = registry.lock().unwrap();
                        if let Some(reading) = dispatch(&mut reg, &packet, now) {
                            drop(reg);
                            let _ = tx.send(reading);
                        }
                    }
                    Err(e) => tracing::warn!("chlorophyll-client: decode failed (len {len}): {e}"),
                }
            }
            Err(ref e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => tracing::warn!("chlorophyll-client: recv error: {e}"),
        }

        if last_request.elapsed() >= REQUEST_INFO_INTERVAL {
            if let Err(e) = send_request_sensor_info(&socket, cfg) {
                tracing::warn!("chlorophyll-client: periodic RequestSensorInfo failed: {e:#}");
            }
            last_request = Instant::now();
        }
    }
}

/// Send a postcard-encoded `Packet` with the given command to the multicast group.
pub fn send_command(cfg: ClientConfig, command: chlorophyll_protocol::PacketCommand, sensor_id: u128) -> Result<()> {
    use chlorophyll_protocol::postcard::to_allocvec;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_multicast_loop_v4(false).ok();
    let dest = SocketAddrV4::new(cfg.group, cfg.port);
    let packet = Packet::new(command, sensor_id);
    let data = to_allocvec(&packet).map_err(|e| anyhow::anyhow!("{e}"))?;
    socket.send_to(&data, dest)?;
    Ok(())
}

fn send_request_sensor_info(socket: &UdpSocket, cfg: ClientConfig) -> Result<()> {
    use chlorophyll_protocol::postcard::to_allocvec;
    let packet = Packet::new(chlorophyll_protocol::PacketCommand::RequestSensorInfo, 0);
    let data = to_allocvec(&packet).map_err(|e| anyhow::anyhow!("{e}"))?;
    let dest = SocketAddrV4::new(cfg.group, cfg.port);
    socket.send_to(&data, dest)?;
    Ok(())
}
