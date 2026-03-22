#![no_std]

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
    DiscoverResponse,
    /// Server → pico unicast: "send data to me"
    StartStreaming,
    /// Server → pico unicast: "stop sending"
    StopStreaming,
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
    pub fn new(command: PacketCommand, id: SensorID) -> Self {
        Self { command, id }
    }

    pub fn command(&self) -> &PacketCommand { &self.command }
    pub fn id(&self) -> SensorID { self.id }
}

/// Builds new packets, storing common data
#[derive(Debug, PartialEq, Clone)]
pub struct PacketBuilder {
    id: SensorID,
}

impl PacketBuilder {
    pub fn new(id: SensorID) -> Self {
        Self { id }
    }

    pub fn build(&self, command: PacketCommand) -> Packet {
        Packet::new(command, self.id)
    }
}
