use std::collections::BTreeMap;

use chlorophyll_protocol::light::Light;
use chlorophyll_protocol::temperature::Temperature;
use chlorophyll_protocol::{DataType, Packet, PacketCommand};
use chrono::{DateTime, Utc};

use crate::reading::{DeviceInfo, Reading, ReadingKind};

/// Tracks known sensors keyed by id. Updated by [`dispatch`] as packets arrive.
#[derive(Debug, Default)]
pub struct Registry {
    devices: BTreeMap<u128, DeviceInfo>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.values().cloned().collect()
    }
}

/// Apply a decoded packet to the registry, returning a [`Reading`] for `DataReading`
/// packets (so the caller can fan it out on a broadcast channel).
pub fn dispatch(registry: &mut Registry, packet: &Packet, now: DateTime<Utc>) -> Option<Reading> {
    let id = packet.id();

    match packet.command() {
        PacketCommand::SensorsInfo(name) => {
            let device = registry.devices.entry(id).or_insert_with(|| DeviceInfo {
                id,
                ..Default::default()
            });
            device.last_seen = Some(now);
            if let Some(name) = name {
                device.name = Some(name.clone());
            }
            None
        }
        PacketCommand::DataReading(data) => {
            let device = registry.devices.entry(id).or_insert_with(|| DeviceInfo {
                id,
                ..Default::default()
            });
            device.last_seen = Some(now);
            let (kind, value) = match data {
                DataType::Temperature(t) => (ReadingKind::Temperature, t.get_as_c()),
                DataType::RelativeHumidity(h) => (ReadingKind::Humidity, h.percent()),
                DataType::Light(l) => (ReadingKind::Light, l.get_as_lux()),
            };
            match kind {
                ReadingKind::Temperature => device.temperature = Some(value),
                ReadingKind::Humidity => device.humidity = Some(value),
                ReadingKind::Light => device.light = Some(value),
            }
            Some(Reading {
                sensor_id: id,
                kind,
                value,
                at: now,
            })
        }
        PacketCommand::RequestSensorInfo | PacketCommand::SetName(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chlorophyll_protocol::humidity::RelativeHumidity;
    use chlorophyll_protocol::light::Lux;
    use chlorophyll_protocol::postcard::{from_bytes, to_allocvec};
    use chlorophyll_protocol::temperature::Celsius;

    #[test]
    fn packet_roundtrips_through_postcard() {
        let packet = Packet::new(
            PacketCommand::DataReading(DataType::Temperature(Celsius::new(21.5))),
            42,
        );
        let bytes = to_allocvec(&packet).unwrap();
        let decoded: Packet = from_bytes(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn dispatch_updates_registry_and_emits_reading() {
        let mut registry = Registry::new();
        let now = Utc::now();

        let info = Packet::new(PacketCommand::SensorsInfo(Some("greenhouse".into())), 7);
        assert!(dispatch(&mut registry, &info, now).is_none());

        let temp = Packet::new(
            PacketCommand::DataReading(DataType::Temperature(Celsius::new(22.0))),
            7,
        );
        let reading = dispatch(&mut registry, &temp, now).expect("reading");
        assert_eq!(reading.sensor_id, 7);
        assert_eq!(reading.kind, ReadingKind::Temperature);
        assert!((reading.value - 22.0).abs() < f32::EPSILON);

        let hum = Packet::new(
            PacketCommand::DataReading(DataType::RelativeHumidity(RelativeHumidity::new(55.0))),
            7,
        );
        dispatch(&mut registry, &hum, now);

        let light = Packet::new(
            PacketCommand::DataReading(DataType::Light(Lux::new(123.0))),
            7,
        );
        dispatch(&mut registry, &light, now);

        let devices = registry.devices();
        assert_eq!(devices.len(), 1);
        let device = &devices[0];
        assert_eq!(device.id, 7);
        assert_eq!(device.name.as_deref(), Some("greenhouse"));
        assert_eq!(device.temperature, Some(22.0));
        assert_eq!(device.humidity, Some(55.0));
        assert_eq!(device.light, Some(123.0));
        assert_eq!(device.last_seen, Some(now));
    }

    #[test]
    fn dispatch_ignores_control_packets() {
        let mut registry = Registry::new();
        let now = Utc::now();

        let request = Packet::new(PacketCommand::RequestSensorInfo, 0);
        assert!(dispatch(&mut registry, &request, now).is_none());

        let set_name = Packet::new(PacketCommand::SetName("greenhouse".into()), 7);
        assert!(dispatch(&mut registry, &set_name, now).is_none());

        assert!(registry.devices().is_empty());
    }
}
