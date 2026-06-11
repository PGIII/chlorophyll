use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadingKind {
    Temperature,
    Humidity,
    Light,
}

impl ReadingKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ReadingKind::Temperature => "temperature",
            ReadingKind::Humidity => "humidity",
            ReadingKind::Light => "light",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub sensor_id: u128,
    pub kind: ReadingKind,
    pub value: f32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub id: u128,
    pub name: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub temperature: Option<f32>,
    pub humidity: Option<f32>,
    pub light: Option<f32>,
}
