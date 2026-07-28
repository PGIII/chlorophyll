/// Integration test for the sensor-info + streaming protocol.
///
/// The test uses ephemeral loopback ports so it works in any environment
/// (CI, Docker, machines without a physical NIC). The protocol state machine —
/// `RequestSensorInfo` → `SensorsInfo` → `DataReading` — is exercised end-to-end
/// using `chlorophyll_client::registry::dispatch`, the same decode+dispatch
/// function the multicast listener thread uses; only multicast *delivery* is
/// replaced with a direct unicast send.
use chlorophyll_client::registry::{dispatch, Registry};
use chlorophyll_client::ReadingKind;
use chlorophyll_protocol::postcard::{from_bytes, to_allocvec};
use chlorophyll_protocol::temperature::Celsius;
use chlorophyll_protocol::{DataType, Packet, PacketBuilder, PacketCommand};
use chrono::Utc;
use tokio::net::UdpSocket;

const FAKE_DEVICE_ID: u128 = 0xdeadbeef_cafe1234;
const N_READINGS: usize = 5;

#[tokio::test]
async fn test_sensor_info_and_streaming() {
    // ── Fake device ──────────────────────────────────────────────────────────
    // Bind on an ephemeral loopback port to simulate the pico.
    let device_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let device_addr = device_socket.local_addr().unwrap();

    let packet_builder = PacketBuilder::new(FAKE_DEVICE_ID);

    let device_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let (len, src) = device_socket.recv_from(&mut buf).await.unwrap();
        let packet = from_bytes::<Packet>(&buf[..len]).unwrap();
        assert_eq!(packet.command(), &PacketCommand::RequestSensorInfo);

        // Reply with SensorsInfo, then immediately stream DataReadings back to
        // the server (simulating multicast with a direct unicast send).
        let resp = packet_builder.build(PacketCommand::SensorsInfo(Some("greenhouse".into())));
        device_socket.send_to(&to_allocvec(&resp).unwrap(), src).await.unwrap();

        for i in 0..N_READINGS {
            let reading = DataType::Temperature(Celsius::new(20.0 + i as f32));
            let pkt = packet_builder.build(PacketCommand::DataReading(reading));
            device_socket.send_to(&to_allocvec(&pkt).unwrap(), src).await.unwrap();
        }
    });

    // ── Server side ───────────────────────────────────────────────────────────
    let server_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let mut registry = Registry::new();
    let mut readings = Vec::new();

    // Send RequestSensorInfo directly to the fake device (unicast stand-in for
    // the multicast broadcast that the listener thread normally sends).
    let request = Packet::new(PacketCommand::RequestSensorInfo, 0);
    server_socket.send_to(&to_allocvec(&request).unwrap(), device_addr).await.unwrap();

    // Poll until we have N readings (or 5 s timeout).
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let mut buf = [0u8; 1500];
    loop {
        let (len, _src) = server_socket.recv_from(&mut buf).await.unwrap();
        let packet = from_bytes::<Packet>(&buf[..len]).unwrap();
        if let Some(reading) = dispatch(&mut registry, &packet, Utc::now()) {
            readings.push(reading);
        }
        if readings.len() >= N_READINGS {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for readings"
        );
    }

    // ── Assertions ────────────────────────────────────────────────────────────
    assert_eq!(readings.len(), N_READINGS);

    let devices = registry.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, FAKE_DEVICE_ID);
    assert_eq!(devices[0].name.as_deref(), Some("greenhouse"));

    for (i, reading) in readings.iter().enumerate() {
        assert_eq!(reading.sensor_id, FAKE_DEVICE_ID);
        assert_eq!(reading.kind, ReadingKind::Temperature);
        let expected = 20.0 + i as f32;
        assert!(
            (reading.value - expected).abs() < 0.01,
            "reading {i}: expected {expected}°C, got {}°C",
            reading.value
        );
    }

    device_handle.await.unwrap();
}
