use std::sync::{Arc, Mutex};

use anyhow::Result;
use chlorophyll_protocol::PacketCommand;
use tokio::sync::broadcast;

use crate::config::ClientConfig;
use crate::listener;
use crate::reading::{DeviceInfo, Reading};
use crate::registry::Registry;

const READING_CHANNEL_CAPACITY: usize = 256;

/// Handle to a running multicast sensor listener.
///
/// Spawns a dedicated blocking thread that joins the multicast group, decodes
/// postcard `Packet`s, maintains a [`Registry`] of known devices, and fans out
/// [`Reading`]s on a broadcast channel.
#[derive(Debug)]
pub struct SensorClient {
    cfg: ClientConfig,
    registry: Arc<Mutex<Registry>>,
    tx: broadcast::Sender<Reading>,
}

impl SensorClient {
    /// Start listening on the configured multicast group. Spawns a blocking OS
    /// thread for the receive loop (tokio's async UDP readiness doesn't fire for
    /// this multicast socket on macOS).
    pub fn start(cfg: ClientConfig) -> Result<Self> {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let (tx, _rx) = broadcast::channel(READING_CHANNEL_CAPACITY);

        let thread_registry = registry.clone();
        let thread_tx = tx.clone();
        std::thread::spawn(move || listener::run(cfg, thread_registry, thread_tx));

        Ok(Self { cfg, registry, tx })
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Reading> {
        self.tx.subscribe()
    }

    #[must_use]
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.registry.lock().unwrap().devices()
    }

    /// Send `SetName` to the multicast group for `id`. The matching sensor stores
    /// it in NVM and announces a fresh `SensorsInfo` afterwards.
    pub fn set_name(&self, id: u128, name: &str) -> Result<()> {
        listener::send_command(self.cfg, PacketCommand::SetName(name.to_string()), id)
    }

    /// Broadcast `RequestSensorInfo` to the multicast group.
    pub fn request_sensor_info(&self) -> Result<()> {
        listener::send_command(self.cfg, PacketCommand::RequestSensorInfo, 0)
    }
}
