//! JSON API: current sensor snapshot and historical readings.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chlorophyll_client::DeviceInfo;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct SensorSummary {
    pub id_hex: String,
    pub name: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub temperature: Option<f32>,
    pub humidity: Option<f32>,
    pub light: Option<f32>,
}

impl From<DeviceInfo> for SensorSummary {
    fn from(device: DeviceInfo) -> Self {
        Self {
            id_hex: format!("{:032x}", device.id),
            name: device.name,
            last_seen: device.last_seen,
            temperature: device.temperature,
            humidity: device.humidity,
            light: device.light,
        }
    }
}

async fn sensors(State(state): State<AppState>) -> Json<Vec<SensorSummary>> {
    let devices = state.client.devices();
    Json(devices.into_iter().map(SensorSummary::from).collect())
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Unix timestamp in milliseconds; only readings at or after this time are returned.
    #[serde(default)]
    since: i64,
}

#[derive(Debug, Serialize)]
pub struct PointJson {
    /// Unix timestamp in milliseconds.
    pub t: i64,
    pub v: f32,
}

#[derive(Debug, Serialize)]
pub struct SensorSeries {
    pub id_hex: String,
    pub metric: &'static str,
    pub points: Vec<PointJson>,
}

async fn history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<SensorSeries>>, axum::http::StatusCode> {
    let since = Utc.timestamp_millis_opt(query.since).single().unwrap_or(DateTime::UNIX_EPOCH);

    let series = state
        .db
        .history_since(since)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        series
            .into_iter()
            .map(|(id_hex, kind, points)| SensorSeries {
                id_hex,
                metric: kind.as_str(),
                points: points
                    .into_iter()
                    .map(|(at, v)| PointJson { t: at.timestamp_millis(), v })
                    .collect(),
            })
            .collect(),
    ))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sensors", get(sensors))
        .route("/api/sensors/history", get(history))
}
