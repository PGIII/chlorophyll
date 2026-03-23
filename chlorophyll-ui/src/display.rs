use chlorophyll_protocol::{humidity::RelativeHumidity, light::Lux, temperature::Celsius};

/// Pre-averaged sensor values for one display frame.
pub struct DisplayState {
    pub temperature: Option<Celsius>,
    pub humidity: Option<RelativeHumidity>,
    pub lux: Option<Lux>,
    /// Set to `true` when the device detected it was previously reset by the watchdog.
    pub watchdog_reset: bool,
}

/// Trait implemented by every concrete display type.
///
/// A `SensorDisplay` knows how to render a [`DisplayState`] onto its own
/// hardware or buffer.  Different implementations can vary in resolution,
/// color depth, or layout while sharing the same `DisplayState` input.
pub trait SensorDisplay {
    type Error;
    fn render(&mut self, state: &DisplayState) -> Result<(), Self::Error>;
}
