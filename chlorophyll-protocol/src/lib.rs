#![no_std]
#![warn(clippy::pedantic)]

pub mod temperature;
pub mod humidity;
pub mod light;

use crate::{humidity::RelativeHumidity, light::Lux, temperature::Celsius};
pub use postcard;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DataType {
    Temperature(Celsius),
    RelativeHumidity(RelativeHumidity),
    Light(Lux),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum PacketCommand {
    DataReading(DataType),
    /// Server → multicast: "who's online?"
    Discover,
    /// Pico → server unicast: "I'm here" (device id in packet header)
    /// Pico always streams `DataReading` to multicast; no `StartStreaming` needed.
    DiscoverResponse,
}

type SensorID = u128;
/// Chlorophyll packet that can hold a variety of commands
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Packet {
    command: PacketCommand,
    /// Unique ID to identify the sensor
    id: SensorID, 
}
impl Packet {
    #[must_use] 
    pub fn new(command: PacketCommand, id: SensorID) -> Self {
        Self { command, id }
    }

    #[must_use] 
    pub fn command(&self) -> &PacketCommand { &self.command }
    #[must_use] 
    pub fn id(&self) -> SensorID { self.id }
}

/// Builds new packets, storing common data
#[derive(Debug, PartialEq, Clone)]
pub struct PacketBuilder {
    id: SensorID,
}

impl PacketBuilder {
    #[must_use] 
    pub fn new(id: SensorID) -> Self {
        Self { id }
    }

    #[must_use] 
    pub fn build(&self, command: PacketCommand) -> Packet {
        Packet::new(command, self.id)
    }
}
