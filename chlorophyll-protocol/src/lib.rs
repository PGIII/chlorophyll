#![no_std]

pub mod temperature;

pub use postcard;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DataType {
    Temperature(f32),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DataReading {
    pub value: DataType,
}

impl DataReading {
    pub fn new(data: DataType) -> Self {
        Self { value: data }
    }
}
