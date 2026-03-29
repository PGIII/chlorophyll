use serde::{Deserialize, Serialize};

pub trait Light {
    fn get_as_lux(&self) -> f32;
    fn get_as_foot_candles(&self) -> f32;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Clone, Copy)]
pub struct Lux {
    value: f32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct FootCandle {
    value: f32,
}

impl Lux {
    #[must_use] 
    pub fn new(value: f32) -> Self {
        Self { value }
    }
}

impl FootCandle {
    #[must_use] 
    pub fn new(value: f32) -> Self {
        Self { value }
    }
}

const LUX_TO_FC: f32 = 0.09290304;
const FC_TO_LUX: f32 = 10.7639;

impl Light for Lux {
    fn get_as_lux(&self) -> f32 {
        self.value
    }

    fn get_as_foot_candles(&self) -> f32 {
        self.value * LUX_TO_FC
    }
}

impl Light for FootCandle {
    fn get_as_lux(&self) -> f32 {
        self.value * FC_TO_LUX
    }

    fn get_as_foot_candles(&self) -> f32 {
        self.value
    }
}

impl Default for Lux {
    fn default() -> Self { Self { value: 0.0 } }
}

impl core::ops::Add for Lux {
    type Output = Lux;
    fn add(self, rhs: Lux) -> Lux { Lux { value: self.value + rhs.value } }
}

impl core::ops::Div<usize> for Lux {
    type Output = Lux;
    fn div(self, rhs: usize) -> Lux { Lux { value: self.value / rhs as f32 } }
}

impl From<FootCandle> for Lux {
    fn from(fc: FootCandle) -> Self {
        Self {
            value: fc.get_as_lux(),
        }
    }
}

impl From<Lux> for FootCandle {
    fn from(lux: Lux) -> Self {
        Self {
            value: lux.get_as_foot_candles(),
        }
    }
}
