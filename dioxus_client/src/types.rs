use serde::{Deserialize, Serialize};

/// Data sent from server → WASM client on every poll.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SensorSnapshot {
    pub sensors: Vec<SensorRow>,
    /// (unix_timestamp_secs, temp_f)
    pub temp_series: Vec<(i64, f32)>,
    /// (unix_timestamp_secs, humidity_pct)
    pub humidity_series: Vec<(i64, f32)>,
    /// (unix_timestamp_secs, lux)
    pub light_series: Vec<(i64, f32)>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorRow {
    pub id: String, // 16-char hex
    pub temp_f: Option<f32>,
    pub humidity_pct: Option<f32>,
    pub lux: Option<f32>,
    pub age_secs: i64,
}
