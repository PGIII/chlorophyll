/// Pre-averaged sensor values for one display frame.
pub struct DisplayState {
    pub temperature_f: Option<f32>,
    pub humidity_pct: Option<f32>,
    pub lux: Option<f32>,
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
