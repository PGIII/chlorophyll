use core::f32;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RelativeHumidity {
    percent: f32,
}

impl RelativeHumidity {
    pub fn new(percent: f32) -> Self {
        Self { percent }
    }

    /// Return relative humidity as a percent
    pub fn percent(&self) -> f32 {
        self.percent
    }
}

