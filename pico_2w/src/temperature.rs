// Store in Celsius, convert when request

use core::fmt::Display;

pub struct Celsius {
    value: f32,
}

struct Farenheit {
    value: f32,
}

pub trait Temperature {
    fn get_as_c(&self) -> f32;
    fn get_as_f(&self) -> f32;
}

impl Celsius {
    pub fn new(value: f32) -> Self {
        Self { value }
    }
}

impl From<Farenheit> for Celsius {
    fn from(value: Farenheit) -> Self {
        Self {
            value: value.get_as_c(),
        }
    }
}

impl Display for Celsius {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:.2}C", self.value)
    }
}

impl Temperature for Celsius {
    fn get_as_c(&self) -> f32 {
        self.value
    }

    fn get_as_f(&self) -> f32 {
        (self.value * 9.0 / 5.0) + 32.0
    }
}

impl Farenheit {
    pub fn new(value: f32) -> Self {
        Self { value }
    }
}

impl Display for Farenheit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:.2}F", self.value)
    }
}

impl Temperature for Farenheit {
    fn get_as_c(&self) -> f32 {
        (self.value - 32.0) * 5.0 / 9.0
    }

    fn get_as_f(&self) -> f32 {
        self.value
    }
}

impl From<Celsius> for Farenheit {
    fn from(value: Celsius) -> Self {
        Self {
            value: value.get_as_f(),
        }
    }
}
