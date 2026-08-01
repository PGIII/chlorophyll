//! Persistent device configuration stored in a single NOR-flash sector.
//!
//! Layout: `[magic: u32][checksum: u32][postcard payload...]`
//!
//! The checksum is a wrapping byte-sum over the postcard payload, used to detect corruption
//! or uninitialised flash. The magic guards against reading data written by unrelated firmware.
//!
//! Enable the `alloc` crate feature to use `String` for the name field (e.g. on std targets).
//! Without it the name is a `heapless::String<64>` (no dynamic allocation, suitable for `no_std`).

use serde::{Deserialize, Serialize};

pub const MAGIC: u32 = 0xC410_F14C; // "chlorophyll config"

/// Maximum serialized size of `DeviceConfig` payload (conservative upper bound).
const MAX_PAYLOAD: usize = 128;

/// Maximum sensor name length when using the heapless (no-alloc) representation.
pub const MAX_NAME_LEN: usize = 64;

#[cfg(feature = "alloc")]
extern crate alloc;

/// Human-readable name for this sensor, set via the `SetName` protocol command.
#[cfg(feature = "alloc")]
pub type SensorName = alloc::string::String;
#[cfg(not(feature = "alloc"))]
pub type SensorName = heapless::String<MAX_NAME_LEN>;

/// Device configuration persisted across power cycles.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: SensorName,
}

fn checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
}

/// Try to read a `DeviceConfig` from `storage` at `offset`.
/// Returns `None` if the sector is uninitialised, corrupted, or the checksum fails.
///
/// Bound is `ReadNorFlash` rather than the more general `ReadStorage` because
/// `embassy_rp::flash::Flash` — the only caller — implements the former and not the
/// latter. It also matches [`save`], which needs `NorFlash` to erase.
pub fn load<S>(storage: &mut S, offset: u32) -> Option<DeviceConfig>
where
    S: embedded_storage::nor_flash::ReadNorFlash,
{
    let mut buf = [0u8; 8 + MAX_PAYLOAD];
    storage.read(offset, &mut buf).ok()?;

    let magic = u32::from_le_bytes(buf[..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }
    let stored_sum = u32::from_le_bytes(buf[4..8].try_into().ok()?);
    let payload = &buf[8..];
    if checksum(payload) != stored_sum {
        return None;
    }
    postcard::from_bytes::<DeviceConfig>(payload).ok()
}

/// Erase the sector at `offset` (size `erase_size` bytes) and write `config`.
pub fn save<F>(flash: &mut F, offset: u32, erase_size: u32, config: &DeviceConfig)
where
    F: embedded_storage::nor_flash::NorFlash,
{
    let Ok(payload) = postcard::to_vec::<_, MAX_PAYLOAD>(config) else {
        return;
    };
    let sum = checksum(&payload);

    let mut buf = [0u8; 8 + MAX_PAYLOAD];
    buf[..4].copy_from_slice(&MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&sum.to_le_bytes());
    buf[8..8 + payload.len()].copy_from_slice(&payload);

    let _ = flash.erase(offset, offset + erase_size);
    let _ = flash.write(offset, &buf);
}
