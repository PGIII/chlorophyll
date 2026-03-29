use core::f32;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct RelativeHumidity {
    percent: f32,
}

impl RelativeHumidity {
    #[must_use] 
    pub fn new(percent: f32) -> Self {
        Self { percent }
    }

    /// Return relative humidity as a percent
    #[must_use] 
    pub fn percent(&self) -> f32 {
        self.percent
    }
}

impl Default for RelativeHumidity {
    fn default() -> Self { Self { percent: 0.0 } }
}

impl core::ops::Add for RelativeHumidity {
    type Output = RelativeHumidity;
    fn add(self, rhs: RelativeHumidity) -> RelativeHumidity { RelativeHumidity { percent: self.percent + rhs.percent } }
}

impl core::ops::Div<usize> for RelativeHumidity {
    type Output = RelativeHumidity;
    fn div(self, rhs: usize) -> RelativeHumidity { RelativeHumidity { percent: self.percent / rhs as f32 } }
}

