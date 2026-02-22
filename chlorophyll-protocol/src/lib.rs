#![no_std]

pub mod temperature;

use crate::temperature::Celsius;
pub use postcard;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DataType {
    Temperature(Celsius),
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
