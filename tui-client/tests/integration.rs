/// Integration test for the multicast discovery + unicast streaming protocol.
///
/// The test uses ephemeral loopback ports so it works in any environment
/// (CI, Docker, machines without a physical NIC).  The protocol state machine
/// — Discover → DiscoverResponse → StartStreaming → DataReading — is exercised
/// end-to-end; only the multicast *delivery* step is replaced with a direct
/// unicast send, which is the only part that depends on kernel/NIC multicast
/// support.
use std::collections::HashMap;
use std::net::SocketAddr;

use chlorophyll_protocol::postcard::{from_bytes, to_allocvec};
use chlorophyll_protocol::temperature::{Celsius, Temperature};
use chlorophyll_protocol::{DataType, Packet, PacketBuilder, PacketCommand};
use tokio::net::UdpSocket;
use tui_client::app::{DataEntry, process_packets};

const FAKE_DEVICE_ID: u128 = 0xdeadbeef_cafe1234;
const N_READINGS: usize = 5;

#[tokio::test]
async fn test_discovery_and_streaming() {
    // ── Fake device ──────────────────────────────────────────────────────────
    // Bind on an ephemeral loopback port to simulate the pico.
    let device_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let device_addr = device_socket.local_addr().unwrap();

    let packet_builder = PacketBuilder::new(FAKE_DEVICE_ID);

    let device_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            let (len, src) = device_socket.recv_from(&mut buf).await.unwrap();
            let packet = from_bytes::<Packet>(&buf[..len]).unwrap();
            match packet.command() {
                PacketCommand::Discover => {
                    // Unicast DiscoverResponse back to whoever asked.
                    let resp = packet_builder.build(PacketCommand::DiscoverResponse);
                    let data = to_allocvec(&resp).unwrap();
                    device_socket.send_to(&data, src).await.unwrap();
                }
                PacketCommand::StartStreaming => {
                    // Send N temperature readings back to the server.
                    for i in 0..N_READINGS {
                        let reading = DataType::Temperature(Celsius::new(20.0 + i as f32));
                        let pkt = packet_builder.build(PacketCommand::DataReading(reading));
                        let data = to_allocvec(&pkt).unwrap();
                        device_socket.send_to(&data, src).await.unwrap();
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    // ── Server side ───────────────────────────────────────────────────────────
    let server_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let mut known_devices: HashMap<u128, SocketAddr> = HashMap::new();
    let mut readings: Vec<DataEntry> = Vec::new();

    // Send Discover directly to the fake device (unicast stand-in for the
    // multicast broadcast that send_discover() would normally use in prod).
    let discover = Packet::new(PacketCommand::Discover, 0);
    let data = to_allocvec(&discover).unwrap();
    server_socket.send_to(&data, device_addr).await.unwrap();

    // Poll until we have N readings (or 5 s timeout).
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        process_packets(&server_socket, &mut known_devices, &mut readings)
            .await
            .unwrap();
        if readings.len() >= N_READINGS {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for readings"
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // ── Assertions ────────────────────────────────────────────────────────────
    assert_eq!(readings.len(), N_READINGS);
    assert!(
        known_devices.contains_key(&FAKE_DEVICE_ID),
        "fake device not in known_devices after DiscoverResponse"
    );

    for (i, entry) in readings.iter().enumerate() {
        assert_eq!(entry.sensor_id, FAKE_DEVICE_ID);
        match &entry.data_type {
            DataType::Temperature(celsius) => {
                let expected = 20.0 + i as f32;
                assert!(
                    (celsius.get_as_c() - expected).abs() < 0.01,
                    "reading {i}: expected {expected}°C, got {}°C",
                    celsius.get_as_c()
                );
            }
            other => panic!("expected Temperature, got {:?}", other),
        }
    }

    device_handle.await.unwrap();
}
