#![no_std]
#![warn(clippy::pedantic)]
extern crate alloc;
use alloc::string::String;

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
    /// Pico → server unicast: "I'm here"; carries the sensor's NVM name if configured.
    DiscoverResponse(Option<String>),
    /// Server → multicast: "sensor matching packet.id, set your name to this string."
    SetName(String),
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
