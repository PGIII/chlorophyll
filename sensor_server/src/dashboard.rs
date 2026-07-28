//! HTML dashboard: sensor table + per-metric history charts with periodic refreshes.

use askama::Template;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use chlorophyll_client::{DeviceInfo, ReadingKind};
use chrono::Utc;
use serde::Deserialize;

use crate::state::AppState;
use crate::svg;

/// A selectable history window. `bucket_secs` is chosen so every range yields roughly
/// 700 points per series — enough to fill the chart's width without shipping a megabyte
/// of path data for the longer windows.
pub struct HistoryRange {
    pub key: &'static str,
    pub label: &'static str,
    pub hours: i64,
    pub bucket_secs: i64,
}

pub const RANGES: &[HistoryRange] = &[
    HistoryRange { key: "12h", label: "12 hours", hours: 12, bucket_secs: 60 },
    HistoryRange { key: "24h", label: "24 hours", hours: 24, bucket_secs: 120 },
    HistoryRange { key: "7d", label: "7 days", hours: 168, bucket_secs: 900 },
    HistoryRange { key: "30d", label: "30 days", hours: 720, bucket_secs: 3600 },
];

const DEFAULT_RANGE: usize = 1;

fn resolve_range(key: Option<&str>) -> &'static HistoryRange {
    key.and_then(|k| RANGES.iter().find(|r| r.key == k))
        .unwrap_or(&RANGES[DEFAULT_RANGE])
}

#[derive(Deserialize, Default)]
pub struct RangeQuery {
    range: Option<String>,
}

pub struct SensorRow {
    pub name: String,
    pub id_hex: String,
    pub temperature: Option<f32>,
    pub humidity: Option<f32>,
    pub light: Option<f32>,
    pub age: String,
}

impl From<DeviceInfo> for SensorRow {
    fn from(device: DeviceInfo) -> Self {
        let id_hex = format!("{:032x}", device.id);
        let name = device
            .name
            .clone()
            .unwrap_or_else(|| format!("sensor {}", &id_hex[24..]));
        Self {
            name,
            id_hex,
            temperature: device.temperature,
            humidity: device.humidity,
            light: device.light,
            age: format_age(device.last_seen),
        }
    }
}

fn format_age(last_seen: Option<chrono::DateTime<Utc>>) -> String {
    match last_seen {
        Some(at) => {
            let secs = (Utc::now() - at).num_seconds().max(0);
            match secs {
                0..=119 => format!("{secs}s ago"),
                120..=7199 => format!("{}m ago", secs / 60),
                _ => format!("{}h ago", secs / 3600),
            }
        }
        None => "never".to_string(),
    }
}

#[derive(Template)]
#[template(path = "sensors_table.html")]
struct SensorsTableTemplate {
    rows: Vec<SensorRow>,
}

#[derive(Template)]
#[template(path = "sensor_charts.html")]
struct SensorChartsTemplate {
    charts: Vec<MetricChart>,
    range_label: &'static str,
    range_key: &'static str,
    ranges: &'static [HistoryRange],
}

pub struct MetricChart {
    pub title: &'static str,
    pub svg: Option<String>,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    table: String,
    charts: String,
    range_key: &'static str,
}

fn build_rows(state: &AppState) -> Vec<SensorRow> {
    let mut devices = state.client.devices();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices.into_iter().map(SensorRow::from).collect()
}

async fn build_charts(
    state: &AppState,
    range: &HistoryRange,
) -> Result<Vec<MetricChart>, axum::http::StatusCode> {
    let now = Utc::now();
    let from = now - chrono::Duration::hours(range.hours);

    let series = state
        .db
        .history_bucketed(from, now, range.bucket_secs)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up display names for the legend/title from the live registry.
    let names: std::collections::HashMap<String, String> = state
        .client
        .devices()
        .into_iter()
        .map(|d| {
            (
                format!("{:032x}", d.id),
                d.name.unwrap_or_else(|| format!("sensor {:032x}", d.id)),
            )
        })
        .collect();

    let metrics = [
        (ReadingKind::Temperature, "Temperature", "\u{b0}C"),
        (ReadingKind::Humidity, "Humidity", "%"),
        (ReadingKind::Light, "Light", " lux"),
    ];

    let mut charts = Vec::with_capacity(metrics.len());
    for (kind, title, unit) in metrics {
        let mut chart_series = Vec::new();
        for (i, (id_hex, series_kind, points)) in series.iter().enumerate() {
            if *series_kind != kind {
                continue;
            }
            let label = names.get(id_hex).map_or(id_hex.as_str(), String::as_str);
            chart_series.push((label, svg::series_color(i), points.as_slice()));
        }
        let svg_series: Vec<svg::Series> = chart_series
            .into_iter()
            .map(|(label, color, points)| svg::Series {
                label,
                color,
                points,
            })
            .collect();
        let svg = svg::line_chart(&svg_series, from, now, unit);
        charts.push(MetricChart { title, svg });
    }

    Ok(charts)
}

async fn dashboard(
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let range = resolve_range(query.range.as_deref());

    let rows = build_rows(&state);
    let table = SensorsTableTemplate { rows }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let charts = build_charts(&state, range).await?;
    let charts = SensorChartsTemplate {
        charts,
        range_label: range.label,
        range_key: range.key,
        ranges: RANGES,
    }
    .render()
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let body = DashboardTemplate {
        table,
        charts,
        range_key: range.key,
    }
    .render()
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Html(body))
}

async fn sensors_table_partial(
    State(state): State<AppState>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let rows = build_rows(&state);
    let body = SensorsTableTemplate { rows }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Html(body))
}

async fn sensor_charts_partial(
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let range = resolve_range(query.range.as_deref());
    let charts = build_charts(&state, range).await?;
    let body = SensorChartsTemplate {
        charts,
        range_label: range.label,
        range_key: range.key,
        ranges: RANGES,
    }
    .render()
    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Html(body))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/partials/sensors-table", get(sensors_table_partial))
        .route("/partials/sensor-charts", get(sensor_charts_partial))
}
