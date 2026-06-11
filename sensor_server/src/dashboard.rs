//! HTML dashboard: sensor table + per-metric history charts, polled via htmx.

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use chlorophyll_client::{DeviceInfo, ReadingKind};
use chrono::Utc;

use crate::state::AppState;
use crate::svg;

const HISTORY_WINDOW_HOURS: i64 = 12;

fn nav() -> Vec<orbit_ui::NavLink> {
    vec![orbit_ui::NavLink {
        label: "Sensors".to_string(),
        href: "/".to_string(),
    }]
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
        let name = device.name.clone().unwrap_or_else(|| format!("sensor {}", &id_hex[24..]));
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
}

fn build_rows(state: &AppState) -> Vec<SensorRow> {
    let mut devices = state.client.devices();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices.into_iter().map(SensorRow::from).collect()
}

async fn build_charts(state: &AppState) -> Result<Vec<MetricChart>, axum::http::StatusCode> {
    let now = Utc::now();
    let from = now - chrono::Duration::hours(HISTORY_WINDOW_HOURS);

    let series = state
        .db
        .history_since(from)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up display names for the legend/title from the live registry.
    let names: std::collections::HashMap<String, String> = state
        .client
        .devices()
        .into_iter()
        .map(|d| (format!("{:032x}", d.id), d.name.unwrap_or_else(|| format!("sensor {:032x}", d.id))))
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
            .map(|(label, color, points)| svg::Series { label, color, points })
            .collect();
        let svg = svg::line_chart(&svg_series, from, now, unit);
        charts.push(MetricChart { title, svg });
    }

    Ok(charts)
}

async fn dashboard(State(state): State<AppState>) -> Result<Html<String>, axum::http::StatusCode> {
    let rows = build_rows(&state);
    let table = SensorsTableTemplate { rows }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let charts = build_charts(&state).await?;
    let charts = SensorChartsTemplate { charts }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let body = DashboardTemplate { table, charts }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let page = orbit_ui::render_page("Chlorophyll", &nav(), &body)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Html(page))
}

async fn sensors_table_partial(State(state): State<AppState>) -> Result<Html<String>, axum::http::StatusCode> {
    let rows = build_rows(&state);
    let body = SensorsTableTemplate { rows }
        .render()
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Html(body))
}

async fn sensor_charts_partial(State(state): State<AppState>) -> Result<Html<String>, axum::http::StatusCode> {
    let charts = build_charts(&state).await?;
    let body = SensorChartsTemplate { charts }
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
