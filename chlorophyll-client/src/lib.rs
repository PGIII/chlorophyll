#![warn(clippy::pedantic)]

pub mod client;
pub mod config;
#[cfg(feature = "sqlite")]
pub mod db;
pub mod listener;
pub mod reading;
pub mod registry;

pub use client::SensorClient;
pub use config::ClientConfig;
pub use reading::{DeviceInfo, Reading, ReadingKind};
