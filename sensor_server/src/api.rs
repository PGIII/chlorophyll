//! JSON API: current sensor snapshot, historical readings, and sensor naming.
//!
//! This is the interface consumers use instead of joining the multicast group themselves.
//! Only one process per host can practically own the sensor feed, and every extra listener
//! duplicates the decode/registry logic, so downstreams should read from here.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chlorophyll_client::{DeviceInfo, ReadingKind};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Upper bound on points returned per series when the caller doesn't pick a bucket.
/// Keeps a 30-day request the same order of magnitude as an hourly one.
const MAX_POINTS_PER_SERIES: i64 = 1000;

/// Stored data is never finer than the ingest bucket, so asking for less is pointless.
const MIN_BUCKET_SECS: i64 = chlorophyll_client::rollup::INGEST_BUCKET_SECS;

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

fn parse_id_hex(id_hex: &str) -> Option<u128> {
    u128::from_str_radix(id_hex.trim_start_matches("0x"), 16).ok()
}

async fn sensors(State(state): State<AppState>) -> Json<Vec<SensorSummary>> {
    let devices = state.client.devices();
    Json(devices.into_iter().map(SensorSummary::from).collect())
}

async fn sensor(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
) -> Result<Json<SensorSummary>, axum::http::StatusCode> {
    let id = parse_id_hex(&id_hex).ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    state
        .client
        .devices()
        .into_iter()
        .find(|d| d.id == id)
        .map(|d| Json(SensorSummary::from(d)))
        .ok_or(axum::http::StatusCode::NOT_FOUND)
}

#[derive(Debug, Deserialize, Default)]
pub struct HistoryQuery {
    /// Unix milliseconds; only readings at or after this time are returned.
    #[serde(default)]
    since: i64,
    /// Unix milliseconds; defaults to now.
    until: Option<i64>,
    /// Averaging window in seconds. Omitted means "pick one that keeps the response
    /// bounded", which is what most callers want.
    bucket: Option<i64>,
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
    /// Width of the averaging window these points represent, in seconds.
    pub bucket_secs: i64,
    pub points: Vec<PointJson>,
}

/// Resolve the window and bucket for a history request.
///
/// `earliest` is the oldest stored reading; an open-ended `since` is pulled forward to it
/// so the auto-bucket is sized against real data rather than the epoch.
fn resolve_window(
    query: &HistoryQuery,
    earliest: Option<DateTime<Utc>>,
) -> (DateTime<Utc>, DateTime<Utc>, i64) {
    let requested = Utc
        .timestamp_millis_opt(query.since)
        .single()
        .unwrap_or(DateTime::UNIX_EPOCH);
    let from = match earliest {
        Some(oldest) if oldest > requested => oldest,
        _ => requested,
    };
    let to = query
        .until
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
        .unwrap_or_else(Utc::now);

    let span_secs = (to - from).num_seconds().max(1);
    let bucket = query
        .bucket
        .unwrap_or(span_secs / MAX_POINTS_PER_SERIES)
        .max(MIN_BUCKET_SECS);

    (from, to, bucket)
}

async fn history_series(
    state: &AppState,
    query: &HistoryQuery,
    only: Option<u128>,
) -> Result<Vec<SensorSeries>, axum::http::StatusCode> {
    let earliest = state
        .db
        .earliest()
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let (from, to, bucket) = resolve_window(query, earliest);

    let series = state
        .db
        .history_bucketed(from, to, bucket)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(series
        .into_iter()
        .filter(|(id_hex, _, _)| only.is_none_or(|id| parse_id_hex(id_hex) == Some(id)))
        .map(|(id_hex, kind, points)| SensorSeries {
            id_hex,
            metric: kind.as_str(),
            bucket_secs: bucket,
            points: points
                .into_iter()
                .map(|(at, v)| PointJson { t: at.timestamp_millis(), v })
                .collect(),
        })
        .collect())
}

async fn history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<SensorSeries>>, axum::http::StatusCode> {
    Ok(Json(history_series(&state, &query, None).await?))
}

async fn sensor_history(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<SensorSeries>>, axum::http::StatusCode> {
    let id = parse_id_hex(&id_hex).ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    Ok(Json(history_series(&state, &query, Some(id)).await?))
}

#[derive(Debug, Deserialize)]
pub struct SetNameRequest {
    pub name: String,
}

/// Push a name to a sensor's NVM over multicast.
///
/// Exposed so downstream consumers don't need their own multicast socket just to rename a
/// sensor; the sensor echoes the new name back and every consumer picks it up from
/// `/api/sensors`.
async fn set_name(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
    Json(body): Json<SetNameRequest>,
) -> Result<axum::http::StatusCode, axum::http::StatusCode> {
    let id = parse_id_hex(&id_hex).ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    if body.name.trim().is_empty() {
        return Err(axum::http::StatusCode::BAD_REQUEST);
    }
    state
        .client
        .set_name(id, &body.name)
        .map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;
    Ok(axum::http::StatusCode::ACCEPTED)
}

/// Metrics the API can return, so clients don't hardcode the list.
async fn metrics() -> Json<Vec<&'static str>> {
    Json(vec![
        ReadingKind::Temperature.as_str(),
        ReadingKind::Humidity.as_str(),
        ReadingKind::Light.as_str(),
    ])
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/metrics", get(metrics))
        .route("/api/sensors", get(sensors))
        .route("/api/sensors/history", get(history))
        .route("/api/sensors/{id_hex}", get(sensor))
        .route("/api/sensors/{id_hex}/history", get(sensor_history))
        .route("/api/sensors/{id_hex}/name", post(set_name))
}
